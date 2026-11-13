use std::io::{self, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charset { Ascii, Utf8 }

#[derive(Clone, Copy)]
pub(crate) enum Unit<'a> { Character(char), Invalid(&'a [u8]) }

pub(crate) struct Units<'a> { pub(crate) charset: Charset, pub(crate) remaining: &'a [u8] }

impl<'a> Iterator for Units<'a> {
    type Item = Unit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let first = *self.remaining.first()?;
        let width = match first { 0..=127 => 1, 0xc2..=0xdf => 2, 0xe0..=0xef => 3, 0xf0..=0xf4 => 4, _ => 0 };
        if self.charset == Charset::Ascii {
            let bytes = &self.remaining[..1];
            self.remaining = &self.remaining[1..];
            return Some(if first < 128 { Unit::Character(char::from(first)) } else { Unit::Invalid(bytes) });
        }
        if width > 0 && let Some(bytes) = self.remaining.get(..width)
            && let Ok(text) = std::str::from_utf8(bytes) {
            let character = text.chars().next().unwrap();
            self.remaining = &self.remaining[width..];
            return Some(Unit::Character(character));
        }
        let end = 1 + self.remaining[1..].iter().take_while(|byte| matches!(**byte, 0x80..=0xbf | 0xfe..=0xff)).count();
        let bytes = &self.remaining[..end];
        self.remaining = &self.remaining[end..];
        Some(Unit::Invalid(bytes))
    }
}

impl Charset {
    pub fn resolve(explicit: Option<&str>, locale: Option<&str>) -> io::Result<Self> {
        if let Some(name) = explicit.filter(|name| !name.is_empty()) {
            return match name {
                "ascii" | "US-ASCII" | "ANSI_X3.4-1968" => Ok(Self::Ascii),
                "utf-8" | "UTF-8" | "utf8" | "UTF8" => Ok(Self::Utf8),
                _ => Err(io::Error::new(io::ErrorKind::Unsupported, format!("unsupported character set: {name:?}"))),
            };
        }
        let locale = locale.unwrap_or("C");
        let lower = locale.to_ascii_lowercase();
        if lower.contains("utf-8") || lower.contains("utf8") { return Ok(Self::Utf8); }
        if matches!(lower.as_str(), "" | "c" | "posix") { return Ok(Self::Ascii); }
        Err(io::Error::new(io::ErrorKind::Unsupported, format!("unsupported locale character set: {locale:?}")))
    }

    pub(crate) fn binary_character(self, character: char) -> bool {
        let code = u32::from(character);
        if character.is_control() && !matches!(code, 8 | 9 | 10 | 12 | 13) { return true; }
        self == Self::Utf8 && matches!(code, 0x2028 | 0x2029 | 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
    }

    pub(crate) fn looks_binary(self, sample: &[u8]) -> bool {
        Units { charset: self, remaining: sample }.filter(|unit| match unit {
            Unit::Character(character) => self.binary_character(*character), Unit::Invalid(_) => true,
        }).take(6).count() == 6
    }

    pub fn write_filename(self, bytes: &[u8], output: &mut impl Write) -> io::Result<()> {
        for unit in (Units { charset: self, remaining: bytes }) {
            match unit {
                Unit::Invalid(bytes) => for byte in bytes { write!(output, "<{byte:02X}>")?; },
                Unit::Character('\u{1b}') => output.write_all(b"ESC")?,
                Unit::Character(character) if character.is_ascii_control() => output.write_all(&[b'^', character as u8 ^ 0x40])?,
                Unit::Character(character) if self.binary_character(character) => write!(output, "<U+{:04X}>", u32::from(character))?,
                Unit::Character(character) => {
                    let mut bytes = [0; 4];
                    output.write_all(character.encode_utf8(&mut bytes).as_bytes())?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_explicit_charset_before_locale() {
        assert_eq!(Charset::resolve(None, Some("C")).unwrap(), Charset::Ascii);
        assert_eq!(Charset::resolve(None, Some("en_US.UTF-8")).unwrap(), Charset::Utf8);
        assert_eq!(Charset::resolve(Some("ascii"), Some("en_US.UTF-8")).unwrap(), Charset::Ascii);
        assert_eq!(Charset::resolve(Some("utf-8"), Some("C")).unwrap(), Charset::Utf8);
        assert_eq!(Charset::resolve(Some("utf8"), Some("C")).unwrap(), Charset::Utf8);
        assert_eq!(Charset::resolve(Some("unsupported"), Some("C")).unwrap_err().kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn malformed_utf8_is_counted_without_losing_bytes_in_presentation() {
        let bytes = [0xe0, 0x80, 0x80, b'x', 0xff];
        assert!(!Charset::Utf8.looks_binary(&bytes));
        let mut text = Vec::new();
        Charset::Utf8.write_filename(&bytes, &mut text).unwrap();
        assert_eq!(text, b"<E0><80><80>x<FF>");
        assert!(Charset::Ascii.looks_binary(&[255; 6]));
        assert!(!Charset::Utf8.looks_binary(&[255; 6]));
        assert!(Charset::Utf8.looks_binary(&[255, b'x'].repeat(6)));
    }
}
