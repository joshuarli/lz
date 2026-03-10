use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x0001;

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Escape,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Backspace,
    Delete,
    Enter,
    Unknown,
}

pub struct KeyReader {
    tty: File,
    buf: [u8; 64],
    pos: usize,
    len: usize,
}

impl KeyReader {
    pub fn new() -> io::Result<Self> {
        let tty = File::open("/dev/tty")?;
        Ok(KeyReader {
            tty,
            buf: [0; 64],
            pos: 0,
            len: 0,
        })
    }

    /// Raw fd for the tty, used to set raw mode on the correct fd.
    pub fn fd(&self) -> i32 {
        self.tty.as_raw_fd()
    }

    /// Read a key, blocking until one is available.
    pub fn read_key(&mut self) -> io::Result<Key> {
        let byte = self.next_byte_blocking()?;
        self.parse_byte(byte)
    }

    /// Read a key with a timeout in milliseconds.
    /// Returns None if no key is available within the timeout.
    pub fn read_key_timeout(&mut self, timeout_ms: i32) -> io::Result<Option<Key>> {
        match self.next_byte_timeout(timeout_ms)? {
            Some(byte) => Ok(Some(self.parse_byte(byte)?)),
            None => Ok(None),
        }
    }

    // --- Buffered byte access ---

