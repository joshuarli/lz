use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;

struct OptionSpec { shorts: &'static [u8], names: &'static [&'static str], value: bool }

const OPTIONS: &[OptionSpec] = &[
    OptionSpec { shorts: b"?", names: &["help"], value: false },
    OptionSpec { shorts: b"aA", names: &["search-skip-screen"], value: false },
    OptionSpec { shorts: b"b", names: &["buffers"], value: true },
    OptionSpec { shorts: b"B", names: &["auto-buffers"], value: false },
    OptionSpec { shorts: b"cC", names: &["clear-screen"], value: false },
    OptionSpec { shorts: b"d", names: &["dumb"], value: false },
    OptionSpec { shorts: b"D", names: &["color"], value: true },
    OptionSpec { shorts: b"eE", names: &["quit-at-eof"], value: false },
    OptionSpec { shorts: b"f", names: &["force"], value: false },
    OptionSpec { shorts: b"F", names: &["quit-if-one-screen"], value: false },
    OptionSpec { shorts: b"gG", names: &["hilite-search"], value: false },
    OptionSpec { shorts: b"h", names: &["max-back-scroll"], value: true },
    OptionSpec { shorts: b"iI", names: &["ignore-case"], value: false },
    OptionSpec { shorts: b"j", names: &["jump-target"], value: true },
    OptionSpec { shorts: b"J", names: &["status-column"], value: false },
    OptionSpec { shorts: b"k", names: &["lesskey-file"], value: true },
    OptionSpec { shorts: b"", names: &["lesskey-src"], value: true },
    OptionSpec { shorts: b"", names: &["lesskey-content"], value: true },
    OptionSpec { shorts: b"K", names: &["quit-on-intr"], value: false },
    OptionSpec { shorts: b"L", names: &["no-lessopen"], value: false },
    OptionSpec { shorts: b"mM", names: &["long-prompt"], value: false },
    OptionSpec { shorts: b"nN", names: &["line-numbers"], value: false },
    OptionSpec { shorts: b"oO", names: &["log-file"], value: true },
    OptionSpec { shorts: b"p", names: &["pattern"], value: true },
    OptionSpec { shorts: b"P", names: &["prompt"], value: true },
    OptionSpec { shorts: b"qQ", names: &["quiet", "silent"], value: false },
    OptionSpec { shorts: b"rR", names: &["raw-control-chars"], value: false },
    OptionSpec { shorts: b"s", names: &["squeeze-blank-lines"], value: false },
    OptionSpec { shorts: b"S", names: &["chop-long-lines"], value: false },
    OptionSpec { shorts: b"t", names: &["tag"], value: true },
    OptionSpec { shorts: b"T", names: &["tag-file"], value: true },
    OptionSpec { shorts: b"uU", names: &["underline-special"], value: false },
    OptionSpec { shorts: b"V", names: &["version"], value: false },
    OptionSpec { shorts: b"wW", names: &["hilite-unread"], value: false },
    OptionSpec { shorts: b"x", names: &["tabs"], value: true },
    OptionSpec { shorts: b"X", names: &["no-init"], value: false },
    OptionSpec { shorts: b"y", names: &["max-forw-scroll"], value: true },
    OptionSpec { shorts: b"z", names: &["window"], value: true },
    OptionSpec { shorts: b"\"", names: &["quotes"], value: true },
    OptionSpec { shorts: b"~", names: &["tilde"], value: false },
    OptionSpec { shorts: b"#", names: &["shift"], value: true },
    OptionSpec { shorts: b"", names: &["exit-follow-on-close"], value: false },
    OptionSpec { shorts: b"", names: &["file-size"], value: false },
    OptionSpec { shorts: b"", names: &["follow-name"], value: false },
    OptionSpec { shorts: b"", names: &["header"], value: true },
    OptionSpec { shorts: b"", names: &["incsearch"], value: false },
    OptionSpec { shorts: b"", names: &["intr"], value: true },
    OptionSpec { shorts: b"", names: &["line-num-width"], value: true },
    OptionSpec { shorts: b"", names: &["match-shift"], value: true },
    OptionSpec { shorts: b"", names: &["modelines"], value: true },
    OptionSpec { shorts: b"", names: &["mouse"], value: false },
    OptionSpec { shorts: b"", names: &["no-keypad"], value: false },
    OptionSpec { shorts: b"", names: &["no-histdups"], value: false },
    OptionSpec { shorts: b"", names: &["no-number-headers"], value: false },
    OptionSpec { shorts: b"", names: &["no-search-header-lines"], value: false },
    OptionSpec { shorts: b"", names: &["no-search-header-columns"], value: false },
    OptionSpec { shorts: b"", names: &["no-search-headers"], value: false },
    OptionSpec { shorts: b"", names: &["no-vbell"], value: false },
    OptionSpec { shorts: b"", names: &["proc-backspace"], value: false },
    OptionSpec { shorts: b"", names: &["proc-return"], value: false },
    OptionSpec { shorts: b"", names: &["proc-tab"], value: false },
    OptionSpec { shorts: b"", names: &["redraw-on-quit"], value: false },
    OptionSpec { shorts: b"", names: &["rscroll"], value: true },
    OptionSpec { shorts: b"", names: &["save-marks"], value: false },
    OptionSpec { shorts: b"", names: &["search-options"], value: true },
    OptionSpec { shorts: b"", names: &["show-preproc-errors"], value: false },
    OptionSpec { shorts: b"", names: &["status-col-width"], value: true },
    OptionSpec { shorts: b"", names: &["status-line"], value: false },
    OptionSpec { shorts: b"", names: &["use-backslash"], value: false },
    OptionSpec { shorts: b"", names: &["use-color"], value: false },
    OptionSpec { shorts: b"", names: &["wheel-lines"], value: true },
    OptionSpec { shorts: b"", names: &["wordwrap"], value: false },
];

pub(super) enum Diagnostic { UnknownShort(u8), UnknownLong(Vec<u8>), Ambiguous(Vec<u8>) }

impl Diagnostic {
    pub(super) fn write(&self, output: &mut impl Write) -> io::Result<()> {
        match self {
            Self::UnknownShort(letter) => { output.write_all(b"There is no -")?; output.write_all(&[*letter])?; output.write_all(b" option")?; }
            Self::UnknownLong(name) => { output.write_all(b"There is no ")?; output.write_all(name)?; output.write_all(b" option")?; }
            Self::Ambiguous(name) => { output.write_all(name)?; output.write_all(b" is an ambiguous abbreviation")?; }
        }
        output.write_all(b" (\"lz --help\" for help)\n")
    }
}

fn prefix(name: &[u8], option: &str) -> bool {
    let option = option.as_bytes();
    if name.is_empty() || name.len() > option.len() { return false; }
    if name[0].is_ascii_uppercase() { name.eq_ignore_ascii_case(&option[..name.len()]) }
    else { option.starts_with(name) }
}

pub(super) fn diagnose(arguments: &[OsString]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut pending_value = false;
    for argument in arguments {
        let argument = argument.as_bytes();
        if pending_value { pending_value = false; continue; }
        if argument == b"--" { break; }
        if argument.starts_with(b"+") && argument.len() > 1 { continue; }
        if argument.len() < 2 || !argument.starts_with(b"-") { break; }
        if let Some(long) = argument.strip_prefix(b"--") {
            let long = long.strip_prefix(b"+").or_else(|| long.strip_prefix(b"!")).unwrap_or(long);
            let name = long.split(|byte| *byte == b'=').next().unwrap();
            let exact = OPTIONS.iter().find(|option| option.names.iter().any(|alias| name.len() == alias.len() && prefix(name, alias)));
            let options: Vec<_> = OPTIONS.iter().filter(|option| option.names.iter().any(|alias| prefix(name, alias))).collect();
            let option = match exact {
                Some(option) => option,
                None if options.len() == 1 => options[0],
                None if options.is_empty() => { diagnostics.push(Diagnostic::UnknownLong(long.to_vec())); continue; }
                None => { diagnostics.push(Diagnostic::Ambiguous(long.to_vec())); continue; }
            };
            pending_value = option.value && !long.contains(&b'=');
        } else {
            let mut flags = &argument[1..];
            if flags.starts_with(b"+") || flags.starts_with(b"!") { flags = &flags[1..]; }
            for (index, letter) in flags.iter().enumerate() {
                if letter.is_ascii_digit() { continue; }
                let option = OPTIONS.iter().find(|option| option.shorts.contains(letter));
                let Some(option) = option else { diagnostics.push(Diagnostic::UnknownShort(*letter)); break; };
                if option.value { pending_value = index + 1 == flags.len(); break; }
            }
        }
    }
    diagnostics
}
