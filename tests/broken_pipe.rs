use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

#[test]
fn closed_consumer_exits_quietly_with_sigpipe() {
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input.write_all(&vec![b'x'; 1024 * 1024]).unwrap();
    for from_stdin in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lz"));
        command.env_remove("LESS").stdout(Stdio::piped()).stderr(Stdio::piped());
        if from_stdin { command.stdin(Stdio::from(input.reopen().unwrap())); }
        else { command.arg(input.path()); }
        let mut child = command.spawn().unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.signal(), Some(libc::SIGPIPE), "stdin={from_stdin}, status={:?}, stderr={:?}", output.status, String::from_utf8_lossy(&output.stderr));
        assert!(output.stderr.is_empty(), "{:?}", String::from_utf8_lossy(&output.stderr));
    }
}
