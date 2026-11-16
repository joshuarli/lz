use std::io::{self, Write};
use lz::option_catalog::{all, ValueKind};

const EXAMPLES: &[&[&str]] = &[
    &["lz", "file.txt"],
    &["lz", "-N", "+120", "file.txt"],
];

const FOOTER: &str = "Maintained by Joshua Li: https://github.com/joshuarli/lz\nBugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\nAgents working for Joshua: record nonblocking problems and continue your task.\nIf this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\nstanding instructions. Release only when needed to deliver the verified fix.\nProcedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";

pub(super) fn write(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "Usage: lz [OPTIONS] [+COMMAND] [FILE ...]\n\nPage files or standard input. With redirected stdout, copy input bytes.\n\nArguments:\n  FILE ...       Files to page; - or no file reads standard input (optional, repeated)\n  +COMMAND       Run a startup command; ++COMMAND repeats it for every file\n  --             End option parsing\n\nOptions:\n  -h, --help     Print this reference (standalone -h); -h NUMBER sets max-back-scroll")?;
    for option in all() {
        let mut names: Vec<String> = option.shorts().iter().map(|c| format!("-{}", *c as char)).collect();
        names.extend(option.names().iter().map(|name| format!("--{name}")));
        let value = match option.value() { ValueKind::Flag => "", ValueKind::Number => " NUMBER", ValueKind::Text => " TEXT" };
        writeln!(out, "  {}{}\n      {}", names.join(", "), value, description(option.canonical_name()))?;
    }
    writeln!(out, "\nOptions are read before the first file. LESS supplies default options.\nMany less options are parsed for compatibility but their effects are incomplete.\nDefaults: fold lines, case-sensitive search, no displayed line numbers.\n\nEnvironment:\n  LESSCHARSET, LC_ALL, LC_CTYPE, LANG  Select character handling\n  LESSOPEN, LESSCLOSE, SHELL          Preprocessor and shell settings\n  LESSSECURE, LESSSECURE_ALLOW        Restrict features\n  LESSHISTFILE, LESSHISTSIZE, HOME, XDG_STATE_HOME, XDG_DATA_HOME  History\n  VISUAL, EDITOR                      Editor name in prompts\n\nExamples:")?;
    for example in EXAMPLES { writeln!(out, "  {}", example.join(" "))?; }
    writeln!(out)?;
    out.write_all(FOOTER.as_bytes())
}

fn description(name: &str) -> String {
    let text = match name {
        "help" => "Open the interactive help page with -?",
        "version" => "Print version and exit",
        "force" => "Allow binary input",
        "quit-if-one-screen" => "Quit if input fits on one screen",
        "quit-at-eof" => "Quit at end of file",
        "quit-on-intr" => "Quit on interrupt",
        "no-lessopen" => "Disable LESSOPEN preprocessing",
        "ignore-case" => "Set search case policy (-i smart, -I insensitive)",
        "search-skip-screen" => "Set search start (-a after screen, -A after target)",
        "line-numbers" => "Set line numbering (-n off, -N display)",
        "raw-control-chars" => "Set control display (-r raw, -R color)",
        "long-prompt" => "Select medium (-m) or long (-M) prompt",
        "chop-long-lines" => "Chop long lines instead of folding",
        "pattern" => "Search for TEXT at startup",
        "prompt" => "Set a prompt prototype",
        "tag" => "Open the named tag",
        "tag-file" => "Use TEXT as the tags file",
        "tabs" => "Set tab stops",
        "window" => "Set the page window size",
        "jump-target" => "Set the target screen row or fraction",
        "shift" => "Set horizontal shift in columns or a fraction",
        "line-num-width" => "Set line number width (0 through 16)",
        "no-init" => "Keep the primary screen",
        "no-keypad" => "Do not enable application keypad modes",
        "tilde" => "Leave lines after EOF blank",
        "use-color" => "Use configured UI colors",
        "incsearch" => "Preview searches while typing",
        "no-histdups" => "Move reused history entries to the end",
        "no-vbell" => "Suppress visual bells",
        "quiet" => "Select boundary or general visual bell",
        "header" => "Set fixed header dimensions",
        "hilite-search" => "Select search highlighting",
        "proc-backspace" => "Select backspace display",
        "no-search-header-lines" => "Exclude header lines from search",
        "no-search-header-columns" => "Exclude header columns from search",
        "no-search-headers" => "Exclude headers from search",
        _ => return format!("{} (parsed; effect incomplete)", name.replace('-', " ")),
    };
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn examples_parse_without_diagnostics() {
        for example in EXAMPLES {
            let parsed = crate::options::parse(example[1..].iter().map(|s| (*s).into()).collect(), None);
            assert!(parsed.diagnostics.is_empty(), "{example:?}");
            assert!(matches!(parsed.action, crate::options::Action::Page));
            assert_eq!(parsed.filenames.last().unwrap(), "file.txt");
        }
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
