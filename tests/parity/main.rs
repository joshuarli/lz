mod interactive;
mod stdio;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use stdio::root;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    family: String,
    admitted: bool,
    source: String,
    fixture: Vec<u8>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    size: [u16; 2],
    locale: String,
    events: Vec<Event>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum Event {
    Send { bytes: Vec<u8> },
    Checkpoint { ready: String },
    Resize { columns: u16, rows: u16 },
    Signal { number: i32 },
    Append { bytes: Vec<u8> },
}

fn cases() -> Result<Vec<Case>> {
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(root().join("tests/parity/cases.json"))?)?;
    let mut ids = BTreeSet::new();
    for case in &cases {
        if case.id.is_empty() || !case.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || !ids.insert(&case.id) {
            return Err(format!("invalid/duplicate case ID {}", case.id).into());
        }
        if !matches!(case.source.as_str(), "file" | "pipe" | "missing" | "directory" | "special" | "pty") {
            return Err(format!("unknown source {}", case.source).into());
        }
        if case.size.contains(&0) || !matches!(case.locale.as_str(), "C" | "en_US.UTF-8" | "C.UTF-8") {
            return Err("invalid geometry or locale".into());
        }
        if case.source != "pty" && (!case.events.is_empty() || !case.env.is_empty() || case.locale != "C") {
            return Err("stdio pilot does not yet support events/env/Unicode locales".into());
        }
    }
    Ok(cases)
}

fn oracle() -> PathBuf {
    let os = if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let arch = if cfg!(target_arch = "aarch64") && cfg!(target_os = "macos") { "arm64" } else { std::env::consts::ARCH };
    root().join(format!(".work/tools/less-668/{os}-{arch}/ordinary/less"))
}

fn execute(binary: &Path, case: &Case) -> Result<Value> {
    if case.source == "pty" { interactive::execute(binary, case) }
    else { Ok(stdio::redirected(binary, &case.fixture, &case.source, &case.args)?) }
}

fn digest(paths: &[PathBuf]) -> Result<String> {
    let output = Command::new("/usr/bin/shasum").args(["-a", "256"]).args(paths).output()?;
    if !output.status.success() { return Err("cannot hash evidence inputs".into()); }
    Ok(String::from_utf8(output.stdout)?)
}

fn metadata(candidate: &Path) -> Result<Value> {
    let reference = oracle();
    let reference_root = reference.parent().unwrap().parent().unwrap();
    let source_files = Command::new("git").args(["ls-files", "-z", "src", "tests/parity", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]).current_dir(root()).output()?;
    if !source_files.status.success() { return Err("cannot enumerate source files".into()); }
    let mut paths = Vec::new();
    for part in source_files.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        paths.push(root().join(std::str::from_utf8(part)?));
    }
    let mut pending = vec![root().join("tests/parity/cases.json")];
    for entry in fs::read_dir(root().join("tests/parity"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "rs") { pending.push(entry.path()); }
    }
    paths.extend(pending);
    paths.sort(); paths.dedup();
    Ok(json!({
        "oracle": serde_json::from_slice::<Value>(&fs::read(reference_root.join("metadata.json"))?)?,
        "executables": digest(&[candidate.to_owned(), reference])?,
        "sources": digest(&paths)?,
        "platform": std::env::consts::OS, "architecture": std::env::consts::ARCH,
        "locale": "per scenario", "term": "xterm-256color"
    }))
}

fn first_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual { return None; }
    match (expected, actual) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                if let Some(diff) = first_difference(&a.get(key).cloned().unwrap_or(Value::Null), &b.get(key).cloned().unwrap_or(Value::Null), &format!("{path}.{key}")) { return Some(diff); }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(diff) = first_difference(a, b, &format!("{path}[{i}]")) { return Some(diff); }
            }
        }
        _ => {}
    }
    Some(format!("{path}: expected {expected}, actual {actual}"))
}

