use std::io::{self, Read, Seek, SeekFrom, Write};
use std::collections::{HashMap, VecDeque};

use crate::ansi::{ControlDisplay, strip_ansi, visible_width};
use crate::charset::Charset;
use crate::layout::{DisplayPrefix, NotationStyle};
use crate::buffer::{LineBuffer, LineFragment};
use crate::history::SearchHistory;
use crate::input::{Key, KeyReader};
use crate::search::{CaseSensitivity, RegexLocale, Search, SearchStart};
use crate::terminal;
use crate::tabs::TabStops;

const FOLLOW_INTERRUPT: char = 'x';

/// Retains the display origin and position-table capacity across file switches.
#[derive(Clone, Copy)]
pub struct DisplayAnchor { row: Option<usize>, position_capacity: u16 }

impl DisplayAnchor {
    pub fn new(screen: terminal::ScreenMode) -> Self {
        Self { row: (screen == terminal::ScreenMode::Preserve).then_some(0), position_capacity: 0 }
    }
}

enum FramePart { Bytes(Vec<u8>), Spill { offset: u64, length: u64 } }

struct Frame {
    bytes: Vec<u8>,
    parts: Vec<FramePart>,
    spill: Option<std::fs::File>,
    relative_row: Option<usize>,
    padding: usize,
    retain_initial_column: bool,
}

impl std::ops::Deref for Frame {
    type Target = Vec<u8>;
    fn deref(&self) -> &Self::Target { &self.bytes }
}

impl std::ops::DerefMut for Frame {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.bytes }
}

impl Frame {
    fn append_color_line(&mut self, buffer: &mut LineBuffer, line: usize, width: usize, charset: Charset, before: impl FnOnce(&mut Self)) -> io::Result<Option<bool>> {
        if self.spill.is_none() { self.spill = Some(crate::storage::anonymous_file()?); }
        let spill = self.spill.as_mut().unwrap();
        let offset = spill.seek(SeekFrom::End(0))?;
        match scan_color_line(buffer, line, width, charset, spill)? {
            ColorLine::Fits(sgr) => {
                let end = spill.stream_position()?;
                before(self);
                self.parts.push(FramePart::Bytes(std::mem::take(&mut self.bytes)));
                self.parts.push(FramePart::Spill { offset, length:end - offset });
                Ok(Some(sgr))
            }
            ColorLine::Unsupported | ColorLine::Waiting => {
                spill.set_len(offset)?;
                spill.seek(SeekFrom::Start(offset))?;
                if self.parts.is_empty() { self.spill = None; }
                Ok(None)
            }
        }
    }

