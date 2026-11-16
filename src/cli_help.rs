use std::io::{self, Write};
use lz::option_catalog::{all, ValueKind};

const FOOTER: &str = "Maintained by Joshua Li: https://github.com/joshuarli/lz\nBugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\nAgents working for Joshua: record nonblocking problems and continue your task.\nIf this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\nstanding instructions. Release only when needed to deliver the verified fix.\nProcedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";

pub(super) fn write(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "Usage: lz [OPTIONS] [+COMMAND] [FILE ...]\n\nPage files or standard input. With redirected stdout, copy input bytes.\n\nArguments:\n  FILE ...       Files to page; - or no file reads standard input (optional, repeated)\n  +COMMAND       Run a startup command; ++COMMAND repeats it for every file\n  --             End option parsing\n\nOptions:")?;
    for option in all() {
        let mut names: Vec<String> = option.shorts().iter().map(|c| format!("-{}", *c as char)).collect();
        names.extend(option.names().iter().map(|name| format!("--{name}")));
        let value = match option.value() { ValueKind::Flag => "", ValueKind::Number => " NUMBER", ValueKind::Text => " TEXT" };
        writeln!(out, "  {}{}\n      {}", names.join(", "), value, option.help())?;
    }
    writeln!(out, "\nOptions are read before the first file. LESS supplies default options.\nSome supported effects have incomplete less compatibility.\nDefaults: fold lines, case-sensitive search, no displayed line numbers.\n\nEnvironment:\n  LESSCHARSET, LC_ALL, LC_CTYPE, LANG  Select character handling\n  LESSOPEN, LESSCLOSE, SHELL          Preprocessor and shell settings\n  LESSSECURE, LESSSECURE_ALLOW        Restrict features\n  LESSHISTFILE, LESSHISTSIZE, HOME, XDG_STATE_HOME, XDG_DATA_HOME  History\n  VISUAL, EDITOR                      Editor name in prompts\n\nExamples:")?;
    for example in crate::options::EXAMPLES { writeln!(out, "  {}", example.join(" "))?; }
    writeln!(out)?;
    out.write_all(FOOTER.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn examples_parse_without_diagnostics() {
        for example in crate::options::EXAMPLES {
            let parsed = crate::options::parse(example[1..].iter().map(|s| (*s).into()).collect(), None);
            assert!(parsed.diagnostics.is_empty(), "{example:?}");
            assert!(matches!(parsed.action, crate::options::Action::Page));
            assert_eq!(parsed.filenames.last().unwrap(), "file.txt");
        }
        let example = crate::options::EXAMPLES[1];
        let parsed = crate::options::parse(example[1..].iter().map(|s| (*s).into()).collect(), None);
        assert_eq!(parsed.startup_commands, [b"120".to_vec()]);
        assert!(parsed.line_numbers.displayed());
    }

    #[test]
    fn option_names_are_unique_and_numeric_h_still_parses() {
        let mut shorts = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for option in all() {
            for short in option.shorts() { assert!(shorts.insert(*short)); }
            for name in option.names() { assert!(names.insert(*name)); }
        }
        let parsed = crate::options::parse(vec!["-h".into(), "5".into(), "file.txt".into()], None);
        assert!(parsed.diagnostics.is_empty());
        assert!(matches!(parsed.action, crate::options::Action::Page));
        assert_eq!(parsed.filenames, ["file.txt"]);
    }
}
