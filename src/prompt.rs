//! less-style prompt string expansion.

pub const SHORT: usize = 0;
pub const MEDIUM: usize = 1;
pub const LONG: usize = 2;

pub(crate) const S_PROTO: &[u8] = b"?n?f%f .?m(%T %i of %m) ..?e(END) ?x- Next\\: %x..%t";
pub(crate) const M_PROTO: &[u8] = b"?n?f%f .?m(%T %i of %m) ..?e(END) ?x- Next\\: %x.:?pB%pB\\%:byte %bB?s/%s...%t";
pub(crate) const M_LONG_PROTO: &[u8] = b"?f%f .?n?m(%T %i of %m) ..?ltlines %lt-%lb?L/%L. :byte %bB?s/%s. .?e(END) ?x- Next\\: %x.:?pB%pB\\%..%t";
pub(crate) const EQ_PROTO: &[u8] = b"?f%f .?m(%T %i of %m) .?ltlines %lt-%lb?L/%L. .byte %bB?s/%s. ?e(END) :?pB%pB\\%..%t";

/// User-set prototype strings; absent entries fall back to the built-in defaults.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PromptStore {
    pub short: Option<Vec<u8>>,
    pub medium: Option<Vec<u8>>,
    pub long: Option<Vec<u8>>,
    pub equals: Option<Vec<u8>>,
    pub help: Option<Vec<u8>>,
    pub waiting: Option<Vec<u8>>,
}

impl PromptStore {
    pub fn main_proto(&self, prompt_type: usize) -> Vec<u8> {
        let index = prompt_type.min(LONG);
        let override_ = [&self.short, &self.medium, &self.long][index];
        override_.clone().unwrap_or_else(|| [S_PROTO, M_PROTO, M_LONG_PROTO][index].to_vec())
    }

    pub fn equals_proto(&self) -> Vec<u8> {
        self.equals.clone().unwrap_or_else(|| EQ_PROTO.to_vec())
    }
}

/// Round-half-to-even multiply-divide, matching less's umuldiv.
pub(crate) fn muldiv(val: u64, num: u64, den: u64) -> u64 {
    let q = val / den;
    let r = val % den;
    let quot = q * num + (r * num) / den;
    let rem = (r * num) % den;
    quot + u64::from(den / 2 < rem + (quot & !den & 1))
}

pub(crate) fn percentage(num: u64, den: u64) -> u64 {
    muldiv(num, 100, den)
}

const SHELL_METACHARS: &[u8] = b"; *?\t\n'\"()<>[]|&^`#\\$%=~{},";

pub(crate) fn shell_quote(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 8);
    for &byte in bytes {
        if SHELL_METACHARS.contains(&byte) {
            if byte == b'\n' {
                out.push(b'"');
                out.push(byte);
                out.push(b'"');
            } else {
                out.push(b'\\');
                out.push(byte);
            }
        } else {
            out.push(byte);
        }
    }
    out
}

/// File position of the logical line at one row of the content area.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct NearPosition {
    pub byte: Option<u64>,
    pub line: Option<u64>,
}

/// Everything the prompt expander needs; computed by the pager each paint.
pub struct PromptContext {
    pub filename: Option<Vec<u8>>,
    pub next_filename: Option<Vec<u8>>,
    pub file_index: u64,
    pub file_count: u64,
    /// Size of the input; None when unknown, for example a live pipe.
    pub file_size: Option<u64>,
    /// Number of lines in the input; None when unknown.
    pub total_lines: Option<u64>,
    /// Byte offset and line number of the content row positions.
    pub top: NearPosition,
    pub middle: NearPosition,
    pub bottom: NearPosition,
    pub bottom_plus_one: NearPosition,
    pub jump: NearPosition,
    /// Number of screen rows including the prompt row (sc_height).
    pub terminal_rows: usize,
    /// Number of fixed header rows.
    pub header_lines: usize,
    /// True when the view is at end of file.
    pub at_eof: bool,
    /// Horizontal shift in columns.
    pub hshift: u64,
    /// True for the first prompt painted after a new file is opened.
    pub new_file: bool,
    /// True when line numbers are tracked (the -n state is off).
    pub line_numbers: bool,
    /// True when the current file was opened through a tags list.
    pub via_tags: bool,
    /// Selected OSC 8 hyperlink URI; None when none is selected.
    pub osc8_uri: Option<Vec<u8>>,
    /// Editor name from VISUAL or EDITOR.
    pub editor: Option<Vec<u8>>,
}