    fn write_to(&mut self, output: &mut impl Write) -> io::Result<()> {
        for part in &self.parts {
            match part {
                FramePart::Bytes(bytes) => output.write_all(bytes)?,
                FramePart::Spill { offset, length } => {
                    let spill = self.spill.as_mut().ok_or_else(|| io::Error::other("missing frame spill"))?;
                    spill.seek(SeekFrom::Start(*offset))?;
                    if io::copy(&mut spill.take(*length), output)? != *length { return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short frame spill")); }
                }
            }
        }
        output.write_all(&self.bytes)
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.parts.clear();
        self.spill = None;
    }

    fn position(&mut self, row: usize) {
        if std::mem::take(&mut self.retain_initial_column) { return; }
        if let Some(previous) = self.relative_row {
            let row = row.saturating_sub(self.padding);
            self.bytes.push(b'\r');
            if previous > row { let _ = write!(self.bytes, "\x1b[{}A", previous - row); }
            for _ in previous..row { self.bytes.extend_from_slice(b"\r\n"); }
            self.relative_row = Some(row);
        } else {
            terminal::move_cursor(&mut self.bytes, row as u16, 0);
        }
    }

    fn wrapped_row(&mut self, row: usize) {
        if self.relative_row.is_some() { self.relative_row = Some(row.saturating_sub(self.padding)); }
    }
}

#[derive(PartialEq, Clone)]
enum Mode {
    Normal,
    SearchInput,
    FilterInput,
    ExamineInput,
    Follow,
    Help,
    FileCommand(std::num::NonZeroUsize),
    QuitPrefix,
    SetMark,
    JumpMark,
    OptionPrefix(OptionCommand),
    OptionLong(OptionCommand, OptionName),
    OptionNotice { repaint: bool },
    CommandError(CommandError),
}

#[derive(Clone, Copy, PartialEq)]
enum OptionCommand { Toggle, ToggleQuietly, Reset, ResetQuietly, Set, SetQuietly, Query }

impl OptionCommand {
    fn modified(self, key: &Key) -> Option<Self> {
        use OptionCommand::*;
        match (self, key) {
            (Toggle, Key::Char('+')) | (Set, Key::Char('+')) => Some(Reset),
            (ToggleQuietly, Key::Char('+')) | (SetQuietly, Key::Char('+')) => Some(ResetQuietly),
            (Reset, Key::Char('+')) => Some(Toggle),
            (ResetQuietly, Key::Char('+')) => Some(ToggleQuietly),
            (Toggle, Key::Char('!')) | (Reset, Key::Char('!')) => Some(Set),
            (ToggleQuietly, Key::Char('!')) | (ResetQuietly, Key::Char('!')) => Some(SetQuietly),
            (Set, Key::Char('!')) => Some(Toggle),
            (SetQuietly, Key::Char('!')) => Some(ToggleQuietly),
            (Toggle, Key::Ctrl('p')) => Some(ToggleQuietly),
            (ToggleQuietly, Key::Ctrl('p')) => Some(Toggle),
            (Reset, Key::Ctrl('p')) => Some(ResetQuietly),
            (ResetQuietly, Key::Ctrl('p')) => Some(Reset),
            (Set, Key::Ctrl('p')) => Some(SetQuietly),
            (SetQuietly, Key::Ctrl('p')) => Some(Set),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq)]
enum OptionName { Typing(String), Raw, Color, LineNumbers, DisplayLineNumbers, Chop, ChopUpper, UseColor, UseColorUpper, IncrementalSearch, IncrementalSearchUpper }

#[derive(Clone, Copy)]
enum HelpPrefix { Colon(std::num::NonZeroUsize), Quit }

/// Whether standalone help has registered files after it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HelpFiles { None, Registered }

#[derive(PartialEq, Clone, Copy)]
enum SearchDir {
    Forward,
    Backward,
}

pub(crate) struct LineEditor {
    pub(crate) content: String,
    pub(crate) cursor: usize, // byte position
}

impl LineEditor {
    pub(crate) fn new() -> Self {
        LineEditor {
            content: String::new(),
            cursor: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
    }

    pub(crate) fn insert(&mut self, ch: char) {
        self.content.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.content[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.content.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub(crate) fn delete(&mut self) {
        if self.cursor < self.content.len() {
            let next = self.content[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.content.drain(self.cursor..self.cursor + next);
        }
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.content[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.cursor < self.content.len() {
            let next = self.content[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor += next;
        }
    }

    pub(crate) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.content.len();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum CommandContext { StartupUnpositioned, StartupPositioned, StartupAborted, Interactive }

#[derive(Clone, PartialEq)]
struct CommandError { action: ErrorAction, context: CommandContext, remainder: Vec<u8>, destination: ErrorDestination, repaint: bool }

#[derive(Clone, PartialEq)]
enum ErrorDestination { Normal, Help }

#[derive(Clone, PartialEq)]
enum ErrorAction { Retry(Key), Dismiss, SearchWrapped(bool) }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MarkName(u8);

#[derive(Clone, Copy, PartialEq, Eq)]
struct MarkPosition { line: usize, leading_rows: usize }

impl MarkName {
    fn new(character: char) -> Option<Self> {
        (character.is_ascii_alphabetic() || matches!(character, '\'' | '#')).then_some(Self(character as u8))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NumericPrefix { Empty, Value { value: usize, leading_zeroes: usize }, Overflow }

impl NumericPrefix {
    fn push(self, character: char) -> Self {
        let (value, leading_zeroes) = match self {
            Self::Empty => (0, 0),
            Self::Value { value, leading_zeroes } => (value, leading_zeroes),
            Self::Overflow => return Self::Overflow,
        };
        if value == 0 && character == '0' {
            return Self::Value { value, leading_zeroes: leading_zeroes.saturating_add(1) };
        }
        value.checked_mul(10).and_then(|value| value.checked_add(character as usize - '0' as usize))
            .filter(|value| *value <= i64::MAX as usize)
            .map_or(Self::Overflow, |value| Self::Value { value, leading_zeroes })
    }
}

#[derive(PartialEq, Eq)]
enum StartupRun { AllCommands, UntilOneScreen }

enum DisplayLine { Mapped(DisplayPrefix), Raw { fragment: LineFragment, tabs: TabStops } }

impl DisplayLine {
    fn complete(&self) -> bool { match self { Self::Mapped(prefix) => prefix.complete, Self::Raw { fragment, .. } => fragment.complete } }
    fn columns(&self) -> usize { match self {
        Self::Mapped(prefix) => prefix.columns,
        Self::Raw { fragment, tabs } => crate::ansi::raw_width(&fragment.text, tabs),
    } }
    fn fills_row(&self, start: usize, width: usize) -> bool { match self {
        Self::Mapped(prefix) => prefix.fills_row(start, width),
        Self::Raw { .. } => self.columns() >= start.saturating_add(width),
    } }
    fn left_clipped_columns(&self, start: usize) -> usize {
        match self { Self::Mapped(prefix) => prefix.left_clipped_columns(start), Self::Raw { .. } => 0 }
    }
    fn search_text<'a>(&self, visible: &'a str, start: usize) -> std::borrow::Cow<'a, str> {
        match self { Self::Mapped(prefix) => prefix.search_text(visible, start), Self::Raw { .. } => std::borrow::Cow::Borrowed(visible) }
    }
    fn matches_search(&self, search: &Search, controls: ControlDisplay) -> bool {
        let bytes = match self {
            Self::Mapped(prefix) => prefix.source(),
            Self::Raw { fragment, .. } => fragment.text.as_bytes(),
        };
        if !bytes.iter().any(|byte| *byte < 32 || *byte == 127) { return true; }
        let text = String::from_utf8_lossy(bytes);
        if controls == ControlDisplay::Standard { search.is_match_raw(&text) } else { search.is_match(&text) }
    }
    fn display(&self, start: usize, width: usize, notation_style: NotationStyle) -> Vec<u8> { match self {
        Self::Mapped(prefix) => prefix.display(start, width, notation_style),
        Self::Raw { fragment, tabs } => crate::ansi::truncate_raw(&fragment.text, start, width, tabs).into_bytes(),
    } }
}

/// The result of probing whether a source fits in a terminal viewport.
pub enum OneScreen { Fits(OneScreenOutput), Overflows, Waiting }

/// Minimum width reserved for a line number.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LineNumberWidth(u8);

impl LineNumberWidth {
    pub const MAX: u8 = 16;
    pub fn new(width: u8) -> Option<Self> { (width <= Self::MAX).then_some(Self(width)) }
    fn columns(self) -> usize { usize::from(self.0) }
}

impl Default for LineNumberWidth {
    fn default() -> Self { Self(7) }
}

enum OneScreenStorage { Memory(Vec<u8>), File(std::fs::File) }

/// Output retained by a successful one-screen probe.
pub struct OneScreenOutput { storage: OneScreenStorage }

impl OneScreenOutput {
    fn new() -> Self { Self { storage: OneScreenStorage::Memory(vec![b'\r']) } }

    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &mut self.storage {
            OneScreenStorage::Memory(output) => { output.extend_from_slice(bytes); Ok(()) }
            OneScreenStorage::File(output) => output.write_all(bytes),
        }
    }

    fn append_file(&mut self, mut input: std::fs::File) -> io::Result<()> {
        if let OneScreenStorage::Memory(bytes) = &mut self.storage {
            let mut output = crate::storage::anonymous_file()?;
            output.write_all(bytes)?;
            self.storage = OneScreenStorage::File(output);
        }
        input.rewind()?;
        let OneScreenStorage::File(output) = &mut self.storage else { unreachable!() };
        io::copy(&mut input, output)?;
        Ok(())
    }

    /// Write the retained screen bytes without materializing a spilled line in memory.
    pub fn write_to(mut self, output: &mut impl Write) -> io::Result<()> {
        match &mut self.storage {
            OneScreenStorage::Memory(bytes) => output.write_all(bytes),
            OneScreenStorage::File(input) => { input.rewind()?; io::copy(input, output)?; Ok(()) }
        }
    }
}

/// Display settings used while probing a source before acquiring the alternate screen.
pub struct OneScreenOptions {
    pub width: u16,
    pub height: u16,
    pub charset: Charset,
    pub controls: ControlDisplay,
    /// Select the tested UI color classes.
    pub use_color: bool,
    pub chop: bool,
    pub line_numbers: bool,
    pub line_number_width: LineNumberWidth,
    pub tab_stops: TabStops,
}

fn is_unbound_ascii(character: char) -> bool {
    matches!(character, '"' | '$' | '*' | ',' | ';' | '@' | 'A' | 'B' | 'C' | 'D' | 'I' | 'L' | 'O' | 'S' | 'U' | 'W' | 'X' | '\\' | '^' | '`' | 'a' | 'c' | 'i' | 'l' | 'o' | 'x' | '~')
}

fn number_gutter(line: usize, width: usize, enabled: bool, minimum: LineNumberWidth) -> String {
    if !enabled { return String::new(); }
    format!("{:>columns$} ", line + 1, columns = minimum.columns()).chars().take(width).collect()
}

enum SgrScan { Text, Escape, Sequence, OscCommand, OscSeparator, OscParams, OscUri, OscEscape, Utf8 { bytes: [u8; 4], len: usize, width: usize } }
enum ColorLine { Fits(bool), Unsupported, Waiting }
enum StreamedLine { Fits(std::fs::File, bool), Unsupported, Waiting }

fn scan_color_line(buffer: &mut LineBuffer, line: usize, width: usize, charset: Charset, output: &mut impl Write) -> io::Result<ColorLine> {
    use unicode_width::UnicodeWidthChar;
    let mut offset = 0u64;
    let mut columns = 0usize;
    let mut sgr = false;
    let mut state = SgrScan::Text;
    loop {
        if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return Ok(ColorLine::Waiting); }
        if terminal::interrupt_pending() { return Ok(ColorLine::Unsupported); }
        let Some(chunk) = buffer.byte_chunk(line, offset, 64 * 1024)? else {
            if let Some(error) = buffer.take_error() { return Err(error); }
            return Ok(ColorLine::Waiting);
        };
        for &byte in &chunk.bytes {
            state = match state {
                SgrScan::Text if byte == 0x1b => SgrScan::Escape,
                SgrScan::Text if (0x20..=0x7e).contains(&byte) => {
                    columns += 1;
                    if columns > width { return Ok(ColorLine::Unsupported); }
                    SgrScan::Text
                }
                SgrScan::Text if charset == Charset::Utf8 && matches!(byte, 0xc2..=0xf4) => {
                    let width = match byte { 0xc2..=0xdf => 2, 0xe0..=0xef => 3, _ => 4 };
                    SgrScan::Utf8 { bytes: [byte, 0, 0, 0], len: 1, width }
                }
                SgrScan::Utf8 { mut bytes, len, width: expected } if (0x80..=0xbf).contains(&byte) => {
                    bytes[len] = byte;
                    if len + 1 < expected { SgrScan::Utf8 { bytes, len: len + 1, width: expected } }
                    else {
                        let Ok(text) = std::str::from_utf8(&bytes[..expected]) else { return Ok(ColorLine::Unsupported); };
                        let character = text.chars().next().unwrap();
                        if character.is_control() || charset.binary_character(character) { return Ok(ColorLine::Unsupported); }
                        let Some(character_width) = character.width() else { return Ok(ColorLine::Unsupported); };
                        columns += character_width;
                        if columns > width { return Ok(ColorLine::Unsupported); }
                        SgrScan::Text
                    }
                }
                SgrScan::Escape if byte == b'[' => SgrScan::Sequence,
                SgrScan::Escape if byte == b']' => SgrScan::OscCommand,
                SgrScan::Sequence if byte == b'm' => { sgr = true; SgrScan::Text }
                SgrScan::Sequence if b"0123456789:;[?!\"'#%()*+ ".contains(&byte) => SgrScan::Sequence,
                SgrScan::OscCommand if byte == b'8' => SgrScan::OscSeparator,
                SgrScan::OscSeparator if byte == b';' => SgrScan::OscParams,
                SgrScan::OscParams if byte == b';' => SgrScan::OscUri,
                SgrScan::OscParams if (0x20..=0x7e).contains(&byte) => SgrScan::OscParams,
                SgrScan::OscUri if byte == 0x07 => SgrScan::Text,
                SgrScan::OscUri if byte == 0x1b => SgrScan::OscEscape,
                SgrScan::OscUri if (0x20..=0x7e).contains(&byte) => SgrScan::OscUri,
                SgrScan::OscEscape if byte == b'\\' => SgrScan::Text,
                _ => return Ok(ColorLine::Unsupported),
            };
        }
        output.write_all(&chunk.bytes)?;
        offset += chunk.bytes.len() as u64;
        if chunk.complete {
            return Ok(if matches!(state, SgrScan::Text) { ColorLine::Fits(sgr) } else { ColorLine::Unsupported });
        }
        if chunk.bytes.is_empty() {
            if let Some(error) = buffer.take_error() { return Err(error); }
            return Ok(ColorLine::Waiting);
        }
    }
}

fn stream_color_line(buffer: &mut LineBuffer, line: usize, width: usize, charset: Charset) -> io::Result<StreamedLine> {
    let mut file = crate::storage::anonymous_file()?;
    match scan_color_line(buffer, line, width, charset, &mut file)? {
        ColorLine::Fits(sgr) => Ok(StreamedLine::Fits(file, sgr)),
        ColorLine::Unsupported => Ok(StreamedLine::Unsupported),
        ColorLine::Waiting => Ok(StreamedLine::Waiting),
    }
}

/// Read bounded display prefixes and retain the rendered output when the source fits.
pub fn one_screen(buffer: &mut LineBuffer, options: OneScreenOptions) -> io::Result<OneScreen> {
    let width = usize::from(options.width).max(1);
    let height = usize::from(options.height).max(1);
    let mut output = OneScreenOutput::new();
    let mut rows = 0;
    for index in 0..height {
        let gutter = number_gutter(index, width, options.line_numbers, options.line_number_width);
        let content_width = width.saturating_sub(gutter.len()).max(1);
        let limit = width.saturating_mul(height - rows).saturating_add(1).saturating_mul(16).max(4096);
        let fragment = if options.controls == ControlDisplay::Raw {
            buffer.line_prefix(index, limit)?.map(|fragment| DisplayLine::Raw { fragment, tabs:options.tab_stops.clone() })
        } else {
            let wrap = (!options.chop).then(|| std::num::NonZeroUsize::new(content_width).unwrap());
            buffer.byte_prefix(index, limit)?.map(|fragment| DisplayLine::Mapped(DisplayPrefix::new(fragment.bytes, options.charset, fragment.complete, wrap, options.controls, &options.tab_stops)))
        };
        let Some(fragment) = fragment else {
            if let Some(error) = buffer.take_error() { return Err(error); }
            if !buffer.is_finished() { return Ok(OneScreen::Waiting); }
            output.append(b"\r\x1b[K")?;
            return Ok(OneScreen::Fits(output));
        };
        if options.chop && fragment.columns() > content_width { return Ok(OneScreen::Overflows); }
        let line_rows = if options.chop { 1 } else { fragment.columns().div_ceil(content_width).max(1) };
        if rows.saturating_add(line_rows) >= height { return Ok(OneScreen::Overflows); }
        if !fragment.complete() {
            if options.controls == ControlDisplay::Color && fragment.columns() <= content_width {
                match stream_color_line(buffer, index, content_width, options.charset)? {
                    StreamedLine::Fits(file, sgr) => {
                        let mut number = Vec::new();
                        Pager::write_number_gutter(&mut number, &gutter, options.use_color);
                        output.append(&number)?;
                        output.append_file(file)?;
                        if sgr { output.append(b"\x1b[m")?; }
                        output.append(b"\r\n")?;
                        rows += 1;
                        continue;
                    }
                    StreamedLine::Waiting => return Ok(OneScreen::Waiting),
                    StreamedLine::Unsupported => {}
                }
            }
            return Ok(if buffer.state() == crate::buffer::SourceState::Waiting { OneScreen::Waiting } else { OneScreen::Overflows });
        }
        for row in 0..line_rows {
            let mut number = Vec::new();
            Pager::write_number_gutter(&mut number, &gutter, options.use_color);
            output.append(&number)?;
            output.append(&fragment.display(row * content_width, width.saturating_sub(gutter.len()), NotationStyle::for_ui_color(options.use_color)))?;
            if row + 1 < line_rows && fragment.fills_row(row * content_width, content_width) { output.append(b" \x08")?; }
            else { output.append(b"\r\n")?; }
        }
        rows += line_rows;
    }
    Ok(OneScreen::Overflows)
}

#[derive(Clone)]
pub enum PagerExit { Quit, Interrupted, VisitFile(FileVisit), Examine(std::ffi::OsString) }

/// Select default continuation or exit status 2 after an interrupt.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum InterruptPolicy { #[default] Continue, Quit }

/// Select when a displayed end of file causes a file transition or exit.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum EofPolicy { #[default] Manual, AfterEof, AtEof }

/// A file index validated against the configured command-line file list.
#[derive(Clone, Copy)]
pub struct FileVisit { index: usize, startup_command: Option<StartupFileSwitch> }

#[derive(Clone, Copy, PartialEq)]
enum FileCommand { Next, Previous }

impl FileCommand {
    fn key(self) -> u8 { match self { Self::Next => b'n', Self::Previous => b'p' } }
}

impl FileVisit {
    pub fn index(self) -> usize { self.index }

    pub fn initial_display(self, position: Option<SavedPosition>) -> InitialDisplay {
        match self.startup_command {
            Some(switch) if position.is_none() => InitialDisplay::StartupFileSwitch(switch),
            _ => InitialDisplay::FileSwitch(position),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct StartupFileSwitch { command: FileCommand, continuation: CommandContinuation }

#[derive(Clone, Copy, PartialEq)]
enum CommandContinuation { Inline, NextCommand }

#[derive(Clone, Copy, PartialEq)]
pub enum InitialDisplay { Startup, StartupFileSwitch(StartupFileSwitch), FileSwitch(Option<SavedPosition>) }

#[derive(Clone, Copy)]
struct ScreenFraction(u32);

impl ScreenFraction {
    fn parse(digits: &[u8]) -> Option<Self> {
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) { return None; }
        let mut fraction = 0u32;
        for &digit in digits.iter().take(6) { fraction = fraction * 10 + u32::from(digit - b'0'); }
        for _ in digits.len().min(6)..6 { fraction *= 10; }
        Some(Self(fraction))
    }

    fn scale(self, dimension: u16) -> usize {
        let product = u64::from(dimension) * u64::from(self.0);
        let whole = product / 1_000_000;
        let remainder = product % 1_000_000;
        (whole + u64::from(remainder > 500_000 || (remainder == 500_000 && whole % 2 != 0))) as usize
    }
}

#[derive(Clone, Copy)]
enum ShiftValue { Default, Fixed(std::num::NonZeroUsize), Fraction(ScreenFraction) }

/// A validated horizontal shift, expressed in columns or as a fraction of the screen.
#[derive(Clone, Copy)]
pub struct HorizontalShift(ShiftValue);

impl HorizontalShift {
    /// Parse the less-compatible integer or decimal-fraction form.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if let Some(digits) = bytes.strip_prefix(b".") {
            return Some(Self(ShiftValue::Fraction(ScreenFraction::parse(digits)?)));
        }
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) { return None; }
        let mut columns = 0u32;
        for &digit in bytes { columns = columns.checked_mul(10)?.checked_add(u32::from(digit - b'0'))?; }
        if columns > i32::MAX as u32 { return None; }
        Some(Self(std::num::NonZeroUsize::new(columns as usize).map_or(ShiftValue::Default, ShiftValue::Fixed)))
    }
}

#[derive(Clone, Copy, Default)]
enum JumpRow { #[default] Default, Line(i32), Fraction(ScreenFraction) }

/// A validated screen row for positioning line and search targets.
#[derive(Clone, Copy, Default)]
pub struct JumpTarget(JumpRow);

impl JumpTarget {
    /// Parse an integer screen row or a fraction of terminal height.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if let Some(digits) = bytes.strip_prefix(b".") { return Some(Self(JumpRow::Fraction(ScreenFraction::parse(digits)?))); }
        let text = std::str::from_utf8(bytes).ok()?;
        if text.is_empty() || text.starts_with('+') { return None; }
        let line: i32 = text.parse().ok()?;
        if line == i32::MIN { return None; }
        Some(Self(JumpRow::Line(line)))
    }

    fn row(self, height: u16) -> usize {
        let line = match self.0 {
            JumpRow::Default => 1i64,
            JumpRow::Line(line) => i64::from(line),
            JumpRow::Fraction(fraction) => fraction.scale(height) as i64,
        };
        line.clamp(1, i64::from(height).max(1)) as usize - 1
    }
}

/// Numeric scrolling defaults belong to the terminal session, across file visits.
#[derive(Clone, Copy)]
pub struct ScrollDefaults {
    half_page: std::num::NonZeroUsize,
    horizontal: Option<std::num::NonZeroUsize>,
    horizontal_fraction: Option<ScreenFraction>,
    window: WindowSize,
}

#[derive(Clone, Copy)]
enum WindowSize { Fixed(std::num::NonZeroUsize), ScreenMinus(u32) }

impl WindowSize {
    fn lines(self, terminal_height: u16) -> usize {
        match self {
            Self::Fixed(lines) => lines.get(),
            Self::ScreenMinus(lines) => usize::from(terminal_height).saturating_sub(lines as usize),
        }
    }
}

impl ScrollDefaults {
    pub fn new(terminal_height: u16) -> Self {
        Self { half_page: std::num::NonZeroUsize::new(usize::from(terminal_height).div_ceil(2).max(1)).unwrap(), horizontal: None, horizontal_fraction: None, window: WindowSize::ScreenMinus(1) }
    }

    /// Set the default horizontal shift; fractional settings follow terminal resizes.
    pub fn set_horizontal_shift(&mut self, shift: HorizontalShift) {
        (self.horizontal, self.horizontal_fraction) = match shift.0 {
            ShiftValue::Default => (None, None),
            ShiftValue::Fixed(columns) => (Some(columns), None),
            ShiftValue::Fraction(fraction) => (None, Some(fraction)),
        };
    }

    fn horizontal_step(self, width: u16) -> usize {
        let fraction = self.horizontal_fraction.map(|fraction| fraction.scale(width)).filter(|columns| *columns > 0);
        self.horizontal.map(std::num::NonZeroUsize::get).or(fraction).unwrap_or_else(|| (usize::from(width) / 2).max(1))
    }

    fn resize(&mut self, height: u16) {
        self.half_page = std::num::NonZeroUsize::new(usize::from(height).div_ceil(2).max(1)).unwrap();
        if self.horizontal_fraction.is_some() { self.horizontal = None; }
    }

    /// Positive settings select rows; nonpositive settings subtract from the current terminal height.
    pub fn set_window(&mut self, setting: i32) {
        self.window = if setting > 0 { WindowSize::Fixed(std::num::NonZeroUsize::new(setting as usize).unwrap()) }
            else { WindowSize::ScreenMinus(setting.unsigned_abs()) };
    }
}

/// A saved source line; wrapped-row and horizontal offsets are not represented.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SavedPosition { line: usize }

pub struct FileStatus { index: usize, count: usize, next_name: Option<String> }

impl FileStatus {
    /// Build file-list prompt context for a zero-based index in a nonempty list.
    pub fn new(names: &[std::ffi::OsString], index: usize, charset: Charset) -> io::Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        if index >= names.len() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "file index is outside the file list")); }
        let next_name = names.get(index + 1).map(|name| {
            let mut bytes = Vec::new();
            charset.write_filename(name.as_bytes(), &mut bytes)?;
            String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }).transpose()?;
        Ok(Self { index, count: names.len(), next_name })
    }
}

/// Search pattern and direction retained between files in one session.
pub struct SearchSession { search: Option<Search>, direction: SearchDir, highlight: bool, nonmatch: bool, wrap: bool }

impl Default for SearchSession {
    fn default() -> Self { Self { search: None, direction: SearchDir::Forward, highlight: true, nonmatch: false, wrap: false } }
}

/// Selected line-number option state; not all counting behavior is implemented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineNumberMode {
    /// The no-line-number option state.
    Off,
    /// The default line-number option state.
    #[default]
    Enabled,
    /// Display line numbers beside each line.
    Displayed,
}

impl LineNumberMode {
    /// Return whether numbers are displayed beside each line.
    pub fn displayed(self) -> bool { self == Self::Displayed }
}

pub struct Config {
    pub interrupt_policy: InterruptPolicy,
    pub eof_policy: EofPolicy,
    pub bell_policy: terminal::BellPolicy,
    pub scroll_defaults: ScrollDefaults,
    pub display_anchor: DisplayAnchor,
    pub files: FileStatus,
    pub initial_display: InitialDisplay,
    pub startup_commands: Vec<Vec<u8>>,
    pub charset: Charset,
    pub controls: ControlDisplay,
    /// Select UI colors; prompt, notice and tested search colors are implemented.
    pub use_color: bool,
    /// Enable incremental search while editing a search pattern.
    pub incremental_search: bool,
    /// Allow interactive examine-file commands.
    pub allow_examine: bool,
    pub chop: bool,
    pub line_numbers: LineNumberMode,
    pub line_number_width: LineNumberWidth,
    pub tab_stops: TabStops,
    pub show_eof_markers: bool,
    pub search_case: CaseSensitivity,
    /// Character interpretation used for regular expressions.
    pub regex_locale: RegexLocale,
    pub search_start: SearchStart,
    pub search_session: SearchSession,
    pub jump_target: JumpTarget,
}

pub struct Pager {
    interrupt_policy: InterruptPolicy,
    eof_policy: EofPolicy,
    bell_policy: terminal::BellPolicy,
    eof_displayed: bool,
    startup_echo: Option<FileCommand>,
    paint_before_commands: bool,
    scroll_defaults: ScrollDefaults,
    display_anchor: DisplayAnchor,
    buffer: LineBuffer,
    top_line: usize,
    initial_position: Option<SavedPosition>,
    marks: HashMap<MarkName, MarkPosition>,
    left_col: usize,
    wrap: bool,
    line_numbers: LineNumberMode,
    line_number_width: LineNumberWidth,
    tab_stops: TabStops,
    show_eof_markers: bool,
    search: Option<Search>,
    search_inherited: bool,
    search_highlight: bool,
    search_nonmatch: bool,
    search_input_nonmatch: bool,
    search_input_keep_position: bool,
    search_input_wrap: bool,
    search_wrap: bool,
    incremental_search: bool,
    incremental_preview: Option<Search>,
    search_case: CaseSensitivity,
    regex_locale: RegexLocale,
    search_start: SearchStart,
    jump_target: JumpTarget,
    last_visible_line: Option<usize>,
    search_dir: SearchDir,
    current_match_line: Option<usize>,
    mode: Mode,
    help_top: usize,
    help_rows: usize,
    help_prefix: Option<HelpPrefix>,
    standalone_help: Option<HelpFiles>,
    search_input: LineEditor,
    filter_input: LineEditor,
    examine_input: LineEditor,
    allow_examine: bool,
    restore_after_failed_examine: bool,
    filter: Option<Search>,
    filtered_lines: Vec<usize>,
    history: SearchHistory,
    term_width: u16,
    term_height: u16,
    charset: Charset,
    controls: ControlDisplay,
    use_color: bool,
    status_msg: Option<String>,
    filename: Option<String>,
    filename_prompt: bool,
    compact_initial: bool,
    squished: bool,
    bottom_aligned: bool,
    leading_rows: usize,
    forced_leading_rows: bool,
    end_mark_from: Option<MarkPosition>,
    follow_one_screen: bool,
    follow_screen_overflowed: bool,
    bottom_gap: usize,
    startup_commands: VecDeque<Vec<u8>>,
    numeric_prefix: NumericPrefix,
    content_painted: bool,
    files: FileStatus,
    exit: PagerExit,
    command_context: CommandContext,
    bell_requested: bool,
    boundary_bell_requested: bool,
    early_bells: usize,
}

impl Pager {
    pub fn new(
        buffer: LineBuffer,
        config: Config,
        filename: Option<String>,
        history: SearchHistory,
    ) -> Self {
        let (w, h) = terminal::get_terminal_size();
        let mut display_anchor = config.display_anchor;
        display_anchor.position_capacity = display_anchor.position_capacity.max(h);
        let search_inherited = config.search_session.search.is_some();
        Pager {
            interrupt_policy: config.interrupt_policy,
            eof_policy: config.eof_policy,
            bell_policy: config.bell_policy,
            eof_displayed: false,
            startup_echo: match config.initial_display { InitialDisplay::StartupFileSwitch(switch) => Some(switch.command), _ => None },
            paint_before_commands: matches!(config.initial_display, InitialDisplay::StartupFileSwitch(StartupFileSwitch { continuation: CommandContinuation::NextCommand, .. })) && !config.startup_commands.is_empty(),
            scroll_defaults: config.scroll_defaults,
            display_anchor,
            buffer,
            top_line: match config.initial_display { InitialDisplay::FileSwitch(Some(position)) => position.line, _ => 0 },
            initial_position: match config.initial_display { InitialDisplay::FileSwitch(position) => position, _ => None },
            marks: HashMap::new(),
            left_col: 0,
            wrap: !config.chop,
            line_numbers: config.line_numbers,
            line_number_width: config.line_number_width,
            tab_stops: config.tab_stops,
            show_eof_markers: config.show_eof_markers,
            search: config.search_session.search,
            search_inherited,
            search_highlight: config.search_session.highlight,
            search_nonmatch: config.search_session.nonmatch,
            search_input_nonmatch: false,
            search_input_keep_position: false,
            search_input_wrap: false,
            search_wrap: config.search_session.wrap,
            incremental_search: config.incremental_search,
            incremental_preview: None,
            search_case: config.search_case,
            regex_locale: config.regex_locale,
            search_start: config.search_start,
            jump_target: config.jump_target,
            last_visible_line: None,
            search_dir: config.search_session.direction,
            current_match_line: None,
            mode: Mode::Normal,
            help_top: 0,
            help_rows: 0,
            help_prefix: None,
            standalone_help: None,
            search_input: LineEditor::new(),
            filter_input: LineEditor::new(),
            examine_input: LineEditor::new(),
            allow_examine: config.allow_examine,
            restore_after_failed_examine: false,
            filter: None,
            filtered_lines: Vec::new(),
            history,
            term_width: w,
            term_height: h,
            charset: config.charset,
            controls: config.controls,
            use_color: config.use_color,
            status_msg: None,
            filename,
            filename_prompt: true,
            compact_initial: config.initial_display == InitialDisplay::Startup && config.startup_commands.is_empty(),
            squished: false,
            bottom_aligned: false,
            leading_rows: 0,
            forced_leading_rows: false,
            end_mark_from: None,
            follow_one_screen: false,
            follow_screen_overflowed: false,
            bottom_gap: 0,
            startup_commands: config.startup_commands.into(),
            numeric_prefix: NumericPrefix::Empty,
            content_painted: false,
            files: config.files,
            exit: PagerExit::Quit,
            command_context: CommandContext::StartupUnpositioned,
            bell_requested: false,
            boundary_bell_requested: false,
            early_bells: 0,
        }
    }

