use std::ops::Range;
use std::num::NonZeroUsize;

use unicode_width::UnicodeWidthChar;

use crate::charset::{Charset, Unit, Units};
use crate::ansi::{ControlDisplay, ColorSpan, color_span};

#[derive(Clone, Copy)]
enum Representation { Literal, Notation, Tab, Sgr, Hyperlink }

struct Glyph {
    source: Range<usize>,
    columns: Range<usize>,
    representation: Representation,
    bold: bool,
    underline: bool,
}

pub(crate) struct DisplayPrefix {
    bytes: Vec<u8>,
    charset: Charset,
    glyphs: Vec<Glyph>,
    pub(crate) columns: usize,
    pub(crate) complete: bool,
}

impl DisplayPrefix {
    pub(crate) fn new(mut bytes: Vec<u8>, charset: Charset, complete: bool, wrap_width: Option<NonZeroUsize>, controls: ControlDisplay) -> Self {
        if !complete && charset == Charset::Utf8 && let Err(error) = std::str::from_utf8(&bytes)
            && error.error_len().is_none() { bytes.truncate(error.valid_up_to()); }
        let mut glyphs: Vec<Glyph> = Vec::new();
        let mut overstrike: Option<Glyph> = None;
        let mut last_overstrike = (false, false);
        let mut columns = 0;
        let mut units = Units { charset, remaining: &bytes };
        while !units.remaining.is_empty() {
            let start = bytes.len() - units.remaining.len();
            let unit = units.next().unwrap();
            let end = bytes.len() - units.remaining.len();
            if controls == ControlDisplay::Color && matches!(unit, Unit::Character('\u{1b}')) {
                let (length, representation) = match color_span(&bytes[start..], charset) {
                    ColorSpan::Sgr(length) => (length, Some(Representation::Sgr)),
                    ColorSpan::Hyperlink(length) => (length, Some(Representation::Hyperlink)),
                    ColorSpan::Discard(length) => (length, None),
                    ColorSpan::Incomplete => break,
                };
                units.remaining = &bytes[start + length..];
                if let Some(representation) = representation {
                    glyphs.push(Glyph { source: start..start + length, columns: columns..columns, representation, bold: false, underline: false });
                }
                continue;
            }
            if matches!(unit, Unit::Character('\u{8}')) && !glyphs.last().is_some_and(|glyph| matches!(glyph.representation, Representation::Notation | Representation::Sgr | Representation::Hyperlink)) {
                overstrike = glyphs.pop();
                while overstrike.as_ref().is_some_and(|glyph| glyph.columns.is_empty()) && !glyphs.is_empty() { overstrike = glyphs.pop(); }
                if let Some(previous) = &overstrike { columns = previous.columns.start; }
                continue;
            }
            let mut source = start..end;
            let mut bold = false;
            let mut underline = false;
            let (mut representation, mut width) = match unit {
                Unit::Character('\t') => {
                    for _ in 0..8 - columns % 8 {
                        glyphs.push(Glyph { source: start..end, columns: columns..columns + 1, representation: Representation::Tab, bold: false, underline: false });
                        columns += 1;
                    }
                    overstrike = None;
                    continue;
                }
                Unit::Character(character) if character.is_control() || charset.binary_character(character) => {
                    let mut notation = Vec::new();
                    charset.write_filename(&bytes[start..end], &mut notation).unwrap();
                    (Representation::Notation, notation.len())
                }
                Unit::Character(character) => (Representation::Literal, UnicodeWidthChar::width(character).unwrap_or(0)),
                Unit::Invalid(invalid) => {
                    for offset in 0..invalid.len() {
                        columns = Self::place(columns, 4, wrap_width);
                        glyphs.push(Glyph { source: start + offset..start + offset + 1, columns: columns..columns + 4, representation: Representation::Notation, bold: false, underline: false });
                        columns += 4;
                    }
                    overstrike = None;
                    continue;
                }
            };
            if let Some(previous) = overstrike.take() {
                bold = previous.bold;
                underline = previous.underline;
                let previous_character = match previous.representation {
                    Representation::Tab => Some(' '),
                    Representation::Literal => std::str::from_utf8(&bytes[previous.source.clone()]).unwrap().chars().next(),
                    Representation::Notation | Representation::Sgr | Representation::Hyperlink => None,
                };
                if let Unit::Character(character) = unit {
                    if previous_character == Some(character) {
                        if character != '_' { bold = true; }
                        else if bold || underline { bold = true; underline = true; }
                        else if last_overstrike != (false, false) { (bold, underline) = last_overstrike; }
                        else { bold = true; }
                    } else if character == '_' {
                        underline = true;
                        source = previous.source;
                        representation = previous.representation;
                        width = previous.columns.len();
                    } else if previous_character == Some('_') { underline = true; }
                }
            }
            if bold || underline { last_overstrike = (bold, underline); }
            if !matches!(representation, Representation::Tab) { columns = Self::place(columns, width, wrap_width); }
            glyphs.push(Glyph { source, columns: columns..columns + width, representation, bold, underline });
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

    pub(crate) fn display(&self, start: usize, width: usize) -> Vec<u8> {
        let end = start.saturating_add(width);
        let mut output = Vec::new();
        let mut attributes = (false, false, false);
        let mut source_sgr: Vec<&[u8]> = Vec::new();
        for glyph in &self.glyphs {
            if matches!(glyph.representation, Representation::Sgr | Representation::Hyperlink) {
                if glyph.columns.start > end { break; }
                if attributes != (false, false, false) {
                    output.extend_from_slice(b"\x1b[m");
                    for sequence in &source_sgr { output.extend_from_slice(sequence); }
                    attributes = (false, false, false);
                }
                let sequence = &self.bytes[glyph.source.clone()];
                output.extend_from_slice(sequence);
                if matches!(glyph.representation, Representation::Sgr) { source_sgr.push(sequence); }
                continue;
            }
            if glyph.columns.end <= start && !glyph.columns.is_empty() { continue; }
            if glyph.columns.start >= end { break; }
            let notation = matches!(glyph.representation, Representation::Notation);
            let next_attributes = (notation, glyph.bold, glyph.underline);
            if attributes != next_attributes {
                if attributes != (false, false, false) {
                    output.extend_from_slice(b"\x1b[m");
                    for sequence in &source_sgr { output.extend_from_slice(sequence); }
                }
                let codes = [(notation, "7"), (glyph.bold, "1"), (glyph.underline, "4")].into_iter().filter_map(|(enabled, code)| enabled.then_some(code)).collect::<Vec<_>>();
                if !codes.is_empty() { output.extend_from_slice(format!("\x1b[{}m", codes.join(";")).as_bytes()); }
                attributes = next_attributes;
            }
            let visible = glyph.columns.end.min(end).saturating_sub(glyph.columns.start.max(start));
            match glyph.representation {
                Representation::Literal if glyph.columns.start < start || glyph.columns.end > end => output.resize(output.len() + visible, b' '),
                Representation::Literal => output.extend_from_slice(&self.bytes[glyph.source.clone()]),
                Representation::Tab => output.resize(output.len() + visible, b' '),
                Representation::Sgr | Representation::Hyperlink => unreachable!(),
                Representation::Notation => {
                    let mut text = Vec::new();
                    self.charset.write_filename(&self.bytes[glyph.source.clone()], &mut text).unwrap();
                    let skip = start.saturating_sub(glyph.columns.start);
                    output.extend_from_slice(&text[skip..skip + visible]);
                }
            }
        }
        if attributes != (false, false, false) || !source_sgr.is_empty() { output.extend_from_slice(b"\x1b[m"); }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_map_to_columns_before_clipping() {
        let layout = DisplayPrefix::new(b"A\tB\x01\xff".to_vec(), Charset::Utf8, true, None, ControlDisplay::Standard);
        assert_eq!(layout.columns, 15);
        assert_eq!(layout.display(0, 8), "A       ".as_bytes());
        assert_eq!(layout.display(8, 7), "B\x1b[7m^A<FF>\x1b[m".as_bytes());
        assert!(layout.glyphs[1..8].iter().enumerate().all(|(index, glyph)| glyph.source == (1..2) && glyph.columns == (index + 1..index + 2)));
    }

    #[test]
    fn atomic_notation_and_wide_characters_leave_short_rows() {
        let layout = DisplayPrefix::new(b"abcdefg\x01B".to_vec(), Charset::Ascii, true, NonZeroUsize::new(8), ControlDisplay::Standard);
        assert_eq!(layout.display(0, 8), "abcdefg".as_bytes());
        assert!(!layout.fills_row(0, 8));
        assert_eq!(layout.display(8, 8), "\x1b[7m^A\x1b[mB".as_bytes());
        let layout = DisplayPrefix::new("abcdefg界B".as_bytes().to_vec(), Charset::Utf8, true, NonZeroUsize::new(8), ControlDisplay::Standard);
        assert_eq!(layout.display(0, 8), "abcdefg".as_bytes());
        assert_eq!(layout.display(8, 8), "界B".as_bytes());
    }

    #[test]
    fn backspace_replaces_a_base_and_its_combining_marks() {
        let layout = DisplayPrefix::new("e\u{301}\u{8}e".as_bytes().to_vec(), Charset::Utf8, true, None, ControlDisplay::Standard);
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.display(0, 8), "\x1b[1me\x1b[m".as_bytes());
        let layout = DisplayPrefix::new(b"X\x08_\x08X".to_vec(), Charset::Ascii, true, None, ControlDisplay::Standard);
        assert_eq!(layout.display(0, 8), "\x1b[1;4mX\x1b[m".as_bytes());
    }

    #[test]
    fn split_scalar_waits_for_more_source_bytes() {
        let layout = DisplayPrefix::new(vec![b'x', 0xe2, 0x82], Charset::Utf8, false, None, ControlDisplay::Standard);
        assert_eq!(layout.display(0, 8), "x".as_bytes());
        let layout = DisplayPrefix::new(vec![b'x', 0xe2, 0x82], Charset::Utf8, true, None, ControlDisplay::Standard);
        assert_eq!(layout.display(0, 16), "x\x1b[7m<E2><82>\x1b[m".as_bytes());
    }
}
