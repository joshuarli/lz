use std::io::{self, Write};
use std::collections::{HashMap, VecDeque};

use crate::ansi::{ControlDisplay, strip_ansi, truncate_to_width, visible_width};
use crate::charset::Charset;
use crate::layout::DisplayPrefix;
use crate::buffer::{LineBuffer, LineFragment};
use crate::history::SearchHistory;
use crate::input::{Key, KeyReader};
use crate::search::{CaseSensitivity, Search};
use crate::terminal;

const FOLLOW_INTERRUPT: char = 'x';

/// Retains the display origin across file switches in one terminal session.
#[derive(Clone, Copy)]
pub struct DisplayAnchor { row: Option<usize> }

impl DisplayAnchor {
    pub fn new(screen: terminal::ScreenMode) -> Self {
        Self { row: (screen == terminal::ScreenMode::Preserve).then_some(0) }
    }
}

struct Frame {
    bytes: Vec<u8>,
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
    Follow,
    Help,
    FileCommand(std::num::NonZeroUsize),
    QuitPrefix,
    SetMark,
    JumpMark,
    CommandError(CommandError),
}

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
struct CommandError { action: ErrorAction, context: CommandContext, remainder: Vec<u8> }

#[derive(Clone, PartialEq)]
enum ErrorAction { Retry(Key), Dismiss }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MarkName(u8);