    /// Get next byte from internal buffer, refilling via blocking read if empty.
    fn next_byte_blocking(&mut self) -> io::Result<u8> {
        if self.pos < self.len {
            let b = self.buf[self.pos];
            self.pos += 1;
            return Ok(b);
        }
        // Buffer empty — do a blocking read
        self.fill_blocking()?;
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    /// Get next byte from internal buffer, refilling via timed poll+read if empty.
    fn next_byte_timeout(&mut self, timeout_ms: i32) -> io::Result<Option<u8>> {
        if self.pos < self.len {
            let b = self.buf[self.pos];
            self.pos += 1;
            return Ok(Some(b));
        }
        // Buffer empty — poll, then try to read
        if !self.poll_ready(timeout_ms)? {
            return Ok(None);
        }
        match self.try_fill()? {
            0 => Ok(None),
            _ => {
                let b = self.buf[self.pos];
                self.pos += 1;
                Ok(Some(b))
            }
        }
    }

    /// Blocking read into internal buffer. Reads at least 1 byte.
    fn fill_blocking(&mut self) -> io::Result<()> {
        self.pos = 0;
        let n = self.tty.read(&mut self.buf)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "tty closed"));
        }
        self.len = n;
        Ok(())
    }

    /// Non-blocking read into internal buffer. Returns bytes read.
    fn try_fill(&mut self) -> io::Result<usize> {
        self.pos = 0;
        match self.tty.read(&mut self.buf) {
            Ok(n) => {
                self.len = n;
                Ok(n)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.len = 0;
                Ok(0)
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                self.len = 0;
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }

    /// Poll the tty fd for readability. Returns true if data is available.
    fn poll_ready(&self, timeout_ms: i32) -> io::Result<bool> {
        let fd = self.tty.as_raw_fd();
        let mut pfd = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let ret = unsafe { poll(&mut pfd, 1, timeout_ms) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(err);
        }
        Ok(ret > 0 && (pfd.revents & POLLIN) != 0)
    }

    // --- Key parsing ---

    fn parse_byte(&mut self, byte: u8) -> io::Result<Key> {
        match byte {
            0x1b => self.parse_escape(),
            0x0d | 0x0a => Ok(Key::Enter),
            0x7f => Ok(Key::Backspace),
            0x08 => Ok(Key::Backspace),
            b if b <= 0x1a => Ok(Key::Ctrl((b + b'a' - 1) as char)),
            b => {
                let ch = self.read_utf8_char(b)?;
                Ok(Key::Char(ch))
            }
        }
    }

    fn parse_escape(&mut self) -> io::Result<Key> {
        // If we already have more bytes buffered, consume immediately (no timeout).
        // Otherwise wait up to 50ms for the next byte.
        match self.next_byte_timeout(50)? {
            None => Ok(Key::Escape),
            Some(b'[') => self.parse_csi(),
            Some(b'O') => self.parse_ss3(),
            Some(_) => Ok(Key::Escape),
        }
    }

    fn parse_csi(&mut self) -> io::Result<Key> {
        match self.next_byte_timeout(50)? {
            None => Ok(Key::Unknown),
            Some(b'A') => Ok(Key::Up),
            Some(b'B') => Ok(Key::Down),
            Some(b'C') => Ok(Key::Right),
            Some(b'D') => Ok(Key::Left),
            Some(b'H') => Ok(Key::Home),
            Some(b'F') => Ok(Key::End),
            Some(b) if b.is_ascii_digit() => {
                let mut num = (b - b'0') as u32;
                loop {
                    match self.next_byte_timeout(50)? {
                        None => return Ok(Key::Unknown),
                        Some(b'~') => break,
                        Some(b';') => {
                            self.consume_csi_remainder()?;
                            return Ok(Key::Unknown);
                        }
                        Some(d) if d.is_ascii_digit() => {
                            num = num * 10 + (d - b'0') as u32;
                        }
                        Some(b'A') => return Ok(Key::Up),
                        Some(b'B') => return Ok(Key::Down),
                        Some(b'C') => return Ok(Key::Right),
                        Some(b'D') => return Ok(Key::Left),
                        Some(b'H') => return Ok(Key::Home),
                        Some(b'F') => return Ok(Key::End),
                        Some(_) => return Ok(Key::Unknown),
                    }
                }
                match num {
                    1 | 7 => Ok(Key::Home),
                    2 => Ok(Key::Unknown),
                    3 => Ok(Key::Delete),
                    4 | 8 => Ok(Key::End),
                    5 => Ok(Key::PageUp),
                    6 => Ok(Key::PageDown),
                    _ => Ok(Key::Unknown),
                }
            }
            Some(_) => Ok(Key::Unknown),
        }
    }

    fn parse_ss3(&mut self) -> io::Result<Key> {
        match self.next_byte_timeout(50)? {
            None => Ok(Key::Unknown),
            Some(b'A') => Ok(Key::Up),
            Some(b'B') => Ok(Key::Down),
            Some(b'C') => Ok(Key::Right),
            Some(b'D') => Ok(Key::Left),
            Some(b'H') => Ok(Key::Home),
            Some(b'F') => Ok(Key::End),
            Some(_) => Ok(Key::Unknown),
        }
    }

    fn consume_csi_remainder(&mut self) -> io::Result<()> {
        loop {
            match self.next_byte_timeout(50)? {
                None => return Ok(()),
                Some(b) if (0x40..=0x7e).contains(&b) => return Ok(()),
                Some(_) => continue,
            }
        }
    }

    fn read_utf8_char(&mut self, first: u8) -> io::Result<char> {
        let width = match first {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return Ok(char::REPLACEMENT_CHARACTER),
        };
        if width == 1 {
            return Ok(first as char);
        }
        let mut bytes = [0u8; 4];
        bytes[0] = first;
        for byte in bytes.iter_mut().take(width).skip(1) {
            match self.next_byte_timeout(50)? {
                Some(b) => *byte = b,
                None => return Ok(char::REPLACEMENT_CHARACTER),
            }
        }
        match std::str::from_utf8(&bytes[..width]) {
            Ok(s) => Ok(s.chars().next().unwrap_or(char::REPLACEMENT_CHARACTER)),
            Err(_) => Ok(char::REPLACEMENT_CHARACTER),
        }
    }
}

/// Parse keys from a byte slice (for testing).
/// Returns all keys that can be parsed from the given bytes.
#[cfg(test)]
pub fn parse_keys_from_bytes(bytes: &[u8]) -> Vec<Key> {
    // Build a fake KeyReader backed by a pipe so we can feed it bytes
    use std::os::unix::io::FromRawFd;

    let (read_fd, write_fd) = {
        let mut fds = [0i32; 2];
        unsafe {
            extern "C" {
                fn pipe(fds: *mut i32) -> i32;
            }
            if pipe(fds.as_mut_ptr()) != 0 {
                return vec![];
            }
        }
        (fds[0], fds[1])
    };

    // Write bytes to the pipe
    {
        let mut write_file = unsafe { File::from_raw_fd(write_fd) };
        use std::io::Write;
        let _ = write_file.write_all(bytes);
        // drop closes the write end, so reads will see EOF after the data
    }

    let read_file = unsafe { File::from_raw_fd(read_fd) };
    let mut reader = KeyReader {
        tty: read_file,
        buf: [0; 64],
        pos: 0,
        len: 0,
    };

    let mut keys = Vec::new();
    loop {
        // Use a short timeout since we have all data already
        match reader.read_key_timeout(10) {
            Ok(Some(key)) => keys.push(key),
            Ok(None) => break,
            Err(_) => break,
        }
    }
    keys
}
