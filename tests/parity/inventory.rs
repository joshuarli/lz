use super::{Result, Value, cases, digest, json, metadata, oracle, root};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    reference_sha256: String,
    entries: Vec<Entry>,
    upstream: Vec<Upstream>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    family: String,
    title: String,
    anchor: Anchor,
    slice: String,
    platforms: Vec<String>,
    cases: Vec<String>,
    implementation: String,
    evidence: String,
    cause: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor { file: String, line: usize, text: String }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upstream {
    id: String,
    file: String,
    sha256: String,
    instrumented: String,
    cases: Vec<String>,
    disposition: String,
    cause: String,
}

fn report_at(pointer: &str) -> Result<Value> {
    let path = PathBuf::from(fs::read_to_string(root().join(pointer))?);
    let report: Value = serde_json::from_slice(&fs::read(path.join("results.json"))?)?;
    let binary = if report["candidate_kind"] == "current" { PathBuf::from(env!("CARGO_BIN_EXE_lz")) }
        else { root().join(".work/tools/baseline-target/debug/lz") };
    if report["metadata"] != metadata(&binary)? { return Err(format!("stale evidence at {pointer}; rerun its full differential command").into()); }
    Ok(report)
}

fn safe_relative(path: &str) -> bool {
    Path::new(path).components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

pub fn check_and_report() -> Result<()> {
    let inventory: Inventory = serde_json::from_slice(&fs::read(root().join("tests/parity/inventory.json"))?)?;
    let reference = oracle();
    let base = reference.parent().unwrap();
    let build: Value = serde_json::from_slice(&fs::read(base.parent().unwrap().join("metadata.json"))?)?;
    if inventory.reference_sha256 != build["archive_sha256"] { return Err("inventory reference archive differs".into()); }
    let scenarios = cases()?;
    let ids: BTreeSet<_> = scenarios.iter().map(|case| case.id.as_str()).collect();
    let current = report_at(".work/parity-current")?;
    let baseline = report_at(".work/parity-baseline")?;
    let upstream_run: Value = serde_json::from_slice(&fs::read(root().join(".work/upstream-results.json"))?)?;
    if upstream_run["metadata"] != build || upstream_run["outcome"] != "pass" { return Err("upstream evidence differs or failed".into()); }
    let mut reports = BTreeMap::new();
    for (name, report) in [("current", &current), ("baseline", &baseline)] {
        let results = report["results"].as_array().ok_or("invalid results")?;
        let mut outcomes = BTreeMap::new();
        for result in results {
            let id = result["case"]["id"].as_str().ok_or("missing case ID")?;
            let original = scenarios.iter().find(|case| case.id == id).ok_or("result has unknown case")?;
            if serde_json::to_value(original)? != result["case"] { return Err("result scenario is stale".into()); }
            let outcome = result["outcome"].as_str().ok_or("missing outcome")?;
            if !matches!(outcome, "pass" | "mismatch" | "harness-error" | "needs-environment") || outcomes.insert(id, outcome).is_some() { return Err("invalid/duplicate result".into()); }
        }
        if outcomes.keys().copied().collect::<BTreeSet<_>>() != ids { return Err(format!("{name} result accounting incomplete; run MODE=all").into()); }
        reports.insert(name, outcomes);
    }
    let mut seen = BTreeSet::new();
    let mut anchors = BTreeSet::new();
    let mut counts: BTreeMap<&str, [usize; 4]> = BTreeMap::new();
    for entry in &inventory.entries {
        if !seen.insert(&entry.id) || !safe_relative(&entry.anchor.file) || entry.anchor.line == 0 { return Err("invalid inventory ID or anchor".into()); }
        let source = fs::read_to_string(base.join(&entry.anchor.file))?;
        if source.lines().nth(entry.anchor.line - 1) != Some(entry.anchor.text.as_str()) { return Err(format!("stale source anchor {}", entry.id).into()); }
        if entry.anchor.file == "less.nro" { anchors.insert(entry.anchor.line); }
        if entry.slice.is_empty() || entry.title.is_empty() || entry.cause.is_empty() { return Err("missing ownership/title/cause".into()); }
        if !matches!(entry.implementation.as_str(), "unassessed" | "unimplemented" | "partial" | "implemented") || !matches!(entry.evidence.as_str(), "not tested" | "known incompatible" | "partially compatible" | "compatible in the named tested scenarios" | "platform-specific with evidence") { return Err("unknown implementation/evidence state".into()); }
        for id in &entry.cases { if !ids.contains(id.as_str()) { return Err("unknown scenario link".into()); } }
        if entry.evidence == "compatible in the named tested scenarios" && (entry.cases.is_empty() || entry.cases.iter().any(|id| reports["current"][id.as_str()] != "pass")) { return Err(format!("unsupported compatibility claim {}", entry.id).into()); }
        if !entry.platforms.is_empty() {
            let count = counts.entry(&entry.family).or_default();
            count[0] += 1;
            count[1] += usize::from(entry.implementation == "implemented");
            count[2] += usize::from(!entry.cases.is_empty());
            count[3] += usize::from(entry.evidence == "compatible in the named tested scenarios");
        }
    }
    let manual = fs::read_to_string(base.join("less.nro"))?;
    let end = manual.lines().position(|line| line == ".SH \"SEE ALSO\"").ok_or("manual section missing")?;
    for (index, line) in manual.lines().enumerate().take(end) {
        if (line.starts_with(".IP ") || line.starts_with(".SH ")) && !anchors.contains(&(index + 1)) { return Err(format!("unaccounted manual surface at {}", index + 1).into()); }
    }
    let expected: BTreeSet<_> = fs::read_dir(base.join("lesstest/lt"))?.map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned())).collect::<std::io::Result<_>>()?;
    let mut upstream_names = BTreeSet::new();
    for upstream in &inventory.upstream {
        if !safe_relative(&upstream.file) || upstream.id.is_empty() || upstream.disposition.is_empty() || upstream.cause.is_empty() { return Err("invalid upstream disposition".into()); }
        let file = base.join(&upstream.file);
        upstream_names.insert(file.file_name().unwrap().to_string_lossy().into_owned());
        if !digest(std::slice::from_ref(&file))?.starts_with(&upstream.sha256) { return Err("upstream input digest differs".into()); }
        for id in &upstream.cases { if !ids.contains(id.as_str()) { return Err("invalid upstream translation link".into()); } }
        if upstream.instrumented != "pass" { return Err("upstream instrumented disposition lacks pass".into()); }
    }
    if expected != upstream_names || expected.len() != 14 { return Err("upstream accounting incomplete".into()); }
    let mut markdown = String::from("# less 668 parity coverage\n\nGenerated from `inventory.json` and fingerprint-checked full current/baseline results. Counts are manual/source entries, not proof of all interactions. Platform: macOS arm64, POSIX regex, system ncurses. Four platform-only manual entries are outside Unix scope. Other platforms and S14 performance remain unproved.\n\n| Family | Applicable entries | Implemented | Tested by named cases | Compatible in named scenarios |\n| --- | ---: | ---: | ---: | ---: |\n");
    for (family, count) in &counts { markdown.push_str(&format!("| {family} | {} | {} | {} | {} |\n", count[0], count[1], count[2], count[3])); }
    markdown.push_str("\nUntested entries stay in the denominator. A named scenario does not cover every boundary of its linked manual entry. See the canonical inventory for every anchor, cause, owner and platform.\n\n| Corpus | Pass | Mismatch | Harness error | Needs environment | Total |\n| --- | ---: | ---: | ---: | ---: | ---: |\n");
    let mut result_counts = BTreeMap::new();
    for (name, outcomes) in &reports {
        let count = |state: &str| outcomes.values().filter(|s| **s == state).count();
        markdown.push_str(&format!("| {name} | {} | {} | {} | {} | {} |\n", count("pass"), count("mismatch"), count("harness-error"), count("needs-environment"), outcomes.len()));
        result_counts.insert(*name, json!({"pass":count("pass"),"mismatch":count("mismatch"),"harness-error":count("harness-error"),"needs-environment":count("needs-environment"),"total":outcomes.len()}));
    }
    markdown.push_str("\nAll 14 instrumented upstream tests pass against instrumented less. Two have partial generic VT translations; twelve await translation. These are oracle validation results, not lz passes. Discovery mismatches remain strict failures in `make parity-test MODE=all`. Identity normalization changes only the exact program token in recognized option-diagnostic help hints. Other diagnostic bytes and all other locations remain strict.\n");
    fs::write(root().join("tests/parity/coverage.md"), markdown)?;
    fs::write(root().join("tests/parity/coverage.json"), serde_json::to_vec_pretty(&json!({"counts":counts,"outcomes":result_counts,"metadata":current["metadata"],"upstream":upstream_run}))?)?;
    println!("{} inventory entries; 14 upstream dispositions; all {} scenarios accounted for on current and baseline", inventory.entries.len(), ids.len());
    Ok(())
}