    /// Whether one-screen follow grew beyond the viewport before it was interrupted.
    pub fn follow_screen_overflowed(&self) -> bool { self.follow_screen_overflowed }

    /// Whether the initial short-file display has not yet been repositioned.
    pub fn is_squished(&self) -> bool { self.squished }

    /// Retain the first displayed source line for a later visit to this file.
    pub fn saved_position(&self) -> Option<SavedPosition> {
        if !self.content_painted { return None; }
        self.display_line_idx(self.top_line).filter(|line| *line < self.buffer.len()).map(|line| SavedPosition { line })
    }

    pub fn display_anchor(&self) -> DisplayAnchor { self.display_anchor }

    pub fn scroll_defaults(&self) -> ScrollDefaults { self.scroll_defaults }

    /// Return the control-display mode to carry to the next file.
    pub fn control_display(&self) -> ControlDisplay { self.controls }

    /// Return the selected pager UI color mode.
    pub fn use_color(&self) -> bool { self.use_color }
    /// Return whether interactive incremental search is enabled.
    pub fn incremental_search(&self) -> bool { self.incremental_search }

    /// Return the selected line-number option state.
    pub fn line_numbers(&self) -> LineNumberMode { self.line_numbers }

    /// Return whether long lines are chopped.
    pub fn chop(&self) -> bool { !self.wrap }

    /// Transfer the active search to the next file in the session.
    pub fn take_search_session(&mut self) -> SearchSession { SearchSession { search:self.search.take(), direction:self.search_dir, highlight:self.search_highlight, nonmatch:self.search_nonmatch, wrap:self.search_wrap } }

    /// Transfer startup input that remains after a file switch to the next pager.
    pub fn take_startup_commands(&mut self) -> Vec<Vec<u8>> { self.startup_commands.drain(..).collect() }

    pub fn into_history(mut self) -> SearchHistory {
        if self.content_painted && !self.buffer.is_empty() { self.history.note_position(); }
        self.history
    }

    fn content_height(&self) -> usize {
        if self.term_height > 1 {
            (self.term_height - 1) as usize
        } else {
            1
        }
    }

    fn run_startup_commands(&mut self, keys: &mut KeyReader, policy: StartupRun) -> io::Result<bool> {
        while let Some(command) = self.startup_commands.pop_front() {
            keys.set_command_input(command);
            while keys.has_command_input() {
                if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return Ok(true); }
                if terminal::take_interrupt() { return Ok(self.interrupt(keys)); }
                if self.handle_key(keys.read_key()?) {
                    if let PagerExit::VisitFile(visit) = &mut self.exit {
                        let remainder = keys.take_command_input();
                        if !remainder.is_empty() {
                            if let Some(switch) = &mut visit.startup_command { switch.continuation = CommandContinuation::Inline; }
                            self.startup_commands.push_front(remainder);
                        }
                    }
                    return Ok(true);
                }
                if matches!(self.mode, Mode::CommandError(_)) { break; }
                if self.command_context == CommandContext::StartupAborted { keys.set_command_input(Vec::new()); break; }
            }
            if let Mode::CommandError(error) = &mut self.mode {
                error.remainder = keys.take_command_input();
                return Ok(false);
            }
            if self.command_context == CommandContext::StartupAborted { self.command_context = CommandContext::StartupUnpositioned; continue; }
            if matches!(self.mode, Mode::SearchInput | Mode::FilterInput) { self.handle_key(Key::Enter); }
            else if self.numeric_prefix != NumericPrefix::Empty { self.handle_normal_key(Key::Char('g')); }
            if matches!(self.mode, Mode::CommandError(_)) { return Ok(false); }
            if policy == StartupRun::UntilOneScreen && self.mode == Mode::Normal && self.top_line == 0 && self.filter.is_none() {
                self.startup_commands.clear();
                break;
            }
            if !self.startup_commands.is_empty() {
                self.filename_prompt = !self.content_painted;
                self.render()?;
                self.command_context = CommandContext::Interactive;
            }
        }
        self.filename_prompt = !self.content_painted;
        self.command_context = CommandContext::Interactive;
        Ok(false)
    }

    /// Execute startup commands before deciding whether a one-screen source can exit.
    pub fn run_one_screen_startup(&mut self, keys: &mut KeyReader) -> io::Result<PagerExit> {
        if self.run_startup_commands(keys, StartupRun::UntilOneScreen)? { return self.finish_run(); }
        self.flush_early_bell()?;
        if self.mode == Mode::Normal && self.top_line == 0 && self.filter.is_none() {
            self.render_frame(false)?;
            return self.finish_run();
        }
        if self.mode == Mode::Follow {
            self.bottom_aligned = true;
            self.follow_one_screen = true;
        }
        self.run(keys)
    }

    /// Display the help document as the initial view, exiting when it is closed.
    pub fn run_help(&mut self, keys: &mut KeyReader, files: HelpFiles) -> io::Result<PagerExit> {
        self.mode = Mode::Help;
        self.standalone_help = Some(files);
        let width = usize::from(self.term_width).max(1);
        self.help_rows = Self::help_lines().enumerate().map(|(index, line)| self.help_line(index, line, width).3).sum();
        self.run(keys)
    }

