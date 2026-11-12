use lz::{buffer, history, input, pager, terminal};

use std::fs::File;
use std::io::{self, Read};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Args {
    follow: bool,
    force: bool,
    raw_mode: bool,
    filename: Option<String>,
    stdin_explicit: bool,
    help: bool,
    version: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        follow: false,
        force: false,
        raw_mode: false,
        filename: None,
        stdin_explicit: false,
        help: false,
        version: false,
    };

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut end_of_flags = false;

    for arg in &argv {
        if end_of_flags {
            if args.filename.is_some() {
                eprintln!("lz: too many arguments");
                process::exit(2);
            }
            args.filename = Some(arg.clone());
            continue;
        }

        match arg.as_str() {
            "--" => end_of_flags = true,
            "-" => args.stdin_explicit = true,
            "--follow" => args.follow = true,
            "--force" => args.force = true,
            "-r" | "--raw" => args.raw_mode = true,
            "--help" | "-h" => args.help = true,
            "--version" | "-V" => args.version = true,
            // Silently ignore unknown flags and +commands (e.g. less-style
            // options passed by programs that assume PAGER is less).
            s if s.starts_with('-') || s.starts_with('+') => {}
            _ => {
                if args.filename.is_some() {
                    eprintln!("lz: too many arguments");
                    process::exit(2);
                }
                args.filename = Some(arg.clone());
            }
        }
    }

    args
}

fn print_help() {
    println!(
        "lz {} — a minimal pager

USAGE: lz [OPTIONS] [FILE]

OPTIONS:
    --follow    Follow mode (like tail -f)
    --force     View binary files
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

fn run() -> Result<(), String> {
    let args = parse_args();

    if args.help {
        print_help();
        return Ok(());
    }

    if args.version {
        println!("lz {}", VERSION);
        return Ok(());
    }

    let is_stdin = args.filename.is_none() || args.stdin_explicit;

    // If stdin is a tty and no file given, that's an error
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
                return Ok(()); // Empty input, exit 0
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
        let filename = args.filename.as_ref().unwrap();
        let mut file = File::open(filename).map_err(|e| format!("{}: {}", filename, e))?;

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
                return Ok(()); // Empty file, exit 0
            }
        }

        let lb = buffer::LineBuffer::from_file(file);
        (lb, Some(filename.clone()))
    };

    // Open /dev/tty for key input
    let mut keys = input::KeyReader::new().map_err(|e| format!("Cannot open /dev/tty: {}", e))?;

    // Set up terminal
    terminal::install_signal_handlers();
    terminal::enable_raw_mode(keys.fd()).map_err(|e| format!("Cannot enable raw mode: {}", e))?;

    let mut stdout = io::stdout();
    terminal::enter_alt_screen(&mut stdout).ok();
    terminal::hide_cursor(&mut stdout).ok();

    // Load search history
    let history_path = {
        let state_dir = std::env::var("XDG_STATE_HOME").ok().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| String::from("~"));
            format!("{}/.local/state", home)
        });
        std::path::PathBuf::from(state_dir).join("lz/history")
    };
    let history = history::SearchHistory::load(&history_path);

    // Run pager
    let mut pager = pager::Pager::new(buf, args.follow, args.raw_mode, display_name, history);
    let result = pager.run(&mut keys);

    // Save search history before restoring terminal
    pager.history().save(&history_path);

    // Restore terminal (always, even on error)
    terminal::restore_terminal();

    result.map_err(|e| e.to_string())
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
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("lz: {}", e);
            process::exit(1);
        }
    }
}
