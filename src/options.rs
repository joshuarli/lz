use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use lz::search::{CaseSensitivity, SearchStart};
use lz::ansi::ControlDisplay;

#[derive(Clone, Copy)]
enum ValueKind { Flag, Number, Text }

struct OptionSpec { shorts: &'static [u8], names: &'static [&'static str], value: ValueKind }

const OPTIONS: &[OptionSpec] = &[
    OptionSpec { shorts: b"?", names: &["help"], value: ValueKind::Flag },
    OptionSpec { shorts: b"aA", names: &["search-skip-screen"], value: ValueKind::Flag },
    OptionSpec { shorts: b"b", names: &["buffers"], value: ValueKind::Number },
    OptionSpec { shorts: b"B", names: &["auto-buffers"], value: ValueKind::Flag },
    OptionSpec { shorts: b"cC", names: &["clear-screen"], value: ValueKind::Flag },
    OptionSpec { shorts: b"d", names: &["dumb"], value: ValueKind::Flag },
    OptionSpec { shorts: b"D", names: &["color"], value: ValueKind::Text },
    OptionSpec { shorts: b"eE", names: &["quit-at-eof"], value: ValueKind::Flag },
    OptionSpec { shorts: b"f", names: &["force"], value: ValueKind::Flag },
    OptionSpec { shorts: b"F", names: &["quit-if-one-screen"], value: ValueKind::Flag },
    OptionSpec { shorts: b"gG", names: &["hilite-search"], value: ValueKind::Flag },
    OptionSpec { shorts: b"h", names: &["max-back-scroll"], value: ValueKind::Number },
    OptionSpec { shorts: b"iI", names: &["ignore-case"], value: ValueKind::Flag },
    OptionSpec { shorts: b"j", names: &["jump-target"], value: ValueKind::Text },
    OptionSpec { shorts: b"J", names: &["status-column"], value: ValueKind::Flag },
    OptionSpec { shorts: b"k", names: &["lesskey-file"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["lesskey-src"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["lesskey-content"], value: ValueKind::Text },
    OptionSpec { shorts: b"K", names: &["quit-on-intr"], value: ValueKind::Flag },
    OptionSpec { shorts: b"L", names: &["no-lessopen"], value: ValueKind::Flag },
    OptionSpec { shorts: b"mM", names: &["long-prompt"], value: ValueKind::Flag },
    OptionSpec { shorts: b"nN", names: &["line-numbers"], value: ValueKind::Flag },
    OptionSpec { shorts: b"oO", names: &["log-file"], value: ValueKind::Text },
    OptionSpec { shorts: b"p", names: &["pattern"], value: ValueKind::Text },
    OptionSpec { shorts: b"P", names: &["prompt"], value: ValueKind::Text },
    OptionSpec { shorts: b"qQ", names: &["quiet", "silent"], value: ValueKind::Flag },
    OptionSpec { shorts: b"rR", names: &["raw-control-chars"], value: ValueKind::Flag },
    OptionSpec { shorts: b"s", names: &["squeeze-blank-lines"], value: ValueKind::Flag },
    OptionSpec { shorts: b"S", names: &["chop-long-lines"], value: ValueKind::Flag },
    OptionSpec { shorts: b"t", names: &["tag"], value: ValueKind::Text },
    OptionSpec { shorts: b"T", names: &["tag-file"], value: ValueKind::Text },
    OptionSpec { shorts: b"uU", names: &["underline-special"], value: ValueKind::Flag },
    OptionSpec { shorts: b"V", names: &["version"], value: ValueKind::Flag },
    OptionSpec { shorts: b"wW", names: &["hilite-unread"], value: ValueKind::Flag },
    OptionSpec { shorts: b"x", names: &["tabs"], value: ValueKind::Text },
    OptionSpec { shorts: b"X", names: &["no-init"], value: ValueKind::Flag },
    OptionSpec { shorts: b"y", names: &["max-forw-scroll"], value: ValueKind::Number },
    OptionSpec { shorts: b"z", names: &["window"], value: ValueKind::Number },
    OptionSpec { shorts: b"\"", names: &["quotes"], value: ValueKind::Text },
    OptionSpec { shorts: b"~", names: &["tilde"], value: ValueKind::Flag },
    OptionSpec { shorts: b"#", names: &["shift"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["exit-follow-on-close"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["file-size"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["follow-name"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["header"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["incsearch"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["intr"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["line-num-width"], value: ValueKind::Number },
    OptionSpec { shorts: b"", names: &["match-shift"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["modelines"], value: ValueKind::Number },
    OptionSpec { shorts: b"", names: &["mouse"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-keypad"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-histdups"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-number-headers"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-search-header-lines"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-search-header-columns"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-search-headers"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["no-vbell"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["proc-backspace"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["proc-return"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["proc-tab"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["redraw-on-quit"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["rscroll"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["save-marks"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["search-options"], value: ValueKind::Text },
    OptionSpec { shorts: b"", names: &["show-preproc-errors"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["status-col-width"], value: ValueKind::Number },
    OptionSpec { shorts: b"", names: &["status-line"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["use-backslash"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["use-color"], value: ValueKind::Flag },
    OptionSpec { shorts: b"", names: &["wheel-lines"], value: ValueKind::Number },
    OptionSpec { shorts: b"", names: &["wordwrap"], value: ValueKind::Flag },
];

pub(super) enum Diagnostic {
    UnknownShort(u8),
    UnknownLong(Vec<u8>),
    Ambiguous(Vec<u8>),
    MissingValue(Vec<u8>),
    InvalidNumber(Vec<u8>),
    Overflow(Vec<u8>),
    FlagAssignment(Vec<u8>),
}

impl Diagnostic {
    pub(super) fn write(&self, output: &mut impl Write) -> io::Result<()> {
        match self {
            Self::UnknownShort(letter) => { output.write_all(b"There is no -")?; output.write_all(&[*letter])?; output.write_all(b" option")?; }
            Self::UnknownLong(name) => { output.write_all(b"There is no ")?; output.write_all(name)?; output.write_all(b" option")?; }
            Self::Ambiguous(name) => { output.write_all(name)?; output.write_all(b" is an ambiguous abbreviation")?; }
            Self::MissingValue(label) => { output.write_all(b"Value is required after ")?; output.write_all(label)?; return output.write_all(b"\n"); }
            Self::InvalidNumber(label) => { output.write_all(b"Number is required after ")?; output.write_all(label)?; return output.write_all(b"\n"); }
            Self::Overflow(label) => { output.write_all(b"Number too large in '")?; output.write_all(label)?; return output.write_all(b"'\n"); }
            Self::FlagAssignment(name) => { output.write_all(b"The ")?; output.write_all(name)?; return output.write_all(b" option should not be followed by =\n"); }
        }
        output.write_all(b" (\"lz --help\" for help)\n")
    }
}

pub(super) enum Action { Page, Help, Version, Stop }

pub(super) struct Args {
    pub terminal: lz::terminal::InteractiveOptions,
    pub startup_commands: Vec<Vec<u8>>,
    pub every_file_command: Option<Vec<u8>>,
    pub interrupt_policy: lz::pager::InterruptPolicy,
    pub tag: Option<OsString>,
    pub tags_file: Option<OsString>,
    pub force: bool,
    pub use_lessopen: bool,
    pub quit_if_one_screen: bool,
    pub eof_policy: lz::pager::EofPolicy,
    pub bell_policy: lz::terminal::BellPolicy,
    pub control_display: ControlDisplay,
    pub chop: bool,
    pub line_numbers: bool,
    pub show_eof_markers: bool,
    pub window: Option<i32>,
    pub search_case: CaseSensitivity,
    pub search_start: SearchStart,
    pub filenames: Vec<OsString>,
    pub stdin_explicit: bool,
    pub action: Action,
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for Args {
    fn default() -> Self {
        Self { terminal: Default::default(), startup_commands: Vec::new(), every_file_command: None, interrupt_policy: Default::default(), tag: None, tags_file: None, force: false, use_lessopen: true, quit_if_one_screen: false, eof_policy: Default::default(), bell_policy: Default::default(), control_display: ControlDisplay::default(), chop: false, line_numbers: false, show_eof_markers: true, window: None, search_case: CaseSensitivity::Sensitive,
            search_start: SearchStart::default(), filenames: Vec::new(), stdin_explicit: false, action: Action::Page, diagnostics: Vec::new() }
    }
}

fn prefix(name: &[u8], option: &str) -> bool {
    let option = option.as_bytes();
    if name.is_empty() || name.len() > option.len() { return false; }
    if name[0].is_ascii_uppercase() { name.eq_ignore_ascii_case(&option[..name.len()]) }
    else { option.starts_with(name) }
}

fn lookup(name: &[u8]) -> Result<&'static OptionSpec, bool> {
    if let Some(exact) = OPTIONS.iter().find(|option| option.names.iter().any(|alias| name.len() == alias.len() && prefix(name, alias))) { return Ok(exact); }
    let mut matches = OPTIONS.iter().filter(|option| option.names.iter().any(|alias| prefix(name, alias)));
    match (matches.next(), matches.next()) { (Some(option), None) => Ok(option), (None, _) => Err(false), _ => Err(true) }
}

fn description(option: &OptionSpec) -> Vec<u8> {
    if let Some(letter) = option.shorts.first() {
        [b"-".as_slice(), &[*letter], b" (--", option.names[0].as_bytes(), b")"].concat()
    } else { [b"--".as_slice(), option.names[0].as_bytes()].concat() }
}

enum NumberScan { Number { bytes: usize, value: i32 }, Missing, Overflow(usize) }

fn scan_number(bytes: &[u8]) -> NumberScan {
    let leading = bytes.iter().take_while(|byte| byte.is_ascii_whitespace()).count();
    let sign = usize::from(bytes.get(leading) == Some(&b'-'));
    let start = leading + sign;
    let digits = bytes[start..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 { return NumberScan::Missing; }
    let mut value = 0u64;
    let mut overflow = false;
    for byte in &bytes[start..start + digits] {
        value = value.saturating_mul(10).saturating_add(u64::from(byte - b'0'));
        overflow |= value > i32::MAX as u64;
    }
    if overflow { NumberScan::Overflow(start + digits) }
    else { NumberScan::Number { bytes: start + digits, value: if sign == 0 { value as i32 } else { -(value as i32) } } }
}

#[derive(Default)]
struct Scanner { args: Args, pending: Option<&'static OptionSpec>, no_vbell: bool }

impl Scanner {
    fn apply_flag(&mut self, option: &OptionSpec, upper: bool, reset: bool) {
        match option.names[0] {
            "help" => self.args.action = Action::Help,
            "version" => if !matches!(self.args.action, Action::Help) { self.args.action = Action::Version; },
            "force" => self.args.force = !reset,
            "no-lessopen" => self.args.use_lessopen = reset,
            "quit-if-one-screen" => self.args.quit_if_one_screen = !reset,
            "quiet" => self.args.bell_policy = if reset { Default::default() } else if upper { lz::terminal::BellPolicy::Visual } else { lz::terminal::BellPolicy::QuietBoundary },
            "no-vbell" => self.no_vbell = !reset,
            "quit-at-eof" => self.args.eof_policy = if reset { Default::default() } else if upper { lz::pager::EofPolicy::AtEof } else { lz::pager::EofPolicy::AfterEof },
            "quit-on-intr" => self.args.interrupt_policy = if reset { Default::default() } else { lz::pager::InterruptPolicy::Quit },
            "no-init" => self.args.terminal.screen = if reset { Default::default() } else { lz::terminal::ScreenMode::Preserve },
            "no-keypad" => self.args.terminal.keypad = if reset { Default::default() } else { lz::terminal::KeypadMode::Normal },
            "ignore-case" => self.args.search_case = if reset { CaseSensitivity::Sensitive } else if upper { CaseSensitivity::Insensitive } else { CaseSensitivity::Smart },
            "search-skip-screen" => self.args.search_start = if reset { SearchStart::Default } else if upper { SearchStart::AfterTarget } else { SearchStart::AfterScreen },
            "raw-control-chars" => self.args.control_display = if reset { ControlDisplay::default() } else if upper { ControlDisplay::Color } else { ControlDisplay::Raw },
            "chop-long-lines" => self.args.chop = !reset,
            "line-numbers" => self.args.line_numbers = !reset && upper,
            "tilde" => self.args.show_eof_markers = reset,
            _ => {}
        }
    }

    fn value(&mut self, option: &OptionSpec, payload: &[u8], label: Vec<u8>) -> usize {
        match option.value {
            ValueKind::Flag => 0,
            ValueKind::Text => {
                let end = payload.iter().position(|byte| *byte == b'$').unwrap_or(payload.len());
                match option.names[0] {
                    "pattern" => self.args.startup_commands.push([b"/".as_slice(), &payload[..end]].concat()),
                    "tag" => self.args.tag = Some(OsString::from_vec(payload[..end].to_vec())),
                    "tag-file" => self.args.tags_file = Some(OsString::from_vec(payload[..end].to_vec())),
                    _ => {}
                }
                end
            },
            ValueKind::Number => match scan_number(payload) {
                NumberScan::Number { bytes, value } => {
                    if option.names[0] == "window" { self.args.window = Some(value); }
                    bytes
                },
                NumberScan::Missing => { self.args.diagnostics.push(Diagnostic::InvalidNumber(label)); 0 }
                NumberScan::Overflow(bytes) => { self.args.diagnostics.push(Diagnostic::Overflow(label)); bytes }
            },
        }
    }

    fn word(&mut self, word: &[u8]) {
        let mut cursor = 0;
        let mut reset = false;
        while cursor < word.len() {
            let byte = word[cursor];
            cursor += 1;
            match byte {
                b' ' | b'\t' | b'$' => continue,
                b'+' => {
                    let end = word[cursor..].iter().position(|byte| *byte == b'$').map_or(word.len(), |end| cursor + end);
                    let command = &word[cursor..end];
                    if let Some(command) = command.strip_prefix(b"+") { self.args.every_file_command = Some(command.to_vec()); }
                    else { self.args.startup_commands.push(command.to_vec()); }
                    cursor = end;
                    continue;
                }
                b'-' if word.get(cursor) != Some(&b'-') => {
                    reset = word.get(cursor) == Some(&b'+');
                    cursor += usize::from(reset);
                    continue;
                }
                _ => {}
            }
            let (option, label, upper) = if byte == b'-' {
                cursor += 1;
                if word.get(cursor) == Some(&b'+') { reset = true; cursor += 1; }
                let start = cursor;
                while cursor < word.len() && !matches!(word[cursor], b'=' | b' ' | b'\t' | b'$') { cursor += 1; }
                let name = &word[start..cursor];
                let option = match lookup(name) {
                    Ok(option) => option,
                    Err(ambiguous) => {
                        self.args.diagnostics.push(if ambiguous { Diagnostic::Ambiguous(word[start..].to_vec()) } else { Diagnostic::UnknownLong(word[start..].to_vec()) });
                        return;
                    }
                };
                if word.get(cursor) == Some(&b'=') {
                    if matches!(option.value, ValueKind::Flag) { self.args.diagnostics.push(Diagnostic::FlagAssignment(word[start..].to_vec())); return; }
                    cursor += 1;
                }
                (option, word[start..].to_vec(), name[0].is_ascii_uppercase())
            } else if byte.is_ascii_digit() {
                cursor -= 1;
                (OPTIONS.iter().find(|option| option.shorts.contains(&b'z')).unwrap(), b"-z".to_vec(), false)
            } else {
                let Some(option) = OPTIONS.iter().find(|option| option.shorts.contains(&byte)) else { self.args.diagnostics.push(Diagnostic::UnknownShort(byte)); return; };
                (option, vec![b'-', byte], byte.is_ascii_uppercase())
            };
            match option.value {
                ValueKind::Flag => self.apply_flag(option, upper, reset),
                ValueKind::Number | ValueKind::Text => {
                    if cursor == word.len() { self.pending = Some(option); return; }
                    cursor += self.value(option, &word[cursor..], label);
                }
            }
        }
    }
}

pub(super) fn parse(arguments: Vec<OsString>, defaults: Option<OsString>) -> Args {
    let mut scanner = Scanner::default();
    if let Some(defaults) = defaults { scanner.word(defaults.as_bytes()); }
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let bytes = argument.as_bytes();
        if bytes == b"--" { scanner.args.filenames.extend(arguments); break; }
        if let Some(option) = scanner.pending.take() {
            scanner.value(option, bytes, description(option));
        } else if bytes.len() > 1 && matches!(bytes[0], b'-' | b'+') {
            scanner.word(bytes);
        } else {
            scanner.args.filenames.push(argument);
            scanner.args.filenames.extend(arguments);
            break;
        }
    }
    if let Some(option) = scanner.pending {
        scanner.args.diagnostics.push(Diagnostic::MissingValue(description(option)));
        scanner.args.action = Action::Stop;
    }
    if scanner.no_vbell { scanner.args.bell_policy = scanner.args.bell_policy.without_visual(); }
    scanner.args.stdin_explicit = scanner.args.filenames.iter().any(|name| name.as_bytes() == b"-");
    if scanner.args.filenames.is_empty() { scanner.args.filenames.push(OsString::from_vec(b"-".to_vec())); }
    scanner.args
}
