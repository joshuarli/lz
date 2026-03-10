use std::io::{self, Write};

use crate::ansi::{strip_ansi, truncate_to_width, visible_width};
use crate::buffer::LineBuffer;
use crate::history::SearchHistory;
use crate::input::{Key, KeyReader};
use crate::search::Search;
use crate::terminal;

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Normal,
    SearchInput,
    FilterInput,
    Follow,
    Help,
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

pub struct Pager {
    buffer: LineBuffer,
    top_line: usize,
    left_col: usize,
    wrap: bool,
    search: Option<Search>,
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
    follow: bool,
    raw_mode: bool,
    status_msg: Option<String>,
    filename: Option<String>,
}

impl Pager {
    pub fn new(
        buffer: LineBuffer,
        follow: bool,
        raw_mode: bool,
        filename: Option<String>,
        history: SearchHistory,
    ) -> Self {
        let (w, h) = terminal::get_terminal_size();
        Pager {
            buffer,
            top_line: 0,
            left_col: 0,
            wrap: false,
            search: None,
            search_dir: SearchDir::Forward,
            current_match_line: None,
            mode: if follow { Mode::Follow } else { Mode::Normal },
            search_input: LineEditor::new(),
            filter_input: LineEditor::new(),
            filter: None,
            filtered_lines: Vec::new(),
            history,
            term_width: w,
            term_height: h,
            follow,
            raw_mode,
            status_msg: None,
            filename,
        }
    }

    pub fn history(&self) -> &SearchHistory {
        &self.history
    }

    fn content_height(&self) -> usize {
        if self.term_height > 1 {
            (self.term_height - 1) as usize
        } else {
            1
        }
    }

