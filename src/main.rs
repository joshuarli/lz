use lz::{buffer, history, pager, terminal};
use lz::search::CaseSensitivity;

use std::fs::File;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::io::{self, IsTerminal, Read, Write};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Args {
    follow: bool,
    force: bool,
    raw_mode: bool,
    search_case: CaseSensitivity,
    filenames: Vec<OsString>,
    stdin_explicit: bool,
    help: bool,
    version: bool,
}

fn case_option(bytes: &[u8]) -> Option<CaseSensitivity> {
    match bytes {
        b"--ignore-case" => Some(CaseSensitivity::Smart),
        b"--IGNORE-CASE" => Some(CaseSensitivity::Insensitive),
        b"-+i" | b"-+I" => Some(CaseSensitivity::Sensitive),
        _ => {
            let flags = bytes.strip_prefix(b"-")?;
            if flags.is_empty() || flags.iter().any(|byte| !matches!(byte, b'i' | b'I')) { return None; }
            Some(if flags.last() == Some(&b'i') { CaseSensitivity::Smart } else { CaseSensitivity::Insensitive })
        }
    }
}

fn parse_args() -> Args {
    let mut args = Args {
        follow: false,
        force: false,
        raw_mode: false,
        search_case: CaseSensitivity::default(),
        filenames: Vec::new(),
        stdin_explicit: false,
        help: false,
        version: false,
    };

    if let Some(defaults) = std::env::var_os("LESS") {
        for token in defaults.as_bytes().split(|byte| byte.is_ascii_whitespace()).filter(|token| !token.is_empty()) {
            let flag = if token.starts_with(b"-") { token.to_vec() } else { [b"-", token].concat() };
            if let Some(case) = case_option(&flag) { args.search_case = case; }
        }
    }

    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut end_of_flags = false;

    for arg in &argv {
        if end_of_flags {
            args.filenames.push(arg.clone());
            continue;
        }

        if let Some(case) = case_option(arg.as_bytes()) { args.search_case = case; continue; }

        match arg.as_bytes() {
            b"--" => end_of_flags = true,
            b"-" => { args.stdin_explicit = true; args.filenames.push(arg.clone()); }
            b"--follow" => args.follow = true,
            b"-f" | b"--force" => args.force = true,
            b"-r" | b"--raw" => args.raw_mode = true,
            b"--help" | b"-h" => args.help = true,
            b"--version" | b"-V" => args.version = true,
            value if value.starts_with(b"-") || value.starts_with(b"+") => {}
            _ => args.filenames.push(arg.clone()),
        }
    }
    if args.filenames.is_empty() { args.filenames.push("-".into()); }
    let mut seen = std::collections::BTreeSet::new();
    args.filenames.retain(|filename| {
        let resolved = if filename == "-" { std::path::PathBuf::from(filename) }
            else { std::fs::canonicalize(filename).unwrap_or_else(|_| filename.into()) };
        seen.insert(resolved)
    });

    args
}

fn print_help() {
    println!(
        "lz {} — a minimal pager

USAGE: lz [OPTIONS] [FILE]

OPTIONS:
    --follow    Follow mode (like tail -f)
    -f, --force View binary files
    -i          Ignore ASCII case unless the pattern has uppercase
    -I          Always ignore ASCII case
    -r, --raw   Raw mode (show ANSI escapes literally)
    -h, --help  Show this help
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
    let args = parse_args();

    if args.help {
        print_help();
        return Ok(ExitAction::Success);
    }

    if args.version {
        println!("lz {}", VERSION);
        return Ok(ExitAction::Success);
    }

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
        // Read from stdin
        let stdin = io::stdin();

        // For stdin binary detection, read a chunk first
        if !args.force {
            let mut check_buf = [0u8; 8192];
            let mut handle = stdin.lock();
            let n = handle.read(&mut check_buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Ok(ExitAction::Success); // Empty input, exit 0
            }
            if check_buf[..n].contains(&0) {
                return Err("Binary content detected. Use --force to view.".to_string());
            }
            drop(handle);

            // We consumed some bytes. Create buffer and prepopulate.
            let mut lb = buffer::LineBuffer::from_stdin(io::stdin());
            let prefix = String::from_utf8_lossy(&check_buf[..n]).to_string();
            lb.prepopulate(&prefix);
            (lb, None)
        } else {
            let lb = buffer::LineBuffer::from_stdin(stdin);
            (lb, None)
        }
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
    let mut pager = pager::Pager::new(buf, pager::Config { follow: args.follow, raw_mode: args.raw_mode, search_case: args.search_case }, display_name, history);
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
