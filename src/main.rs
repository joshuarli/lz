mod options;
mod security;
mod tags;

use lz::{buffer, history, pager, terminal};
use lz::charset::Charset;
use lz::ansi::ControlDisplay;

use std::fs::File;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::io::{self, IsTerminal, Write};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "lz {} — a minimal pager

USAGE: lz [OPTIONS] [FILE]

OPTIONS:
    +F          Start in follow mode
    -f, --force View binary files
    -i          Ignore ASCII case unless the pattern has uppercase
    -I          Always ignore ASCII case
    -r, --raw   Raw mode (show ANSI escapes literally)
    -?, --help  Show this help
    -V, --version  Show version

KEYS:
    j/Down      Scroll down       k/Up        Scroll up
    Space/PgDn  Page down          b/PgUp      Page up
    d/Ctrl-D    Half page down     u/Ctrl-U    Half page up
    g/Home      Top                G/End       Bottom
    Left/Right  Horizontal scroll  w           Toggle wrap
    /           Search forward     ?           Search backward
    n/N         Next/prev match    &           Filter lines
    F           Enter follow mode  h           Help
    q/Esc       Quit

FILE:
    File to view. If omitted or '-', reads stdin.",
        VERSION
    );
}

enum ExitAction { Success, FileError, Terminated, Hangup }

enum BinaryAnswer { Open, Refuse, Quit }

fn read_prompt_key(keys: &mut lz::input::KeyReader) -> io::Result<Option<lz::input::Key>> {
    loop {
        if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return Ok(None); }
        if let Some(key) = keys.read_key_timeout(100)? { return Ok(Some(key)); }
    }
}

fn acknowledge_diagnostics(keys: &mut lz::input::KeyReader) -> io::Result<bool> {
    let mut output = io::stderr().lock();
    output.write_all(b"Press RETURN to continue ")?;
    output.flush()?;
    let Some(key) = read_prompt_key(keys)? else { return Ok(false); };
    if !key.is_prompt_acknowledgement() { keys.unread_key(key)?; }
    output.write_all(b"\r\n")?;
    output.flush()?;
    Ok(true)
}

fn confirm_binary(name: &std::ffi::OsStr, charset: Charset, keys: &mut lz::input::KeyReader) -> io::Result<BinaryAnswer> {
    let mut output = io::stdout().lock();
    output.write_all(b"\"")?;
    charset.write_filename(name.as_bytes(), &mut output)?;
    output.write_all(b"\" may be a binary file.  See it anyway? ")?;
    output.flush()?;
    let Some(key) = read_prompt_key(keys)? else { return Ok(BinaryAnswer::Refuse); };
    let answer = match key { lz::input::Key::Char('y' | 'Y') => BinaryAnswer::Open,
        lz::input::Key::Char('Q') => BinaryAnswer::Quit, _ => BinaryAnswer::Refuse };
    output.write_all(b"\r\n")?;
    output.flush()?;
    Ok(answer)
}

fn finish_session(result: io::Result<ExitAction>, session: terminal::Session) -> Result<ExitAction, String> {
    let cleanup = session.finish();
    match (result, cleanup) {
        (Ok(action), Ok(())) => match terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) {
            0 => Ok(action),
            15 => Ok(ExitAction::Terminated),
            1 => Ok(ExitAction::Hangup),
            signal => Err(format!("unexpected termination signal: {signal}")),
        },
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error.to_string()),
        (Err(error), Err(cleanup)) => Err(format!("{error}; terminal restoration: {cleanup}")),
    }
}

struct OpenedInput { buffer: buffer::LineBuffer, name: Option<String>, binary_name: Option<OsString> }

