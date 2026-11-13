use std::ops::Range;
use std::num::NonZeroUsize;

use unicode_width::UnicodeWidthChar;

use crate::charset::{Charset, Unit, Units};
use crate::ansi::{ControlDisplay, ColorSpan, color_span};
use crate::tabs::TabStops;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotationStyle { Inverse, Color }

impl NotationStyle {
    pub(crate) fn for_ui_color(enabled: bool) -> Self { if enabled { Self::Color } else { Self::Inverse } }
}

#[derive(Clone, Copy)]
enum Representation { Literal, Notation, Tab, Sgr, Hyperlink }

struct Glyph {
    source: Range<usize>,
    columns: Range<usize>,
    representation: Representation,
    bold: bool,
    underline: bool,
}

fn pop_glyph(glyphs: &mut Vec<Glyph>) -> Option<Glyph> {
    if let Some(last) = glyphs.last_mut() && matches!(last.representation, Representation::Tab) && last.columns.len() > 1 {
        let end = last.columns.end;
        last.columns.end -= 1;
        return Some(Glyph { source:last.source.clone(), columns:end - 1..end, representation:Representation::Tab, bold:last.bold, underline:last.underline });
    }
    glyphs.pop()
}

pub(crate) struct DisplayPrefix {
    bytes: Vec<u8>,
    charset: Charset,
    glyphs: Vec<Glyph>,
    pub(crate) columns: usize,
    pub(crate) complete: bool,
}

impl DisplayPrefix {
    pub(crate) fn new(mut bytes: Vec<u8>, charset: Charset, complete: bool, wrap_width: Option<NonZeroUsize>, controls: ControlDisplay, tabs: &TabStops) -> Self {
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
                overstrike = pop_glyph(&mut glyphs);
                while overstrike.as_ref().is_some_and(|glyph| glyph.columns.is_empty()) && !glyphs.is_empty() { overstrike = pop_glyph(&mut glyphs); }
                if let Some(previous) = &overstrike { columns = previous.columns.start; }
                continue;
            }
            let mut source = start..end;
            let mut bold = false;
            let mut underline = false;
            let (mut representation, mut width) = match unit {
                Unit::Character('\t') => {
                    let end_column = columns.saturating_add(tabs.spaces_after(columns));
                    glyphs.push(Glyph { source: start..end, columns: columns..end_column, representation: Representation::Tab, bold: false, underline: false });
                    columns = end_column;
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

    pub(crate) fn source(&self) -> &[u8] { &self.bytes }

    pub(crate) fn left_clipped_columns(&self, start: usize) -> usize {
        self.glyphs.iter().find_map(|glyph| {
            (matches!(glyph.representation, Representation::Literal) && glyph.columns.start < start && glyph.columns.end > start)
                .then(|| glyph.columns.end - start)
        }).unwrap_or(0)
    }

    pub(crate) fn search_text<'a>(&self, visible: &'a str, start: usize) -> std::borrow::Cow<'a, str> {
        if !self.bytes.contains(&b'\x08') { return std::borrow::Cow::Borrowed(visible); }
        let mut targets = self.glyphs.iter().filter_map(|glyph| {
            if glyph.columns.start < start || glyph.source.len() != 1 || !self.bytes[glyph.source.start].is_ascii()
                || self.bytes.get(glyph.source.end..glyph.source.end.saturating_add(2)) != Some(b"\x08_") { None }
            else { Some(glyph.columns.start - start) }
        }).peekable();
        let mut bytes: Option<Vec<u8>> = None;
        let mut column = 0;
        for (offset, character) in visible.char_indices() {
            while targets.peek().is_some_and(|target| *target < column) { targets.next(); }
            if targets.peek() == Some(&column) && character.is_ascii() {
                bytes.get_or_insert_with(|| visible.as_bytes().to_vec())[offset] = b'_';
                targets.next();
            }
            column += UnicodeWidthChar::width(character).unwrap_or(0);
        }
        bytes.map_or(std::borrow::Cow::Borrowed(visible), |bytes| std::borrow::Cow::Owned(String::from_utf8(bytes).unwrap()))
    }

