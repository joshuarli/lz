mod ansi;
mod buffer;
mod history;
mod input;
mod pager;
mod search;
mod terminal;

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
            s if s.starts_with('-') => {
                eprintln!("lz: unknown option: {}", s);
                process::exit(2);
            }
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
    /           Search             n/N         Next/prev match
    F           Enter follow mode  q/Esc       Quit

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
            prepopulate_buffer(&mut lb, &prefix);
            (lb, None)
        } else {
            let lb = buffer::LineBuffer::from_stdin(stdin);
            (lb, None)
        }
    } else {
        let filename = args.filename.as_ref().unwrap();
        let mut file = File::open(filename).map_err(|e| format!("{}: {}", filename, e))?;

        // Binary check
        if !args.force {
            if buffer::LineBuffer::check_binary(&mut file).map_err(|e| e.to_string())? {
                return Err("Binary file detected. Use --force to view.".to_string());
            }
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

/// Prepopulate a LineBuffer with text that was already read for binary detection.
fn prepopulate_buffer(lb: &mut buffer::LineBuffer, text: &str) {
    if text.is_empty() {
        return;
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    // If text ends with \n, split produces trailing empty — remove it
    if text.ends_with('\n') {
        lines.pop();
    }
    for line in lines {
        let l = line.strip_suffix('\r').unwrap_or(line);
        lb.push_line(l.to_string());
    }
}

fn atty_stdin() -> bool {
    extern "C" {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{parse_keys_from_bytes, Key};
    use crate::pager::LineEditor;

    // =====================================================================
    // Key parsing from raw bytes
    // =====================================================================

    #[test]
    fn parse_ascii_chars() {
        let keys = parse_keys_from_bytes(b"abc");
        assert_eq!(keys, vec![Key::Char('a'), Key::Char('b'), Key::Char('c')]);
    }

    #[test]
    fn parse_enter() {
        let keys = parse_keys_from_bytes(b"\r");
        assert_eq!(keys, vec![Key::Enter]);

        let keys = parse_keys_from_bytes(b"\n");
        assert_eq!(keys, vec![Key::Enter]);
    }

    #[test]
    fn parse_backspace() {
        let keys = parse_keys_from_bytes(b"\x7f");
        assert_eq!(keys, vec![Key::Backspace]);

        let keys = parse_keys_from_bytes(b"\x08");
        assert_eq!(keys, vec![Key::Backspace]);
    }

    #[test]
    fn parse_ctrl_keys() {
        // Ctrl-A = 0x01, Ctrl-C = 0x03, Ctrl-D = 0x04
        let keys = parse_keys_from_bytes(b"\x01");
        assert_eq!(keys, vec![Key::Ctrl('a')]);

        let keys = parse_keys_from_bytes(b"\x03");
        assert_eq!(keys, vec![Key::Ctrl('c')]);

        let keys = parse_keys_from_bytes(b"\x04");
        assert_eq!(keys, vec![Key::Ctrl('d')]);
    }

    #[test]
    fn parse_arrow_keys() {
        let keys = parse_keys_from_bytes(b"\x1b[A");
        assert_eq!(keys, vec![Key::Up]);

        let keys = parse_keys_from_bytes(b"\x1b[B");
        assert_eq!(keys, vec![Key::Down]);

        let keys = parse_keys_from_bytes(b"\x1b[C");
        assert_eq!(keys, vec![Key::Right]);

        let keys = parse_keys_from_bytes(b"\x1b[D");
        assert_eq!(keys, vec![Key::Left]);
    }

    #[test]
    fn parse_home_end() {
        let keys = parse_keys_from_bytes(b"\x1b[H");
        assert_eq!(keys, vec![Key::Home]);

        let keys = parse_keys_from_bytes(b"\x1b[F");
        assert_eq!(keys, vec![Key::End]);
    }

    #[test]
    fn parse_page_up_down() {
        let keys = parse_keys_from_bytes(b"\x1b[5~");
        assert_eq!(keys, vec![Key::PageUp]);

        let keys = parse_keys_from_bytes(b"\x1b[6~");
        assert_eq!(keys, vec![Key::PageDown]);
    }

    #[test]
    fn parse_delete() {
        let keys = parse_keys_from_bytes(b"\x1b[3~");
        assert_eq!(keys, vec![Key::Delete]);
    }

    #[test]
    fn parse_ss3_arrows() {
        let keys = parse_keys_from_bytes(b"\x1bOA");
        assert_eq!(keys, vec![Key::Up]);

        let keys = parse_keys_from_bytes(b"\x1bOB");
        assert_eq!(keys, vec![Key::Down]);
    }

    #[test]
    fn parse_multiple_keys_in_sequence() {
        let keys = parse_keys_from_bytes(b"q\x1b[A\x1b[Bj");
        assert_eq!(keys, vec![Key::Char('q'), Key::Up, Key::Down, Key::Char('j')]);
    }

    #[test]
    fn parse_home_end_numeric_variants() {
        // ESC [ 1 ~ = Home, ESC [ 4 ~ = End
        let keys = parse_keys_from_bytes(b"\x1b[1~");
        assert_eq!(keys, vec![Key::Home]);

        let keys = parse_keys_from_bytes(b"\x1b[4~");
        assert_eq!(keys, vec![Key::End]);

        // ESC [ 7 ~ = Home, ESC [ 8 ~ = End (rxvt style)
        let keys = parse_keys_from_bytes(b"\x1b[7~");
        assert_eq!(keys, vec![Key::Home]);

        let keys = parse_keys_from_bytes(b"\x1b[8~");
        assert_eq!(keys, vec![Key::End]);
    }

    #[test]
    fn parse_modified_arrows_consumed() {
        // ESC [ 1 ; 5 A = Ctrl+Up — should be consumed as Unknown
        let keys = parse_keys_from_bytes(b"\x1b[1;5A");
        assert_eq!(keys, vec![Key::Unknown]);
    }

    #[test]
    fn parse_utf8_char() {
        // é = 0xc3 0xa9
        let keys = parse_keys_from_bytes(&[0xc3, 0xa9]);
        assert_eq!(keys, vec![Key::Char('é')]);

        // 日 = 0xe6 0x97 0xa5
        let keys = parse_keys_from_bytes(&[0xe6, 0x97, 0xa5]);
        assert_eq!(keys, vec![Key::Char('日')]);
    }

    // =====================================================================
    // LineEditor
    // =====================================================================

    #[test]
    fn editor_new_is_empty() {
        let ed = LineEditor::new();
        assert_eq!(ed.content, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_insert_ascii() {
        let mut ed = LineEditor::new();
        ed.insert('h');
        ed.insert('i');
        assert_eq!(ed.content, "hi");
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_insert_utf8() {
        let mut ed = LineEditor::new();
        ed.insert('é');
        assert_eq!(ed.content, "é");
        assert_eq!(ed.cursor, 2); // é is 2 bytes in UTF-8
    }

    #[test]
    fn editor_backspace() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        ed.backspace();
        assert_eq!(ed.content, "ab");
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_backspace_utf8() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('é');
        ed.backspace();
        assert_eq!(ed.content, "a");
        assert_eq!(ed.cursor, 1);
    }

    #[test]
    fn editor_backspace_at_start() {
        let mut ed = LineEditor::new();
        ed.backspace(); // should be a no-op
        assert_eq!(ed.content, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_delete() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        ed.move_home();
        ed.delete();
        assert_eq!(ed.content, "bc");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_delete_at_end() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.delete(); // no-op, cursor at end
        assert_eq!(ed.content, "a");
    }

    #[test]
    fn editor_move_left_right() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        assert_eq!(ed.cursor, 3);

        ed.move_left();
        assert_eq!(ed.cursor, 2);
        ed.move_left();
        assert_eq!(ed.cursor, 1);
        ed.move_right();
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_move_left_at_start() {
        let mut ed = LineEditor::new();
        ed.move_left();
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_move_right_at_end() {
        let mut ed = LineEditor::new();
        ed.insert('x');
        ed.move_right();
        assert_eq!(ed.cursor, 1); // stays at end
    }

    #[test]
    fn editor_home_end() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');

        ed.move_home();
        assert_eq!(ed.cursor, 0);

        ed.move_end();
        assert_eq!(ed.cursor, 3);
    }

    #[test]
    fn editor_clear() {
        let mut ed = LineEditor::new();
        ed.insert('x');
        ed.insert('y');
        ed.clear();
        assert_eq!(ed.content, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn editor_insert_in_middle() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('c');
        ed.move_left();
        ed.insert('b');
        assert_eq!(ed.content, "abc");
        assert_eq!(ed.cursor, 2);
    }

    #[test]
    fn editor_backspace_in_middle() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('b');
        ed.insert('c');
        ed.move_left(); // cursor after 'b'
        ed.backspace();
        assert_eq!(ed.content, "ac");
        assert_eq!(ed.cursor, 1);
    }

    #[test]
    fn editor_delete_utf8_in_middle() {
        let mut ed = LineEditor::new();
        ed.insert('a');
        ed.insert('é');
        ed.insert('b');
        ed.move_home();
        ed.move_right(); // after 'a', before 'é'
        ed.delete();
        assert_eq!(ed.content, "ab");
    }

    // =====================================================================
    // prepopulate_buffer
    // =====================================================================

    #[test]
    fn prepopulate_simple() {
        let mut lb = buffer::LineBuffer::from_lines(Vec::new());
        prepopulate_buffer(&mut lb, "line1\nline2\nline3\n");
        assert_eq!(lb.lines, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn prepopulate_no_trailing_newline() {
        let mut lb = buffer::LineBuffer::from_lines(Vec::new());
        prepopulate_buffer(&mut lb, "line1\nline2");
        assert_eq!(lb.lines, vec!["line1", "line2"]);
    }

    #[test]
    fn prepopulate_crlf() {
        let mut lb = buffer::LineBuffer::from_lines(Vec::new());
        prepopulate_buffer(&mut lb, "line1\r\nline2\r\n");
        assert_eq!(lb.lines, vec!["line1", "line2"]);
    }

    #[test]
    fn prepopulate_single_line() {
        let mut lb = buffer::LineBuffer::from_lines(Vec::new());
        prepopulate_buffer(&mut lb, "just one line\n");
        assert_eq!(lb.lines, vec!["just one line"]);
    }

    #[test]
    fn prepopulate_empty() {
        let mut lb = buffer::LineBuffer::from_lines(Vec::new());
        prepopulate_buffer(&mut lb, "");
        assert!(lb.lines.is_empty());
    }
}
