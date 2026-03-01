use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Stdin};

const MAX_LINE_LEN: usize = 64 * 1024; // 64KB
const BINARY_CHECK_SIZE: usize = 8 * 1024; // 8KB

enum Source {
    File(BufReader<File>),
    Stdin(BufReader<Stdin>),
}

impl Source {
    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        match self {
            Source::File(r) => r.read_line(buf),
            Source::Stdin(r) => r.read_line(buf),
        }
    }
}

pub struct LineBuffer {
    pub lines: Vec<String>,
    source: Option<Source>,
    finished: bool,
}

impl LineBuffer {
    pub fn from_file(file: File) -> Self {
        LineBuffer {
            lines: Vec::new(),
            source: Some(Source::File(BufReader::new(file))),
            finished: false,
        }
    }

    pub fn from_stdin(stdin: Stdin) -> Self {
        LineBuffer {
            lines: Vec::new(),
            source: Some(Source::Stdin(BufReader::new(stdin))),
            finished: false,
        }
    }

    /// Check first 8KB for null bytes. Returns true if binary.
    pub fn check_binary(file: &mut File) -> io::Result<bool> {
        let mut buf = [0u8; BINARY_CHECK_SIZE];
        let n = file.read(&mut buf)?;
        use std::io::Seek;
        file.seek(io::SeekFrom::Start(0))?;
        Ok(buf[..n].contains(&0))
    }

    /// Push a pre-parsed line into the buffer.
    pub fn push_line(&mut self, line: String) {
        self.lines.push(line);
    }

    /// Create a buffer from pre-existing lines (for testing).
    #[cfg(test)]
    pub fn from_lines(lines: Vec<String>) -> Self {
        LineBuffer {
            lines,
            source: None,
            finished: true,
        }
    }

    /// Read more lines, up to `count`. Returns number of new lines read.
    pub fn read_more(&mut self, count: usize) -> usize {
        if self.finished {
            return 0;
        }
        let source = match &mut self.source {
            Some(s) => s,
            None => return 0,
        };

        let mut read = 0;
        for _ in 0..count {
            let mut line = String::new();
            match source.read_line(&mut line) {
                Ok(0) => {
                    self.finished = true;
                    break;
                }
                Ok(_) => {
                    // Strip trailing newline
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }
                    // Truncate excessively long lines
                    if line.len() > MAX_LINE_LEN {
                        line.truncate(MAX_LINE_LEN);
                        line.push_str(" [truncated]");
                    }
                    self.lines.push(line);
                    read += 1;
                }
                Err(_) => {
                    self.finished = true;
                    break;
                }
            }
        }
        read
    }

    /// Ensure at least `n+1` lines are loaded (so line index `n` is available).
    fn ensure_line(&mut self, n: usize) {
        while self.lines.len() <= n && !self.finished {
            if self.read_more(256) == 0 {
                break;
            }
        }
    }

    /// Get line at index, reading forward if necessary.
    pub fn get_line(&mut self, n: usize) -> Option<&str> {
        self.ensure_line(n);
        self.lines.get(n).map(|s| s.as_str())
    }

    /// Number of currently cached lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Total line count, or None if not fully read yet.
    pub fn line_count(&self) -> Option<usize> {
        if self.finished {
            Some(self.lines.len())
        } else {
            None
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Read all remaining lines.
    pub fn read_all(&mut self) {
        while !self.finished {
            if self.read_more(4096) == 0 {
                break;
            }
        }
    }

    /// Try to read new lines (for follow mode). Returns number of new lines.
    pub fn poll_new_lines(&mut self) -> usize {
        if self.finished {
            // For follow mode, the file might have been appended to.
            self.finished = false;
        }
        self.read_more(256)
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
        assert!(buf.len() >= 1);
    }

    #[test]
    fn read_all() {
        let (file, _tmp) = make_temp_file("a\nb\nc\n");
        let mut buf = LineBuffer::from_file(file);

        buf.read_all();
        assert!(buf.is_finished());
        assert_eq!(buf.line_count(), Some(3));
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn push_line() {
        let (file, _tmp) = make_temp_file("");
        let mut buf = LineBuffer::from_file(file);

        buf.push_line("injected".to_string());
        assert_eq!(buf.lines[0], "injected");
        assert_eq!(buf.len(), 1);
    }

    // --- binary detection ---

    #[test]
    fn binary_detection_positive() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&[0x00, 0x01, 0x02]).unwrap();
        tmp.flush().unwrap();
        let mut file = File::open(tmp.path()).unwrap();

        assert!(LineBuffer::check_binary(&mut file).unwrap());
    }

    #[test]
    fn binary_detection_negative() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"just text\n").unwrap();
        tmp.flush().unwrap();
        let mut file = File::open(tmp.path()).unwrap();

        assert!(!LineBuffer::check_binary(&mut file).unwrap());
    }

    #[test]
    fn binary_detection_rewinds() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello\n").unwrap();
        tmp.flush().unwrap();
        let mut file = File::open(tmp.path()).unwrap();

        LineBuffer::check_binary(&mut file).unwrap();
        // After binary check, file should be rewound — reading should work
        let mut buf = LineBuffer::from_file(file);
        assert_eq!(buf.get_line(0), Some("hello"));
    }

    // --- read_more ---

    #[test]
    fn read_more_partial() {
        let (file, _tmp) = make_temp_file("a\nb\nc\nd\ne\n");
        let mut buf = LineBuffer::from_file(file);

        let n = buf.read_more(2);
        assert_eq!(n, 2);
        assert_eq!(buf.len(), 2);
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

    // --- line truncation ---

    #[test]
    fn long_line_truncated() {
        let long = "x".repeat(70_000);
        let content = format!("{}\n", long);
        let (file, _tmp) = make_temp_file(&content);
        let mut buf = LineBuffer::from_file(file);

        let line = buf.get_line(0).unwrap();
        assert!(line.len() < 70_000);
        assert!(line.ends_with("[truncated]"));
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
