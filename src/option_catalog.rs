/// The kind of value an option accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind { Flag, Number, Text }

/// Names and value kind for an option.
pub struct OptionSpec { shorts: &'static [u8], names: &'static [&'static str], value: ValueKind, help: &'static str }

const OPTIONS: &[OptionSpec] = &[
    OptionSpec { shorts: b"?", names: &["help"], value: ValueKind::Flag, help: "Print this reference with --help; -? opens the less help page" },
    OptionSpec { shorts: b"aA", names: &["search-skip-screen"], value: ValueKind::Flag, help: "Set search start (-a after screen, -A after target)" },
    OptionSpec { shorts: b"b", names: &["buffers"], value: ValueKind::Number, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"B", names: &["auto-buffers"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"cC", names: &["clear-screen"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"d", names: &["dumb"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"D", names: &["color"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"eE", names: &["quit-at-eof"], value: ValueKind::Flag, help: "Quit after an EOF attempt (-e) or upon reaching EOF (-E)" },
    OptionSpec { shorts: b"f", names: &["force"], value: ValueKind::Flag, help: "Allow binary input" },
    OptionSpec { shorts: b"F", names: &["quit-if-one-screen"], value: ValueKind::Flag, help: "Quit if input fits on one screen" },
    OptionSpec { shorts: b"gG", names: &["hilite-search"], value: ValueKind::Flag, help: "Highlight only the current match (-g) or none (-G)" },
    OptionSpec { shorts: b"h", names: &["max-back-scroll"], value: ValueKind::Number, help: "A number is accepted for compatibility but has no effect; standalone -h prints this reference" },
    OptionSpec { shorts: b"iI", names: &["ignore-case"], value: ValueKind::Flag, help: "Set search case policy (-i smart, -I insensitive)" },
    OptionSpec { shorts: b"j", names: &["jump-target"], value: ValueKind::Text, help: "Set the target screen row or fraction" },
    OptionSpec { shorts: b"J", names: &["status-column"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"k", names: &["lesskey-file"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["lesskey-src"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["lesskey-content"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"K", names: &["quit-on-intr"], value: ValueKind::Flag, help: "Quit with status 2 on interrupt" },
    OptionSpec { shorts: b"L", names: &["no-lessopen"], value: ValueKind::Flag, help: "Disable LESSOPEN preprocessing" },
    OptionSpec { shorts: b"mM", names: &["long-prompt"], value: ValueKind::Flag, help: "Select medium (-m) or long (-M) prompt" },
    OptionSpec { shorts: b"nN", names: &["line-numbers"], value: ValueKind::Flag, help: "Disable line counting (-n) or display line numbers (-N)" },
    OptionSpec { shorts: b"oO", names: &["log-file"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"p", names: &["pattern"], value: ValueKind::Text, help: "Search for TEXT at startup" },
    OptionSpec { shorts: b"P", names: &["prompt"], value: ValueKind::Text, help: "Set a prompt prototype; selectors s, m, M, =, h and w choose a slot" },
    OptionSpec { shorts: b"qQ", names: &["quiet", "silent"], value: ValueKind::Flag, help: "Use visual bell at boundaries (-q) or for all errors (-Q)" },
    OptionSpec { shorts: b"rR", names: &["raw-control-chars"], value: ValueKind::Flag, help: "Set control display (-r raw, -R color)" },
    OptionSpec { shorts: b"s", names: &["squeeze-blank-lines"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"S", names: &["chop-long-lines"], value: ValueKind::Flag, help: "Chop long lines instead of folding" },
    OptionSpec { shorts: b"t", names: &["tag"], value: ValueKind::Text, help: "Open the named tag" },
    OptionSpec { shorts: b"T", names: &["tag-file"], value: ValueKind::Text, help: "Use TEXT as the tags file" },
    OptionSpec { shorts: b"uU", names: &["underline-special"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"V", names: &["version"], value: ValueKind::Flag, help: "Print version and exit" },
    OptionSpec { shorts: b"wW", names: &["hilite-unread"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"x", names: &["tabs"], value: ValueKind::Text, help: "Set tab stops as a comma-separated list" },
    OptionSpec { shorts: b"X", names: &["no-init"], value: ValueKind::Flag, help: "Keep the primary screen" },
    OptionSpec { shorts: b"y", names: &["max-forw-scroll"], value: ValueKind::Number, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"z", names: &["window"], value: ValueKind::Number, help: "Set page window size; nonpositive values are relative to screen height" },
    OptionSpec { shorts: b"\"", names: &["quotes"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"~", names: &["tilde"], value: ValueKind::Flag, help: "Leave lines after EOF blank" },
    OptionSpec { shorts: b"#", names: &["shift"], value: ValueKind::Text, help: "Set horizontal shift in columns or a fraction" },
    OptionSpec { shorts: b"", names: &["exit-follow-on-close"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["file-size"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["follow-name"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["header"], value: ValueKind::Text, help: "Set fixed header dimensions" },
    OptionSpec { shorts: b"", names: &["incsearch"], value: ValueKind::Flag, help: "Preview searches while typing" },
    OptionSpec { shorts: b"", names: &["intr"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["line-num-width"], value: ValueKind::Number, help: "Set line number width (0 through 16)" },
    OptionSpec { shorts: b"", names: &["match-shift"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["modelines"], value: ValueKind::Number, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["mouse"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["no-keypad"], value: ValueKind::Flag, help: "Do not enable application keypad and cursor modes" },
    OptionSpec { shorts: b"", names: &["no-histdups"], value: ValueKind::Flag, help: "Move reused history entries to the end" },
    OptionSpec { shorts: b"", names: &["no-number-headers"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["no-search-header-lines"], value: ValueKind::Flag, help: "Exclude header lines from search" },
    OptionSpec { shorts: b"", names: &["no-search-header-columns"], value: ValueKind::Flag, help: "Exclude header columns from search" },
    OptionSpec { shorts: b"", names: &["no-search-headers"], value: ValueKind::Flag, help: "Exclude headers from search" },
    OptionSpec { shorts: b"", names: &["no-vbell"], value: ValueKind::Flag, help: "Suppress visual bells" },
    OptionSpec { shorts: b"", names: &["proc-backspace"], value: ValueKind::Flag, help: "Use overstrike display; uppercase --PROC-BACKSPACE displays backspaces as controls" },
    OptionSpec { shorts: b"", names: &["proc-return"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["proc-tab"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["redraw-on-quit"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["rscroll"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["save-marks"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["search-options"], value: ValueKind::Text, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["show-preproc-errors"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["status-col-width"], value: ValueKind::Number, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["status-line"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["use-backslash"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["use-color"], value: ValueKind::Flag, help: "Use configured UI colors" },
    OptionSpec { shorts: b"", names: &["wheel-lines"], value: ValueKind::Number, help: "Accepted for compatibility; has no effect." },
    OptionSpec { shorts: b"", names: &["wordwrap"], value: ValueKind::Flag, help: "Accepted for compatibility; has no effect." },
];

impl OptionSpec {
    /// Return accepted short option letters.
    pub fn shorts(&self) -> &'static [u8] { self.shorts }
    /// Return the canonical long option name.
    pub fn canonical_name(&self) -> &'static str { self.names[0] }
    /// Return all accepted long names.
    pub fn names(&self) -> &'static [&'static str] { self.names }
    /// Return the option's help text.
    pub fn help(&self) -> &'static str { self.help }
    /// Return the option's value kind.
    pub fn value(&self) -> ValueKind { self.value }
}

/// Whether a long option name is unknown or matches several options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionLookupError { Unknown, Ambiguous }

impl std::fmt::Display for OptionLookupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self { Self::Unknown => "unknown option name", Self::Ambiguous => "ambiguous option name" })
    }
}

impl std::error::Error for OptionLookupError {}

/// Return known option names and value kinds.
pub fn all() -> &'static [OptionSpec] { OPTIONS }

fn prefix(name: &[u8], option: &str) -> bool {
    let option = option.as_bytes();
    if name.is_empty() || name.len() > option.len() { return false; }
    if name[0].is_ascii_uppercase() { name.eq_ignore_ascii_case(&option[..name.len()]) }
    else { option.starts_with(name) }
}

/// Resolve an exact or unique abbreviated long option name.
pub fn lookup(name: &[u8]) -> Result<&'static OptionSpec, OptionLookupError> {
    if let Some(exact) = OPTIONS.iter().find(|option| option.names.iter().any(|alias| name.len() == alias.len() && prefix(name, alias))) { return Ok(exact); }
    let mut matches = OPTIONS.iter().filter(|option| option.names.iter().any(|alias| prefix(name, alias)));
    match (matches.next(), matches.next()) { (Some(option), None) => Ok(option), (None, _) => Err(OptionLookupError::Unknown), _ => Err(OptionLookupError::Ambiguous) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_distinguishes_aliases_abbreviations_and_errors() {
        assert_eq!(lookup(b"silent").unwrap().canonical_name(), "quiet");
        assert_eq!(lookup(b"raw-control").unwrap().canonical_name(), "raw-control-chars");
        assert_eq!(lookup(b"RAW-CONTROL").unwrap().canonical_name(), "raw-control-chars");
        assert!(matches!(lookup(b"no"), Err(OptionLookupError::Ambiguous)));
        assert!(matches!(lookup(b"unknown-option"), Err(OptionLookupError::Unknown)));
        assert!(matches!(lookup(b""), Err(OptionLookupError::Unknown)));
    }
}