fn open_input(filename: &std::ffi::OsStr, charset: Charset, controls: ControlDisplay, force: bool) -> io::Result<OpenedInput> {
    if filename == "-" {
        return Ok(OpenedInput { buffer: buffer::LineBuffer::from_stdin(io::stdin())?, name: None, binary_name: None });
    }
    let file = File::open(filename)?;
    let binary_name = if !force && buffer::LineBuffer::check_binary(&file, charset, controls)? { Some(filename.to_owned()) } else { None };
    let mut name = Vec::new();
    charset.write_filename(filename.as_bytes(), &mut name)?;
    let name = String::from_utf8(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(OpenedInput { buffer: buffer::LineBuffer::from_file(file), name: Some(name), binary_name })
}

fn run() -> Result<ExitAction, String> {
    let security = security::Policy::from_environment();
    security.write_diagnostics(&mut io::stderr().lock()).map_err(|error| error.to_string())?;
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut args = options::parse(argv, std::env::var_os("LESS"));
    for diagnostic in &args.diagnostics {
        diagnostic.write(&mut io::stderr().lock()).map_err(|error| error.to_string())?;
    }
    match args.action {
        options::Action::Help => { print_help(); return Ok(ExitAction::Success); }
        options::Action::Version => { println!("lz {}", VERSION); return Ok(ExitAction::Success); }
        options::Action::Stop => return Ok(ExitAction::Success),
        options::Action::Page => {}
    }
    let mut seen = std::collections::BTreeSet::new();
    args.filenames.retain(|filename| {
        let resolved = if filename == "-" { std::path::PathBuf::from(filename) }
            else { std::fs::canonicalize(filename).unwrap_or_else(|_| filename.into()) };
        seen.insert(resolved)
    });

    if !io::stdout().is_terminal() {
        let mut output = io::stdout().lock();
        for filename in &args.filenames {
            if filename == "-" {
                io::copy(&mut io::stdin().lock(), &mut output).map_err(|error| error.to_string())?;
                continue;
            }
            match File::open(filename) {
                Ok(mut file) => {
                    let metadata = file.metadata().map_err(|error| error.to_string())?;
                    if metadata.is_dir() {
                        output.write_all(filename.as_bytes()).map_err(|error| error.to_string())?;
                        writeln!(output, " is a directory").map_err(|error| error.to_string())?;
                    } else if !metadata.is_file() && !args.force {
                        output.write_all(filename.as_bytes()).map_err(|error| error.to_string())?;
                        writeln!(output, " is not a regular file (use -f to see it)").map_err(|error| error.to_string())?;
                    } else {
                        io::copy(&mut file, &mut output).map_err(|error| error.to_string())?;
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    let message = message.rsplit_once(" (os error ").map_or(message.as_str(), |(text, _)| text);
                    output.write_all(filename.as_bytes()).map_err(|error| error.to_string())?;
                    writeln!(output, ": {}", message).map_err(|error| error.to_string())?;
                }
            }
        }
        output.flush().map_err(|error| error.to_string())?;
        return Ok(ExitAction::Success);
    }

    if let Some(tag) = &args.tag {
        if args.stdin_explicit || args.filenames.iter().any(|filename| filename != "-") {
            eprintln!("No filenames allowed with -t option");
            return Ok(ExitAction::FileError);
        }
        let selected = tags::select(args.tags_file.as_deref().unwrap_or(std::ffi::OsStr::new("tags")), tag)
            .map_err(|error| error.to_string())?;
        args.filenames = vec![selected.filename];
        args.startup_commands.insert(0, format!("{}g", selected.line).into_bytes());
    }
    let is_stdin = args.filenames.first().is_some_and(|name| name == "-");

    if is_stdin && atty_stdin() && !args.stdin_explicit {
        eprintln!("lz: missing filename (use '-' for stdin)");
        process::exit(2);
    }

    let text_setting = |name| match std::env::var(name) {
        Ok(value) => Ok(Some(value)), Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(format!("{name}: {error}")),
    };
    let explicit_charset = text_setting("LESSCHARSET")?;
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"].into_iter().map(text_setting).collect::<Result<Vec<_>, _>>()?
        .into_iter().flatten().find(|value| !value.is_empty());
    let charset = Charset::resolve(explicit_charset.as_deref(), locale.as_deref()).map_err(|error| error.to_string())?;
    let mut opened = open_input(&args.filenames[0], charset, args.control_display, args.force)
        .map_err(|error| format!("{}: {error}", args.filenames[0].to_string_lossy()))?;
    let primary = opened.binary_name.is_some() || security.has_diagnostics();
    let screen = if primary { terminal::Screen::Primary } else { terminal::Screen::Interactive(args.terminal) };
    let mut session = terminal::Session::start(screen).map_err(|error| error.to_string())?;
    if let Some(name) = opened.binary_name.take() {
        match confirm_binary(&name, charset, session.keys_mut()) {
            Ok(BinaryAnswer::Refuse) => return finish_session(Ok(ExitAction::FileError), session),
            Ok(BinaryAnswer::Quit) => return finish_session(Ok(ExitAction::Success), session),
            Err(error) => return finish_session(Err(error), session),
            Ok(BinaryAnswer::Open) => {}
        }
    }

    if security.has_diagnostics() {
        match acknowledge_diagnostics(session.keys_mut()) {
            Ok(true) => {}, Ok(false) => return finish_session(Ok(ExitAction::Success), session),
            Err(error) => return finish_session(Err(error), session),
        }
    }
    if primary && let Err(error) = session.enter_interactive(args.terminal) { return finish_session(Err(error), session); }
    let history_file = if security.allows(security::Feature::History) { history::HistoryFile::from_environment() } else { None };
    let mut history = history_file.as_ref().map(history::HistoryFile::load).unwrap_or_default();

    let mut file_index = 0;
    let mut initial_display = pager::InitialDisplay::Startup;
    let mut display_anchor = pager::DisplayAnchor::new(args.terminal.screen);
    let mut positions = vec![None; args.filenames.len()];
    let result = loop {
        let files = match pager::FileStatus::new(&args.filenames, file_index, charset) { Ok(files) => files, Err(error) => break Err(error) };
        let mut pager = pager::Pager::new(opened.buffer, pager::Config { files, initial_display, display_anchor, startup_commands: std::mem::take(&mut args.startup_commands), charset, controls: args.control_display, chop: args.chop, line_numbers: args.line_numbers, search_case: args.search_case }, opened.name, history);
        let result = pager.run(session.keys_mut());
        positions[file_index] = pager.saved_position();
        display_anchor = pager.display_anchor();
        history = pager.into_history();
        match result {
            Err(error) => break Err(error),
            Ok(pager::PagerExit::Quit) => break Ok(ExitAction::Success),
            Ok(pager::PagerExit::NextFile) => file_index += 1,
            Ok(pager::PagerExit::PreviousFile) => file_index -= 1,
        }
        opened = match open_input(&args.filenames[file_index], charset, args.control_display, args.force) {
            Ok(input) => input, Err(error) => break Err(error),
        };
        if let Some(name) = opened.binary_name.take() {
            match confirm_binary(&name, charset, session.keys_mut()) {
                Ok(BinaryAnswer::Refuse) => break Ok(ExitAction::FileError),
                Ok(BinaryAnswer::Quit) => break Ok(ExitAction::Success),
                Ok(BinaryAnswer::Open) => {},
                Err(error) => break Err(error),
            }
        }
        initial_display = pager::InitialDisplay::FileSwitch(positions[file_index]);
    };
    if let Some(file) = history_file { file.save(&mut history); }
    finish_session(result, session)
}

fn atty_stdin() -> bool {
    unsafe extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(0) != 0 }
}

fn main() {
    // Set panic hook to restore terminal
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        terminal::restore_terminal();
        default_hook(info);
    }));

    match run() {
        Ok(ExitAction::Success) => process::exit(0),
        Ok(ExitAction::FileError) => process::exit(1),
        Ok(ExitAction::Terminated) => process::exit(15),
        Ok(ExitAction::Hangup) => {
            unsafe extern "C" { fn signal(number: i32, handler: usize) -> usize; fn raise(number: i32) -> i32; }
            // SAFETY: signal 1 is SIGHUP on supported Unix targets; 0 restores its default disposition.
            unsafe { signal(1, 0); raise(1); }
            process::exit(129);
        }
        Err(e) => {
            eprintln!("lz: {}", e);
            process::exit(1);
        }
    }
}