impl MarkName {
    fn new(character: char) -> Option<Self> {
        (character.is_ascii_alphabetic() || matches!(character, '\'' | '#')).then_some(Self(character as u8))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NumericPrefix { Empty, Value(usize), Overflow }

enum DisplayLine { Mapped(DisplayPrefix), Raw(LineFragment) }

impl DisplayLine {
    fn complete(&self) -> bool { match self { Self::Mapped(prefix) => prefix.complete, Self::Raw(fragment) => fragment.complete } }
    fn columns(&self) -> usize { match self {
        Self::Mapped(prefix) => prefix.columns,
        Self::Raw(fragment) => unicode_width::UnicodeWidthStr::width(fragment.text.as_str()),
    } }
    fn fills_row(&self, start: usize, width: usize) -> bool { match self {
        Self::Mapped(prefix) => prefix.fills_row(start, width),
        Self::Raw(_) => self.columns() >= start.saturating_add(width),
    } }
    fn display(&self, start: usize, width: usize) -> Vec<u8> { match self {
        Self::Mapped(prefix) => prefix.display(start, width),
        Self::Raw(fragment) => truncate_to_width(&fragment.text, start, width, true).into_bytes(),
    } }
}

#[derive(Clone, Copy)]
pub enum PagerExit { Quit, Interrupted, VisitFile(FileVisit) }

/// Select default continuation or exit status 2 after an interrupt.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum InterruptPolicy { #[default] Continue, Quit }

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

/// Numeric scrolling defaults belong to the terminal session, across file visits.
#[derive(Clone, Copy)]
pub struct ScrollDefaults {
    half_page: std::num::NonZeroUsize,
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
        Self { half_page: std::num::NonZeroUsize::new(usize::from(terminal_height).div_ceil(2).max(1)).unwrap(), window: WindowSize::ScreenMinus(1) }
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

pub struct Config {
    pub interrupt_policy: InterruptPolicy,
    pub scroll_defaults: ScrollDefaults,
    pub display_anchor: DisplayAnchor,
    pub files: FileStatus,
    pub initial_display: InitialDisplay,
    pub startup_commands: Vec<Vec<u8>>,
    pub charset: Charset,
    pub controls: ControlDisplay,
    pub chop: bool,
    pub line_numbers: bool,
    pub search_case: CaseSensitivity,
}

pub struct Pager {
    interrupt_policy: InterruptPolicy,
    startup_echo: Option<FileCommand>,
    paint_before_commands: bool,
    scroll_defaults: ScrollDefaults,
    display_anchor: DisplayAnchor,
    buffer: LineBuffer,
    top_line: usize,
    marks: HashMap<MarkName, usize>,
    left_col: usize,
    wrap: bool,
    line_numbers: bool,
    search: Option<Search>,
    search_case: CaseSensitivity,
    search_dir: SearchDir,
    current_match_line: Option<usize>,
    mode: Mode,
    search_input: LineEditor,
    filter_input: LineEditor,
    filter: Option<Search>,
    filtered_lines: Vec<usize>,
    history: SearchHistory,
    term_width: u16,
    term_height: u16,
    charset: Charset,
    controls: ControlDisplay,
    status_msg: Option<String>,
    filename: Option<String>,
    filename_prompt: bool,
    compact_initial: bool,
    startup_commands: VecDeque<Vec<u8>>,
    numeric_prefix: NumericPrefix,
    content_painted: bool,
    files: FileStatus,
    exit: PagerExit,
    command_context: CommandContext,
    bell_requested: bool,
}

impl Pager {
    pub fn new(
        buffer: LineBuffer,
        config: Config,
        filename: Option<String>,
        history: SearchHistory,
    ) -> Self {
        let (w, h) = terminal::get_terminal_size();
        Pager {
            interrupt_policy: config.interrupt_policy,
            startup_echo: match config.initial_display { InitialDisplay::StartupFileSwitch(switch) => Some(switch.command), _ => None },
            paint_before_commands: matches!(config.initial_display, InitialDisplay::StartupFileSwitch(StartupFileSwitch { continuation: CommandContinuation::NextCommand, .. })) && !config.startup_commands.is_empty(),
            scroll_defaults: config.scroll_defaults,
            display_anchor: config.display_anchor,
            buffer,
            top_line: match config.initial_display { InitialDisplay::FileSwitch(Some(position)) => position.line, _ => 0 },
            marks: HashMap::new(),
            left_col: 0,
            wrap: !config.chop,
            line_numbers: config.line_numbers,
            search: None,
            search_case: config.search_case,
            search_dir: SearchDir::Forward,
            current_match_line: None,
            mode: Mode::Normal,
            search_input: LineEditor::new(),
            filter_input: LineEditor::new(),
            filter: None,
            filtered_lines: Vec::new(),
            history,
            term_width: w,
            term_height: h,
            charset: config.charset,
            controls: config.controls,
            status_msg: None,
            filename,
            filename_prompt: true,
            compact_initial: config.initial_display == InitialDisplay::Startup && config.startup_commands.is_empty(),
            startup_commands: config.startup_commands.into(),
            numeric_prefix: NumericPrefix::Empty,
            content_painted: false,
            files: config.files,
            exit: PagerExit::Quit,
            command_context: CommandContext::StartupUnpositioned,
            bell_requested: false,
        }
    }

    /// Retain the first displayed source line for a later visit to this file.
    pub fn saved_position(&self) -> Option<SavedPosition> {
        if !self.content_painted { return None; }
        self.display_line_idx(self.top_line).filter(|line| *line < self.buffer.len()).map(|line| SavedPosition { line })
    }

    pub fn display_anchor(&self) -> DisplayAnchor { self.display_anchor }

    pub fn scroll_defaults(&self) -> ScrollDefaults { self.scroll_defaults }

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

    fn run_startup_commands(&mut self, keys: &mut KeyReader) -> io::Result<bool> {
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

    pub fn run(&mut self, keys: &mut KeyReader) -> io::Result<PagerExit> {
        if std::mem::take(&mut self.paint_before_commands) {
            self.render()?;
            self.command_context = CommandContext::Interactive;
        }
        if self.run_startup_commands(keys)? { return self.finish_run(); }

        let mut redraw = true;
        loop {
            if self.command_context != CommandContext::Interactive && !matches!(self.mode, Mode::CommandError(_)) {
                if self.run_startup_commands(keys)? { return self.finish_run(); }
                redraw = true;
            }
            if terminal::TERMINATION_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 {
                return self.finish_run();
            }
            if terminal::take_interrupt() {
                if self.interrupt(keys) { return self.finish_run(); }
                redraw = true;
            }
            if terminal::TSTP_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                terminal::handle_suspend()?;
            }
            if terminal::CONT_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst)
                && terminal::handle_resume()? == terminal::ResumeOutcome::Restored {
                self.filename_prompt = false;
                let (w, h) = terminal::get_terminal_size();
                self.term_width = w;
                self.term_height = h;
                redraw = true;
                continue;
            }
            if terminal::WINCH_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let (w, h) = terminal::get_terminal_size();
                self.term_width = w;
                self.term_height = h;
                redraw = true;
                continue;
            }

            if self.buffer.refresh_waiting() { redraw = true; }
            if let Some(error) = self.buffer.take_error() { return Err(error); }
            if redraw { self.render()?; redraw = false; }

            let key = if self.mode == Mode::Follow {
                let new_lines = self.buffer.poll_new_lines();
                if new_lines > 0 {
                    self.scroll_to_bottom();
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
            let quit_requested = if key == Key::Ctrl('c') { self.interrupt(keys) } else { self.handle_key(key) };
            if quit_requested {
                return self.finish_run();
            }
        }
    }

    fn finish_run(&mut self) -> io::Result<PagerExit> {
        if matches!(self.exit, PagerExit::Interrupted) && std::mem::take(&mut self.bell_requested) {
            let mut output = io::stdout().lock();
            output.write_all(&[7])?;
            output.flush()?;
        }
        Ok(self.exit)
    }

    fn interrupt(&mut self, keys: &mut KeyReader) -> bool {
        if self.interrupt_policy == InterruptPolicy::Quit {
            match self.mode {
                Mode::SearchInput => self.handle_search_input_key(Key::Ctrl('c')),
                Mode::FilterInput => self.handle_filter_input_key(Key::Ctrl('c')),
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
        if matches!(self.mode, Mode::SetMark | Mode::JumpMark | Mode::FileCommand(_) | Mode::CommandError(_)) {
            self.mode = Mode::Normal;
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

    fn handle_key(&mut self, key: Key) -> bool {
        match self.mode.clone() {
            Mode::Normal => self.handle_normal_key(key),
            Mode::SetMark | Mode::JumpMark => { self.handle_mark_key(key); false }
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
                    Key::Char('q' | 'Q') => return true,
                    Key::Escape => {},
                    _ => self.bell_requested = true,
                }
                false
            }
            Mode::SearchInput => {
                self.handle_search_input_key(key);
                false
            }
            Mode::FilterInput => {
                self.handle_filter_input_key(key);
                false
            }
            Mode::Help => {
                match key {
                    Key::Char('h') | Key::Char('q') | Key::Escape => self.mode = Mode::Normal,
                    _ => {}
                }
                false
            }
            Mode::Follow => self.handle_follow_key(key),
            Mode::CommandError(error) => {
                self.mode = Mode::Normal;
                self.command_context = error.context;
                self.status_msg = None;
                if let ErrorAction::Retry(command) = error.action && self.handle_normal_key(command) { return true; }
                if self.command_context == CommandContext::StartupAborted { self.command_context = CommandContext::StartupUnpositioned; }
                else if !error.remainder.is_empty() { self.startup_commands.push_front(error.remainder); }
                if !key.is_prompt_acknowledgement() { return self.handle_key(key); }
                false
            }
        }
    }

    fn handle_normal_key(&mut self, key: Key) -> bool {
        self.filename_prompt = false;
        self.status_msg = None;
        if let Key::Char(character @ '0'..='9') = key {
            let previous = match self.numeric_prefix { NumericPrefix::Empty => Some(0), NumericPrefix::Value(value) => Some(value), NumericPrefix::Overflow => None };
            let value = previous.and_then(|value| value.checked_mul(10)).and_then(|value| value.checked_add(character as usize - '0' as usize))
                .filter(|value| *value <= i64::MAX as usize);
            self.numeric_prefix = value.map_or(NumericPrefix::Overflow, NumericPrefix::Value);
            return false;
        }
        let count = match std::mem::replace(&mut self.numeric_prefix, NumericPrefix::Empty) {
            NumericPrefix::Empty => None,
            NumericPrefix::Value(value) => (value != 0).then_some(value),
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
        match key {
            Key::Char('q' | 'Q') | Key::Escape => return true,
            Key::Ctrl('c') => self.bell_requested = true,
            Key::Char('Z') => self.mode = Mode::QuitPrefix,
            Key::Char(':') => self.mode = Mode::FileCommand(std::num::NonZeroUsize::new(count.unwrap_or(1)).unwrap()),
            Key::Char('m') => self.mode = Mode::SetMark,
            Key::Char('\'') => self.mode = Mode::JumpMark,

            Key::Char('j' | 'e') | Key::Ctrl('e' | 'n') | Key::Down | Key::Enter => self.scroll_down(count.unwrap_or(1)),
            Key::Char('k' | 'y') | Key::Ctrl('y' | 'p' | 'k') | Key::Up => self.scroll_up(count.unwrap_or(1)),
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
                if amount == 0 { self.bell_requested |= terminal::request_eof_bell(std::time::SystemTime::now()); }
                else if matches!(key, Key::Char('b' | 'w') | Key::PageUp | Key::Ctrl('b')) { self.scroll_up(amount); }
                else { self.scroll_down(amount); }
            }
            Key::Char('g' | '<') | Key::Home => {
                let previous = self.top_line;
                self.position_initialized();
                let target = count.unwrap_or(1).saturating_sub(1);
                if self.command_context == CommandContext::Interactive && self.content_painted && self.top_line == target { self.bell_requested = true; }
                self.top_line = target;
                self.buffer.get_line(self.top_line);
                self.remember_jump_from(previous);
                self.left_col = 0;
            }
            Key::Char('G' | '>') | Key::End => {
                let previous = self.top_line;
                self.position_initialized();
                if let Some(count) = count { self.top_line = count.saturating_sub(1); self.buffer.get_line(self.top_line); }
                else { self.buffer.read_all(); self.scroll_to_bottom(); }
                self.remember_jump_from(previous);
            }
            Key::Right => {
                if !self.wrap {
                    self.left_col = self.left_col.saturating_add(8);
                }
            }
            Key::Left => {
                if !self.wrap {
                    self.left_col = self.left_col.saturating_sub(8);
                }
            }
            Key::Char('/') => {
                self.search_dir = SearchDir::Forward;
                self.mode = Mode::SearchInput;
                self.search_input.clear();
                self.history.reset_cursor();
            }
            Key::Char('?') => {
                self.search_dir = SearchDir::Backward;
                self.mode = Mode::SearchInput;
                self.search_input.clear();
                self.history.reset_cursor();
            }
            Key::Char('n') => self.search_same_dir(),
            Key::Char('N') => self.search_opposite_dir(),
            Key::Char('&') => {
                self.mode = Mode::FilterInput;
                self.filter_input.clear();
            }
            Key::Char('F') => {
                self.position_initialized();
                self.mode = Mode::Follow;
                self.buffer.read_all();
                self.scroll_to_bottom();
            }
            Key::Char('h') => {
                self.mode = Mode::Help;
            }
            _ => {}
        }
        false
    }

    fn command_error(&mut self, message: String, action: ErrorAction) {
        self.status_msg = Some(format!("{message}  (press RETURN)"));
        self.mode = Mode::CommandError(CommandError { action, context: self.command_context, remainder: Vec::new() });
    }

    fn remember_jump_from(&mut self, previous_line: usize) {
        let height = self.content_height();
        // less 668 can scroll backward by exactly one screen without replacing the last mark.
        let large = self.top_line >= previous_line.saturating_add(height) || previous_line > self.top_line.saturating_add(height);
        if self.content_painted && previous_line < self.buffer.len() && large { self.marks.insert(MarkName(b'\''), previous_line); }
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
                '^' => { let previous = self.top_line; self.top_line = 0; self.remember_jump_from(previous); return; }
                '$' => { let previous = self.top_line; self.buffer.read_all(); self.scroll_to_bottom(); self.remember_jump_from(previous); return; }
                '.' => return,
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
            if self.content_painted && self.top_line < self.buffer.len() { self.marks.insert(name, self.top_line); }
            else { self.bell_requested = true; }
        }
        else if let Some(line) = self.marks.get(&name).copied().or_else(|| (character == '\'').then_some(0)) {
            let previous = self.top_line;
            self.position_initialized();
            self.top_line = line;
            self.remember_jump_from(previous);
        } else { self.command_error("Mark not set".into(), ErrorAction::Dismiss); }
    }

    fn position_initialized(&mut self) {
        self.startup_echo = None;
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
        match key {
            Key::Escape => self.mode = Mode::Normal,
            Key::Ctrl('c') => { self.history.push(&self.search_input.content); self.mode = Mode::Normal; self.bell_requested = true; }
            Key::Enter => {
                let pattern = self.search_input.content.clone();
                self.mode = Mode::Normal;
                self.history.push(&pattern);
                if pattern.is_empty() { return; }
                match Search::new(&pattern, self.search_case) {
                    Ok(s) => {
                        self.search = Some(s);
                        self.current_match_line = None;
                        self.search_same_dir();
                    }
                    Err(e) => {
                        self.status_msg = Some(e);
                    }
                }
            }
            Key::Up => {
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
            Key::Backspace => self.search_input.backspace(),
            Key::Delete => self.search_input.delete(),
            Key::Left => self.search_input.move_left(),
            Key::Right => self.search_input.move_right(),
            Key::Home => self.search_input.move_home(),
            Key::End => self.search_input.move_end(),
            Key::Char(ch) => self.search_input.insert(ch),
            _ => {}
        }
    }

    fn search_same_dir(&mut self) {
        let forward = self.search_dir == SearchDir::Forward;
        self.do_search(forward);
    }

    fn search_opposite_dir(&mut self) {
        let forward = self.search_dir == SearchDir::Backward;
        self.do_search(forward);
    }

    fn do_search(&mut self, forward: bool) {
        let search = match self.search.take() {
            Some(s) => s,
            None => return,
        };
        let from = if forward {
            self.current_match_line
                .map(|l| l + 1)
                .unwrap_or(self.top_line)
        } else {
            self.current_match_line.unwrap_or(self.top_line)
        };
        match search.find_next_line(&mut self.buffer, from, forward) {
            Some(line) => {
                self.position_initialized();
                self.current_match_line = Some(line);
                self.top_line = line;
            }
            None => {
                self.status_msg = Some("Pattern not found".to_string());
            }
        }
        self.search = Some(search);
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
                match Search::new(&pattern, self.search_case) {
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
        self.filter = None;
        self.filtered_lines.clear();
    }

    fn scroll_down(&mut self, lines: usize) {
        if self.wrap {
            self.scroll_down_wrapped(lines);
            return;
        }
        self.top_line = self.top_line.saturating_add(lines);
        self.clamp_top_line();
    }

    fn scroll_up(&mut self, lines: usize) {
        if self.wrap {
            self.scroll_up_wrapped(lines);
            return;
        }
        self.top_line = self.top_line.saturating_sub(lines);
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
        match self.buffer.get_line(line_idx) {
            Some(line) => {
                let vw = if self.controls == ControlDisplay::Raw {
                    unicode_width::UnicodeWidthStr::width(line)
                } else {
                    visible_width(line)
                };
                if vw == 0 {
                    1
                } else {
                    vw.div_ceil(w)
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

    fn render(&mut self) -> io::Result<()> {
        if !self.content_painted && !matches!(self.mode, Mode::CommandError(_)) {
            self.buffer.byte_prefix(0, 0)?;
            if self.buffer.is_empty() && self.buffer.state() == crate::buffer::SourceState::Waiting { return Ok(()); }
        }
        let mut buf = Frame { bytes: Vec::with_capacity(4096), relative_row: self.display_anchor.row, padding: 0, retain_initial_column: false };
        let w = self.term_width as usize;
        let ch = self.content_height();

        if self.bell_requested { buf.push(7); self.bell_requested = false; }
        buf.extend_from_slice(b"\x1b[?25l");
        if matches!(self.mode, Mode::CommandError(_)) {
            if !self.content_painted { for row in 0..ch { buf.position(row); terminal::clear_line(&mut buf); } }
            if let Some(row) = self.display_anchor.row { buf.padding = ch.saturating_sub(row); }
            self.render_status(&mut buf, w);
            buf.extend_from_slice(b"\x1b[?25h");
            let mut stdout = io::stdout().lock();
            stdout.write_all(&buf)?;
            return stdout.flush();
        }
        let first_row = if self.mode == Mode::Help { 0 } else { self.initial_content_row(w, ch)? };
        let primary = self.display_anchor.row.is_some();
        if let Some(command) = self.startup_echo.take() {
            if !primary { terminal::move_cursor(&mut buf, self.term_height.saturating_sub(1), 0); }
            buf.extend_from_slice(b"\r\x1b[K :");
            buf.push(command.key());
            buf.relative_row = Some(0);
            buf.retain_initial_column = true;
        }
        if buf.relative_row.is_some() { buf.padding = first_row; }
        else { for row in 0..first_row { buf.position(row); terminal::clear_line(&mut buf); } }
        if self.mode == Mode::Help {
            Self::render_help(&mut buf, w, ch);
        } else if self.wrap {
            self.render_wrapped(&mut buf, w, ch, first_row)?;
        } else {
            self.render_nowrap(&mut buf, w, ch, first_row)?;
        }

        if let Some(error) = self.buffer.take_error() { return Err(error); }
        self.render_status(&mut buf, w);
        self.display_anchor.row = if primary { buf.relative_row } else { None };
        if self.display_anchor.row.is_none() { self.compact_initial = false; }
        self.content_painted = true;
        buf.extend_from_slice(b"\x1b[?25h");

        let mut stdout = io::stdout().lock();
        stdout.write_all(&buf)?;
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
        if !self.line_numbers { return String::new(); }
        let gutter = format!("{:>7} ", line + 1);
        gutter.chars().take(width).collect()
    }

    fn write_number_gutter(buf: &mut Vec<u8>, gutter: &str) {
        if let Some(first) = gutter.bytes().position(|byte| byte.is_ascii_digit()) {
            let last = gutter.bytes().rposition(|byte| byte.is_ascii_digit()).unwrap() + 1;
            buf.extend_from_slice(&gutter.as_bytes()[..first]);
            buf.extend_from_slice(b"\x1b[1m");
            buf.extend_from_slice(&gutter.as_bytes()[first..last]);
            buf.extend_from_slice(b"\x1b[m");
            buf.extend_from_slice(&gutter.as_bytes()[last..]);
        } else { buf.extend_from_slice(gutter.as_bytes()); }
    }

    fn display_prefix(&mut self, line: usize, bytes: usize) -> io::Result<Option<DisplayLine>> {
        if self.controls != ControlDisplay::Raw {
            let width = self.term_width as usize;
            let wrap_width = self.wrap.then(|| std::num::NonZeroUsize::new(width.saturating_sub(self.number_gutter(line, width).len()).max(1)).unwrap());
            Ok(self.buffer.byte_prefix(line, bytes)?.map(|fragment| DisplayLine::Mapped(DisplayPrefix::new(fragment.bytes, self.charset, fragment.complete, wrap_width, self.controls))))
        } else {
            Ok(self.buffer.line_prefix(line, bytes)?.map(DisplayLine::Raw))
        }
    }

    fn wrapped_prefix_rows(&self, fragment: &DisplayLine, actual: usize, width: usize, available: usize) -> usize {
        if !fragment.complete() { return available; }
        let columns = width.saturating_sub(self.number_gutter(actual, width).len()).max(1);
        fragment.columns().div_ceil(columns).max(1)
    }

    fn initial_content_row(&mut self, width: usize, height: usize) -> io::Result<usize> {
        if !self.compact_initial { return Ok(0); }
        let mut rows = 0;
        let mut index = self.top_line;
        while rows < height {
            let Some(actual) = self.display_line_idx(index) else { return Ok(0); };
            let Some(fragment) = self.display_prefix(actual, width.saturating_mul(height - rows).saturating_add(1).saturating_mul(16).max(4096))? else {
                return Ok(if self.buffer.is_finished() { height - rows } else { 0 });
            };
            rows += if self.wrap { self.wrapped_prefix_rows(&fragment, actual, width, height - rows) } else { 1 };
            index += 1;
        }
        Ok(0)
    }

    fn write_eof_marker(&self, buf: &mut Vec<u8>) {
        if !(self.compact_initial && self.buffer.is_finished()) { buf.extend_from_slice(b"\x1b[1m~\x1b[m"); }
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
            let display_idx = self.top_line.saturating_add(row - first_row);
            let actual = self.display_line_idx(display_idx);
            let fragment = match actual { Some(index) => self.display_prefix(index, self.left_col.saturating_add(w).saturating_add(1).saturating_mul(16).max(4096))?, None => None };
            match actual.zip(fragment) {
                Some((line_idx, fragment)) => {
                    let gutter = self.number_gutter(line_idx, w);
                    Self::write_number_gutter(buf, &gutter);
                    let content_width = w.saturating_sub(gutter.len());
                    let line_width = fragment.columns();
                    let chopped = (!fragment.complete() || line_width > self.left_col.saturating_add(content_width)) && content_width > 0;
                    let display = fragment.display(self.left_col, content_width.saturating_sub(usize::from(chopped)));
                    self.write_line_with_search(buf, &display);
                    if chopped { buf.extend_from_slice(b"\x1b[7m>\x1b[m"); }
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
                    let gutter_width = self.number_gutter(actual, w).len();
                    let content_width = w.saturating_sub(gutter_width).max(1);
                    let rows_needed = self.wrapped_prefix_rows(&fragment, actual, w, ch - screen_row);
                    let available_rows = ch - screen_row;
                    for wrap_row in 0..rows_needed {
                        if screen_row >= ch { break; }
                        if wrap_row == 0 { buf.position(screen_row); }
                        let gutter = self.number_gutter(actual, w);
                        Self::write_number_gutter(buf, &gutter);
                        let display = fragment.display(wrap_row * content_width, w.saturating_sub(gutter.len()));
                        self.write_line_with_search(buf, &display);
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

    fn write_line_with_search(&self, buf: &mut Vec<u8>, display: &[u8]) {
        let Ok(display) = std::str::from_utf8(display) else { buf.extend_from_slice(display); return; };
        if let Some(ref search) = self.search
            && search.is_match(display)
        {
            {
                let stripped = strip_ansi(display);
                let matches = search.find_matches_stripped(&stripped);
                if !matches.is_empty() {
                    let mut result = String::with_capacity(stripped.len() + matches.len() * 16);
                    let mut last_end = 0;
                    for (start, end) in &matches {
                        if *start > last_end {
                            result.push_str(&stripped[last_end..*start]);
                        }
                        result.push_str("\x1b[7m");
                        result.push_str(&stripped[*start..*end]);
                        result.push_str("\x1b[m");
                        last_end = *end;
                    }
                    if last_end < stripped.len() {
                        result.push_str(&stripped[last_end..]);
                    }
                    buf.extend_from_slice(result.as_bytes());
                    return;
                }
            }
        }
        buf.extend_from_slice(display.as_bytes());
    }

    fn render_help(buf: &mut Frame, w: usize, ch: usize) {
        const HELP: &[&str] = &[
            "",
            "  NAVIGATION",
            "    j / Down / Enter   Scroll down one line",
            "    k / Up             Scroll up one line",
            "    d / Ctrl-D         Half page down",
            "    u / Ctrl-U         Half page up",
            "    Space / PgDn       Page down",
            "    b / PgUp           Page up",
            "    g / Home           Go to top",
            "    G / End            Go to bottom",
            "    Left / Right       Horizontal scroll",
            "",
            "  SEARCH",
            "    /                  Search forward",
            "    ?                  Search backward",
            "    n                  Next match",
            "    N                  Previous match",
            "    &                  Filter lines by pattern",
            "",
            "  OTHER",
            "    z / w              Forward / backward window",
            "    F                  Follow mode (tail -f)",
            "    h                  Toggle this help",
            "    q / Q / :q / :Q / ZZ  Quit",
        ];

        for row in 0..ch {
            buf.position(row);
            if let Some(line) = HELP.get(row) {
                let truncated = if line.len() > w { &line[..w] } else { line };
                buf.extend_from_slice(truncated.as_bytes());
            }
            terminal::clear_line(buf);
        }
    }

    fn render_status(&self, buf: &mut Frame, w: usize) {
        buf.position(self.term_height.saturating_sub(1) as usize);
        let status = self.build_status();
        let emphasized = self.mode == Mode::Follow || (self.filename_prompt && (self.filename.is_some() || self.files.count > 1)) || status.starts_with("(END)") || self.status_msg.is_some();
        if emphasized { buf.extend_from_slice(b"\x1b[7m"); }
        let mut suffix_width = 0;
        let mut start = status.len();
        for (index, character) in status.char_indices().rev() {
            suffix_width += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            if suffix_width > w { break; }
            start = index;
        }
        let mut columns = 0;
        for character in status[start..].chars() {
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
        if self.mode == Mode::QuitPrefix { return " Z".into(); }
        if matches!(self.mode, Mode::FileCommand(_)) { return " :".into(); }
        if self.mode == Mode::SetMark { return "set mark: ".into(); }
        if self.mode == Mode::JumpMark { return "goto mark: ".into(); }
        if self.mode == Mode::SearchInput {
            let ch = if self.search_dir == SearchDir::Forward {
                '/'
            } else {
                '?'
            };
            return format!("{}{}", ch, self.search_input.content);
        }

        if self.mode == Mode::FilterInput {
            return format!("&/{}", self.filter_input.content);
        }

        if self.mode == Mode::Help {
            return "h: return".to_string();
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
        if self.buffer.is_finished() && self.top_line.saturating_add(self.content_height()) >= self.display_line_count() {
            let mut status = if self.filename_prompt { name.map_or_else(|| "(END)".to_owned(), |name| format!("{name} (END)")) } else { "(END)".to_owned() };
            if let Some(next) = &self.files.next_name { status.push_str(&format!(" - Next: {next}")); }
            return status;
        }
        if self.filename_prompt { return name.unwrap_or_else(|| ":".to_owned()); }

        ":".to_owned()
    }
}
