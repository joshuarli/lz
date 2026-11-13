mod options;
mod security;
mod tags;
mod preprocessor;

use lz::{buffer, history, pager, terminal};
use lz::charset::Charset;
use lz::ansi::ControlDisplay;

use std::fs::File;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::io::{self, IsTerminal, Write};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

enum ExitAction { Success, FileError, Interrupted, Terminated, Hangup }

enum BinaryAnswer { Open, Refuse, Quit }

enum DiagnosticAnswer { Continue, Interrupted, Terminated }

fn read_prompt_key(keys: &mut lz::input::KeyReader, bell_policy: terminal::BellPolicy) -> io::Result<Option<lz::input::Key>> {
    loop {
        if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return Ok(None); }
        let key = if terminal::take_interrupt() { Some(lz::input::Key::Ctrl('c')) } else { keys.read_key_timeout(100)? };
        if let Some(key) = key {
            if key == lz::input::Key::Ctrl('c') {
                let mut output = io::stdout().lock();
                bell_policy.write_to(&mut output, terminal::BellKind::General)?;
                output.flush()?;
            }
            return Ok(Some(key));
        }
    }
}

fn acknowledge_diagnostics(keys: &mut lz::input::KeyReader, bell_policy: terminal::BellPolicy) -> io::Result<DiagnosticAnswer> {
    let mut output = io::stderr().lock();
    output.write_all(b"Press RETURN to continue ")?;
    output.flush()?;
    let Some(key) = read_prompt_key(keys, bell_policy)? else { return Ok(DiagnosticAnswer::Terminated); };
    let answer = if key == lz::input::Key::Ctrl('c') { DiagnosticAnswer::Interrupted } else { DiagnosticAnswer::Continue };
    if !key.is_prompt_acknowledgement() { keys.unread_key(key)?; }
    output.write_all(b"\r\n")?;
    output.flush()?;
    Ok(answer)
}

fn confirm_binary(name: &std::ffi::OsStr, charset: Charset, keys: &mut lz::input::KeyReader, bell_policy: terminal::BellPolicy) -> io::Result<BinaryAnswer> {
    let mut output = io::stdout().lock();
    output.write_all(b"\"")?;
    charset.write_filename(name.as_bytes(), &mut output)?;
    output.write_all(b"\" may be a binary file.  See it anyway? ")?;
    output.flush()?;
    let Some(key) = read_prompt_key(keys, bell_policy)? else { return Ok(BinaryAnswer::Refuse); };
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

struct AlternateInput { original: OsString, replacement: OsString }

struct OpenedInput { buffer: buffer::LineBuffer, name: Option<String>, binary_name: Option<OsString>, alternate: Option<AlternateInput> }

enum CloseContext { Switching { squished: bool, interrupt_policy: pager::InterruptPolicy, bell_policy: terminal::BellPolicy }, Exiting }

fn close_alternate(alternate: Option<AlternateInput>, session: &mut terminal::Session, context: CloseContext) -> io::Result<()> {
    let Some(alternate) = alternate else { return Ok(()); };
    if matches!(preprocessor::close(&alternate.original, &alternate.replacement, session)?, preprocessor::CloseOutcome::InvalidTemplate) {
        const MESSAGE: &str = "LESSCLOSE ignored; must contain no more than 2 %s";
        match context {
            CloseContext::Exiting => eprintln!("{MESSAGE}"),
            CloseContext::Switching { squished, interrupt_policy, bell_policy } => {
                let mut frame = Vec::new();
                let (_, height) = terminal::get_terminal_size();
                if squished {
                    for row in 0..height.saturating_sub(1) {
                        terminal::move_cursor(&mut frame, row, 0);
                        terminal::clear_line(&mut frame);
                        frame.extend_from_slice(b"\x1b[1m~\x1b[m");
                    }
                }
                terminal::move_cursor(&mut frame, height.saturating_sub(1), 0);
                terminal::clear_line(&mut frame);
                write!(frame, "\x1b[7m{MESSAGE}  (press RETURN)\x1b[m")?;
                let mut output = io::stdout().lock();
                output.write_all(&frame)?;
                output.flush()?;
                drop(output);
                if let Some(key) = read_prompt_key(session.keys_mut(), bell_policy)? {
                    if key == lz::input::Key::Ctrl('c') && interrupt_policy == pager::InterruptPolicy::Quit {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "postprocessor diagnostic interrupted"));
                    }
                    if !key.is_prompt_acknowledgement() { session.keys_mut().unread_key(key)?; }
                }
            }
        }
    }
    Ok(())
}

