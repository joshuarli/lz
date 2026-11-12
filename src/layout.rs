use std::ops::Range;
use std::num::NonZeroUsize;

use unicode_width::UnicodeWidthChar;

use crate::charset::{Charset, Unit, Units};

#[derive(Clone, Copy)]
enum Representation { Literal, Notation, Tab }

struct Glyph {
    source: Range<usize>,
    columns: Range<usize>,
    representation: Representation,
}

pub(crate) struct DisplayPrefix {
    bytes: Vec<u8>,
    charset: Charset,
    glyphs: Vec<Glyph>,
    pub(crate) columns: usize,
    pub(crate) complete: bool,
}

impl DisplayPrefix {
    pub(crate) fn new(mut bytes: Vec<u8>, charset: Charset, complete: bool, wrap_width: Option<NonZeroUsize>) -> Self {
        if !complete && charset == Charset::Utf8 && let Err(error) = std::str::from_utf8(&bytes)
            && error.error_len().is_none() { bytes.truncate(error.valid_up_to()); }
        let mut glyphs = Vec::new();
        let mut columns = 0;
        let mut units = Units { charset, remaining: &bytes };
        while !units.remaining.is_empty() {
            let start = bytes.len() - units.remaining.len();
            let unit = units.next().unwrap();
            let end = bytes.len() - units.remaining.len();
            let (representation, width) = match unit {
                Unit::Character('\t') => (Representation::Tab, 8 - columns % 8),
                Unit::Character(character) if character.is_control() || charset.binary_character(character) => {
                    let mut notation = Vec::new();
                    charset.write_filename(&bytes[start..end], &mut notation).unwrap();
                    (Representation::Notation, notation.len())
                }
                Unit::Character(character) => (Representation::Literal, UnicodeWidthChar::width(character).unwrap_or(0)),
                Unit::Invalid(invalid) => {
                    for offset in 0..invalid.len() {
                        columns = Self::place(columns, 4, wrap_width);
                        glyphs.push(Glyph { source: start + offset..start + offset + 1, columns: columns..columns + 4, representation: Representation::Notation });
                        columns += 4;
                    }
                    continue;
                }
            };
            if !matches!(representation, Representation::Tab) { columns = Self::place(columns, width, wrap_width); }
            glyphs.push(Glyph { source: start..end, columns: columns..columns + width, representation });
            columns += width;
        }
        Self { bytes, charset, glyphs, columns, complete }
    }

    fn place(column: usize, width: usize, wrap_width: Option<NonZeroUsize>) -> usize {
        if let Some(wrap_width) = wrap_width {
            let wrap_width = wrap_width.get();
            let remaining = wrap_width - column % wrap_width;
            if width <= wrap_width && width > remaining { return column + remaining; }
        }
        column
    }

    pub(crate) fn fills_row(&self, start: usize, width: usize) -> bool {
        let end = start.saturating_add(width);
        self.glyphs.iter().any(|glyph| glyph.columns.start < end && glyph.columns.end >= end)
    }

    pub(crate) fn display(&self, start: usize, width: usize) -> String {
        let end = start.saturating_add(width);
        let mut output = Vec::new();
        let mut inverse = false;
        for glyph in &self.glyphs {
            if glyph.columns.end <= start && !glyph.columns.is_empty() { continue; }
            if glyph.columns.start >= end { break; }
            let notation = matches!(glyph.representation, Representation::Notation);
            if notation != inverse {
                output.extend_from_slice(if notation { b"\x1b[7m" } else { b"\x1b[m" });
                inverse = notation;
            }
            let visible = glyph.columns.end.min(end).saturating_sub(glyph.columns.start.max(start));
            match glyph.representation {
                Representation::Literal if glyph.columns.start < start || glyph.columns.end > end => output.resize(output.len() + visible, b' '),
                Representation::Literal => output.extend_from_slice(&self.bytes[glyph.source.clone()]),
                Representation::Tab => output.resize(output.len() + visible, b' '),
                Representation::Notation => {
                    let mut text = Vec::new();
                    self.charset.write_filename(&self.bytes[glyph.source.clone()], &mut text).unwrap();
                    let skip = start.saturating_sub(glyph.columns.start);
                    output.extend_from_slice(&text[skip..skip + visible]);
                }
            }
        }
        if inverse { output.extend_from_slice(b"\x1b[m"); }
        String::from_utf8(output).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_map_to_columns_before_clipping() {
        let layout = DisplayPrefix::new(b"A\tB\x01\xff".to_vec(), Charset::Utf8, true, None);
        assert_eq!(layout.columns, 15);
        assert_eq!(layout.display(0, 8), "A       ");
        assert_eq!(layout.display(8, 7), "B\x1b[7m^A<FF>\x1b[m");
        assert_eq!(layout.glyphs[1].source, 1..2);
        assert_eq!(layout.glyphs[1].columns, 1..8);
    }

    #[test]
    fn atomic_notation_and_wide_characters_leave_short_rows() {
        let layout = DisplayPrefix::new(b"abcdefg\x01B".to_vec(), Charset::Ascii, true, NonZeroUsize::new(8));
        assert_eq!(layout.display(0, 8), "abcdefg");
        assert!(!layout.fills_row(0, 8));
        assert_eq!(layout.display(8, 8), "\x1b[7m^A\x1b[mB");
        let layout = DisplayPrefix::new("abcdefg界B".as_bytes().to_vec(), Charset::Utf8, true, NonZeroUsize::new(8));
        assert_eq!(layout.display(0, 8), "abcdefg");
        assert_eq!(layout.display(8, 8), "界B");
    }

    #[test]
    fn split_scalar_waits_for_more_source_bytes() {
        let layout = DisplayPrefix::new(vec![b'x', 0xe2, 0x82], Charset::Utf8, false, None);
        assert_eq!(layout.display(0, 8), "x");
        let layout = DisplayPrefix::new(vec![b'x', 0xe2, 0x82], Charset::Utf8, true, None);
        assert_eq!(layout.display(0, 16), "x\x1b[7m<E2><82>\x1b[m");
    }
}
