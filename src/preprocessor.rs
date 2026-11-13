use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::Command;

pub(super) struct Template { bytes: Vec<u8> }

#[derive(Debug)]
pub(super) struct InvalidTemplate;
impl std::fmt::Display for InvalidTemplate {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str("LESSOPEN ignored: must contain exactly one %s")
    }
}
impl std::error::Error for InvalidTemplate {}

impl Template {
    pub fn from_environment() -> Result<Option<Self>, InvalidTemplate> {
        let Some(value) = std::env::var_os("LESSOPEN") else { return Ok(None); };
        let mut bytes = value.as_bytes();
        if bytes.is_empty() || bytes[0] == b'|' { return Ok(None); }
        if bytes[0] == b'-' { bytes = &bytes[1..]; }
        let mut placeholders = 0;
        let mut cursor = 0;
        while cursor < bytes.len() {
            if bytes[cursor] == b'%' {
                cursor += 1;
                match bytes.get(cursor) {
                    Some(b's') => placeholders += 1,
                    Some(b'%') => {},
                    _ => return Err(InvalidTemplate),
                }
            }
            cursor += 1;
        }
        if placeholders != 1 { return Err(InvalidTemplate); }
        Ok(Some(Self { bytes:bytes.to_vec() }))
    }

    pub fn run(&self, filename: &OsStr, session: &mut lz::terminal::Session) -> io::Result<Option<OsString>> {
        if filename == "-" { return Ok(None); }
        let quoted = quote(filename.as_bytes());
        let mut command = Vec::new();
        let mut cursor = 0;
        while cursor < self.bytes.len() {
            if self.bytes[cursor] == b'%' {
                cursor += 1;
                if self.bytes[cursor] == b's' { command.extend_from_slice(&quoted); }
                else { command.push(b'%'); }
            } else { command.push(self.bytes[cursor]); }
            cursor += 1;
        }
        let shell = std::env::var_os("SHELL").filter(|shell| !shell.is_empty());
        if let Some(shell) = shell {
            command = [shell.as_bytes(), b" -c ", &quote(&command)].concat();
        }
        let mut process = Command::new("/bin/sh");
        process.arg("-c").arg(OsString::from_vec(command));
        let line = match session.read_helper_line(process) {
            Ok(line) => line,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(None),
            Err(error) => return Err(error),
        };
        let line = line.split(|byte| *byte == 0).next().unwrap_or_default();
        Ok((!line.is_empty()).then(|| OsString::from_vec(line.to_vec())))
    }
}

fn quote(bytes: &[u8]) -> Vec<u8> {
    let mut quoted = vec![b'\''];
    for byte in bytes {
        if *byte == b'\'' { quoted.extend_from_slice(b"'\\''"); }
        else { quoted.push(*byte); }
    }
    quoted.push(b'\'');
    quoted
}
