use std::fs::File;
use std::io::{self, Read, Stdin};
use std::os::unix::fs::FileExt;
use std::os::fd::{AsFd, AsRawFd};

use crate::storage::{Offsets, anonymous_file};
use crate::charset::Charset;
use crate::ansi::ControlDisplay;

const BLOCK_BYTES: usize = 64 * 1024;
const BINARY_CHECK_SIZE: usize = 256;

enum Source {
    File(File),
    Pipe { input: File, spool: File },
}

impl Source {
    fn file(&self) -> &File {
        match self { Self::File(file) => file, Self::Pipe { spool, .. } => spool }
    }

    fn pull(&mut self, offset: u64, bytes: &mut [u8]) -> io::Result<Option<usize>> {
        match self {
            Self::File(file) => file.read_at(bytes, offset).map(Some),
            Self::Pipe { input, spool } => {
                if !crate::readiness::readable(input.as_raw_fd(), 0)? { return Ok(None); }
                let count = input.read(bytes)?;
                spool.write_all_at(&bytes[..count], offset)?;
                Ok(Some(count))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceState { Ready, Waiting, Eof, Failed }

pub struct ByteFragment {
    pub bytes: Vec<u8>,
    pub complete: bool,
}

pub struct LineFragment {
    pub text: String,
    pub complete: bool,
}

pub struct LineBuffer {
    source: Source,
    starts: Offsets,
    scanned: u64,
    state: SourceState,
    error: Option<io::Error>,
    line: String,
}

impl LineBuffer {
    pub fn from_file(file: File) -> Self {
        Self { source: Source::File(file), starts: Offsets::new(), scanned: 0, state: SourceState::Ready, error: None, line: String::new() }
    }

    pub fn from_stdin(stdin: Stdin) -> io::Result<Self> {
        Self::from_stream(File::from(stdin.as_fd().try_clone_to_owned()?))
    }

    fn from_stream(input: File) -> io::Result<Self> {
        Ok(Self { source: Source::Pipe { input, spool: anonymous_file()? }, starts: Offsets::new(), scanned: 0, state: SourceState::Ready, error: None, line: String::new() })
    }

    pub fn state(&self) -> SourceState { self.state }

    pub(crate) fn refresh_waiting(&mut self) -> bool {
        self.state == SourceState::Waiting && self.scan()
    }

    pub fn take_error(&mut self) -> Option<io::Error> { self.error.take() }

    fn scan(&mut self) -> bool {
        if matches!(self.state, SourceState::Eof | SourceState::Failed) { return false; }
        let result = (|| -> io::Result<Option<usize>> {
            let mut bytes = [0; BLOCK_BYTES];
            let Some(count) = self.source.pull(self.scanned, &mut bytes)? else { return Ok(None); };
            for (index, byte) in bytes[..count].iter().enumerate() {
                if *byte == b'\n' { self.starts.push(self.scanned + index as u64 + 1)?; }
            }
            self.scanned += count as u64;
            Ok(Some(count))
        })();
        match result {
            Ok(Some(0)) => { self.state = SourceState::Eof; false }
            Ok(Some(_)) => { self.state = SourceState::Ready; true }
            Ok(None) => { self.state = SourceState::Waiting; false }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => { self.state = SourceState::Waiting; false }
            Err(error) => { self.error = Some(error); self.state = SourceState::Failed; false }
        }
    }

    fn ensure_line(&mut self, index: usize) -> bool {
        while self.len() <= index {
            if !self.scan() { return false; }
        }
        true
    }

    fn byte_fragment(&mut self, index: usize, limit: Option<usize>) -> io::Result<Option<ByteFragment>> {
        if !self.ensure_line(index) { return Ok(None); }
        let start = self.starts.get(index)?;
        while self.starts.len() <= index + 1 && !self.is_finished() {
            if limit.is_some_and(|limit| self.scanned - start >= limit as u64) || !self.scan() { break; }
        }
        let complete = self.starts.len() > index + 1 || self.is_finished();
        let end = if self.starts.len() > index + 1 { self.starts.get(index + 1)? - 1 } else { self.scanned };
        let length = end - start;
        let length = limit.map_or(length, |limit| length.min(limit as u64));
        let length = usize::try_from(length).map_err(|_| io::Error::other("line exceeds addressable memory"))?;
        let mut bytes = vec![0; length];
        self.source.file().read_exact_at(&mut bytes, start)?;
        let complete = complete && start + length as u64 == end;
        Ok(Some(ByteFragment { bytes, complete }))
    }

    /// Read at most bytes from a line, excluding LF and retaining CR and arbitrary source bytes.
    pub fn raw_byte_prefix(&mut self, index: usize, bytes: usize) -> io::Result<Option<ByteFragment>> {
        self.byte_fragment(index, Some(bytes))
    }

    fn content_fragment(&mut self, index: usize, limit: Option<usize>) -> io::Result<Option<ByteFragment>> {
        let Some(mut fragment) = self.byte_fragment(index, limit)? else { return Ok(None); };
        if fragment.complete && fragment.bytes.last() == Some(&b'\r') && self.starts.len() > index + 1 { fragment.bytes.pop(); }
        Ok(Some(fragment))
    }

    pub(crate) fn byte_prefix(&mut self, index: usize, bytes: usize) -> io::Result<Option<ByteFragment>> {
        self.content_fragment(index, Some(bytes))
    }

    fn fragment(&mut self, index: usize, limit: Option<usize>) -> io::Result<Option<LineFragment>> {
        let Some(ByteFragment { mut bytes, complete }) = self.content_fragment(index, limit)? else { return Ok(None); };
        if !complete && let Err(error) = std::str::from_utf8(&bytes)
            && error.error_len().is_none() { bytes.truncate(error.valid_up_to()); }
        Ok(Some(LineFragment { text: String::from_utf8_lossy(&bytes).into_owned(), complete }))
    }

    /// Read a bounded display prefix without requiring the end of the logical line.
    pub fn line_prefix(&mut self, index: usize, bytes: usize) -> io::Result<Option<LineFragment>> {
        self.fragment(index, Some(bytes))
    }

    /// Materialize a logical line for operations that explicitly need the whole line.
    pub fn get_line(&mut self, index: usize) -> Option<&str> {
        match self.fragment(index, None) {
            Ok(Some(fragment)) => { self.line = fragment.text; Some(&self.line) }
            Ok(None) => None,
            Err(error) => { self.error = Some(error); self.state = SourceState::Failed; None }
        }
    }

    pub fn len(&self) -> usize {
        self.starts.len() - 1 + usize::from(self.scanned > self.starts.last())
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn is_finished(&self) -> bool { self.state == SourceState::Eof }

    /// Finish a waiting stream at its buffered boundary after an interrupt.
    pub fn finish_interrupted_stream(&mut self) -> io::Result<()> {
        if self.state != SourceState::Waiting { return Err(io::Error::new(io::ErrorKind::InvalidInput, "stream is not waiting")); }
        self.state = SourceState::Eof;
        Ok(())
    }

    pub fn read_more(&mut self, count: usize) -> usize {
        let before = self.len();
        while self.len().saturating_sub(before) < count && self.scan() {}
        self.len().saturating_sub(before)
    }

    pub fn read_all(&mut self) { while self.scan() {} }

    pub fn poll_new_lines(&mut self) -> usize {
        if self.state == SourceState::Eof { self.state = SourceState::Ready; }
        let before = self.len();
        self.scan();
        self.len().saturating_sub(before)
    }

    /// Inspect the initial sample without changing the file position.
    pub fn check_binary(file: &File, charset: Charset, controls: ControlDisplay) -> io::Result<bool> {
        let mut bytes = [0; BINARY_CHECK_SIZE];
        let count = file.read_at(&mut bytes, 0)?;
        let sample = &bytes[..count];
        Ok(if controls == ControlDisplay::Color { charset.looks_binary(&crate::ansi::without_sample_escapes(sample, charset)) } else { charset.looks_binary(sample) })
    }

    #[cfg(test)]
    pub fn from_lines(lines: Vec<String>) -> Self {
        let file = anonymous_file().unwrap();
        let mut offset = 0;
        for line in lines { file.write_all_at(line.as_bytes(), offset).unwrap(); offset += line.len() as u64; file.write_all_at(b"\n", offset).unwrap(); offset += 1; }
        let mut buffer = Self::from_file(file);
        buffer.read_all();
        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_file(content: &str) -> (File, tempfile::NamedTempFile) {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        tmp.flush().unwrap();
        let file = File::open(tmp.path()).unwrap();
        (file, tmp)
    }

    // --- LineBuffer from file ---

    #[test]
    fn raw_prefixes_preserve_line_bytes_while_display_normalizes_crlf() {
        let (file, _tmp) = make_temp_file("first\r\nlast\r");
        let mut buffer = LineBuffer::from_file(file);
        let prefix = buffer.raw_byte_prefix(0, 3).unwrap().unwrap();
        assert_eq!(prefix.bytes, b"fir");
        assert!(!prefix.complete);
        let first = buffer.raw_byte_prefix(0, 20).unwrap().unwrap();
        assert_eq!(first.bytes, b"first\r");
        assert!(first.complete);
        assert_eq!(buffer.get_line(0), Some("first"));
        let last = buffer.raw_byte_prefix(1, 20).unwrap().unwrap();
        assert_eq!(last.bytes, b"last\r");
        assert!(last.complete);
        assert_eq!(buffer.get_line(1), Some("last\r"));
    }

    #[test]
    fn open_stream_preserves_bytes_and_distinguishes_waiting_from_eof() {
        use std::os::fd::OwnedFd;
        use std::os::unix::net::UnixStream;
        let (mut producer, consumer) = UnixStream::pair().unwrap();
        let descriptor: OwnedFd = consumer.into();
        let mut buffer = LineBuffer::from_stream(File::from(descriptor)).unwrap();
        assert!(buffer.line_prefix(0, 4096).unwrap().is_none());
        assert_eq!(buffer.state(), SourceState::Waiting);
        producer.write_all(b"part").unwrap();
        let fragment = buffer.line_prefix(0, 4096).unwrap().unwrap();
        assert_eq!(fragment.text, "part");
        assert!(!fragment.complete);
        assert_eq!(buffer.state(), SourceState::Waiting);
        producer.write_all(&[255, b'\n', b't', b'a', b'i', b'l']).unwrap();
        assert!(buffer.refresh_waiting());
        let fragment = buffer.line_prefix(0, 4096).unwrap().unwrap();
        assert!(fragment.complete);
        let mut bytes = [0; 10];
        buffer.source.file().read_exact_at(&mut bytes, 0).unwrap();
        assert_eq!(&bytes, b"part\xff\ntail");
        assert_eq!(buffer.get_line(1), Some("tail"));
        assert_eq!(buffer.state(), SourceState::Waiting);
        drop(producer);
        assert_eq!(buffer.get_line(1), Some("tail"));
        assert_eq!(buffer.state(), SourceState::Eof);
        assert_eq!(buffer.get_line(2), None);
    }

    #[test]
    fn display_prefix_does_not_read_a_sixteen_mib_line() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let block = vec![b'x'; BLOCK_BYTES];
        for _ in 0..256 { tmp.write_all(&block).unwrap(); }
        tmp.write_all(b"suffix\nnext\n").unwrap();
        let mut buffer = LineBuffer::from_file(File::open(tmp.path()).unwrap());
        let fragment = buffer.line_prefix(0, 4096).unwrap().unwrap();
        assert_eq!(fragment.text, "x".repeat(4096));
        assert!(!fragment.complete);
        assert_eq!(buffer.scanned, BLOCK_BYTES as u64);
        assert_eq!(buffer.get_line(1), Some("next"));
        assert!(buffer.get_line(0).unwrap().ends_with("suffix"));
    }

    #[test]
    fn source_bytes_survive_invalid_utf8_and_split_characters() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let mut bytes = "€".repeat(24_000).into_bytes();
        bytes.extend_from_slice(&[255, 0, b'\n']);
        tmp.write_all(&bytes).unwrap();
        let mut buffer = LineBuffer::from_file(File::open(tmp.path()).unwrap());
        let fragment = buffer.line_prefix(0, 4097).unwrap().unwrap();
        assert_eq!(fragment.text, "€".repeat(1365));
        assert!(!fragment.complete);
        buffer.read_all();
        let mut retained = vec![0; bytes.len()];
        buffer.source.file().read_exact_at(&mut retained, 0).unwrap();
        assert_eq!(retained, bytes);
    }

    #[test]
    fn source_read_error_is_distinct_from_eof() {
        let dir = tempfile::tempdir().unwrap();
        let mut buffer = LineBuffer::from_file(File::open(dir.path()).unwrap());
        assert_eq!(buffer.get_line(0), None);
        assert_eq!(buffer.state(), SourceState::Failed);
        assert!(!buffer.is_finished());
        assert!(buffer.take_error().is_some());
    }

    #[test]
    fn read_simple_file() {
        let (file, _tmp) = make_temp_file("line1\nline2\nline3\n");
        let mut buf = LineBuffer::from_file(file);

        assert_eq!(buf.get_line(0), Some("line1"));
        assert_eq!(buf.get_line(1), Some("line2"));
        assert_eq!(buf.get_line(2), Some("line3"));
        assert_eq!(buf.get_line(3), None);
    }

    #[test]
    fn read_no_trailing_newline() {
        let (file, _tmp) = make_temp_file("line1\nline2");
        let mut buf = LineBuffer::from_file(file);

        assert_eq!(buf.get_line(0), Some("line1"));
        assert_eq!(buf.get_line(1), Some("line2"));
        assert_eq!(buf.get_line(2), None);
    }

    #[test]
    fn read_empty_file() {
        let (file, _tmp) = make_temp_file("");
        let mut buf = LineBuffer::from_file(file);

        assert_eq!(buf.get_line(0), None);
        assert!(buf.is_finished());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn read_crlf_lines() {
        let (file, _tmp) = make_temp_file("line1\r\nline2\r\n");
        let mut buf = LineBuffer::from_file(file);

        assert_eq!(buf.get_line(0), Some("line1"));
        assert_eq!(buf.get_line(1), Some("line2"));
    }

    #[test]
    fn read_single_empty_line() {
        let (file, _tmp) = make_temp_file("\n");
        let mut buf = LineBuffer::from_file(file);

        assert_eq!(buf.get_line(0), Some(""));
        assert_eq!(buf.get_line(1), None);
    }

    #[test]
    fn lazy_loading() {
        let (file, _tmp) = make_temp_file("a\nb\nc\nd\ne\n");
        let mut buf = LineBuffer::from_file(file);

        // Before any access, nothing is loaded
        assert_eq!(buf.len(), 0);
        assert!(!buf.is_finished());

        // Accessing line 0 triggers lazy load (reads in chunks of 256)
        assert_eq!(buf.get_line(0), Some("a"));
        assert!(!buf.is_empty());
    }

    #[test]
    fn read_all() {
        let (file, _tmp) = make_temp_file("a\nb\nc\n");
        let mut buf = LineBuffer::from_file(file);

        buf.read_all();
        assert!(buf.is_finished());
        assert_eq!(buf.len(), 3);
    }

    // --- binary detection ---

    #[test]
    fn binary_detection_positive() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05]).unwrap();
        tmp.flush().unwrap();
        let file = File::open(tmp.path()).unwrap();

        assert!(LineBuffer::check_binary(&file, Charset::Ascii, ControlDisplay::Standard).unwrap());
    }

    #[test]
    fn binary_detection_negative() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"just text\n").unwrap();
        tmp.flush().unwrap();
        let file = File::open(tmp.path()).unwrap();

        assert!(!LineBuffer::check_binary(&file, Charset::Ascii, ControlDisplay::Standard).unwrap());
    }

