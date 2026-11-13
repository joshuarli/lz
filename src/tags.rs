use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::num::NonZeroUsize;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

pub(crate) struct Selection { pub(crate) filename: OsString, pub(crate) address: Address }

pub(crate) enum Address { Line(NonZeroUsize), Pattern(Pattern) }

pub(crate) struct Pattern { prefix: Vec<u8>, end: PatternEnd }

enum PatternEnd { Prefix, Line }

impl Address {
    pub(crate) fn locate(&self, source: &mut lz::buffer::LineBuffer) -> io::Result<NonZeroUsize> {
        let pattern = match self { Self::Line(line) => return Ok(*line), Self::Pattern(pattern) => pattern };
        let mut index = 0usize;
        loop {
            if lz::terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Tag search interrupted"));
            }
            let Some(fragment) = source.raw_byte_prefix(index, pattern.prefix.len() + 1)? else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "Tag not found"));
            };
            if fragment.bytes.starts_with(&pattern.prefix) && (matches!(pattern.end, PatternEnd::Prefix)
                || matches!(fragment.bytes.get(pattern.prefix.len()), Some(0 | b'\r'))
                || (fragment.complete && fragment.bytes.len() == pattern.prefix.len())) {
                return index.checked_add(1).and_then(NonZeroUsize::new).ok_or_else(|| io::Error::other("Tag line number is too large"));
            }
            index = index.checked_add(1).ok_or_else(|| io::Error::other("Tag line number is too large"))?;
        }
    }
}

fn address(bytes: &[u8]) -> io::Result<Address> {
    if bytes.first().is_some_and(u8::is_ascii_digit) {
        let mut value = 0usize;
        for byte in bytes.iter().take_while(|byte| byte.is_ascii_digit()) {
            value = value.checked_mul(10).and_then(|value| value.checked_add((byte - b'0') as usize))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Tag line number is too large"))?;
        }
        return NonZeroUsize::new(value).map(Address::Line)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "Unsupported tag address"));
    }
    let delimiter = bytes[0];
    let mut index = 1 + usize::from(bytes.get(1) == Some(&b'^'));
    let mut prefix = Vec::new();
    while let Some(&byte) = bytes.get(index) {
        if byte == delimiter { break; }
        if byte == b'\\' {
            index += 1;
            let Some(&escaped) = bytes.get(index) else { break; };
            prefix.push(escaped);
        } else { prefix.push(byte); }
        index += 1;
    }
    let end = if prefix.last() == Some(&b'$') { prefix.pop(); PatternEnd::Line } else { PatternEnd::Prefix };
    Ok(Address::Pattern(Pattern { prefix, end }))
}

fn skip_spaces(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) { bytes = &bytes[1..]; }
    bytes
}

pub(crate) fn select(path: &OsStr, name: &OsStr) -> io::Result<Selection> {
    let mut input = BufReader::new(File::open(path)?);
    let mut record = Vec::new();
    loop {
        record.clear();
        // less 668 reads tag records with a 1024-byte fgets buffer, including its NUL terminator.
        if (&mut input).take(1023).read_until(b'\n', &mut record)? == 0 { break; }
        if let Some(end) = record.iter().position(|byte| *byte == 0) { record.truncate(end); }
        if record.first() == Some(&b'!') { continue; }
        let Some(rest) = record.strip_prefix(name.as_bytes()) else { continue; };
        if !rest.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) { continue; }
        let rest = skip_spaces(rest);
        let Some(end) = rest.iter().position(|byte| matches!(byte, b' ' | b'\t')) else { continue; };
        let filename = &rest[..end];
        let location = skip_spaces(&rest[end..]);
        if location.is_empty() { continue; }
        return Ok(Selection { filename: OsString::from_vec(filename.to_vec()), address: address(location)? });
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "No such tag in tags file"))
}
