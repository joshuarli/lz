use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::num::NonZeroUsize;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

pub(crate) struct Selection { pub(crate) filename: OsString, pub(crate) line: NonZeroUsize }

pub(crate) fn select(path: &OsStr, name: &OsStr) -> io::Result<Selection> {
    let mut input = BufReader::new(File::open(path)?);
    let mut record = Vec::new();
    loop {
        record.clear();
        // less 668 reads tag records with a 1024-byte fgets buffer, including its NUL terminator.
        if (&mut input).take(1023).read_until(b'\n', &mut record)? == 0 { break; }
        if let Some(end) = record.iter().position(|byte| *byte == 0) { record.truncate(end); }
        if record.first() == Some(&b'!') { continue; }
        let mut fields = record.split(|byte| matches!(byte, b' ' | b'\t')).filter(|field| !field.is_empty());
        if fields.next() != Some(name.as_bytes()) { continue; }
        let Some(filename) = fields.next() else { continue; };
        let Some(address) = fields.next() else { continue; };
        let mut value = 0usize;
        let mut digits = 0;
        for byte in address.iter().take_while(|byte| byte.is_ascii_digit()) {
            value = value.checked_mul(10).and_then(|value| value.checked_add((byte - b'0') as usize))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Tag line number is too large"))?;
            digits += 1;
        }
        let line = NonZeroUsize::new(value).filter(|_| digits != 0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "Unsupported tag address"))?;
        return Ok(Selection { filename: OsString::from_vec(filename.to_vec()), line });
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "No such tag in tags file"))
}