const WHERE_TOP: usize = 0;
const WHERE_MIDDLE: usize = 1;
const WHERE_BOTTOM: usize = 2;
const WHERE_BOTTOM_PLUS_ONE: usize = 3;
const WHERE_JUMP: usize = 4;

impl PromptContext {
    fn where_position(&self, where_: usize) -> NearPosition {
        match where_ {
            WHERE_TOP => self.top,
            WHERE_MIDDLE => self.middle,
            WHERE_BOTTOM => self.bottom,
            WHERE_BOTTOM_PLUS_ONE => self.bottom_plus_one,
            _ => self.jump,
        }
    }
}

struct Message {
    bytes: Vec<u8>,
}

impl Message {
    fn push(&mut self, byte: u8) { self.bytes.push(byte); }
    fn push_str(&mut self, s: &[u8]) { self.bytes.extend_from_slice(s); }
    fn quest(&mut self) { self.push_str(b"?"); }
    fn push_uint(&mut self, value: u64) {
        let mut buf = [0u8; 20];
        let mut index = buf.len();
        let mut remaining = value;
        loop {
            index -= 1;
            buf[index] = b'0' + (remaining % 10) as u8;
            remaining /= 10;
            if remaining == 0 { break; }
        }
        self.bytes.extend_from_slice(&buf[index..]);
    }
    fn strip_trailing_spaces(&mut self) {
        while self.bytes.last() == Some(&b' ') { self.bytes.pop(); }
    }
}

/// Parse the where-char that may follow a condition or escape.
/// Returns the where index and the number of bytes to consume after the lead byte.
fn parse_where(proto: &[u8], index: usize) -> (usize, usize) {
    match proto.get(index) {
        Some(b't') => (WHERE_TOP, 1),
        Some(b'm') => (WHERE_MIDDLE, 1),
        Some(b'b') => (WHERE_BOTTOM, 1),
        Some(b'B') => (WHERE_BOTTOM_PLUS_ONE, 1),
        Some(b'j') => (WHERE_JUMP, 1),
        _ => (WHERE_TOP, 0),
    }
}

/// Skip a false conditional, honoring nesting and escapes.
fn skip_condition(proto: &[u8], mut index: usize) -> usize {
    let mut level = 1usize;
    loop {
        let Some(&byte) = proto.get(index) else { return index; };
        index += 1;
        match byte {
            b'?' => level += 1,
            b':' => {
                if level == 1 { return index; }
            }
            b'.' => {
                level -= 1;
                if level == 0 { return index; }
            }
            b'\\' if index < proto.len() => { index += 1; }
            _ => {}
        }
    }
}

/// Expand a prototype string with the given context.
pub fn expand(proto: &[u8], ctx: &PromptContext) -> Vec<u8> {
    let mut message = Message { bytes: Vec::with_capacity(128) };
    let mut index = 0;
    while index < proto.len() {
        let byte = proto[index];
        index += 1;
        match byte {
            b'\\' => {
                if let Some(&next) = proto.get(index) {
                    message.push(next);
                    index += 1;
                }
            }
            b'?' => {
                let Some(&condition) = proto.get(index) else { break };
                index += 1;
                let (where_, consumed) = match condition {
                    b'b' | b'd' | b'l' | b'p' | b'P' => parse_where(proto, index),
                    _ => (WHERE_TOP, 0),
                };
                index += consumed;
                if !condition_true(condition, where_, ctx, message.bytes.len()) {
                    index = skip_condition(proto, index);
                }
            }
            b':' => { index = skip_condition(proto, index); }
            b'.' => {}
            b'%' => {
                let Some(&escape) = proto.get(index) else { break };
                index += 1;
                let (where_, consumed) = match escape {
                    b'b' | b'd' | b'l' | b'p' | b'P' => parse_where(proto, index),
                    _ => (WHERE_TOP, 0),
                };
                index += consumed;
                expand_escape(escape, where_, ctx, &mut message);
            }
            _ => message.push(byte),
        }
    }
    message.bytes
}