    pub(crate) fn display(&self, start: usize, width: usize, notation_style: NotationStyle) -> Vec<u8> {
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
            if glyph.columns.start >= end && !(glyph.columns.is_empty() && glyph.columns.start == end && end > start) { break; }
            let notation = matches!(glyph.representation, Representation::Notation);
            let next_attributes = (notation, glyph.bold, glyph.underline);
            if attributes != next_attributes {
                if attributes != (false, false, false) {
                    output.extend_from_slice(b"\x1b[m");
                    for sequence in &source_sgr { output.extend_from_slice(sequence); }
                }
                let colored_notation = notation && notation_style == NotationStyle::Color;
                let codes = [(notation && !colored_notation, "7"), (colored_notation, "30"), (colored_notation, "101"), (glyph.bold, "1"), (glyph.underline, "4")].into_iter().filter_map(|(enabled, code)| enabled.then_some(code)).collect::<Vec<_>>();
                if !codes.is_empty() { output.extend_from_slice(format!("\x1b[{}m", codes.join(";")).as_bytes()); }
                attributes = next_attributes;
            }
            let visible = glyph.columns.end.min(end).saturating_sub(glyph.columns.start.max(start));
            match glyph.representation {
                Representation::Literal if glyph.columns.start < start => {
                    output.extend_from_slice(if notation_style == NotationStyle::Color { b"\x1b[30;46m" } else { b"\x1b[7m" });
                    output.resize(output.len() + visible, b' ');
                    output.extend_from_slice(b"\x1b[m");
                    for sequence in &source_sgr { output.extend_from_slice(sequence); }
                    attributes = (false, false, false);
                }
                Representation::Literal if glyph.columns.end > end => output.resize(output.len() + visible, b' '),
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
        let layout = DisplayPrefix::new(b"A\tB\x01\xff".to_vec(), Charset::Utf8, true, None, ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.columns, 15);
        assert_eq!(layout.display(0, 8, NotationStyle::Inverse), "A       ".as_bytes());
        assert_eq!(layout.display(8, 7, NotationStyle::Inverse), "B\x1b[7m^A<FF>\x1b[m".as_bytes());
        assert_eq!(layout.display(3, 3, NotationStyle::Inverse), b"   ");
    }

    #[test]
    fn atomic_notation_and_wide_characters_leave_short_rows() {
        let layout = DisplayPrefix::new(b"abcdefg\x01B".to_vec(), Charset::Ascii, true, NonZeroUsize::new(8), ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.display(0, 8, NotationStyle::Inverse), "abcdefg".as_bytes());
        assert!(!layout.fills_row(0, 8));
        assert_eq!(layout.display(8, 8, NotationStyle::Inverse), "\x1b[7m^A\x1b[mB".as_bytes());
        let layout = DisplayPrefix::new("abcdefg界B".as_bytes().to_vec(), Charset::Utf8, true, NonZeroUsize::new(8), ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.display(0, 8, NotationStyle::Inverse), "abcdefg".as_bytes());
        assert_eq!(layout.display(8, 8, NotationStyle::Inverse), "界B".as_bytes());
    }

    #[test]
    fn backspace_replaces_a_base_and_its_combining_marks() {
        let layout = DisplayPrefix::new("e\u{301}\u{8}e".as_bytes().to_vec(), Charset::Utf8, true, None, ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.display(0, 8, NotationStyle::Inverse), "\x1b[1me\x1b[m".as_bytes());
        let layout = DisplayPrefix::new(b"X\x08_\x08X".to_vec(), Charset::Ascii, true, None, ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.display(0, 8, NotationStyle::Inverse), "\x1b[1;4mX\x1b[m".as_bytes());
    }

    #[test]
    fn split_scalar_waits_for_more_source_bytes() {
        let layout = DisplayPrefix::new(vec![b'x', 0xe2, 0x82], Charset::Utf8, false, None, ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.display(0, 8, NotationStyle::Inverse), "x".as_bytes());
        let layout = DisplayPrefix::new(vec![b'x', 0xe2, 0x82], Charset::Utf8, true, None, ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.display(0, 16, NotationStyle::Inverse), "x\x1b[7m<E2><82>\x1b[m".as_bytes());
    }

    #[test]
    fn combining_mark_at_right_edge_stays_with_visible_base() {
        let layout = DisplayPrefix::new("xe\u{301}\u{327}z".as_bytes().to_vec(), Charset::Utf8, true, None, ControlDisplay::Standard, &TabStops::default());
        assert_eq!(layout.display(0, 2, NotationStyle::Inverse), "xe\u{301}\u{327}".as_bytes());
        assert_eq!(layout.display(1, 1, NotationStyle::Inverse), "e\u{301}\u{327}".as_bytes());
        assert_eq!(layout.display(0, 0, NotationStyle::Inverse), b"");
    }
}
