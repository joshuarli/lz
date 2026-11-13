/// The kind of value an option accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind { Flag, Number, Text }

/// Names and value kind for an option.
pub struct OptionSpec { shorts: &'static [u8], names: &'static [&'static str], value: ValueKind }

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

impl OptionSpec {
    /// Return accepted short option letters.
    pub fn shorts(&self) -> &'static [u8] { self.shorts }
    /// Return the canonical long option name.
    pub fn canonical_name(&self) -> &'static str { self.names[0] }
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