fn with_cleanup<T>(result: io::Result<T>, cleanup: io::Result<()>) -> io::Result<T> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!("{error}; postprocessor: {cleanup}"))),
    }
}

fn close_opened<T>(result: io::Result<T>, opened: OpenedInput, session: &mut terminal::Session) -> io::Result<T> {
    let OpenedInput { buffer, alternate, .. } = opened;
    drop(buffer);
    with_cleanup(result, close_alternate(alternate, session, CloseContext::Exiting))
}

fn finish_opened(result: io::Result<ExitAction>, opened: OpenedInput, mut session: terminal::Session) -> Result<ExitAction, String> {
    let result = close_opened(result, opened, &mut session);
    finish_session(result, session)
}

fn open_input(filename: &std::ffi::OsStr, charset: Charset, controls: ControlDisplay, force: bool, preprocessor: Option<&preprocessor::Template>, session: &mut terminal::Session) -> io::Result<OpenedInput> {
    let replacement = preprocessor.map(|template| template.run(filename, session)).transpose()?.flatten();
    let alternate = replacement.map(|replacement| AlternateInput { original: filename.to_owned(), replacement });
    let result = (|| {
        if filename == "-" {
            return Ok((buffer::LineBuffer::from_stdin(io::stdin())?, None, None));
        }
        let file = File::open(alternate.as_ref().map_or(filename, |alternate| alternate.replacement.as_os_str()))?;
        let binary_name = if !force && buffer::LineBuffer::check_binary(&file, charset, controls)? { Some(filename.to_owned()) } else { None };
        let mut name = Vec::new();
        charset.write_filename(filename.as_bytes(), &mut name)?;
        let name = String::from_utf8(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok((buffer::LineBuffer::from_file(file), Some(name), binary_name))
    })();
    match result {
        Ok((buffer, name, binary_name)) => Ok(OpenedInput { buffer, name, binary_name, alternate }),
        Err(error) => with_cleanup(Err(error), close_alternate(alternate, session, CloseContext::Exiting)),
    }
}

fn run() -> Result<ExitAction, String> {
    let security = security::Policy::from_environment();
    security.write_diagnostics(&mut io::stderr().lock()).map_err(|error| error.to_string())?;
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut args = options::parse(argv, std::env::var_os("LESS"));
    for diagnostic in &args.diagnostics {
        diagnostic.write(&mut io::stderr().lock()).map_err(|error| error.to_string())?;
    }
    let help_requested = matches!(args.action, options::Action::Help);
    match args.action {
        options::Action::Help if !io::stdout().is_terminal() => { io::stdout().lock().write_all(include_bytes!("less-help.txt")).map_err(|error| error.to_string())?; return Ok(ExitAction::Success); }
        options::Action::Help => {}
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

    let mut tag_address = None;
    if let Some(tag) = &args.tag {
        if args.stdin_explicit || args.filenames.iter().any(|filename| filename != "-") {
            eprintln!("No filenames allowed with -t option");
            return Ok(ExitAction::FileError);
        }
        let selected = tags::select(args.tags_file.as_deref().unwrap_or(std::ffi::OsStr::new("tags")), tag)
            .map_err(|error| error.to_string())?;
        args.filenames = vec![selected.filename];
        tag_address = Some(selected.address);
    }
    let is_stdin = args.filenames.first().is_some_and(|name| name == "-");

    if !help_requested && is_stdin && atty_stdin() && !args.stdin_explicit {
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
    let suspension = if security.allows(security::Feature::Stop) { terminal::SuspendPolicy::Allow } else { terminal::SuspendPolicy::Ignore };
    let mut session = terminal::Session::start(terminal::Screen::Primary, suspension).map_err(|error| error.to_string())?;
    let preprocessing = if !help_requested && args.use_lessopen && security.allows(security::Feature::Lessopen) { preprocessor::Template::from_environment() } else { Ok(None) };
    let preprocessing_diagnostic = preprocessing.as_ref().err().filter(|diagnostic| diagnostic.applies_to(&args.filenames[0]));
    if let Some(diagnostic) = preprocessing_diagnostic { write!(io::stderr().lock(), "{diagnostic}\r\n").map_err(|error| error.to_string())?; }
    let preprocessor = preprocessing.as_ref().ok().and_then(Option::as_ref);
    let mut opened = if help_requested {
        OpenedInput { buffer:buffer::LineBuffer::from_file(File::open("/dev/null").map_err(|error| error.to_string())?), name:None, binary_name:None, alternate:None }
    } else {
        open_input(&args.filenames[0], charset, args.control_display, args.force, preprocessor, &mut session)
            .map_err(|error| format!("{}: {error}", args.filenames[0].to_string_lossy()))?
    };
    if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return finish_opened(Ok(ExitAction::Success), opened, session); }
    if let Some(name) = opened.binary_name.take() {
        match confirm_binary(&name, charset, session.keys_mut(), args.bell_policy) {
            Ok(BinaryAnswer::Refuse) => return finish_opened(Ok(ExitAction::FileError), opened, session),
            Ok(BinaryAnswer::Quit) => return finish_opened(Ok(ExitAction::Success), opened, session),
            Err(error) => return finish_opened(Err(error), opened, session),
            Ok(BinaryAnswer::Open) => {}
        }
    }

    if security.has_diagnostics() || preprocessing_diagnostic.is_some() || args.diagnostics.iter().any(options::Diagnostic::needs_acknowledgment) {
        match acknowledge_diagnostics(session.keys_mut(), args.bell_policy) {
            Ok(DiagnosticAnswer::Continue) => {},
            Ok(DiagnosticAnswer::Interrupted) => {
                if args.interrupt_policy == pager::InterruptPolicy::Quit { return finish_opened(Ok(ExitAction::Interrupted), opened, session); }
            }
            Ok(DiagnosticAnswer::Terminated) => return finish_opened(Ok(ExitAction::Success), opened, session),
            Err(error) => return finish_opened(Err(error), opened, session),
        }
    }
    let mut startup_one_screen = false;
    if !help_requested && args.quit_if_one_screen && args.filenames.len() == 1 && tag_address.is_none() {
        loop {
            let (width, height) = terminal::get_terminal_size();
            let options = pager::OneScreenOptions { width, height, charset, controls:args.control_display, chop:args.chop, line_numbers:args.line_numbers, line_number_width:args.line_number_width, tab_stops:args.tab_stops.clone() };
            match pager::one_screen(&mut opened.buffer, options) {
                Ok(pager::OneScreen::Fits(output)) => {
                    if !args.startup_commands.is_empty() || args.every_file_command.is_some() {
                        startup_one_screen = true;
                        break;
                    }
                    if args.terminal.screen == terminal::ScreenMode::Preserve
                        && let Err(error) = session.enter_interactive(args.terminal) { return finish_opened(Err(error), opened, session); }
                    let result = output.write_to(&mut io::stdout().lock()).map(|()| ExitAction::Success);
                    return finish_opened(result, opened, session);
                }
                Ok(pager::OneScreen::Overflows) => break,
                Ok(pager::OneScreen::Waiting) => {
                    if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return finish_opened(Ok(ExitAction::Success), opened, session); }
                    if terminal::take_interrupt() {
                        let mut output = io::stdout().lock();
                        let result = args.bell_policy.write_to(&mut output, terminal::BellKind::General).and_then(|()| {
                            if args.interrupt_policy == pager::InterruptPolicy::Quit { output.write_all(b"\r\r\x1b[K")?; }
                            output.flush()
                        });
                        if let Err(error) = result { return finish_opened(Err(error), opened, session); }
                        drop(output);
                        if args.interrupt_policy == pager::InterruptPolicy::Quit { return finish_opened(Ok(ExitAction::Interrupted), opened, session); }
                        if let Err(error) = opened.buffer.finish_interrupted_stream() { return finish_opened(Err(error), opened, session); }
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    opened.buffer.read_more(1);
                }
                Err(error) => return finish_opened(Err(error), opened, session),
            }
        }
    }
    if (!startup_one_screen || args.terminal.screen == terminal::ScreenMode::Preserve)
        && let Err(error) = session.enter_interactive(args.terminal) { return finish_opened(Err(error), opened, session); }
    if let Some(address) = tag_address {
        match address.locate(&mut opened.buffer) {
            Ok(line) => args.startup_commands.insert(0, format!("{line}g").into_bytes()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return finish_opened(Ok(ExitAction::Success), opened, session),
            Err(error) => return finish_opened(Err(error), opened, session),
        }
    }
    let history_file = if security.allows(security::Feature::History) { history::HistoryFile::from_environment() } else { None };
    let mut history = history_file.as_ref().map(history::HistoryFile::load).unwrap_or_default();
    history.set_duplicate_policy(args.history_duplicates);

    let mut file_index = 0;
    let mut initial_display = pager::InitialDisplay::Startup;
    let mut display_anchor = pager::DisplayAnchor::new(if startup_one_screen { terminal::ScreenMode::Preserve } else { args.terminal.screen });
    let mut scroll_defaults = pager::ScrollDefaults::new(terminal::get_terminal_size().1);
    if let Some(window) = args.window { scroll_defaults.set_window(window); }
    if let Some(shift) = args.horizontal_shift { scroll_defaults.set_horizontal_shift(shift); }
    let mut positions = vec![None; args.filenames.len()];
    let result = loop {
        let files = match pager::FileStatus::new(&args.filenames, file_index, charset) { Ok(files) => files, Err(error) => break close_opened(Err(error), opened, &mut session) };
        let mut startup_commands = std::mem::take(&mut args.startup_commands);
        if let Some(command) = &args.every_file_command { startup_commands.push(command.clone()); }
        let alternate = opened.alternate.take();
        let mut pager = pager::Pager::new(opened.buffer, pager::Config { files, initial_display, display_anchor, scroll_defaults, startup_commands, interrupt_policy: args.interrupt_policy, eof_policy: args.eof_policy, bell_policy: args.bell_policy, charset, controls: args.control_display, chop: args.chop, line_numbers: args.line_numbers, line_number_width: args.line_number_width, tab_stops: args.tab_stops.clone(), show_eof_markers: args.show_eof_markers, search_case: args.search_case, search_start: args.search_start }, opened.name, history);
        let result = if help_requested { pager.run_help(session.keys_mut()) }
            else if std::mem::take(&mut startup_one_screen) { pager.run_one_screen_startup(session.keys_mut()) }
            else { pager.run(session.keys_mut()) };
        let result = if pager.follow_screen_overflowed() && args.terminal.screen == terminal::ScreenMode::Alternate {
            with_cleanup(result, session.defer_deinitialization(args.terminal))
        } else { result };
        positions[file_index] = pager.saved_position();
        let squished = pager.is_squished();
        display_anchor = pager.display_anchor();
        scroll_defaults = pager.scroll_defaults();
        args.startup_commands = pager.take_startup_commands();
        history = pager.into_history();
        let visit = match result {
            Err(error) => break with_cleanup(Err(error), close_alternate(alternate, &mut session, CloseContext::Exiting)),
            Ok(pager::PagerExit::Quit) => break close_alternate(alternate, &mut session, CloseContext::Exiting).map(|()| ExitAction::Success),
            Ok(pager::PagerExit::Interrupted) => break close_alternate(alternate, &mut session, CloseContext::Exiting).map(|()| ExitAction::Interrupted),
            Ok(pager::PagerExit::VisitFile(visit)) => visit,
        };
        file_index = visit.index();
        opened = match open_input(&args.filenames[file_index], charset, args.control_display, args.force, preprocessor, &mut session) {
            Ok(input) => input, Err(error) => break with_cleanup(Err(error), close_alternate(alternate, &mut session, CloseContext::Exiting)),
        };
        if let Some(name) = opened.binary_name.take() {
            let answer = match confirm_binary(&name, charset, session.keys_mut(), args.bell_policy) {
                Ok(BinaryAnswer::Refuse) => Some(Ok(ExitAction::FileError)),
                Ok(BinaryAnswer::Quit) => Some(Ok(ExitAction::Success)),
                Ok(BinaryAnswer::Open) => None,
                Err(error) => Some(Err(error)),
            };
            if let Some(result) = answer {
                let result = close_opened(result, opened, &mut session);
                break with_cleanup(result, close_alternate(alternate, &mut session, CloseContext::Exiting));
            }
        }
        if let Err(error) = close_alternate(alternate, &mut session, CloseContext::Switching { squished, interrupt_policy:args.interrupt_policy, bell_policy:args.bell_policy }) {
            let result = if error.kind() == io::ErrorKind::Interrupted && args.interrupt_policy == pager::InterruptPolicy::Quit { Ok(ExitAction::Interrupted) } else { Err(error) };
            break close_opened(result, opened, &mut session);
        }
        initial_display = visit.initial_display(positions[file_index]);
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
        Ok(ExitAction::Interrupted) => process::exit(2),
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