    pub fn run(&mut self, keys: &mut KeyReader) -> io::Result<PagerExit> {
        if std::mem::take(&mut self.paint_before_commands) {
            self.render()?;
            self.command_context = CommandContext::Interactive;
        }
        if self.run_startup_commands(keys, StartupRun::AllCommands)? { return self.finish_run(); }

        let mut redraw = true;
        loop {
            if self.command_context != CommandContext::Interactive && !matches!(self.mode, Mode::CommandError(_)) {
                if self.run_startup_commands(keys, StartupRun::AllCommands)? { return self.finish_run(); }
                redraw = true;
            }
            if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 {
                return self.finish_run();
            }
            if terminal::take_interrupt() {
                if self.interrupt(keys) { return self.finish_run(); }
                redraw = true;
            }
            if self.finish_follow_one_screen()? { return self.finish_run(); }
            if terminal::TSTP_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                terminal::handle_suspend()?;
            }
            if terminal::CONT_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst)
                && terminal::handle_resume()? == terminal::ResumeOutcome::Restored {
                self.filename_prompt = false;
                let (w, h) = terminal::get_terminal_size();
                if (w, h) != (self.term_width, self.term_height) { self.scroll_defaults.resize(h); }
                self.resize_position_table(h);
                self.term_width = w;
                self.term_height = h;
                redraw = true;
                continue;
            }
            if terminal::WINCH_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.filename_prompt = false;
                let (w, h) = terminal::get_terminal_size();
                if (w, h) != (self.term_width, self.term_height) { self.scroll_defaults.resize(h); }
                self.resize_position_table(h);
                self.term_width = w;
                self.term_height = h;
                self.leading_rows = 0;
                redraw = true;
                continue;
            }

            self.flush_early_bell()?;
            if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return self.finish_run(); }
            if self.buffer.refresh_waiting() { redraw = true; }
            if let Some(error) = self.buffer.take_error() { return Err(error); }
            if redraw {
                self.render()?;
                redraw = false;
                if self.eof_policy == EofPolicy::AtEof && self.eof_displayed && self.files.index + 1 == self.files.count && self.mode == Mode::Normal {
                    return self.finish_run();
                }
            }

            if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { return self.finish_run(); }
            if terminal::interrupt_pending() || terminal::TSTP_FLAG.load(std::sync::atomic::Ordering::SeqCst)
                || terminal::CONT_FLAG.load(std::sync::atomic::Ordering::SeqCst) || terminal::WINCH_FLAG.load(std::sync::atomic::Ordering::SeqCst) { continue; }

            let key = if self.mode == Mode::Follow {
                let new_lines = self.buffer.poll_new_lines();
                if new_lines > 0 {
                    self.leading_rows = 0;
                    self.scroll_to_bottom();
                    self.bottom_aligned = true;
                    redraw = true;
                }
                match keys.read_key_timeout(100)? {
                    Some(k) => k,
                    None => continue,
                }
            } else {
                match keys.read_key_timeout(100)? { Some(key) => key, None => continue }
            };

            redraw = true;
            let previous_top = self.top_line;
            let quit_requested = if key == Key::Ctrl('c') { self.interrupt(keys) } else { self.handle_key(key) };
            if self.top_line != previous_top { self.squished = false; }
            if !quit_requested && self.finish_follow_one_screen()? { return self.finish_run(); }
            if quit_requested {
                return self.finish_run();
            }
        }
    }

    fn finish_follow_one_screen(&mut self) -> io::Result<bool> {
        if !self.follow_one_screen || self.mode != Mode::Normal { return Ok(false); }
        self.follow_one_screen = false;
        let options = OneScreenOptions { width:self.term_width, height:self.term_height, charset:self.charset, controls:self.controls, use_color:self.use_color, chop:!self.wrap, line_numbers:self.line_numbers.displayed(), line_number_width:self.line_number_width, tab_stops:self.tab_stops.clone() };
        match one_screen(&mut self.buffer, options)? {
            OneScreen::Fits(_) => {},
            OneScreen::Overflows => { self.follow_screen_overflowed = true; return Ok(false); },
            OneScreen::Waiting => return Ok(false),
        }
        self.bottom_aligned = true;
        self.bottom_gap = 1;
        self.render_frame(false)?;
        Ok(true)
    }

    fn flush_early_bell(&mut self) -> io::Result<()> {
        let count = std::mem::take(&mut self.early_bells);
        if count > 0 {
            let mut output = io::stdout().lock();
            for _ in 0..count {
                self.bell_policy.write_to(&mut output, terminal::BellKind::General)?;
                if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 { break; }
            }
            output.flush()?;
        }
        Ok(())
    }

    fn finish_run(&mut self) -> io::Result<PagerExit> {
        self.flush_early_bell()?;
        if matches!(self.exit, PagerExit::Interrupted) && std::mem::take(&mut self.bell_requested) {
            let mut output = io::stdout().lock();
            self.bell_policy.write_to(&mut output, terminal::BellKind::General)?;
            if self.follow_one_screen { output.write_all(b"\r\x1b[K")?; }
            output.flush()?;
        }
        Ok(std::mem::replace(&mut self.exit, PagerExit::Quit))
    }

    /// Report an examine-file failure while keeping the current pager active.
    pub fn examine_error(&mut self, message: String) {
        let repaint = self.squished;
        if repaint { self.position_initialized(); }
        self.restore_after_failed_examine = true;
        self.command_error(message, ErrorAction::Dismiss);
        if let Mode::CommandError(error) = &mut self.mode { error.repaint = repaint; }
    }

    fn interrupt(&mut self, keys: &mut KeyReader) -> bool {
        if self.interrupt_policy == InterruptPolicy::Quit {
            match self.mode {
                Mode::SearchInput => self.handle_search_input_key(Key::Ctrl('c')),
                Mode::FilterInput => self.handle_filter_input_key(Key::Ctrl('c')),
                Mode::ExamineInput => { self.mode = Mode::Normal; },
                _ => {},
            }
            self.bell_requested = true;
            self.exit = PagerExit::Interrupted;
            return true;
        }
        if self.filter.is_some() {
            let line = self.display_line_idx(self.top_line).unwrap_or(0);
            self.clear_filter();
            self.top_line = line;
        }
        keys.set_command_input(Vec::new());
        self.startup_commands.clear();
        self.startup_echo = None;
        self.numeric_prefix = NumericPrefix::Empty;
        self.command_context = CommandContext::Interactive;
        self.bell_requested = true;
        if matches!(self.mode, Mode::SetMark | Mode::JumpMark | Mode::FileCommand(_) | Mode::OptionPrefix(_) | Mode::OptionLong(_, _) | Mode::OptionNotice { .. } | Mode::CommandError(_)) {
            self.mode = if matches!(&self.mode, Mode::CommandError(error) if error.destination == ErrorDestination::Help) { Mode::Help } else { Mode::Normal };
            self.status_msg = None;
            self.filename_prompt = false;
            false
        } else {
            let filename_prompt = self.filename_prompt;
            let quit = self.handle_key(Key::Ctrl('c'));
            if !self.content_painted { self.filename_prompt = filename_prompt; }
            quit
        }
    }

    fn apply_control_option(&mut self, command: OptionCommand, choice: char) {
        let selected = if choice == 'r' { ControlDisplay::Raw } else { ControlDisplay::Color };
        self.controls = match command {
            OptionCommand::Toggle | OptionCommand::ToggleQuietly => if self.controls == selected { ControlDisplay::Standard } else { selected },
            OptionCommand::Reset | OptionCommand::ResetQuietly => ControlDisplay::Standard,
            OptionCommand::Set | OptionCommand::SetQuietly => selected,
            OptionCommand::Query => self.controls,
        };
        let repaint = self.squished;
        if repaint { self.position_initialized(); }
        let message = match self.controls {
            ControlDisplay::Standard => "Display control characters as ^X",
            ControlDisplay::Raw => "Display control characters directly (not recommended)",
            ControlDisplay::Color => "Display ANSI sequences directly, other control characters as ^X",
        };
        if !matches!(command, OptionCommand::ToggleQuietly | OptionCommand::ResetQuietly | OptionCommand::SetQuietly) {
            self.status_msg = Some(format!("{message}  (press RETURN)"));
            self.mode = Mode::OptionNotice { repaint };
        }
    }

    fn apply_line_number_option(&mut self, command: OptionCommand, choice: char) {
        let selected = if choice == 'n' { LineNumberMode::Enabled } else { LineNumberMode::Displayed };
        self.line_numbers = match command {
            OptionCommand::Toggle | OptionCommand::ToggleQuietly => if self.line_numbers == selected { LineNumberMode::Off } else { selected },
            OptionCommand::Reset | OptionCommand::ResetQuietly => LineNumberMode::Enabled,
            OptionCommand::Set | OptionCommand::SetQuietly => if choice == 'n' { LineNumberMode::Off } else { LineNumberMode::Displayed },
            OptionCommand::Query => self.line_numbers,
        };
        let repaint = self.squished;
        if repaint { self.position_initialized(); }
        let message = match self.line_numbers { LineNumberMode::Off => "Don't use line numbers", LineNumberMode::Enabled => "Use line numbers", LineNumberMode::Displayed => "Constantly display line numbers" };
        if !matches!(command, OptionCommand::ToggleQuietly | OptionCommand::ResetQuietly | OptionCommand::SetQuietly) {
            self.status_msg = Some(format!("{message}  (press RETURN)"));
            self.mode = Mode::OptionNotice { repaint };
        }
    }

    fn apply_chop_option(&mut self, command: OptionCommand) {
        self.wrap = match command {
            OptionCommand::Toggle | OptionCommand::ToggleQuietly => !self.wrap,
            OptionCommand::Reset | OptionCommand::ResetQuietly => true,
            OptionCommand::Set | OptionCommand::SetQuietly => false,
            OptionCommand::Query => self.wrap,
        };
        let repaint = self.squished;
        if repaint { self.position_initialized(); }
        let message = if self.wrap { "Fold long lines" } else { "Chop long lines" };
        if !matches!(command, OptionCommand::ToggleQuietly | OptionCommand::ResetQuietly | OptionCommand::SetQuietly) {
            self.status_msg = Some(format!("{message}  (press RETURN)"));
            self.mode = Mode::OptionNotice { repaint };
        }
    }

    fn resize_position_table(&mut self, height: u16) {
        let grew = height > self.display_anchor.position_capacity;
        self.display_anchor.position_capacity = self.display_anchor.position_capacity.max(height);
        if grew && self.content_painted && self.top_line == 0 && self.leading_rows == 0 && self.filter.is_none()
            && let Some(initial) = self.initial_position {
            self.top_line = initial.line;
        }
    }

    fn apply_use_color_option(&mut self, command: OptionCommand) {
        self.use_color = match command {
            OptionCommand::Toggle | OptionCommand::ToggleQuietly => !self.use_color,
            OptionCommand::Reset | OptionCommand::ResetQuietly => false,
            OptionCommand::Set | OptionCommand::SetQuietly => true,
            OptionCommand::Query => self.use_color,
        };
        let repaint = self.squished;
        if repaint { self.position_initialized(); }
        let message = if self.use_color { "Use color" } else { "Don't use color" };
        if !matches!(command, OptionCommand::ToggleQuietly | OptionCommand::ResetQuietly | OptionCommand::SetQuietly) {
            self.status_msg = Some(format!("{message}  (press RETURN)"));
            self.mode = Mode::OptionNotice { repaint };
        }
    }

    fn apply_incremental_search_option(&mut self, command: OptionCommand) {
        self.incremental_search = match command {
            OptionCommand::Toggle | OptionCommand::ToggleQuietly => !self.incremental_search,
            OptionCommand::Reset | OptionCommand::ResetQuietly => false,
            OptionCommand::Set | OptionCommand::SetQuietly => true,
            OptionCommand::Query => self.incremental_search,
        };
        let repaint = self.squished;
        if repaint { self.position_initialized(); }
        let message = if self.incremental_search { "Incremental search is on" } else { "Incremental search is off" };
        if !matches!(command, OptionCommand::ToggleQuietly | OptionCommand::ResetQuietly | OptionCommand::SetQuietly) {
            self.status_msg = Some(format!("{message}  (press RETURN)"));
            self.mode = Mode::OptionNotice { repaint };
        }
    }

    fn show_file_info(&mut self) {
        if self.filter.is_none() && self.files.count == 1
            && let (Some(name), Some(last)) = (&self.filename, self.last_visible_line)
            && let Some((position, size)) = self.buffer.regular_file_position(last) {
            let first = self.top_line + 1;
            let progress = if self.eof_displayed { "(END)".to_owned() } else {
                let numerator = u128::from(position) * 100;
                let denominator = u128::from(size);
                let quotient = numerator / denominator;
                let remainder = numerator % denominator;
                let rounded = quotient + u128::from(remainder > denominator / 2 || (denominator % 2 == 0 && remainder == denominator / 2 && quotient % 2 == 1));
                format!("{rounded}%")
            };
            let message = format!("{name} lines {first}-{}/{total} byte {position}/{size} {progress}  (press RETURN)", last + 1, total = self.buffer.len());
            let repaint = self.squished;
            if repaint { self.position_initialized(); }
            self.status_msg = Some(message);
            self.mode = Mode::OptionNotice { repaint };
        }
    }

    fn begin_examine(&mut self) {
        if self.allow_examine {
            self.examine_input.clear();
            self.mode = Mode::ExamineInput;
        } else {
            let repaint = self.squished;
            if repaint { self.position_initialized(); }
            self.command_error("Command not available".into(), ErrorAction::Dismiss);
            if let Mode::CommandError(error) = &mut self.mode { error.repaint = repaint; }
        }
    }

    fn handle_examine_key(&mut self, key: Key) -> bool {
        match key {
            Key::Enter => {
                self.mode = Mode::Normal;
                if !self.examine_input.content.is_empty() {
                    self.exit = PagerExit::Examine(std::ffi::OsString::from(&self.examine_input.content));
                    return true;
                }
            }
            Key::Escape => self.mode = Mode::Normal,
            Key::Ctrl('c') => { if self.squished { self.position_initialized(); } self.mode = Mode::Normal; },
            Key::Backspace if self.examine_input.content.is_empty() => self.mode = Mode::Normal,
            Key::Backspace => self.examine_input.backspace(),
            Key::Delete => self.examine_input.delete(),
            Key::Left => self.examine_input.move_left(),
            Key::Right => self.examine_input.move_right(),
            Key::Home => self.examine_input.move_home(),
            Key::End => self.examine_input.move_end(),
            Key::Char(character) => self.examine_input.insert(character),
            _ => {}
        }
        false
    }

    fn handle_key(&mut self, key: Key) -> bool {
        match self.mode.clone() {
            Mode::Normal => self.handle_normal_key(key),
            Mode::SetMark | Mode::JumpMark => { self.handle_mark_key(key); false }
            Mode::OptionPrefix(command) => {
                if let Some(command) = command.modified(&key) { self.mode = Mode::OptionPrefix(command); return false; }
                if (command == OptionCommand::Query && key == Key::Char('_')) || (command != OptionCommand::Query && key == Key::Char('-')) {
                    self.mode = Mode::OptionLong(command, OptionName::Typing(String::new()));
                    return false;
                }
                self.mode = Mode::Normal;
                if let Key::Char(choice @ ('r' | 'R')) = key {
                    self.apply_control_option(command, choice);
                } else if let Key::Char(choice @ ('n' | 'N')) = key {
                    self.apply_line_number_option(command, choice);
                } else if key == Key::Char('S') {
                    self.apply_chop_option(command);
                } else if key != Key::Escape { self.bell_requested = true; }
                false
            }
            Mode::OptionLong(command, name) => {
                if let OptionName::Typing(ref name) = name && name.is_empty() && let Some(modified) = command.modified(&key) {
                    self.mode = Mode::OptionLong(modified, OptionName::Typing(name.clone()));
                    return false;
                }
                match (name, key) {
                    (OptionName::Raw, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_control_option(command, 'r'); }
                    (OptionName::Color, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_control_option(command, 'R'); }
                    (OptionName::LineNumbers, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_line_number_option(command, 'n'); }
                    (OptionName::DisplayLineNumbers, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_line_number_option(command, 'N'); }
                    (OptionName::Chop | OptionName::ChopUpper, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_chop_option(command); }
                    (OptionName::UseColor | OptionName::UseColorUpper, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_use_color_option(command); }
                    (OptionName::IncrementalSearch | OptionName::IncrementalSearchUpper, Key::Enter | Key::Char('=')) => { self.mode = Mode::Normal; self.apply_incremental_search_option(command); }
                    (OptionName::Typing(name), Key::Enter | Key::Char('=')) => {
                        let repaint = self.squished;
                        if repaint { self.position_initialized(); }
                        self.status_msg = Some(format!("There is no --{name} option  (press RETURN)"));
                        self.mode = Mode::OptionNotice { repaint };
                    }
                    (_, Key::Escape) | (OptionName::Raw | OptionName::Color | OptionName::LineNumbers | OptionName::DisplayLineNumbers | OptionName::Chop | OptionName::ChopUpper | OptionName::UseColor | OptionName::UseColorUpper | OptionName::IncrementalSearch | OptionName::IncrementalSearchUpper, Key::Backspace) => self.mode = Mode::Normal,
                    (OptionName::Typing(mut name), Key::Backspace) => {
                        if name.is_empty() { self.mode = Mode::Normal; }
                        else { name.pop(); self.mode = Mode::OptionLong(command, OptionName::Typing(name)); }
                    }
                    (OptionName::Typing(mut name), Key::Char(c)) if c.is_ascii() => {
                        name.push(c);
                        match crate::option_catalog::lookup(name.as_bytes()) {
                            Ok(option) if option.canonical_name() == "raw-control-chars" => {
                                let completed = if name.as_bytes()[0].is_ascii_uppercase() { OptionName::Color } else { OptionName::Raw };
                                self.mode = Mode::OptionLong(command, completed);
                            }
                            Ok(option) if option.canonical_name() == "line-numbers" => {
                                let completed = if name.as_bytes()[0].is_ascii_uppercase() { OptionName::DisplayLineNumbers } else { OptionName::LineNumbers };
                                self.mode = Mode::OptionLong(command, completed);
                            }
                            Ok(option) if option.canonical_name() == "chop-long-lines" => {
                                let completed = if name.as_bytes()[0].is_ascii_uppercase() { OptionName::ChopUpper } else { OptionName::Chop };
                                self.mode = Mode::OptionLong(command, completed);
                            }
                            Ok(option) if option.canonical_name() == "incsearch" => {
                                let completed = if name.as_bytes()[0].is_ascii_uppercase() { OptionName::IncrementalSearchUpper } else { OptionName::IncrementalSearch };
                                self.mode = Mode::OptionLong(command, completed);
                            }
                            Ok(option) if option.canonical_name() == "use-color" => {
                                let completed = if name.as_bytes()[0].is_ascii_uppercase() { OptionName::UseColorUpper } else { OptionName::UseColor };
                                self.mode = Mode::OptionLong(command, completed);
                            }
                            result => {
                                if matches!(result, Err(crate::option_catalog::OptionLookupError::Unknown)) { self.bell_requested = true; }
                                self.mode = Mode::OptionLong(command, OptionName::Typing(name));
                            }
                        }
                    }
                    _ => {}
                }
                false
            }
            Mode::OptionNotice { .. } => {
                self.mode = Mode::Normal;
                self.status_msg = None;
                if !key.is_prompt_acknowledgement() { return self.handle_key(key); }
                false
            }
            Mode::QuitPrefix => {
                self.mode = Mode::Normal;
                match key {
                    Key::Char('Z') => return true,
                    _ => self.bell_requested = true,
                }
                false
            }
            Mode::FileCommand(count) => {
                self.mode = Mode::Normal;
                match key {
                    Key::Char(direction @ ('n' | 'p')) => {
                        let count = count.get();
                        let target = if direction == 'n' { self.files.index.checked_add(count) } else { self.files.index.checked_sub(count) };
                        if let Some(index) = target.filter(|index| *index < self.files.count) {
                            let startup_command = (self.command_context != CommandContext::Interactive)
                                .then_some(StartupFileSwitch {
                                    command: if direction == 'n' { FileCommand::Next } else { FileCommand::Previous },
                                    continuation: CommandContinuation::NextCommand,
                                });
                            self.exit = PagerExit::VisitFile(FileVisit { index, startup_command });
                            return true;
                        }
                        let ordinal = if count > 1 { "(N-th) " } else { "" };
                        let direction = if direction == 'n' { "next" } else { "previous" };
                        self.command_error(format!("No {ordinal}{direction} file"), ErrorAction::Dismiss);
                    }
                    Key::Char('f') => self.show_file_info(),
                    Key::Char('e') => self.begin_examine(),
                    Key::Char('q' | 'Q') => return true,
                    Key::Escape => {},
                    _ => self.bell_requested = true,
                }
                false
            }
            Mode::ExamineInput => self.handle_examine_key(key),
            Mode::SearchInput => {
                self.handle_search_input_key(key);
                false
            }
            Mode::FilterInput => {
                self.handle_filter_input_key(key);
                false
            }
            Mode::Help => {
                self.handle_help_key(key);
                self.standalone_help.is_some() && self.mode == Mode::Normal
            }
            Mode::Follow => self.handle_follow_key(key),
            Mode::CommandError(error) => {
                self.mode = if error.destination == ErrorDestination::Help { Mode::Help } else { Mode::Normal };
                self.command_context = error.context;
                self.status_msg = None;
                if let ErrorAction::Retry(ref command) = error.action && self.handle_normal_key(command.clone()) { return true; }
                if self.command_context == CommandContext::StartupAborted { self.command_context = CommandContext::StartupUnpositioned; }
                else if !error.remainder.is_empty() { self.startup_commands.push_front(error.remainder); }
                if self.restore_after_failed_examine { self.filename_prompt = true; }
                if !key.is_prompt_acknowledgement() {
                    if self.handle_key(key) { return true; }
                    if let ErrorAction::SearchWrapped(forward) = error.action && self.mode == Mode::Normal {
                        self.search_wrapped_notice(forward, false);
                    }
                    return false;
                }
                self.restore_after_failed_examine = false;
                if let ErrorAction::SearchWrapped(forward) = error.action { self.search_wrapped_notice(forward, false); }
                false
            }
        }
    }

    fn handle_normal_key(&mut self, key: Key) -> bool {
        if !matches!(key, Key::Char('/' | '?')) { self.restore_after_failed_examine = false; }
        self.filename_prompt = false;
        self.status_msg = None;
        if let Key::Char(character @ '0'..='9') = key {
            self.numeric_prefix = self.numeric_prefix.push(character);
            return false;
        }
        let count = match std::mem::replace(&mut self.numeric_prefix, NumericPrefix::Empty) {
            NumericPrefix::Empty => None,
            NumericPrefix::Value { value, .. } => (value != 0).then_some(value),
            NumericPrefix::Overflow => {
                self.command_error("Integer is too big".into(), ErrorAction::Retry(key));
                return false;
            }
        };
        if self.command_context == CommandContext::StartupUnpositioned
            && matches!(key, Key::Char('j' | 'k' | 'd' | 'u' | 'b' | ' ' | 'z' | 'w') | Key::Down | Key::Up | Key::Enter | Key::PageDown | Key::PageUp | Key::Ctrl('d' | 'u' | 'b' | 'f')) {
            self.command_context = CommandContext::StartupAborted;
            self.bell_requested = true;
            return false;
        }
        if self.eof_policy != EofPolicy::Manual && self.eof_displayed && self.command_context == CommandContext::Interactive
            && matches!(key, Key::Char('j' | 'e' | 'd' | ' ' | 'f' | 'z') | Key::Down | Key::Enter | Key::PageDown | Key::Ctrl('e' | 'n' | 'd' | 'f' | 'v')) {
            if self.files.index + 1 < self.files.count { self.exit = PagerExit::VisitFile(FileVisit { index:self.files.index + 1, startup_command:None }); }
            return true;
        }
        match key {
            Key::Char('q' | 'Q') | Key::Escape => return true,
            Key::Char('E') => self.begin_examine(),
            Key::Ctrl('c') => self.bell_requested = true,
            Key::Char('Z') => self.mode = Mode::QuitPrefix,
            Key::Char('-') => self.mode = Mode::OptionPrefix(OptionCommand::Toggle),
            Key::Char('_') => self.mode = Mode::OptionPrefix(OptionCommand::Query),
            Key::Char(':') => self.mode = Mode::FileCommand(std::num::NonZeroUsize::new(count.unwrap_or(1)).unwrap()),
            Key::Char('r') | Key::Ctrl('r' | 'l') => self.position_initialized(),
            Key::Char('R') => {
                self.position_initialized();
                let line = self.display_line_idx(self.top_line).unwrap_or(0);
                let past_end = self.buffer.reload_regular_file(line);
                if self.filter.is_some() { self.rebuild_filter(); }
                if past_end {
                    self.top_line = 0;
                    self.command_error("Cannot seek to that file position".into(), ErrorAction::Dismiss);
                }
            }
            Key::Char('=') | Key::Ctrl('g') => self.show_file_info(),
            Key::Char('m') => self.mode = Mode::SetMark,
            Key::Char('\'') => self.mode = Mode::JumpMark,

            Key::Char('j' | 'e') | Key::Ctrl('e' | 'n') | Key::Down | Key::Enter => self.scroll_down(count.unwrap_or(1)),
            Key::Char('k' | 'y') | Key::Ctrl('y' | 'p' | 'k') | Key::Up => self.scroll_up(count.unwrap_or(1)),
            Key::Char('J') => {
                let leading = self.leading_rows;
                let forced = self.forced_leading_rows;
                self.position_initialized();
                let moved = leading.min(count.unwrap_or(1));
                self.leading_rows = leading - moved;
                self.forced_leading_rows = forced && self.leading_rows > 0;
                let target = self.top_line.saturating_add(count.unwrap_or(1) - moved);
                if self.filter.is_none() { self.buffer.get_line(target); }
                self.top_line = target.min(self.display_line_count().saturating_sub(1));
            }
            Key::Char('K' | 'Y') => {
                let leading = self.leading_rows;
                let forced = self.forced_leading_rows;
                self.position_initialized();
                let amount = count.unwrap_or(1);
                let moved = self.top_line.min(amount);
                self.top_line -= moved;
                self.leading_rows = leading.saturating_add(amount - moved).min(self.content_height());
                self.forced_leading_rows = self.leading_rows > 0 && (forced || amount > moved);
            }
            Key::Char('d' | 'u') | Key::Ctrl('d' | 'u') => {
                if let Some(amount) = count.and_then(std::num::NonZeroUsize::new) { self.scroll_defaults.half_page = amount; }
                let amount = self.scroll_defaults.half_page.get();
                if matches!(key, Key::Char('d') | Key::Ctrl('d')) { self.scroll_down(amount); }
                else { self.scroll_up(amount); }
            }
            Key::Char(' ' | 'f' | 'z' | 'b' | 'w') | Key::PageDown | Key::PageUp | Key::Ctrl('f' | 'v' | 'b') => {
                if matches!(key, Key::Char('z' | 'w')) && let Some(amount) = count.and_then(std::num::NonZeroUsize::new) {
                    self.scroll_defaults.window = WindowSize::Fixed(amount);
                }
                let amount = count.unwrap_or_else(|| self.scroll_defaults.window.lines(self.term_height));
                if amount == 0 { self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now()); }
                else if matches!(key, Key::Char('b' | 'w') | Key::PageUp | Key::Ctrl('b')) { self.scroll_up(amount); }
                else { self.scroll_down(amount); }
            }
            Key::Char('g' | '<') | Key::Home => {
                let previous = self.mark_position();
                let previous_leading = self.leading_rows;
                self.position_initialized();
                let target = count.unwrap_or(1).saturating_sub(1);
                let row = if count.is_some() { self.jump_target.row(self.term_height) } else { 0 };
                let top = target.saturating_sub(row);
                let leading = row.saturating_sub(target);
                if self.command_context == CommandContext::Interactive && self.content_painted && self.top_line == top && previous_leading == leading { self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now()); }
                self.top_line = top;
                self.leading_rows = leading;
                self.buffer.get_line(target);
                self.remember_jump_from(previous);
                self.left_col = 0;
            }
            Key::Char('G' | '>') | Key::End => {
                let bottom_aligned = count.is_none();
                let previous = self.mark_position();
                self.position_initialized();
                self.bottom_aligned = bottom_aligned;
                if let Some(count) = count {
                    let target = count.saturating_sub(1);
                    let row = self.jump_target.row(self.term_height);
                    self.top_line = target.saturating_sub(row);
                    self.leading_rows = row.saturating_sub(target);
                    self.buffer.get_line(target);
                }
                else { self.buffer.read_all(); self.scroll_to_bottom(); }
                self.remember_jump_from(previous);
            }
            Key::HorizontalStart => { self.position_initialized(); self.left_col = 0; }
            Key::ToggleSearchHighlight => {
                if self.search.is_some() && (self.current_match_line.is_some() || self.search_inherited) { self.search_highlight = !self.search_highlight; }
                else { self.command_error("No previous regular expression".to_string(), ErrorAction::Dismiss); }
            }
            Key::ClearSearchHighlight => {
                self.search = None;
                self.search_highlight = false;
                self.search_inherited = false;
                self.current_match_line = None;
            }
            Key::Right | Key::Left => {
                self.position_initialized();
                if let Some(amount) = count.and_then(std::num::NonZeroUsize::new) { self.scroll_defaults.horizontal = Some(amount); }
                let amount = self.scroll_defaults.horizontal_step(self.term_width);
                if key == Key::Right { self.left_col = self.left_col.saturating_add(amount); }
                else { self.left_col = self.left_col.saturating_sub(amount); }
            }
            Key::Char('/') => {
                self.search_dir = SearchDir::Forward;
                self.mode = Mode::SearchInput;
                self.search_input.clear();
                self.search_input_nonmatch = false;
                self.search_input_keep_position = false;
                self.search_input_wrap = false;
                self.incremental_preview = None;
                self.history.reset_cursor();
            }
            Key::Char('?') => {
                self.search_dir = SearchDir::Backward;
                self.mode = Mode::SearchInput;
                self.search_input.clear();
                self.search_input_nonmatch = false;
                self.search_input_keep_position = false;
                self.search_input_wrap = false;
                self.incremental_preview = None;
                self.history.reset_cursor();
            }
            Key::Char('n') => self.search_same_dir(),
            Key::Char('N') => self.search_opposite_dir(),
            Key::Char('&') => {
                self.mode = Mode::FilterInput;
                self.filter_input.clear();
            }
            Key::Char('F') => {
                let bottom_aligned = self.command_context != CommandContext::Interactive;
                self.position_initialized();
                self.bottom_aligned = bottom_aligned;
                self.mode = Mode::Follow;
                self.buffer.read_all();
                self.scroll_to_bottom();
            }
            Key::Char('h' | 'H') => {
                self.help_top = 0;
                self.help_prefix = None;
                self.mode = Mode::Help;
            }
            Key::Char(character) if is_unbound_ascii(character) => self.early_bells = self.early_bells.saturating_add(1),
            _ => {}
        }
        false
    }

    fn command_error(&mut self, message: String, action: ErrorAction) {
        self.status_msg = Some(format!("{message}  (press RETURN)"));
        self.mode = Mode::CommandError(CommandError { action, context: self.command_context, remainder: Vec::new(), destination: ErrorDestination::Normal, repaint: false });
    }

    fn handle_help_key(&mut self, key: Key) {
        if let Some(prefix) = self.help_prefix.take() {
            if matches!((prefix, &key), (HelpPrefix::Colon(_), Key::Char('q' | 'Q')) | (HelpPrefix::Quit, Key::Char('Z'))) {
                self.leave_help();
            } else if let (HelpPrefix::Colon(count), Key::Char(direction @ ('n' | 'p')), Some(files)) = (prefix, &key, self.standalone_help) {
                if *direction == 'n' && files == HelpFiles::Registered && count.get() <= self.files.count {
                    self.exit = PagerExit::VisitFile(FileVisit { index:count.get() - 1, startup_command:None });
                    self.mode = Mode::Normal;
                } else {
                    let ordinal = if count.get() > 1 { "(N-th) " } else { "" };
                    let name = if *direction == 'n' { "next" } else { "previous" };
                    self.command_error(format!("No {ordinal}{name} file"), ErrorAction::Dismiss);
                    if let Mode::CommandError(error) = &mut self.mode { error.destination = ErrorDestination::Help; }
                }
            } else if key != Key::Ctrl('c') { self.bell_requested = true; }
            return;
        }
        if let Key::Char(character @ '0'..='9') = key {
            self.numeric_prefix = self.numeric_prefix.push(character);
            return;
        }
        let count = match std::mem::replace(&mut self.numeric_prefix, NumericPrefix::Empty) {
            NumericPrefix::Empty | NumericPrefix::Value { value: 0, .. } => None,
            NumericPrefix::Value { value, .. } => Some(value),
            NumericPrefix::Overflow => { self.bell_requested = true; return; }
        };
        let last = self.help_rows.saturating_sub(self.content_height());
        let previous = self.help_top;
        match key {
            Key::Char('q' | 'Q') => {
                self.leave_help();
                return;
            }
            Key::Char(':') => { self.help_prefix = Some(HelpPrefix::Colon(std::num::NonZeroUsize::new(count.unwrap_or(1)).unwrap())); return; }
            Key::Char('Z') => { self.help_prefix = Some(HelpPrefix::Quit); return; }
            Key::Escape => { self.bell_requested = true; return; }
            Key::Char('h' | 'H') => return,
            Key::Char('g' | '<') | Key::Home => self.help_top = count.map_or(0, |line| self.help_line_start(line.saturating_sub(1))).min(last),
            Key::Char('G' | '>') | Key::End => self.help_top = count.map_or(last, |line| self.help_line_start(line.saturating_sub(1))).min(last),
            Key::Char('j' | 'e') | Key::Ctrl('e' | 'n') | Key::Down | Key::Enter => self.help_top = self.help_top.saturating_add(count.unwrap_or(1)).min(last),
            Key::Char('k' | 'y') | Key::Ctrl('y' | 'p' | 'k') | Key::Up => self.help_top = self.help_top.saturating_sub(count.unwrap_or(1)),
            Key::Char('d' | 'u') | Key::Ctrl('d' | 'u') => {
                if let Some(amount) = count.and_then(std::num::NonZeroUsize::new) { self.scroll_defaults.half_page = amount; }
                let amount = self.scroll_defaults.half_page.get();
                if matches!(key, Key::Char('d') | Key::Ctrl('d')) { self.help_top = self.help_top.saturating_add(amount).min(last); }
                else { self.help_top = self.help_top.saturating_sub(amount); }
            }
            Key::Char(' ' | 'f' | 'z' | 'b' | 'w') | Key::PageDown | Key::PageUp | Key::Ctrl('f' | 'v' | 'b') => {
                if matches!(key, Key::Char('z' | 'w')) && let Some(amount) = count.and_then(std::num::NonZeroUsize::new) {
                    self.scroll_defaults.window = WindowSize::Fixed(amount);
                }
                let amount = count.unwrap_or_else(|| self.scroll_defaults.window.lines(self.term_height));
                if matches!(key, Key::Char('b' | 'w') | Key::PageUp | Key::Ctrl('b')) { self.help_top = self.help_top.saturating_sub(amount); }
                else { self.help_top = self.help_top.saturating_add(amount).min(last); }
            }
            _ => return,
        }
        if self.help_top == previous {
            self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
        }
    }

    fn leave_help(&mut self) {
        self.mode = Mode::Normal;
        self.filename_prompt = true;
        self.squished = false;
        self.compact_initial = false;
    }

    fn mark_position(&self) -> MarkPosition { MarkPosition { line:self.top_line, leading_rows:self.leading_rows } }

    fn remember_jump_from(&mut self, previous: MarkPosition) {
        let height = self.content_height();
        // less 668 can scroll backward by exactly one screen without replacing the last mark.
        let large = self.top_line >= previous.line.saturating_add(height) || previous.line > self.top_line.saturating_add(height);
        if self.content_painted && previous.line < self.buffer.len() && large { self.marks.insert(MarkName(b'\''), previous); }
    }

    fn handle_mark_key(&mut self, key: Key) {
        let setting = self.mode == Mode::SetMark;
        self.mode = Mode::Normal;
        let character = match key {
            Key::Char(character) => character,
            Key::Enter | Key::Backspace => return,
            Key::Escape => '\u{1b}',
            Key::Ctrl(character) => char::from(character as u8 & 0x1f),
            _ => { self.bell_requested = true; return; }
        };
        if !setting {
            match character {
                '^' => { self.handle_normal_key(Key::Char('g')); return; }
                '$' => {
                    let previous = self.mark_position();
                    self.position_initialized();
                    self.buffer.read_all();
                    self.scroll_to_bottom();
                    self.bottom_aligned = true;
                    self.end_mark_from = Some(previous);
                    self.remember_jump_from(previous);
                    return;
                }
                '.' => {
                    if self.content_painted {
                        self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
                        if self.command_context == CommandContext::Interactive {
                            self.squished = false;
                            self.compact_initial = false;
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        let Some(name) = MarkName::new(character) else {
            let mut bytes = Vec::new();
            let mut encoded = [0; 4];
            self.charset.write_filename(character.encode_utf8(&mut encoded).as_bytes(), &mut bytes).unwrap();
            self.command_error(format!("Invalid mark letter {}", String::from_utf8(bytes).unwrap()), ErrorAction::Dismiss);
            return;
        };
        if setting {
            if self.content_painted && self.top_line < self.buffer.len() {
                self.marks.insert(name, self.mark_position());
                if self.command_context == CommandContext::Interactive {
                    self.squished = false;
                    self.compact_initial = false;
                }
            }
            else { self.bell_requested = true; }
        }
        else if let Some(position) = self.marks.get(&name).copied().or_else(|| (character == '\'').then_some(MarkPosition { line:0, leading_rows:0 })) {
            let previous = self.mark_position();
            self.position_initialized();
            if self.content_painted && previous == position {
                self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
            }
            self.top_line = position.line;
            self.leading_rows = if position.leading_rows < self.content_height() { position.leading_rows } else { 0 };
            self.remember_jump_from(previous);
        } else { self.command_error("Mark not set".into(), ErrorAction::Dismiss); }
    }

    fn position_initialized(&mut self) {
        self.squished = false;
        self.bottom_aligned = false;
        self.leading_rows = 0;
        self.forced_leading_rows = false;
        self.startup_echo = None;
        if self.content_painted && self.command_context == CommandContext::Interactive { self.compact_initial = false; }
        if self.command_context == CommandContext::StartupUnpositioned { self.command_context = CommandContext::StartupPositioned; }
    }

    fn handle_follow_key(&mut self, key: Key) -> bool {
        if key == Key::Ctrl(FOLLOW_INTERRUPT) && self.interrupt_policy == InterruptPolicy::Quit {
            self.exit = PagerExit::Interrupted;
            return true;
        }
        if matches!(key, Key::Ctrl(character) if character == 'c' || character == FOLLOW_INTERRUPT) {
            self.mode = Mode::Normal;
            if key == Key::Ctrl('c') { self.bell_requested = true; }
            self.status_msg = None;
        }
        false
    }

    fn handle_search_input_key(&mut self, key: Key) {
        let before = self.incremental_search.then(|| self.search_input.content.clone());
        match key {
            Key::Escape => self.mode = Mode::Normal,
            Key::Ctrl('c') => { self.history.push(&self.search_input.content); self.mode = Mode::Normal; self.bell_requested = true; }
            Key::Ctrl('n') | Key::Char('!') if self.search_input.content.is_empty() => { self.search_input_nonmatch = !self.search_input_nonmatch; }
            Key::Ctrl('k') if self.search_input.content.is_empty() => { self.search_input_keep_position = !self.search_input_keep_position; }
            Key::Ctrl('w') if self.search_input.content.is_empty() => { self.search_input_wrap = !self.search_input_wrap; }
            Key::Enter => {
                let pattern = self.search_input.content.clone();
                self.mode = Mode::Normal;
                self.history.push(&pattern);
                if pattern.is_empty() { self.incremental_preview = None; return; }
                if let Some(search) = self.incremental_preview.take() {
                    self.search = Some(search);
                    self.search_nonmatch = self.search_input_nonmatch;
                    self.search_wrap = self.search_input_wrap;
                    self.search_inherited = false;
                    self.search_highlight = true;
                    self.current_match_line = Some(self.target_line());
                    return;
                }
                match Search::new_with_locale(&pattern, self.search_case, self.regex_locale) {
                    Ok(s) => {
                        self.search = Some(s);
                        self.search_nonmatch = self.search_input_nonmatch;
                        self.search_wrap = self.search_input_wrap;
                        self.search_inherited = false;
                        self.current_match_line = None;
                        self.do_search(self.search_dir == SearchDir::Forward, self.search_input_keep_position);
                        if self.restore_after_failed_examine && self.mode == Mode::Normal {
                            self.filename_prompt = true;
                            self.restore_after_failed_examine = false;
                        }
                    }
                    Err(e) => {
                        self.status_msg = Some(e);
                    }
                }
            }
            Key::Up => {
                if self.history.at_oldest() { self.bell_requested = true; }
                if let Some(entry) = self.history.older(&self.search_input.content) {
                    self.search_input.content = entry.to_string();
                    self.search_input.cursor = self.search_input.content.len();
                } else { self.bell_requested = true; }
            }
            Key::Down => match self.history.newer() {
                Some(entry) => {
                    self.search_input.content = entry.to_string();
                    self.search_input.cursor = self.search_input.content.len();
                }
                None => {
                    self.search_input.content = self.history.draft().to_string();
                    self.search_input.cursor = self.search_input.content.len();
                }
            },
            Key::Backspace if self.search_input.content.is_empty() => self.mode = Mode::Normal,
            Key::Backspace => self.search_input.backspace(),
            Key::Delete => self.search_input.delete(),
            Key::Left => self.search_input.move_left(),
            Key::Right => self.search_input.move_right(),
            Key::Home => self.search_input.move_home(),
            Key::End => self.search_input.move_end(),
            Key::Char(ch) => self.search_input.insert(ch),
            _ => {}
        }
        if self.mode == Mode::SearchInput && before.as_deref().is_some_and(|before| before != self.search_input.content) {
            self.update_incremental_search();
        } else if self.mode != Mode::SearchInput {
            self.incremental_preview = None;
        }
    }

    fn update_incremental_search(&mut self) {
        let Ok(search) = Search::new_with_locale(&self.search_input.content, self.search_case, self.regex_locale) else {
            self.incremental_preview = None;
            return;
        };
        let target = self.target_line();
        let forward = self.search_dir == SearchDir::Forward;
        let from = if forward { target } else { target.saturating_add(1) };
        let found = if self.controls == ControlDisplay::Standard { search.find_next_raw_line(&mut self.buffer, from, forward, self.search_input_nonmatch) }
            else if self.search_input_nonmatch { search.find_next_nonmatching_line(&mut self.buffer, from, forward) }
            else { search.find_next_line(&mut self.buffer, from, forward) };
        if let Some(line) = found {
            self.position_initialized();
            let row = self.jump_target.row(self.term_height);
            self.top_line = line.saturating_sub(row);
            self.leading_rows = row.saturating_sub(line);
            self.incremental_preview = Some(search);
        } else { self.incremental_preview = None; }
    }

    fn search_same_dir(&mut self) {
        let forward = self.search_dir == SearchDir::Forward;
        self.do_search(forward, false);
    }

    fn search_opposite_dir(&mut self) {
        let forward = self.search_dir == SearchDir::Backward;
        self.do_search(forward, false);
    }

    fn target_line(&self) -> usize {
        self.top_line.saturating_add(self.jump_target.row(self.term_height).saturating_sub(self.leading_rows))
    }

    fn search_wrapped_notice(&mut self, forward: bool, repaint: bool) {
        let message = if forward { "Search hit bottom; continuing at top" } else { "Search hit top; continuing at bottom" };
        self.command_error(message.to_owned(), ErrorAction::Dismiss);
        if let Mode::CommandError(error) = &mut self.mode { error.repaint = repaint; }
    }

    fn do_search(&mut self, forward: bool, keep_position: bool) {
        self.search_highlight = true;
        let search = match self.search.take() {
            Some(s) => s,
            None => match self.history.last_pattern() {
                Some(pattern) => match Search::new_with_locale(pattern, self.search_case, self.regex_locale) {
                    Ok(search) => { self.history.note_recalled_pattern(); self.current_match_line = Some(self.target_line()); search }
                    Err(error) => { self.command_error(error, ErrorAction::Dismiss); return; }
                },
                None => { self.command_error("No previous regular expression".to_string(), ErrorAction::Dismiss); return; }
            },
        };
        let last_displayed = self.last_visible_line.and_then(|line| {
            if self.filter.is_some() { self.filtered_lines.binary_search(&line).ok() } else { Some(line) }
        });
        let from = if !forward && !self.content_painted && self.last_visible_line.is_none() && self.current_match_line.is_none() {
            self.buffer.read_all();
            self.display_line_count()
        } else { match (self.search_start, forward) {
            (SearchStart::AfterScreen, true) => last_displayed.map_or(self.top_line, |line| line.saturating_add(1)),
            (SearchStart::AfterScreen, false) => self.top_line,
            (SearchStart::AfterTarget, false) => self.target_line(),
            (SearchStart::AfterTarget, true) if self.content_painted => self.target_line().saturating_add(1),
            (SearchStart::Default, true) if self.current_match_line.is_some() => self.target_line().saturating_add(1),
            (SearchStart::Default, false) if self.current_match_line.is_some() => self.target_line(),
            (SearchStart::Default, false) if self.current_match_line.is_none() && self.search_inherited => self.top_line,
            (SearchStart::Default, false) if self.current_match_line.is_none() => last_displayed.map_or(self.top_line, |line| line.saturating_add(1)),
            (_, true) => self.current_match_line.map_or(self.top_line, |line| line.saturating_add(1)),
            (_, false) => self.current_match_line.unwrap_or(self.top_line),
        } };
        let mut wrapped = false;
        let found = self.find_search_line(&search, from, forward).or_else(|| {
            if !self.search_wrap { return None; }
            let found = if forward {
                self.find_search_line(&search, 0, true).filter(|line| *line < from)
            } else {
                self.buffer.read_all();
                let end = self.display_line_count();
                self.find_search_line(&search, end, false).filter(|line| *line >= from)
            };
            wrapped = self.buffer.is_finished();
            found
        });
        match found {
            Some(line) => {
                self.current_match_line = Some(line);
                if keep_position && self.squished { self.position_initialized(); }
                if !keep_position {
                    self.position_initialized();
                    let row = self.jump_target.row(self.term_height);
                    self.top_line = line.saturating_sub(row);
                    self.leading_rows = row.saturating_sub(line);
                }
                if wrapped { self.search_wrapped_notice(forward, true); }
            }
            None => {
                if self.squished { self.position_initialized(); }
                let action = if wrapped { ErrorAction::SearchWrapped(forward) } else { ErrorAction::Dismiss };
                self.command_error("Pattern not found".to_string(), action);
                if let Mode::CommandError(error) = &mut self.mode { error.repaint = true; }
            }
        }
        self.search = Some(search);
    }

    fn find_search_line(&mut self, search: &Search, from: usize, forward: bool) -> Option<usize> {
        if self.filter.is_some() {
            let count = self.filtered_lines.len();
            let mut matches = |index: &usize| self.buffer.get_line(self.filtered_lines[*index]).is_some_and(|line| {
                let matched = if self.controls == ControlDisplay::Standard { search.is_match_raw(line) } else { search.is_match(line) };
                matched != self.search_nonmatch
            });
            if forward { (from.min(count)..count).find(&mut matches) }
            else { (0..from.min(count)).rev().find(&mut matches) }
        } else if self.controls == ControlDisplay::Standard { search.find_next_raw_line(&mut self.buffer, from, forward, self.search_nonmatch) }
        else if self.search_nonmatch { search.find_next_nonmatching_line(&mut self.buffer, from, forward) }
        else { search.find_next_line(&mut self.buffer, from, forward) }
    }

    fn handle_filter_input_key(&mut self, key: Key) {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
            }
            Key::Ctrl('c') => {
                self.history.push(&self.filter_input.content);
                self.mode = Mode::Normal;
            }
            Key::Enter => {
                let pattern = self.filter_input.content.clone();
                self.mode = Mode::Normal;
                if pattern.is_empty() {
                    // Empty pattern clears the filter
                    self.clear_filter();
                    return;
                }
                match Search::new_with_locale(&pattern, self.search_case, self.regex_locale) {
                    Ok(s) => {
                        self.filter = Some(s);
                        self.rebuild_filter();
                        self.top_line = 0;
                    }
                    Err(e) => {
                        self.status_msg = Some(e);
                    }
                }
            }
            Key::Backspace => self.filter_input.backspace(),
            Key::Delete => self.filter_input.delete(),
            Key::Left => self.filter_input.move_left(),
            Key::Right => self.filter_input.move_right(),
            Key::Home => self.filter_input.move_home(),
            Key::End => self.filter_input.move_end(),
            Key::Char(ch) => self.filter_input.insert(ch),
            _ => {}
        }
    }

    fn rebuild_filter(&mut self) {
        self.filtered_lines.clear();
        let filter = match &self.filter {
            Some(f) => f,
            None => return,
        };
        // Read all lines so we can filter them
        self.buffer.read_all();
        for i in 0..self.buffer.len() {
            if let Some(line) = self.buffer.get_line(i)
                && filter.is_match(line)
            {
                self.filtered_lines.push(i);
            }
        }
    }

    fn clear_filter(&mut self) {
        if self.filter.is_some() {
            self.top_line = self.display_line_idx(self.top_line).unwrap_or(0);
            self.current_match_line = self.current_match_line.and_then(|line| self.display_line_idx(line));
        }
        self.filter = None;
        self.filtered_lines.clear();
    }

    fn scroll_down(&mut self, lines: usize) {
        if self.forced_leading_rows && self.leading_rows > 0 && self.eof_displayed && self.buffer.is_finished() {
            self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
            return;
        }
        let previous = self.top_line;
        let moved_leading = lines.min(self.leading_rows);
        self.leading_rows -= moved_leading;
        let lines = lines - moved_leading;
        if self.wraps() {
            self.scroll_down_wrapped(lines);
        } else {
            self.top_line = self.top_line.saturating_add(lines);
            self.clamp_top_line();
        }
        if moved_leading == 0 && self.content_painted && self.top_line == previous && self.buffer.is_finished()
            && (!self.wraps() || self.wrapped_line_rows(self.top_line) <= self.content_height()) {
            self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        let previous = self.top_line;
        self.squished = false;
        if self.wraps() { self.scroll_up_wrapped(lines); }
        else { self.top_line = self.top_line.saturating_sub(lines); }
        if self.content_painted && self.top_line == previous && previous == 0 {
            self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
        }
    }

    fn scroll_to_bottom(&mut self) {
        let total = self.display_line_count();
        let ch = self.content_height();
        if total > ch {
            self.top_line = total - ch;
        } else {
            self.top_line = 0;
        }
    }

    fn clamp_top_line(&mut self) {
        let ch = self.content_height();
        if self.filter.is_none() {
            self.buffer.get_line(self.top_line.saturating_add(ch));
        }

        let total = self.display_line_count();
        if total <= ch {
            self.top_line = 0;
        } else if self.top_line > total - ch && (self.filter.is_some() || self.buffer.is_finished())
        {
            self.top_line = total - ch;
        }
    }

    // --- Wrapped mode scrolling ---

    fn wrapped_line_rows(&mut self, line_idx: usize) -> usize {
        let w = self.term_width as usize;
        if w == 0 {
            return 1;
        }
        let content_width = w.saturating_sub(self.number_gutter(line_idx, w).len()).max(1);
        match self.buffer.get_line(line_idx) {
            Some(line) => {
                let has_tab = line.contains('\t');
                let vw = if has_tab && self.controls == ControlDisplay::Raw {
                    crate::ansi::raw_width(line, &self.tab_stops)
                } else if has_tab {
                    DisplayPrefix::new(line.as_bytes().to_vec(), self.charset, true, std::num::NonZeroUsize::new(content_width), self.controls, &self.tab_stops).columns
                } else if self.controls == ControlDisplay::Raw {
                    unicode_width::UnicodeWidthStr::width(line)
                } else {
                    visible_width(line)
                };
                if vw == 0 {
                    1
                } else {
                    vw.div_ceil(if has_tab { content_width } else { w })
                }
            }
            None => 1,
        }
    }

    fn scroll_down_wrapped(&mut self, mut screen_rows: usize) {
        while screen_rows > 0 {
            let rows = self.wrapped_line_rows(self.top_line);
            if screen_rows >= rows {
                screen_rows -= rows;
                self.top_line = self.top_line.saturating_add(1);
                if self.buffer.get_line(self.top_line).is_none() {
                    self.top_line = self.top_line.saturating_sub(1);
                    break;
                }
            } else {
                break;
            }
        }
        self.clamp_top_line_wrapped();
    }

    fn scroll_up_wrapped(&mut self, mut screen_rows: usize) {
        while screen_rows > 0 && self.top_line > 0 {
            self.top_line -= 1;
            let rows = self.wrapped_line_rows(self.top_line);
            screen_rows = screen_rows.saturating_sub(rows);
        }
    }

    fn clamp_top_line_wrapped(&mut self) {
        let ch = self.content_height();
        let mut rows_used = 0;
        let mut line = self.top_line;
        loop {
            if self.buffer.get_line(line).is_none() {
                break;
            }
            rows_used += self.wrapped_line_rows(line);
            if rows_used >= ch {
                return;
            }
            line += 1;
        }
        if self.buffer.is_finished() && rows_used < ch {
            while self.top_line > 0 {
                self.top_line -= 1;
                rows_used += self.wrapped_line_rows(self.top_line);
                if rows_used >= ch {
                    break;
                }
            }
        }
    }

    // --- Rendering ---

    fn render(&mut self) -> io::Result<()> { self.render_frame(true) }

    fn render_frame(&mut self, status: bool) -> io::Result<()> {
        if !self.content_painted && !matches!(self.mode, Mode::CommandError(_)) {
            self.buffer.byte_prefix(0, 0)?;
            if self.buffer.is_empty() && self.buffer.state() == crate::buffer::SourceState::Waiting { return Ok(()); }
        }
        let mut buf = Frame { bytes: Vec::with_capacity(4096), parts: Vec::new(), spill: None, relative_row: self.display_anchor.row, padding: 0, retain_initial_column: false };
        let w = self.term_width as usize;
        let ch = self.content_height();

        if let Some(previous) = self.end_mark_from.take() {
            let target = MarkPosition { line:self.top_line, leading_rows:self.initial_content_row(w, ch)? };
            if self.content_painted && previous == target {
                self.boundary_bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now());
            }
        }
        let boundary_bell = std::mem::take(&mut self.boundary_bell_requested);
        let general_bell = std::mem::take(&mut self.bell_requested);
        if boundary_bell && self.bell_policy.is_audible(terminal::BellKind::Boundary) { buf.push(7); }
        if general_bell && self.bell_policy.is_audible(terminal::BellKind::General) { buf.push(7); }
        buf.extend_from_slice(b"\x1b[?25l");
        if matches!(&self.mode, Mode::CommandError(CommandError { repaint:false, .. }) | Mode::OptionNotice { repaint:false }) {
            if !self.content_painted { for row in 0..ch { buf.position(row); terminal::clear_line(&mut buf); } }
            if let Some(row) = self.display_anchor.row { buf.padding = ch.saturating_sub(row); }
            self.render_status(&mut buf, w);
            buf.extend_from_slice(b"\x1b[?25h");
            let mut stdout = io::stdout().lock();
            buf.write_to(&mut stdout)?;
            return stdout.flush();
        }
        let first_row = if self.mode == Mode::Help { 0 } else { self.initial_content_row(w, ch)? }.saturating_sub(self.bottom_gap);
        self.bottom_gap = 0;
        if self.bottom_aligned && self.command_context == CommandContext::Interactive { self.leading_rows = first_row; }
        if !self.content_painted { self.squished = first_row > 0; }
        let primary = self.display_anchor.row.is_some();
        if let Some(command) = self.startup_echo.take() {
            if !primary { terminal::move_cursor(&mut buf, self.term_height.saturating_sub(1), 0); }
            buf.extend_from_slice(b"\r\x1b[K :");
            buf.push(command.key());
            buf.relative_row = Some(0);
            buf.retain_initial_column = true;
        }
        if buf.relative_row.is_some() && !self.bottom_aligned && self.leading_rows == 0 { buf.padding = first_row; }
        else { for row in 0..first_row { buf.position(row); if self.bottom_aligned || self.leading_rows > 0 { self.write_eof_marker(&mut buf); } terminal::clear_line(&mut buf); } }
        if self.mode != Mode::Help { self.last_visible_line = None; }
        if self.mode == Mode::Help {
            self.render_help(&mut buf, w, ch);
        } else if self.wraps() {
            self.render_wrapped(&mut buf, w, ch, first_row)?;
        } else {
            self.render_nowrap(&mut buf, w, ch, first_row)?;
        }

        if let Some(error) = self.buffer.take_error() { return Err(error); }
        if self.eof_policy != EofPolicy::Manual || self.buffer.is_finished() {
            self.eof_displayed = self.visible_end_of_file()?;
            if let Some(error) = self.buffer.take_error() { return Err(error); }
        } else { self.eof_displayed = false; }
        let visual_boundary = boundary_bell && self.bell_policy.is_visual(terminal::BellKind::Boundary);
        let visual_general = general_bell && self.bell_policy.is_visual(terminal::BellKind::General);
        if visual_boundary || visual_general {
            buf.position(ch);
            terminal::clear_line(&mut buf);
            buf.extend_from_slice(b"\x1b[?25h");
            let mut output = io::stdout().lock();
            buf.write_to(&mut output)?;
            output.flush()?;
            buf.clear();
            if visual_boundary { self.bell_policy.write_to(&mut output, terminal::BellKind::Boundary)?; }
            if visual_general { self.bell_policy.write_to(&mut output, terminal::BellKind::General)?; }
        }
        if status { self.render_status(&mut buf, w); }
        else { buf.position(ch); terminal::clear_line(&mut buf); }
        self.display_anchor.row = if primary { buf.relative_row } else { None };
        if self.display_anchor.row.is_none() { self.compact_initial = false; }
        self.content_painted = true;
        self.bottom_aligned = false;
        buf.extend_from_slice(b"\x1b[?25h");

        let mut stdout = io::stdout().lock();
        buf.write_to(&mut stdout)?;
        stdout.flush()?;

        Ok(())
    }

    /// Map a display row to an actual buffer line index.
    fn display_line_idx(&self, display_idx: usize) -> Option<usize> {
        if self.filter.is_some() {
            self.filtered_lines.get(display_idx).copied()
        } else {
            Some(display_idx)
        }
    }

    /// Total number of displayable lines (filtered or total).
    fn display_line_count(&self) -> usize {
        if self.filter.is_some() {
            self.filtered_lines.len()
        } else {
            self.buffer.len()
        }
    }

    fn number_gutter(&self, line: usize, width: usize) -> String {
        number_gutter(line, width, self.line_numbers.displayed(), self.line_number_width)
    }

    fn write_number_gutter(buf: &mut Vec<u8>, gutter: &str, use_color: bool) {
        if let Some(first) = gutter.bytes().position(|byte| byte.is_ascii_digit()) {
            let last = gutter.bytes().rposition(|byte| byte.is_ascii_digit()).unwrap() + 1;
            buf.extend_from_slice(&gutter.as_bytes()[..first]);
            buf.extend_from_slice(if use_color { b"\x1b[1;36m" } else { b"\x1b[1m" });
            buf.extend_from_slice(&gutter.as_bytes()[first..last]);
            buf.extend_from_slice(b"\x1b[m");
            buf.extend_from_slice(&gutter.as_bytes()[last..]);
        } else { buf.extend_from_slice(gutter.as_bytes()); }
    }

    fn wraps(&self) -> bool { self.wrap && self.left_col == 0 }

    fn display_prefix(&mut self, line: usize, bytes: usize) -> io::Result<Option<DisplayLine>> {
        if self.controls != ControlDisplay::Raw {
            let width = self.term_width as usize;
            let wrap_width = self.wraps().then(|| std::num::NonZeroUsize::new(width.saturating_sub(self.number_gutter(line, width).len()).max(1)).unwrap());
            Ok(self.buffer.byte_prefix(line, bytes)?.map(|fragment| DisplayLine::Mapped(DisplayPrefix::new(fragment.bytes, self.charset, fragment.complete, wrap_width, self.controls, &self.tab_stops))))
        } else {
            Ok(self.buffer.line_prefix(line, bytes)?.map(|fragment| DisplayLine::Raw { fragment, tabs:self.tab_stops.clone() }))
        }
    }

    fn can_stream_visible_color_line(&self, line: usize, width: usize, fragment: &DisplayLine) -> bool {
        !fragment.complete() && self.controls == ControlDisplay::Color && self.left_col == 0
            && self.search.is_none() && fragment.columns() <= width && self.buffer.line_readable_without_waiting(line)
    }

    fn visible_color_line_fits(&mut self, line: usize, width: usize, fragment: &DisplayLine) -> io::Result<bool> {
        if !self.can_stream_visible_color_line(line, width, fragment) { return Ok(false); }
        Ok(matches!(scan_color_line(&mut self.buffer, line, width, self.charset, &mut io::sink())?, ColorLine::Fits(_)))
    }

    fn wrapped_prefix_rows(&self, fragment: &DisplayLine, actual: usize, width: usize, available: usize) -> usize {
        if !fragment.complete() { return available; }
        let columns = width.saturating_sub(self.number_gutter(actual, width).len()).max(1);
        fragment.columns().div_ceil(columns).max(1)
    }

    fn visible_end_of_file(&mut self) -> io::Result<bool> {
        let width = usize::from(self.term_width).max(1);
        let height = self.content_height();
        let mut rows = self.leading_rows.min(height);
        if rows == height { return Ok(false); }
        let mut index = self.top_line;
        loop {
            let Some(actual) = self.display_line_idx(index) else { return Ok(self.buffer.is_finished()); };
            let limit = width.saturating_mul(height.saturating_sub(rows)).saturating_add(1).saturating_mul(16).max(4096);
            let Some(fragment) = self.display_prefix(actual, limit)? else { return Ok(self.buffer.is_finished()); };
            let gutter = self.number_gutter(actual, width);
            let content_width = width.saturating_sub(gutter.len()).max(1);
            let streamed = if fragment.complete() { false } else { self.visible_color_line_fits(actual, content_width, &fragment)? };
            if !fragment.complete() && !streamed { return Ok(false); }
            let occupied = if streamed { 1 } else if self.wraps() { self.wrapped_prefix_rows(&fragment, actual, width, height.saturating_sub(rows)) } else { 1 };
            rows = rows.saturating_add(occupied);
            if rows > height { return Ok(false); }
            index += 1;
            if rows == height {
                let Some(next) = self.display_line_idx(index) else { return Ok(self.buffer.is_finished()); };
                return Ok(self.display_prefix(next, 1)?.is_none() && self.buffer.is_finished());
            }
        }
    }

    fn initial_content_row(&mut self, width: usize, height: usize) -> io::Result<usize> {
        if self.leading_rows > 0 { return Ok(self.leading_rows.min(height)); }
        if !self.compact_initial && !self.squished && !self.bottom_aligned { return Ok(0); }
        let mut rows = 0;
        let mut index = self.top_line;
        while rows < height {
            let Some(actual) = self.display_line_idx(index) else { return Ok(0); };
            let Some(fragment) = self.display_prefix(actual, width.saturating_mul(height - rows).saturating_add(1).saturating_mul(16).max(4096))? else {
                return Ok(if self.buffer.is_finished() { height - rows } else { 0 });
            };
            let gutter = self.number_gutter(actual, width);
            let content_width = width.saturating_sub(gutter.len()).max(1);
            let streamed = if fragment.complete() || !self.wraps() { false } else { self.visible_color_line_fits(actual, content_width, &fragment)? };
            rows += if streamed { 1 } else if self.wraps() { self.wrapped_prefix_rows(&fragment, actual, width, height - rows) } else { 1 };
            index += 1;
        }
        Ok(0)
    }

    fn write_eof_marker(&self, buf: &mut Vec<u8>) {
        if self.show_eof_markers && !(self.compact_initial && self.buffer.is_finished()) { buf.extend_from_slice(b"\x1b[1m~\x1b[m"); }
    }

    fn probe_after_complete_line(&mut self, line: usize) -> io::Result<()> {
        if let Some(next) = line.checked_add(1) && self.buffer.len() == next {
            self.buffer.byte_prefix(next, 0)?;
        }
        Ok(())
    }

    fn render_nowrap(&mut self, buf: &mut Frame, w: usize, ch: usize, first_row: usize) -> io::Result<()> {
        for row in first_row..ch {
            buf.position(row);
            terminal::clear_line(buf);
            let display_idx = self.top_line.saturating_add(row - first_row);
            let actual = self.display_line_idx(display_idx);
            let fragment = match actual { Some(index) => self.display_prefix(index, self.left_col.saturating_add(w).saturating_add(1).saturating_mul(16).max(4096))?, None => None };
            match actual.zip(fragment) {
                Some((line_idx, fragment)) => {
                    self.last_visible_line = Some(line_idx);
                    let gutter = self.number_gutter(line_idx, w);
                    Self::write_number_gutter(buf, &gutter, self.use_color);
                    let content_width = w.saturating_sub(gutter.len());
                    if self.can_stream_visible_color_line(line_idx, content_width, &fragment)
                        && let Some(sgr) = buf.append_color_line(&mut self.buffer, line_idx, content_width, self.charset, |_| {})? {
                        if sgr { buf.extend_from_slice(b"\x1b[m"); }
                        if row + 1 == ch { self.probe_after_complete_line(line_idx)?; }
                        terminal::clear_line(buf);
                        continue;
                    }
                    let line_width = fragment.columns();
                    let chopped = (!fragment.complete() || line_width > self.left_col.saturating_add(content_width)) && content_width > 0;
                    let display = fragment.display(self.left_col, content_width.saturating_sub(usize::from(chopped)), NotationStyle::for_ui_color(self.use_color));
                    self.write_line_with_search(buf, &display, &fragment, self.left_col);
                    if chopped { buf.extend_from_slice(if self.use_color { b"\x1b[30;46m>\x1b[m" } else { b"\x1b[7m>\x1b[m" }); }
                    if row + 1 == ch && fragment.complete() { self.probe_after_complete_line(line_idx)?; }
                }
                None => self.write_eof_marker(buf),
            }
            terminal::clear_line(buf);
        }
        Ok(())
    }

    fn render_wrapped(&mut self, buf: &mut Frame, w: usize, ch: usize, first_row: usize) -> io::Result<()> {
        let mut screen_row = first_row;
        let mut display_idx = self.top_line;
        while screen_row < ch {
            let actual = self.display_line_idx(display_idx);
            let fragment = match actual { Some(index) => self.display_prefix(index, w.saturating_mul(ch - screen_row).saturating_add(1).saturating_mul(16).max(4096))?, None => None };
            match actual.zip(fragment) {
                Some((actual, fragment)) => {
                    self.last_visible_line = Some(actual);
                    let gutter_width = self.number_gutter(actual, w).len();
                    let content_width = w.saturating_sub(gutter_width).max(1);
                    let gutter = self.number_gutter(actual, w);
                    let use_color = self.use_color;
                    if self.can_stream_visible_color_line(actual, content_width, &fragment)
                        && let Some(sgr) = buf.append_color_line(&mut self.buffer, actual, content_width, self.charset, |frame| {
                            frame.position(screen_row);
                            Self::write_number_gutter(frame, &gutter, use_color);
                        })? {
                        if sgr { buf.extend_from_slice(b"\x1b[m"); }
                        buf.wrapped_row(screen_row);
                        screen_row += 1;
                        terminal::clear_line(buf);
                        if screen_row == ch { self.probe_after_complete_line(actual)?; }
                        display_idx = display_idx.saturating_add(1);
                        continue;
                    }
                    let rows_needed = self.wrapped_prefix_rows(&fragment, actual, w, ch - screen_row);
                    let available_rows = ch - screen_row;
                    for wrap_row in 0..rows_needed {
                        if screen_row >= ch { break; }
                        if wrap_row == 0 { buf.position(screen_row); }
                        let gutter = self.number_gutter(actual, w);
                        Self::write_number_gutter(buf, &gutter, self.use_color);
                        let display = fragment.display(wrap_row * content_width, w.saturating_sub(gutter.len()), NotationStyle::for_ui_color(self.use_color));
                        self.write_line_with_search(buf, &display, &fragment, wrap_row * content_width);
                        buf.wrapped_row(screen_row);
                        screen_row += 1;
                        if (wrap_row + 1 == rows_needed && fragment.complete()) || !fragment.fills_row(wrap_row * content_width, content_width) {
                            terminal::clear_line(buf);
                            if wrap_row + 1 < rows_needed && screen_row < ch { buf.position(screen_row); }
                        }
                    }
                    if screen_row == ch && (rows_needed > available_rows || !fragment.complete()) {
                        buf.extend_from_slice(b" \x08");
                        if fragment.fills_row((available_rows - 1) * content_width, content_width) { buf.wrapped_row(screen_row); }
                    }
                    if screen_row == ch && rows_needed <= available_rows && fragment.complete() { self.probe_after_complete_line(actual)?; }
                    display_idx = display_idx.saturating_add(1);
                }
                None => {
                    buf.position(screen_row);
                    self.write_eof_marker(buf);
                    terminal::clear_line(buf);
                    screen_row += 1;
                    display_idx = display_idx.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    fn advance_search_column(text: &str, byte: &mut usize, column: &mut usize, target: usize) {
        while *byte < target {
            let character = text[*byte..].chars().next().unwrap();
            *byte += character.len_utf8();
            *column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        }
    }

    fn search_right_boundary(text: &str, boundary: usize) -> usize {
        let mut column = 0;
        for character in text.chars() {
            let end = column + unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            if column < boundary && end > boundary { return column; }
            if end >= boundary { break; }
            column = end;
        }
        boundary
    }

    fn visible_search_range(text: &str, left: usize, right: usize) -> Option<(usize, usize)> {
        let mut column = 0;
        let mut first = None;
        let mut last = 0;
        for (byte, character) in text.char_indices() {
            let width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            let end = column + width;
            let selected = if width == 0 { column > left && column <= right } else { column < right && end > left };
            if selected { first.get_or_insert(byte); last = byte + character.len_utf8(); }
            column = end;
        }
        first.map(|first| (first, last))
    }

    fn write_line_with_search(&self, buf: &mut Vec<u8>, display: &[u8], source: &DisplayLine, start: usize) {
        let Ok(display) = std::str::from_utf8(display) else { buf.extend_from_slice(display); return; };
        if (self.search_highlight || self.incremental_preview.is_some())
            && let Some(search) = self.incremental_preview.as_ref().or(self.search.as_ref())
            && source.matches_search(search, self.controls)
        {
            let stripped = strip_ansi(display);
            let prefix = if start == 0 { String::new() } else {
                let prefix = source.display(0, start, NotationStyle::for_ui_color(self.use_color));
                strip_ansi(&String::from_utf8_lossy(&prefix))
            };
            let prefix_bytes = prefix.len();
            let full = source.display(0, source.columns(), NotationStyle::for_ui_color(self.use_color));
            let context = strip_ansi(&String::from_utf8_lossy(&full));
            let searchable = source.search_text(&context, 0);
            let mut matches: Vec<_> = search.find_matches_stripped(&searchable).into_iter().filter(|(start, end)| start < end).collect();
            if search.uses_byte_regex() {
                let mut aligned: Vec<(usize, usize)> = Vec::new();
                for (mut start, mut end) in matches {
                    while !searchable.is_char_boundary(start) { start -= 1; }
                    while !searchable.is_char_boundary(end) { end += 1; }
                    if let Some((_, previous)) = aligned.last_mut() && *previous > start { *previous = (*previous).max(end); }
                    else { aligned.push((start, end)); }
                }
                matches = aligned;
            }
            if self.search_nonmatch { matches.truncate(1); }
            let matches: Vec<_> = if context.strip_prefix(&prefix).is_some_and(|suffix| suffix.starts_with(&stripped)) {
                let visible_end = prefix_bytes.saturating_add(stripped.len());
                matches.into_iter().filter_map(|(start, end)| {
                    if end > prefix_bytes && start < visible_end {
                        Some((start.saturating_sub(prefix_bytes), end.min(visible_end) - prefix_bytes))
                    } else { None }
                }).collect()
            } else {
                let mut byte = 0;
                let mut column = 0;
                let mut visible = Vec::new();
                let edge = Self::search_right_boundary(&searchable, start.saturating_add(unicode_width::UnicodeWidthStr::width(stripped.as_str())));
                for (first, last) in matches {
                    Self::advance_search_column(&searchable, &mut byte, &mut column, first);
                    let left = column;
                    Self::advance_search_column(&searchable, &mut byte, &mut column, last);
                    let right = column.min(edge);
                    if right > start && let Some(range) = Self::visible_search_range(&stripped, left.saturating_sub(start), right - start) {
                        visible.push(range);
                    }
                }
                visible
            };
            let marker = source.left_clipped_columns(start).min(stripped.len());
            let matches: Vec<_> = matches.into_iter().filter_map(|(first, last)| {
                if last > marker { Some((first.max(marker), last)) } else { None }
            }).collect();
            if !matches.is_empty() {
                let colored = self.use_color;
                let mut source_colors = crate::ansi::SgrColors::default();
                let mut source = 0;
                let mut visible = 0;
                let mut next_match = 0;
                let mut highlighted = false;
                let mut inverse = false;
                while source < display.len() {
                    if highlighted && visible == matches[next_match].1 {
                        if colored { source_colors.restore(buf); }
                        else if !inverse { buf.extend_from_slice(b"\x1b[27m"); }
                        highlighted = false;
                        next_match += 1;
                    }
                    if !highlighted && matches.get(next_match).is_some_and(|(start, _)| *start == visible) {
                        buf.extend_from_slice(if colored { b"\x1b[30;102m" } else { b"\x1b[7m" });
                        highlighted = true;
                    }
                    let escape = if display.as_bytes()[source] == 0x1b { crate::ansi::string_escape_seq_len(display, source) } else { 0 };
                    if escape > 0 {
                        let sequence = &display.as_bytes()[source..source + escape];
                        inverse = crate::ansi::sgr_inverse(sequence, inverse);
                        let sgr = sequence.starts_with(b"\x1b[") && sequence.ends_with(b"m");
                        if colored && sgr { source_colors.apply(sequence); }
                        buf.extend_from_slice(sequence);
                        if highlighted && sgr { buf.extend_from_slice(if colored { b"\x1b[30;102m" } else { b"\x1b[7m" }); }
                        source += escape;
                    } else {
                        buf.push(display.as_bytes()[source]);
                        source += 1;
                        visible += 1;
                    }
                }
                if highlighted {
                    if colored { source_colors.restore(buf); }
                    else if !inverse { buf.extend_from_slice(b"\x1b[27m"); }
                }
                return;
            }
        }
        buf.extend_from_slice(display.as_bytes());
    }

    fn help_lines() -> impl Iterator<Item = &'static [u8]> {
        let bytes = include_bytes!("less-help.txt").as_slice();
        bytes.strip_suffix(b"\n").unwrap_or(bytes).split(|byte| *byte == b'\n')
    }

    fn help_line(&self, line_index: usize, line: &[u8], width: usize) -> (String, DisplayPrefix, usize, usize) {
        let gutter = self.number_gutter(line_index, width);
        let content_width = width.saturating_sub(gutter.len()).max(1);
        let layout = DisplayPrefix::new(line.to_vec(), Charset::Ascii, true, std::num::NonZeroUsize::new(content_width), ControlDisplay::Standard, &self.tab_stops);
        let rows = layout.columns.div_ceil(content_width).max(1);
        (gutter, layout, content_width, rows)
    }

    fn help_line_start(&self, line: usize) -> usize {
        let width = usize::from(self.term_width).max(1);
        Self::help_lines().take(line).enumerate().map(|(index, bytes)| self.help_line(index, bytes, width).3).sum()
    }

    fn render_help(&mut self, buf: &mut Frame, w: usize, ch: usize) {
        let width = w.max(1);
        self.help_rows = Self::help_lines().enumerate().map(|(index, line)| self.help_line(index, line, width).3).sum();
        self.help_top = self.help_top.min(self.help_rows.saturating_sub(ch));
        let mut document_row = 0;
        let mut row = 0;
        let mut continued = false;
        for (line_index, line) in Self::help_lines().enumerate() {
            if row >= ch { break; }
            let (gutter, layout, content_width, rows) = self.help_line(line_index, line, width);
            for offset in 0..rows {
                if document_row < self.help_top { document_row += 1; continue; }
                if row >= ch { break; }
                if row == 0 || !continued { buf.position(row); }
                Self::write_number_gutter(buf, &gutter, self.use_color);
                buf.extend_from_slice(&layout.display(offset * content_width, width.saturating_sub(gutter.len()), NotationStyle::for_ui_color(self.use_color)));
                buf.wrapped_row(row);
                row += 1;
                document_row += 1;
                continued = offset + 1 < rows && layout.fills_row(offset * content_width, content_width);
                if continued && row == ch { buf.extend_from_slice(b" \x08"); buf.wrapped_row(row); }
                if !continued { terminal::clear_line(buf); }
            }
        }
        while row < ch {
            buf.position(row);
            terminal::clear_line(buf);
            row += 1;
        }
    }

    fn render_status(&self, buf: &mut Frame, w: usize) {
        buf.position(self.term_height.saturating_sub(1) as usize);
        let base_status = self.build_status();
        let emphasized = self.mode == Mode::Follow || (self.mode == Mode::Help && self.help_prefix.is_none()) || (self.mode != Mode::Help && self.filename_prompt && (self.filename.is_some() || self.files.count > 1)) || base_status.starts_with("(END)") || self.status_msg.is_some() || (self.use_color && self.mode == Mode::Normal && self.numeric_prefix == NumericPrefix::Empty);
        let prefix = if self.mode == Mode::Normal && self.filter.is_some() && self.status_msg.is_none() { "& " } else { "" };
        let status = format!("{prefix}{base_status}");
        let mut suffix_width = 0;
        let mut start = status.len();
        for (index, character) in status.char_indices().rev() {
            suffix_width += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            if suffix_width > w { break; }
            start = index;
        }
        let mut columns = 0;
        let mut emphasis_started = false;
        for (offset, character) in status[start..].char_indices() {
            if emphasized && !emphasis_started && start + offset >= prefix.len() {
                if self.use_color {
                    buf.extend_from_slice(if self.status_msg.is_some() { b"\x1b[30;103m" } else { b"\x1b[30;106m" });
                } else { buf.extend_from_slice(b"\x1b[7m"); }
                emphasis_started = true;
            }
            let width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            if columns + width > w { break; }
            let mut bytes = [0; 4];
            buf.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
            columns += width;
        }
        buf.extend_from_slice(b"\x1b[m");
        terminal::clear_line(buf);
    }

    fn build_status(&self) -> String {
        if self.mode == Mode::Normal && let NumericPrefix::Value { value, leading_zeroes } = self.numeric_prefix {
            let zeroes = "0".repeat(leading_zeroes.min(usize::from(self.term_width)));
            return if value == 0 { format!(":{zeroes}") } else { format!(":{zeroes}{value}") };
        }
        if self.mode == Mode::QuitPrefix { return " Z".into(); }
        if let Mode::OptionPrefix(command) = self.mode { return match command { OptionCommand::Toggle => "-", OptionCommand::ToggleQuietly => "-(P)", OptionCommand::Reset => "-+", OptionCommand::ResetQuietly => "-(P)+", OptionCommand::Set => "-!", OptionCommand::SetQuietly => "-(P)!", OptionCommand::Query => "_" }.into(); }
        if let Mode::OptionLong(command, ref name) = self.mode {
            let prefix = match command { OptionCommand::Toggle => "--", OptionCommand::ToggleQuietly => "--(P)", OptionCommand::Reset => "--+", OptionCommand::ResetQuietly => "--(P)+", OptionCommand::Set => "--!", OptionCommand::SetQuietly => "--(P)!", OptionCommand::Query => "__" };
            let name = match name { OptionName::Typing(name) => name.as_str(), OptionName::Raw => "raw-control-chars", OptionName::Color => "RAW-CONTROL-CHARS", OptionName::LineNumbers => "line-numbers", OptionName::DisplayLineNumbers => "LINE-NUMBERS", OptionName::Chop => "chop-long-lines", OptionName::ChopUpper => "CHOP-LONG-LINES", OptionName::UseColor => "use-color", OptionName::UseColorUpper => "USE-COLOR", OptionName::IncrementalSearch => "incsearch", OptionName::IncrementalSearchUpper => "INCSEARCH" };
            return format!("{prefix}{name}");
        }
        if matches!(self.mode, Mode::FileCommand(_)) { return " :".into(); }
        if self.mode == Mode::SetMark { return "set mark: ".into(); }
        if self.mode == Mode::JumpMark { return "goto mark: ".into(); }
        if self.mode == Mode::ExamineInput { return format!("Examine: {}", self.examine_input.content); }
        if self.mode == Mode::SearchInput {
            let ch = if self.search_dir == SearchDir::Forward {
                '/'
            } else {
                '?'
            };
            let nonmatch = if self.search_input_nonmatch { "Non-match " } else { "" };
            let keep = if self.search_input_keep_position { "Keep-pos " } else { "" };
            let wrap = if self.search_input_wrap { "Wrap " } else { "" };
            return format!("{nonmatch}{keep}{wrap}{}{}", ch, self.search_input.content);
        }

        if self.mode == Mode::FilterInput {
            return format!("&/{}", self.filter_input.content);
        }

        if self.mode == Mode::Help {
            if let Some(prefix) = self.help_prefix { return match prefix { HelpPrefix::Colon(_) => " :", HelpPrefix::Quit => " Z" }.into(); }
            return if self.help_top.saturating_add(self.content_height()) >= self.help_rows {
                "HELP -- END -- Press g to see it again, or q when done".to_string()
            } else { "HELP -- Press RETURN for more, or q when done".to_string() };
        }

        if self.mode == Mode::Follow {
            return format!("Waiting for data... (^{} or interrupt to abort)", FOLLOW_INTERRUPT.to_ascii_uppercase());
        }

        if let Some(ref msg) = self.status_msg {
            return msg.clone();
        }

        let name = if self.files.count > 1 {
            let prefix = self.filename.as_ref().map_or_else(String::new, |name| format!("{name} "));
            Some(format!("{prefix}(file {} of {})", self.files.index + 1, self.files.count))
        } else { self.filename.clone() };
        if self.buffer.is_finished() && self.eof_displayed {
            let mut status = if self.filename_prompt { name.map_or_else(|| "(END)".to_owned(), |name| format!("{name} (END)")) } else { "(END)".to_owned() };
            if let Some(next) = &self.files.next_name { status.push_str(&format!(" - Next: {next}")); }
            return status;
        }
        if self.filename_prompt { return name.unwrap_or_else(|| ":".to_owned()); }

        ":".to_owned()
    }
}

#[cfg(test)]
mod one_screen_tests {
    use super::*;

    #[test]
    fn streamed_color_lines_respect_charset_width_and_utf8_validity() {
        let mut input = tempfile::NamedTempFile::new().unwrap();
        input.write_all("\u{1b}[31mé界🦀\n".as_bytes()).unwrap();
        input.write_all(b"\x1b[31m\xe7\x95\n\x1b[31m\xe0\x80\x80\n\x1b[31me\xcc\x81\n").unwrap();
        let mut buffer = LineBuffer::from_file(std::fs::File::open(input.path()).unwrap());
        assert!(matches!(scan_color_line(&mut buffer, 0, 5, Charset::Utf8, &mut io::sink()).unwrap(), ColorLine::Fits(true)));
        assert!(matches!(scan_color_line(&mut buffer, 0, 4, Charset::Utf8, &mut io::sink()).unwrap(), ColorLine::Unsupported));
        assert!(matches!(scan_color_line(&mut buffer, 0, 5, Charset::Ascii, &mut io::sink()).unwrap(), ColorLine::Unsupported));
        for line in 1..=2 {
            assert!(matches!(scan_color_line(&mut buffer, line, 80, Charset::Utf8, &mut io::sink()).unwrap(), ColorLine::Unsupported));
        }
        assert!(matches!(scan_color_line(&mut buffer, 3, 1, Charset::Utf8, &mut io::sink()).unwrap(), ColorLine::Fits(true)));
    }

    #[test]
    fn streamed_color_lines_accept_only_complete_osc8_sequences() {
        let mut input = tempfile::NamedTempFile::new().unwrap();
        input.write_all(b"\x1b]8;id=test;https://example.com\x07link\x1b]8;;\x07\n").unwrap();
        input.write_all(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\\n").unwrap();
        input.write_all(b"\x1b]52;;secret\x07\n\x1b]8;;incomplete\n\x1b]8;;bad\x1bX\n").unwrap();
        let mut buffer = LineBuffer::from_file(std::fs::File::open(input.path()).unwrap());
        for line in 0..=1 {
            assert!(matches!(scan_color_line(&mut buffer, line, 4, Charset::Utf8, &mut io::sink()).unwrap(), ColorLine::Fits(false)));
        }
        for line in 2..=4 {
            assert!(matches!(scan_color_line(&mut buffer, line, 80, Charset::Utf8, &mut io::sink()).unwrap(), ColorLine::Unsupported));
        }
    }

    #[test]
    fn frame_spills_preserve_output_order_and_report_write_errors() {
        let mut input = tempfile::NamedTempFile::new().unwrap();
        input.write_all(b"\x1b[31mone\n").unwrap();
        input.write_all(&b"\x1b[0m".repeat(16_384)).unwrap();
        input.write_all(b"wide\n\x1b[32mtwo\n").unwrap();
        let mut buffer = LineBuffer::from_file(std::fs::File::open(input.path()).unwrap());
        let mut frame = Frame { bytes:Vec::new(), parts:Vec::new(), spill:None, relative_row:None, padding:0, retain_initial_column:false };
        frame.extend_from_slice(b"before");
        assert_eq!(frame.append_color_line(&mut buffer, 0, 3, Charset::Ascii, |_| {}).unwrap(), Some(true));
        frame.extend_from_slice(b" between");
        assert_eq!(frame.append_color_line(&mut buffer, 1, 2, Charset::Ascii, |_| panic!("unsupported line inserted")).unwrap(), None);
        assert_eq!(frame.append_color_line(&mut buffer, 2, 3, Charset::Ascii, |_| {}).unwrap(), Some(true));
        frame.extend_from_slice(b" after");
        let mut output = Vec::new();
        frame.write_to(&mut output).unwrap();
        assert_eq!(output, b"before\x1b[31mone between\x1b[32mtwo after");
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> { Err(io::ErrorKind::BrokenPipe.into()) }
            fn flush(&mut self) -> io::Result<()> { Ok(()) }
        }
        assert_eq!(frame.write_to(&mut Closed).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        frame.clear();
        frame.extend_from_slice(b"fresh");
        let mut output = Vec::new();
        frame.write_to(&mut output).unwrap();
        assert_eq!(output, b"fresh");
    }

    #[test]
    fn jump_target_rejects_invalid_settings() {
        assert!(JumpTarget::parse(b"").is_none());
        assert!(JumpTarget::parse(b".").is_none());
        assert!(JumpTarget::parse(b"2147483648").is_none());
        assert_eq!(JumpTarget::parse(b"3").unwrap().row(24), 2);
        assert_eq!(JumpTarget::parse(b".5").unwrap().row(24), 11);
        assert_eq!(JumpTarget::parse(b"-1").unwrap().row(24), 0);
    }

    #[test]
    fn long_color_line_spills_without_materializing_its_output() {
        let mut input = tempfile::NamedTempFile::new().unwrap();
        let block = b"\x1b[0m".repeat(16_384);
        for _ in 0..32 { input.write_all(&block).unwrap(); }
        input.write_all(b"x\n").unwrap();
        let mut buffer = LineBuffer::from_file(std::fs::File::open(input.path()).unwrap());
        let options = OneScreenOptions { width:80, height:24, charset:Charset::Ascii, controls:ControlDisplay::Color, use_color:false, chop:false, line_numbers:false, line_number_width:Default::default(), tab_stops:Default::default() };
        let OneScreen::Fits(output) = one_screen(&mut buffer, options).unwrap() else { panic!("color line should fit"); };
        assert!(matches!(&output.storage, OneScreenStorage::File(_)));
        let mut written = tempfile::NamedTempFile::new().unwrap();
        output.write_to(&mut written).unwrap();
        assert_eq!(written.as_file().metadata().unwrap().len(), (block.len() * 32 + 11) as u64);
    }
}
