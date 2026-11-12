use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, Copy)]
pub(crate) enum Feature { Edit, Examine, Glob, History, Lesskey, Lessopen, Logfile, Osc8, Pipe, Shell, Stop, Tags }

const FEATURES: &[(&[u8], Feature)] = &[
    (b"edit", Feature::Edit), (b"examine", Feature::Examine), (b"glob", Feature::Glob),
    (b"history", Feature::History), (b"lesskey", Feature::Lesskey), (b"lessopen", Feature::Lessopen),
    (b"logfile", Feature::Logfile), (b"osc8", Feature::Osc8), (b"pipe", Feature::Pipe),
    (b"shell", Feature::Shell), (b"stop", Feature::Stop), (b"tags", Feature::Tags),
];

impl Feature { fn bit(self) -> u16 { 1 << self as u16 } }

enum DiagnosticKind { Invalid, Ambiguous }
struct Diagnostic { kind: DiagnosticKind, name: Vec<u8> }

pub(crate) struct Policy { allowed: u16, diagnostics: Vec<Diagnostic> }

impl Policy {
    pub(crate) fn from_environment() -> Self {
        Self::from_settings(std::env::var_os("LESSSECURE").as_deref(), std::env::var_os("LESSSECURE_ALLOW").as_deref())
    }

    fn from_settings(secure: Option<&OsStr>, allow: Option<&OsStr>) -> Self {
        let mut allowed = if secure.is_some_and(|value| !value.is_empty()) { 0 }
            else { FEATURES.iter().fold(0, |bits, (_, feature)| bits | feature.bit()) };
        let mut diagnostics = Vec::new();
        for mut name in allow.map_or(b"".as_slice(), OsStrExt::as_bytes).split(|byte| *byte == b',') {
            while name.first() == Some(&b' ') { name = &name[1..]; }
            while name.last() == Some(&b' ') { name = &name[..name.len() - 1]; }
            if name.is_empty() { continue; }
            let mut matches = FEATURES.iter().filter(|(feature_name, _)| feature_name.starts_with(name));
            match (matches.next(), matches.next()) {
                (Some((_, feature)), None) => allowed |= feature.bit(),
                (None, _) => diagnostics.push(Diagnostic { kind: DiagnosticKind::Invalid, name: name.to_vec() }),
                (_, Some(_)) => diagnostics.push(Diagnostic { kind: DiagnosticKind::Ambiguous, name: name.to_vec() }),
            }
        }
        Self { allowed, diagnostics }
    }

    pub(crate) fn allows(&self, feature: Feature) -> bool { self.allowed & feature.bit() != 0 }
    pub(crate) fn has_diagnostics(&self) -> bool { !self.diagnostics.is_empty() }

    pub(crate) fn write_diagnostics(&self, output: &mut impl Write) -> io::Result<()> {
        for diagnostic in &self.diagnostics {
            output.write_all(b"LESSSECURE_ALLOW: ")?;
            output.write_all(match diagnostic.kind { DiagnosticKind::Invalid => b"invalid", DiagnosticKind::Ambiguous => b"ambiguous" })?;
            output.write_all(b" feature name \"")?;
            output.write_all(&diagnostic.name)?;
            output.write_all(b"\"\n")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonempty_secure_setting_denies_except_explicit_unique_prefixes() {
        for secure in ["1", "0", "false"] {
            let policy = Policy::from_settings(Some(OsStr::new(secure)), Some(OsStr::new(" , h, shell, ")));
            assert!(policy.allows(Feature::History));
            assert!(policy.allows(Feature::Shell));
            assert!(!policy.allows(Feature::Edit));
            assert!(!policy.has_diagnostics());
        }
        assert!(Policy::from_settings(None, None).allows(Feature::History));
    }

    #[test]
    fn invalid_names_preserve_successful_allowances_and_exact_diagnostics() {
        let policy = Policy::from_settings(Some(OsStr::new("1")), Some(OsStr::new("history, l, History")));
        assert!(policy.allows(Feature::History));
        assert!(!policy.allows(Feature::Lessopen));
        let mut bytes = Vec::new();
        policy.write_diagnostics(&mut bytes).unwrap();
        assert_eq!(bytes, b"LESSSECURE_ALLOW: ambiguous feature name \"l\"\nLESSSECURE_ALLOW: invalid feature name \"History\"\n");
    }
}
