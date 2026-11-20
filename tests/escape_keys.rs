use std::io::Write;
use std::time::Duration;
use ptytest::{CommandSpec, Deadline, ExitStatus, PtyTest, Scenario, Size};

#[test]
fn escape_and_unhandled_chords_keep_paging() {
    for bytes in [&b"\x1b"[..], b"\x1bv", b"\x1b ", b"\x1bn", b"\x1bN", b"\x1b}", b"\x1bm", b"\x1b\x06"] {
        let mut input = tempfile::NamedTempFile::new().unwrap();
        for i in 0..100 { writeln!(input, "line {i}").unwrap(); }
        let scenario = Scenario::new("escape-keys").unwrap()
            .command(CommandSpec::new(env!("CARGO_BIN_EXE_lz")).arg(input.path()))
            .size(Size::new(80, 24).unwrap());
        let mut terminal = PtyTest::spawn(scenario).unwrap();
        let deadline = || Deadline::after(Duration::from_secs(3));
        terminal.wait_for_screen(deadline(), "initial line", |screen| screen.row(0).is_some_and(|row| row.contains("line 0"))).unwrap();
        terminal.send_bytes(deadline(), bytes).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        terminal.send_bytes(deadline(), b"j").unwrap();
        terminal.wait_for_screen(deadline(), "next line", |screen| screen.row(0).is_some_and(|row| row.contains("line 1"))).unwrap();
        terminal.send_bytes(deadline(), b"q").unwrap();
        assert_eq!(terminal.wait_for_exit(deadline()).unwrap(), ExitStatus::Code(0));
    }
}

#[test]
fn unknown_escape_chord_rings_in_help_and_keeps_it_open() {
    let mut input = tempfile::NamedTempFile::new().unwrap();
    for i in 0..100 { writeln!(input, "file content {i}").unwrap(); }
    let scenario = Scenario::new("help-escape-chord").unwrap()
        .command(CommandSpec::new(env!("CARGO_BIN_EXE_lz")).arg(input.path()))
        .size(Size::new(80, 24).unwrap());
    let mut terminal = PtyTest::spawn(scenario).unwrap();
    let deadline = || Deadline::after(Duration::from_secs(3));
    terminal.wait_for_screen(deadline(), "file", |screen| screen.row(0).is_some_and(|row| row.contains("file content"))).unwrap();
    terminal.send_bytes(deadline(), b"h").unwrap();
    terminal.wait_for_screen(deadline(), "help", |screen| screen.row(1).is_some_and(|row| row.contains("SUMMARY OF LESS COMMANDS"))).unwrap();
    terminal.send_bytes(deadline(), b"\x1bq").unwrap();
    std::thread::sleep(Duration::from_millis(150));
    terminal.send_bytes(deadline(), b"q").unwrap();
    terminal.wait_for_screen(deadline(), "returned to file", |screen| screen.row(0).is_some_and(|row| row.contains("file content"))).unwrap();
    assert!(terminal.raw_output().contains(&7));
    terminal.send_bytes(deadline(), b"q").unwrap();
    assert_eq!(terminal.wait_for_exit(deadline()).unwrap(), ExitStatus::Code(0));
}
