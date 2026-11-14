use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use lz::search::{CaseSensitivity, SearchStart};
use lz::ansi::{BackspaceDisplay, ControlDisplay};

use lz::option_catalog::{all, lookup, OptionLookupError, OptionSpec, ValueKind};

pub(super) enum Diagnostic {
    UnknownShort(u8),
    UnknownLong(Vec<u8>),
    Ambiguous(Vec<u8>),
    MissingValue(Vec<u8>),
    InvalidNumber(Vec<u8>),
    Overflow(Vec<u8>),
    FlagAssignment(Vec<u8>),
    LineNumberWidthTooLarge,
    NegativeLineNumberWidth,
    InvalidPosition { fraction: bool },
    UnsupportedHeader(Vec<u8>),
}

impl Diagnostic {
    pub(super) fn needs_acknowledgment(&self) -> bool {
        matches!(self, Self::LineNumberWidthTooLarge | Self::NegativeLineNumberWidth | Self::InvalidPosition { .. })
    }

    pub(super) fn write(&self, output: &mut impl Write) -> io::Result<()> {
        match self {
            Self::UnknownShort(letter) => { output.write_all(b"There is no -")?; output.write_all(&[*letter])?; output.write_all(b" option")?; }
            Self::UnknownLong(name) => { output.write_all(b"There is no ")?; output.write_all(name)?; output.write_all(b" option")?; }
            Self::Ambiguous(name) => { output.write_all(name)?; output.write_all(b" is an ambiguous abbreviation")?; }
            Self::MissingValue(label) => { output.write_all(b"Value is required after ")?; output.write_all(label)?; return output.write_all(b"\n"); }
            Self::InvalidNumber(label) => { output.write_all(b"Number is required after ")?; output.write_all(label)?; return output.write_all(b"\n"); }
            Self::Overflow(label) => { output.write_all(b"Number too large in '")?; output.write_all(label)?; return output.write_all(b"'\n"); }
            Self::FlagAssignment(name) => { output.write_all(b"The ")?; output.write_all(name)?; return output.write_all(b" option should not be followed by =\n"); }
            Self::LineNumberWidthTooLarge => return writeln!(output, "Line number width must not be larger than {}", lz::pager::LineNumberWidth::MAX),
            Self::NegativeLineNumberWidth => return output.write_all(b"Line number width must not be negative\n"),
            Self::InvalidPosition { fraction } => return output.write_all(if *fraction { b"Invalid fraction\n" } else { b"Invalid number\n" }),
            Self::UnsupportedHeader(value) => { output.write_all(b"Unsupported header specification: ")?; output.write_all(value)?; return output.write_all(b"\n"); }
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
    pub use_color: bool,
    pub backspaces: BackspaceDisplay,
    pub header: lz::pager::HeaderDimensions,
    pub incremental_search: bool,
    pub hilite_search: lz::pager::HiliteSearch,
    pub no_search_header_lines: bool,
    pub no_search_header_columns: bool,
    pub chop: bool,
    pub line_numbers: lz::pager::LineNumberMode,
    pub line_number_width: lz::pager::LineNumberWidth,
    pub show_eof_markers: bool,
    pub window: Option<i32>,
    pub horizontal_shift: Option<lz::pager::HorizontalShift>,
    pub search_case: CaseSensitivity,
    pub search_start: SearchStart,
    pub jump_target: lz::pager::JumpTarget,
    pub history_duplicates: lz::history::DuplicatePolicy,
    pub tab_stops: lz::tabs::TabStops,
    pub filenames: Vec<OsString>,
    pub stdin_explicit: bool,
    pub action: Action,
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for Args {
    fn default() -> Self {
        Self { terminal: Default::default(), startup_commands: Vec::new(), every_file_command: None, interrupt_policy: Default::default(), tag: None, tags_file: None, force: false, use_lessopen: true, quit_if_one_screen: false, eof_policy: Default::default(), bell_policy: Default::default(), control_display: ControlDisplay::default(), use_color: false, backspaces: BackspaceDisplay::Overstrike, header: Default::default(), incremental_search: false, hilite_search: Default::default(), no_search_header_lines: false, no_search_header_columns: false, chop: false, line_numbers: Default::default(), show_eof_markers: true, window: None, horizontal_shift: None, search_case: CaseSensitivity::Sensitive,
            line_number_width: Default::default(), search_start: SearchStart::default(), jump_target: Default::default(), history_duplicates: Default::default(), tab_stops: Default::default(), filenames: Vec::new(), stdin_explicit: false, action: Action::Page, diagnostics: Vec::new() }
    }
}


fn description(option: &OptionSpec) -> Vec<u8> {
    if let Some(letter) = option.shorts().first() {
        [b"-".as_slice(), &[*letter], b" (--", option.canonical_name().as_bytes(), b")"].concat()
    } else { [b"--".as_slice(), option.canonical_name().as_bytes()].concat() }
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
        match option.canonical_name() {
            "help" => self.args.action = Action::Help,
            "version" => self.args.action = Action::Version,
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
            "no-histdups" => self.args.history_duplicates = if reset { lz::history::DuplicatePolicy::Allow } else { lz::history::DuplicatePolicy::MoveToEnd },
            "raw-control-chars" => self.args.control_display = if reset { ControlDisplay::default() } else if upper { ControlDisplay::Color } else { ControlDisplay::Raw },
            "proc-backspace" => self.args.backspaces = if !reset && upper { BackspaceDisplay::Control } else { BackspaceDisplay::Overstrike },
            "use-color" => self.args.use_color = !reset,
            "incsearch" => self.args.incremental_search = !reset,
            "hilite-search" => self.args.hilite_search = if reset { lz::pager::HiliteSearch::All } else if upper { lz::pager::HiliteSearch::None } else { lz::pager::HiliteSearch::Match },
            "no-search-header-lines" => self.args.no_search_header_lines = !reset,
            "no-search-header-columns" => self.args.no_search_header_columns = !reset,
            "no-search-headers" => { self.args.no_search_header_lines = !reset; self.args.no_search_header_columns = !reset; }
            "chop-long-lines" => self.args.chop = !reset,
            "line-numbers" => self.args.line_numbers = if reset { lz::pager::LineNumberMode::Enabled } else if upper { lz::pager::LineNumberMode::Displayed } else { lz::pager::LineNumberMode::Off },
            "tilde" => self.args.show_eof_markers = reset,
            _ => {}
        }
    }

    fn value(&mut self, option: &OptionSpec, payload: &[u8], label: Vec<u8>) -> usize {
        match option.value() {
            ValueKind::Flag => 0,
            ValueKind::Text => {
                let end = payload.iter().position(|byte| *byte == b'$').unwrap_or(payload.len());
                match option.canonical_name() {
                    "pattern" => self.args.startup_commands.push([b"/".as_slice(), &payload[..end]].concat()),
                    "tag" => self.args.tag = Some(OsString::from_vec(payload[..end].to_vec())),
                    "tag-file" => self.args.tags_file = Some(OsString::from_vec(payload[..end].to_vec())),
                    "jump-target" => {
                        let fraction = payload.starts_with(b".");
                        let start = usize::from(fraction || payload.starts_with(b"-"));
                        let digits = payload[start..end].iter().take_while(|byte| byte.is_ascii_digit()).count();
                        let consumed = start + digits;
                        match lz::pager::JumpTarget::parse(&payload[..consumed]) {
                            Some(target) => self.args.jump_target = target,
                            None => self.args.diagnostics.push(Diagnostic::InvalidPosition { fraction }),
                        }
                        return consumed;
                    },
                    "shift" => {
                        let fraction = payload.starts_with(b".");
                        let start = usize::from(fraction);
                        let digits = payload[start..end].iter().take_while(|byte| byte.is_ascii_digit()).count();
                        let consumed = start + digits;
                        match lz::pager::HorizontalShift::parse(&payload[..consumed]) {
                            Some(shift) => self.args.horizontal_shift = Some(shift),
                            None => self.args.diagnostics.push(Diagnostic::InvalidPosition { fraction }),
                        }
                        return consumed;
                    },
                    "tabs" => if let Some(stops) = lz::tabs::TabStops::parse(&payload[..end]) { self.args.tab_stops = stops; },
                    "header" => {
                        let value = &payload[..end];
                        if let Some(header) = lz::pager::parse_header(value, Default::default()) { self.args.header = header; }
                        else { self.args.diagnostics.push(Diagnostic::UnsupportedHeader(value.to_vec())); }
                    },
                    _ => {}
                }
                end
            },
            ValueKind::Number => match scan_number(payload) {
                NumberScan::Number { bytes, value } => {
                    if option.canonical_name() == "window" { self.args.window = Some(value); }
                    if option.canonical_name() == "line-num-width" {
                        if value < 0 {
                            self.args.line_number_width = Default::default();
                            self.args.diagnostics.push(Diagnostic::NegativeLineNumberWidth);
                        } else if let Ok(width) = u8::try_from(value) && let Some(width) = lz::pager::LineNumberWidth::new(width) {
                            self.args.line_number_width = width;
                        } else {
                            self.args.line_number_width = Default::default();
                            self.args.diagnostics.push(Diagnostic::LineNumberWidthTooLarge);
                        }
                    }
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
                    Err(error) => {
                        self.args.diagnostics.push(match error { OptionLookupError::Ambiguous => Diagnostic::Ambiguous(word[start..].to_vec()), OptionLookupError::Unknown => Diagnostic::UnknownLong(word[start..].to_vec()) });
                        return;
                    }
                };
                if word.get(cursor) == Some(&b'=') {
                    if matches!(option.value(), ValueKind::Flag) { self.args.diagnostics.push(Diagnostic::FlagAssignment(word[start..].to_vec())); return; }
                    cursor += 1;
                }
                (option, word[start..].to_vec(), name[0].is_ascii_uppercase())
            } else if byte.is_ascii_digit() {
                cursor -= 1;
                (all().iter().find(|option| option.shorts().contains(&b'z')).unwrap(), b"-z".to_vec(), false)
            } else {
                let Some(option) = all().iter().find(|option| option.shorts().contains(&byte)) else { self.args.diagnostics.push(Diagnostic::UnknownShort(byte)); return; };
                (option, vec![b'-', byte], byte.is_ascii_uppercase())
            };
            match option.value() {
                ValueKind::Flag => {
                    self.apply_flag(option, upper, reset);
                    if matches!(self.args.action, Action::Version) { return; }
                }
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
        if matches!(scanner.args.action, Action::Version) { break; }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_header_fields_are_reported() {
        let args = parse(vec!["--header=2,5,3".into(), "input".into()], None);
        assert_eq!(args.diagnostics.len(), 1);
        let mut output = Vec::new();
        args.diagnostics[0].write(&mut output).unwrap();
        assert_eq!(output, b"Unsupported header specification: 2,5,3\n");
    }

    #[test]
    fn version_stops_option_scanning() {
        for words in [["--help", "--version", "-Z"], ["--version", "--help", "-Z"]] {
            let args = parse(words.into_iter().map(Into::into).collect(), None);
            assert!(matches!(args.action, Action::Version));
            assert!(args.diagnostics.is_empty());
        }
        let args = parse(vec!["-Z".into(), "--version".into(), "--help".into()], None);
        assert!(matches!(args.action, Action::Version));
        assert_eq!(args.diagnostics.len(), 1);
        let args = parse(vec!["-VZ".into()], None);
        assert!(matches!(args.action, Action::Version));
        assert!(args.diagnostics.is_empty());
        let args = parse(vec!["-Z".into()], Some("--help --version".into()));
        assert!(matches!(args.action, Action::Version));
        assert!(args.diagnostics.is_empty());
    }

    #[test]
    fn negative_line_number_width_reports_an_error() {
        let args = parse(vec!["--line-num-width=-1".into(), "input".into()], None);
        assert_eq!(args.diagnostics.len(), 1);
        let mut output = Vec::new();
        args.diagnostics[0].write(&mut output).unwrap();
        assert_eq!(output, b"Line number width must not be negative\n");
        assert!(args.diagnostics[0].needs_acknowledgment());
    }
}