    #[test]
    fn binary_detection_preserves_file_position() {
        use std::io::{Seek, SeekFrom};
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello\n").unwrap();
        tmp.flush().unwrap();
        let mut file = File::open(tmp.path()).unwrap();
        file.seek(SeekFrom::Start(2)).unwrap();

        LineBuffer::check_binary(&file, Charset::Ascii, ControlDisplay::Standard).unwrap();
        assert_eq!(file.stream_position().unwrap(), 2);
        let mut buf = LineBuffer::from_file(file);
        assert_eq!(buf.get_line(0), Some("hello"));
    }

    // --- read_more ---

    #[test]
    fn read_more_partial() {
        let (file, _tmp) = make_temp_file("a\nb\nc\nd\ne\n");
        let mut buf = LineBuffer::from_file(file);

        let n = buf.read_more(2);
        assert_eq!(n, 5);
        assert_eq!(buf.len(), 5);
        assert!(!buf.is_finished());
    }

    #[test]
    fn read_more_past_eof() {
        let (file, _tmp) = make_temp_file("a\nb\n");
        let mut buf = LineBuffer::from_file(file);

        let n = buf.read_more(100);
        assert_eq!(n, 2);
        assert!(buf.is_finished());
    }

    #[test]
    fn long_line_is_preserved() {
        let long = "x".repeat(70_000);
        let content = format!("{}\n", long);
        let (file, _tmp) = make_temp_file(&content);
        let mut buf = LineBuffer::from_file(file);

        let line = buf.get_line(0).unwrap();
        assert_eq!(line, long);
        assert_eq!(buf.get_line(1), None);
    }

    #[test]
    fn long_utf8_line_is_preserved() {
        let line = "€".repeat(21_846);
        let (file, _tmp) = make_temp_file(&(line.clone() + "\nlast\n"));
        let mut buffer = LineBuffer::from_file(file);
        assert_eq!(buffer.get_line(0), Some(line.as_str()));
        assert_eq!(buffer.get_line(1), Some("last"));
        assert_eq!(buffer.get_line(2), None);
    }

    // --- poll_new_lines ---

    #[test]
    fn poll_resets_finished() {
        let (file, _tmp) = make_temp_file("a\n");
        let mut buf = LineBuffer::from_file(file);

        buf.read_all();
        assert!(buf.is_finished());

        // poll_new_lines resets finished for follow mode
        buf.poll_new_lines();
        // finished gets reset to false, then read_more runs and hits EOF again
        assert!(buf.is_finished());
    }
}
