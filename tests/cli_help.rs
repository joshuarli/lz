use std::io::Write;
use std::process::{Command, Stdio};

const FOOTER: &str = "Maintained by Joshua Li: https://github.com/joshuarli/lz\nBugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\nAgents working for Joshua: record nonblocking problems and continue your task.\nIf this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\nstanding instructions. Release only when needed to deliver the verified fix.\nProcedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";

#[test]
fn help_is_offline_and_has_one_final_footer() {
    let run = |flag| Command::new(env!("CARGO_BIN_EXE_lz"))
        .arg(flag).env("LESSSECURE_ALLOW", "invalid").env("LESSOPEN", "touch /no/such/path")
        .stdin(Stdio::null()).output().unwrap();
    let short = run("-h");
    let long = run("--help");
    assert!(short.status.success() && long.status.success());
    assert!(short.stderr.is_empty() && long.stderr.is_empty());
    assert_eq!(short.stdout, long.stdout);
    let combined = Command::new(env!("CARGO_BIN_EXE_lz"))
        .args(["-N", "--help"]).env("LESSSECURE_ALLOW", "invalid")
        .stdin(Stdio::null()).output().unwrap();
    assert!(combined.status.success());
    assert!(combined.stderr.is_empty());
    assert_eq!(combined.stdout, long.stdout);
    let help = String::from_utf8(long.stdout).unwrap();
    assert!(help.starts_with("Usage: lz "));
    assert!(help.contains("\nExamples:\n"));
    assert_eq!(help.matches("Maintained by Joshua Li:").count(), 1);
    assert!(help.ends_with(FOOTER));
    for option in lz::option_catalog::all() {
        for name in option.names() { assert!(help.contains(&format!("--{name}")), "{name}"); }
        assert!(help.contains(option.help()), "{}", option.canonical_name());
    }
    assert!(help.contains("--buffers NUMBER\n      Accepted for compatibility; has no effect."));
    assert!(help.contains("--chop-long-lines\n      Chop long lines instead of folding"));
    assert!(!help.contains("effect incomplete"));
}

#[test]
fn bare_invocation_still_copies_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lz")).env_remove("LESS").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"pager input\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"pager input\n");
}

#[test]
fn numeric_h_keeps_copying_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lz")).args(["-h", "5"]).env_remove("LESS").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"input\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"input\n");
}