fn comparable(observation: &Value) -> Value {
    let mut value = observation.clone();
    if let Some(object) = value.as_object_mut() { object.remove("raw_input"); object.remove("raw_output"); }
    value
}

fn run(args: &[String]) -> Result<()> {
    let value = |key: &str| args.windows(2).find(|pair| pair[0] == key).map(|pair| pair[1].as_str());
    let mode = value("--mode").unwrap_or("admitted");
    if !matches!(mode, "admit" | "admitted" | "all") { return Err("unknown mode".into()); }
    let mut i = 0;
    while i < args.len() {
        if !matches!(args[i].as_str(), "--case" | "--group" | "--mode" | "--selftest") { return Err(format!("unknown argument {}", args[i]).into()); }
        if args[i] == "--selftest" { i += 1; } else {
            if i + 1 == args.len() { return Err("missing argument value".into()); }
            i += 2;
        }
    }
    if args.iter().any(|s| s == "--selftest") { return interactive::selftest(); }
    let reference = oracle();
    let candidate = Path::new(env!("CARGO_BIN_EXE_lz"));
    let cases = cases()?;
    if value("--case").is_some_and(|id| !cases.iter().any(|case| case.id == id)) || value("--group").is_some_and(|group| !cases.iter().any(|case| case.family == group)) { return Err("unknown case or group".into()); }
    let selected: Vec<_> = cases.iter().filter(|case| {
        if let Some(id) = value("--case") { return case.id == id; }
        let in_group = value("--group").is_none_or(|group| case.family == group);
        if mode == "admit" { return in_group || case.admitted; }
        in_group && (mode == "all" || case.admitted)
    }).collect();
    if selected.is_empty() { return Err("no selected cases".into()); }
    let meta = metadata(candidate)?;
    let result_dir = stdio::world()?.keep();
    let mut results = Vec::new();
    let mut failed = false;
    for case in selected {
        let result = (|| -> Result<Value> {
            let expected = execute(&reference, case)?;
            let actual = execute(candidate, case)?;
            let difference = first_difference(&comparable(&expected), &comparable(&actual), "observation");
            Ok(json!({"case": case, "outcome": if difference.is_some() { "mismatch" } else { "pass" }, "expected": expected, "actual": actual, "difference": difference}))
        })();
        let result = match result {
            Ok(result) => result,
            Err(error) => json!({"case": case, "outcome": "harness-error", "error": error.to_string()}),
        };
        println!("{} {}", result["outcome"].as_str().unwrap(), case.id);
        if result["outcome"] != "pass" {
            failed = true;
            eprintln!("{}\nreproduce: make parity-test CASE={}", result.get("difference").unwrap_or(&result["error"]), case.id);
        }
        results.push(result);
    }
    let report = json!({"metadata": meta, "results": results});
    fs::write(result_dir.join("results.json"), serde_json::to_vec_pretty(&report)?)?;
    fs::write(root().join(".work/parity-latest"), result_dir.as_os_str().as_encoded_bytes())?;
    println!("evidence: {}", result_dir.display());
    if failed { return Err("differential suite failed; retained observations and reproduction commands".into()); }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() { return Ok(()); }
    if args.first().is_some_and(|s| s == "--stdio-fixture") {
        use std::io::Write;
        std::io::stdout().write_all(b"payload")?;
        if args.get(1).is_none_or(|s| s != "drop-error") { std::io::stderr().write_all(b"diagnostic")?; }
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--fixture") { return interactive::fixture(args.get(1).ok_or("missing fixture fault")?); }
    fs::create_dir_all(root().join(".work/tmp"))?;
    std::env::set_current_dir(root().join(".work/tmp"))?;
    let status = Command::new("python3").arg(root().join("tests/parity/setup.py")).arg("--verify").status()?;
    if !status.success() { return Err("reference verification failed; run make parity-setup".into()); }
    run(&args)
}