    pub fn run(&mut self, keys: &mut KeyReader) -> io::Result<()> {
        if self.follow {
            self.buffer.read_all();
            self.scroll_to_bottom();
        }

        loop {
            self.render()?;

            if terminal::TERM_FLAG.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(());
            }
            if terminal::TSTP_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                terminal::handle_suspend();
            }
            if terminal::CONT_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                terminal::handle_resume();
                let (w, h) = terminal::get_terminal_size();
                self.term_width = w;
                self.term_height = h;
                continue;
            }
            if terminal::WINCH_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let (w, h) = terminal::get_terminal_size();
                self.term_width = w;
                self.term_height = h;
                continue;
            }

            let key = if self.mode == Mode::Follow {
                let new_lines = self.buffer.poll_new_lines();
                if new_lines > 0 {
                    self.scroll_to_bottom();
                }
                match keys.read_key_timeout(100)? {
                    Some(k) => k,
                    None => continue,
                }
            } else {
                keys.read_key()?
            };

            if self.handle_key(key) {
                return Ok(());
            }
        }
    }

    fn handle_key(&mut self, key: Key) -> bool {
        match self.mode {
            Mode::Normal => self.handle_normal_key(key),
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
        }
    }

    fn handle_normal_key(&mut self, key: Key) -> bool {
        self.status_msg = None;
        match key {
            Key::Char('q') | Key::Ctrl('c') | Key::Escape => return true,

            Key::Char('j') | Key::Down | Key::Enter => self.scroll_down(1),
            Key::Char('k') | Key::Up => self.scroll_up(1),
            Key::Char('d') | Key::Ctrl('d') => {
                let half = self.content_height() / 2;
                self.scroll_down(half.max(1));
            }
            Key::Char('u') | Key::Ctrl('u') => {
                let half = self.content_height() / 2;
                self.scroll_up(half.max(1));
            }
            Key::Char(' ') | Key::PageDown | Key::Ctrl('f') => {
                let page = self.content_height().saturating_sub(1).max(1);
                self.scroll_down(page);
            }
            Key::Char('b') | Key::PageUp | Key::Ctrl('b') => {
                let page = self.content_height().saturating_sub(1).max(1);
                self.scroll_up(page);
            }
            Key::Char('g') | Key::Home => {
                self.top_line = 0;
                self.left_col = 0;
            }
            Key::Char('G') | Key::End => {
                self.buffer.read_all();
                self.scroll_to_bottom();
            }
            Key::Right => {
                if !self.wrap {
                    self.left_col += 8;
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
            Key::Char('w') => {
                self.wrap = !self.wrap;
                self.left_col = 0;
            }
            Key::Char('F') => {
                self.mode = Mode::Follow;
                self.follow = true;
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

    fn handle_follow_key(&mut self, key: Key) -> bool {
        matches!(key, Key::Char('q') | Key::Escape | Key::Ctrl('c'))
    }

    fn handle_search_input_key(&mut self, key: Key) {
        match key {
            Key::Escape | Key::Ctrl('c') => {
                self.mode = Mode::Normal;
            }
            Key::Enter => {
                let pattern = self.search_input.content.clone();
                self.mode = Mode::Normal;
                if pattern.is_empty() {
                    return;
                }
                self.history.push(&pattern);
                match Search::new(&pattern) {
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
                if let Some(entry) = self.history.prev(&self.search_input.content) {
                    self.search_input.content = entry.to_string();
                    self.search_input.cursor = self.search_input.content.len();
                }
            }
            Key::Down => match self.history.next() {
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
            Key::Escape | Key::Ctrl('c') => {
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
                match Search::new(&pattern) {
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
            if let Some(line) = self.buffer.lines.get(i) {
                if filter.is_match(line) {
                    self.filtered_lines.push(i);
                }
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
        self.top_line += lines;
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
            self.buffer.get_line(self.top_line + ch);
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
                let vw = if self.raw_mode {
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
                self.top_line += 1;
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
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let w = self.term_width as usize;
        let ch = self.content_height();

        buf.extend_from_slice(b"\x1b[?25l");
        terminal::move_cursor(&mut buf, 0, 0);

        if self.mode == Mode::Help {
            Self::render_help(&mut buf, w, ch);
        } else if self.wrap {
            self.render_wrapped(&mut buf, w, ch);
        } else {
            self.render_nowrap(&mut buf, w, ch);
        }

        self.render_status(&mut buf, w);

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

    fn render_nowrap(&mut self, buf: &mut Vec<u8>, w: usize, ch: usize) {
        // Pre-ensure all lines are loaded so we can borrow buffer.lines directly
        if self.filter.is_none() {
            self.buffer.get_line(self.top_line + ch);
        }
        for row in 0..ch {
            terminal::move_cursor(buf, row as u16, 0);
            let display_idx = self.top_line + row;
            match self.display_line_idx(display_idx) {
                Some(line_idx) => match self.buffer.lines.get(line_idx) {
                    Some(line) => {
                        let display = truncate_to_width(line, self.left_col, w, self.raw_mode);
                        self.write_line_with_search(buf, &display, line_idx);
                    }
                    None => {
                        buf.push(b'~');
                    }
                },
                None => {
                    buf.push(b'~');
                }
            }
            terminal::clear_line(buf);
        }
    }

    fn render_wrapped(&mut self, buf: &mut Vec<u8>, w: usize, ch: usize) {
        let mut screen_row = 0;
        let mut display_idx = self.top_line;

        // Pre-ensure a reasonable number of lines are loaded
        if self.filter.is_none() {
            self.buffer.get_line(self.top_line + ch);
        }

        while screen_row < ch {
            let actual_idx = self.display_line_idx(display_idx);
            let line_opt = actual_idx.and_then(|i| self.buffer.lines.get(i));
            match line_opt {
                Some(line) => {
                    let actual = actual_idx.unwrap();
                    let total_width = if self.raw_mode {
                        unicode_width::UnicodeWidthStr::width(line.as_str())
                    } else {
                        visible_width(line)
                    };
                    let rows_needed = if total_width == 0 {
                        1
                    } else {
                        total_width.div_ceil(w)
                    };

                    for wrap_row in 0..rows_needed {
                        if screen_row >= ch {
                            break;
                        }
                        terminal::move_cursor(buf, screen_row as u16, 0);
                        let start_col = wrap_row * w;
                        let display = truncate_to_width(line, start_col, w, self.raw_mode);
                        if wrap_row == 0 {
                            self.write_line_with_search(buf, &display, actual);
                        } else {
                            buf.extend_from_slice(display.as_bytes());
                        }
                        terminal::clear_line(buf);
                        screen_row += 1;
                    }
                    display_idx += 1;
                }
                None => {
                    terminal::move_cursor(buf, screen_row as u16, 0);
                    buf.push(b'~');
                    terminal::clear_line(buf);
                    screen_row += 1;
                    display_idx += 1;
                }
            }
        }
    }

    fn write_line_with_search(&self, buf: &mut Vec<u8>, display: &str, line_idx: usize) {
        if let Some(ref search) = self.search {
            if let Some(original_line) = self.buffer.lines.get(line_idx) {
                if search.is_match(original_line) {
                    let stripped = strip_ansi(display);
                    let matches = search.find_matches_stripped(&stripped);
                    if !matches.is_empty() {
                        let mut result = String::with_capacity(stripped.len() + matches.len() * 16);
                        let mut last_end = 0;
                        for (start, end) in &matches {
                            if *start > last_end {
                                result.push_str(&stripped[last_end..*start]);
                            }
                            result.push_str("\x1b[30;43m");
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
        }
        buf.extend_from_slice(display.as_bytes());
    }

    fn render_help(buf: &mut Vec<u8>, w: usize, ch: usize) {
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
            "    w                  Toggle line wrap",
            "    F                  Follow mode (tail -f)",
            "    h                  Toggle this help",
            "    q / Esc            Quit",
        ];

        for row in 0..ch {
            terminal::move_cursor(buf, row as u16, 0);
            if let Some(line) = HELP.get(row) {
                let truncated = if line.len() > w { &line[..w] } else { line };
                buf.extend_from_slice(truncated.as_bytes());
            }
            terminal::clear_line(buf);
        }
    }

    fn render_status(&self, buf: &mut Vec<u8>, w: usize) {
        const VERSION: &str = concat!("lz ", env!("CARGO_PKG_VERSION"));

        terminal::move_cursor(buf, self.term_height - 1, 0);
        buf.extend_from_slice(b"\x1b[7m");

        let status = self.build_status();
        let truncated = if status.len() > w {
            &status[..w]
        } else {
            status.as_str()
        };
        buf.extend_from_slice(truncated.as_bytes());

        let used = truncated.len();
        let right_start = w.saturating_sub(VERSION.len());
        if right_start > used {
            for _ in 0..(right_start - used) {
                buf.push(b' ');
            }
            buf.extend_from_slice(VERSION.as_bytes());
        } else {
            for _ in 0..w.saturating_sub(used) {
                buf.push(b' ');
            }
        }

        buf.extend_from_slice(b"\x1b[m");
    }

    fn build_status(&self) -> String {
        if self.mode == Mode::SearchInput {
            let ch = if self.search_dir == SearchDir::Forward {
                '/'
            } else {
                '?'
            };
            return format!("{}{}", ch, self.search_input.content);
        }

        if self.mode == Mode::FilterInput {
            return format!("&{}", self.filter_input.content);
        }

        if self.mode == Mode::Help {
            return "h: return".to_string();
        }

        if self.mode == Mode::Follow {
            return "Waiting for data...".to_string();
        }

        if let Some(ref msg) = self.status_msg {
            return msg.clone();
        }

        let mut s = String::new();
        if let Some(ref name) = self.filename {
            s.push_str(name);
        }
        if let Some(ref f) = self.filter {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&format!("[&{}]", f.pattern));
        }
        if self.wrap {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str("[wrap]");
        }
        if s.is_empty() {
            ":".to_string()
        } else {
            s
        }
    }
}
