mod options;

use lz::{buffer, history, pager, terminal};

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

enum ExitAction { Success, Terminated, Hangup }

fn run() -> Result<ExitAction, String> {
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

    let is_stdin = args.filenames.first().is_some_and(|name| name == "-");

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

    if args.filenames.len() > 1 {
        eprintln!("lz: too many arguments");
        process::exit(2);
    }

    if is_stdin && atty_stdin() && !args.stdin_explicit {
        eprintln!("lz: missing filename (use '-' for stdin)");
        process::exit(2);
    }

    let (buf, display_name) = if is_stdin {
        (buffer::LineBuffer::from_stdin(io::stdin()).map_err(|error| error.to_string())?, None)
    } else {
        let filename = &args.filenames[0];
        let mut file = File::open(filename).map_err(|e| format!("{}: {}", filename.to_string_lossy(), e))?;

        // Binary check
        if !args.force && buffer::LineBuffer::check_binary(&mut file).map_err(|e| e.to_string())? {
            return Err("Binary file detected. Use --force to view.".to_string());
        }

        // Empty check
        {
            use std::io::Seek;
            let size = file.seek(io::SeekFrom::End(0)).unwrap_or(0);
            file.seek(io::SeekFrom::Start(0)).ok();
            if size == 0 {
                return Ok(ExitAction::Success); // Empty file, exit 0
            }
        }

        let lb = buffer::LineBuffer::from_file(file);
        (lb, Some(filename.to_string_lossy().into_owned()))
    };

    let mut session = terminal::Session::start().map_err(|error| error.to_string())?;

    // Load search history
    let history_path = if std::env::var_os("LESSHISTFILE").is_some_and(|value| value == "-") { None } else { Some({
        let state_dir = std::env::var("XDG_STATE_HOME").ok().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| String::from("~"));
            format!("{}/.local/state", home)
        });
        std::path::PathBuf::from(state_dir).join("lz/history")
    }) };
    let history = history_path.as_deref().map(history::SearchHistory::load).unwrap_or_default();

    // Run pager
    let mut pager = pager::Pager::new(buf, pager::Config { startup_commands: args.startup_commands, raw_mode: args.raw_mode, chop: args.chop, line_numbers: args.line_numbers, search_case: args.search_case }, display_name, history);
    let result = pager.run(session.keys_mut());

    if let Some(path) = history_path { pager.history().save(&path); }
    let cleanup = session.finish();
    match (result, cleanup) {
        (Ok(()), Ok(())) => match terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) {
            0 => Ok(ExitAction::Success),
            15 => Ok(ExitAction::Terminated),
            1 => Ok(ExitAction::Hangup),
            signal => Err(format!("unexpected termination signal: {signal}")),
        },
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error.to_string()),
        (Err(error), Err(cleanup)) => Err(format!("{error}; terminal restoration: {cleanup}")),
    }
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