fn condition_true(condition: u8, where_: usize, ctx: &PromptContext, message_len: usize) -> bool {
    match condition {
        b'a' => message_len > 0,
        b'b' => ctx.where_position(where_).byte.is_some(),
        b'c' => ctx.hshift != 0,
        b'd' | b'l' => {
            if !ctx.line_numbers { return false; }
            ctx.where_position(where_).line.is_some_and(|line| line != 0)
        }
        b'D' | b'L' => ctx.line_numbers && ctx.file_size.is_some(),
        b'e' => ctx.at_eof,
        b'f' | b'g' => ctx.filename.as_deref().is_some_and(|name| name != b"-"),
        b'm' => ctx.file_count > 1,
        b'n' => ctx.new_file,
        b'p' => {
            ctx.where_position(where_).byte.is_some()
                && ctx.file_size.is_some_and(|size| size > 0)
        }
        b'P' => {
            if !ctx.line_numbers { return false; }
            ctx.where_position(where_).line.is_some_and(|line| line != 0)
                && ctx.file_size.is_some_and(|size| size > 0)
                && ctx.total_lines.is_some()
        }
        b's' | b'B' => ctx.file_size.is_some(),
        b'T' => true,
        b'o' => ctx.osc8_uri.is_some(),
        b'x' => ctx.next_filename.is_some(),
        _ => false,
    }
}

fn page_number(ctx: &PromptContext, line: u64) -> u64 {
    let page_size = ctx.terminal_rows.saturating_sub(ctx.header_lines).saturating_sub(1).max(1) as u64;
    (line - 1) / page_size + 1
}

fn expand_escape(escape: u8, where_: usize, ctx: &PromptContext, message: &mut Message) {
    match escape {
        b'c' => message.push_uint(ctx.hshift),
        b'b' => match ctx.where_position(where_).byte {
            Some(byte) => message.push_uint(byte),
            None => message.quest(),
        },
        b'd' => {
            if !ctx.line_numbers || ctx.terminal_rows <= ctx.header_lines + 1 {
                message.quest();
                return;
            }
            match ctx.where_position(where_).line {
                Some(line) if line != 0 => message.push_uint(page_number(ctx, line)),
                _ => message.quest(),
            }
        }
        b'D' => match ctx.file_size {
            None => message.quest(),
            Some(0) => message.push_uint(0),
            Some(_) => match ctx.total_lines {
                Some(lines) => message.push_uint(page_number(ctx, lines)),
                None => message.quest(),
            },
        },
        b'E' => match &ctx.editor {
            Some(editor) => message.push_str(editor),
            None => message.push_str(b"vi"),
        },
        b'f' => match &ctx.filename {
            Some(name) => message.push_str(name),
            None => message.push_str(b"-"),
        },
        b'F' => match &ctx.filename {
            Some(name) => match name.iter().rposition(|byte| *byte == b'/') {
                Some(index) => message.push_str(&name[index + 1..]),
                None => message.push_str(name),
            },
            None => message.push_str(b"-"),
        },
        b'g' => match &ctx.filename {
            Some(name) => {
                let quoted = shell_quote(name);
                message.push_str(&quoted);
            }
            None => message.push_str(b"-"),
        },
        b'i' => message.push_uint(ctx.file_index),
        b'l' => match ctx.where_position(where_).line {
            Some(line) if line != 0 => message.push_uint(line),
            _ => message.quest(),
        },
        b'L' => match ctx.file_size {
            Some(0) | None => message.quest(),
            Some(_) => match ctx.total_lines {
                Some(lines) => message.push_uint(lines),
                None => message.quest(),
            },
        },
        b'm' => message.push_uint(ctx.file_count),
        b'o' => match &ctx.osc8_uri {
            Some(uri) => message.push_str(uri),
            None => message.quest(),
        },
        b'p' => {
            let position = ctx.where_position(where_);
            match (position.byte, ctx.file_size) {
                (Some(byte), Some(size)) if size > 0 => message.push_uint(percentage(byte, size)),
                _ => message.quest(),
            }
        }
        b'P' => {
            let position = ctx.where_position(where_);
            match (position.line, ctx.total_lines) {
                (Some(line), Some(lines)) if line != 0 && lines > 0 => {
                    message.push_uint(percentage(line, lines + 1))
                }
                _ => message.quest(),
            }
        }
        b's' | b'B' => match ctx.file_size {
            Some(size) => message.push_uint(size),
            None => message.quest(),
        },
        b't' => message.strip_trailing_spaces(),
        b'T' => message.push_str(if ctx.via_tags { b"tag" } else { b"file" }),
        b'x' => match &ctx.next_filename {
            Some(name) => message.push_str(name),
            None => message.quest(),
        },
        _ => {}
    }
}
