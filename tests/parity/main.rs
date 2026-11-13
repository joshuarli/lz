mod resources;
mod spill;
mod fixtures;
mod fuzz;
mod inventory;
mod interactive;
mod stdio;
mod supervisor;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use stdio::root;
use fixtures::Fixture;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
struct NeedsEnvironment(String);
impl std::fmt::Display for NeedsEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str(&self.0) }
}
impl std::error::Error for NeedsEnvironment {}

#[derive(Debug)]
struct TraceViolation(&'static str);
impl std::fmt::Display for TraceViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str(self.0) }
}
impl std::error::Error for TraceViolation {}

fn write_fixture(path: &Path, fixture: &Fixture) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    fixture.validate()?;
    let file = match fs::File::create(path) {
        Ok(file) => file,
        Err(error) => {
            if cfg!(target_os = "macos") && error.raw_os_error() == Some(92)
                && path.file_name().is_some_and(|name| std::str::from_utf8(name.as_bytes()).is_err()) {
                return Err(Box::new(NeedsEnvironment("host filesystem rejects invalid UTF-8 fixture names (EILSEQ); repeat on a byte-preserving Unix filesystem".into())));
            }
            return Err(error.into());
        }
    };
    let mut output = std::io::BufWriter::new(file);
    fixture.write_to(&mut output)?;
    std::io::Write::flush(&mut output)?;
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    family: String,
    admitted: bool,
    source: String,
    #[serde(default)]
    pty_rss_limit: resources::RssLimit,
    #[serde(default)]
    pty_spill_limit: spill::Limit,
    fixture: Fixture,
    args: Vec<Argument>,
    fixture_name: Vec<u8>,
    files: BTreeMap<String, Vec<u8>>,
    env: BTreeMap<String, String>,
    size: [u16; 2],
    locale: String,
    events: Vec<Event>,
    #[serde(default)]
    trace_check: TraceCheck,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TraceCheck { #[default] Captured, PlainText }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum Argument { Text(String), Bytes(ByteArgument) }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ByteArgument { bytes: Vec<u8> }

impl Argument {
    fn os_string(&self) -> OsString {
        match self { Self::Text(text) => text.into(), Self::Bytes(bytes) => OsString::from_vec(bytes.bytes.clone()) }
    }
}

impl From<&str> for Argument { fn from(value: &str) -> Self { Self::Text(value.into()) } }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum Event {
    Send { bytes: Vec<u8> },
    Checkpoint { ready: String, row: Option<usize> },
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
        if !matches!(case.source.as_str(), "file" | "pipe" | "pty" | "pty-pipe") {
            return Err(format!("unknown source {}", case.source).into());
        }
        for name in std::iter::once(OsString::from_vec(case.fixture_name.clone())).chain(case.files.keys().map(OsString::from)) {
            if name.is_empty() || !Path::new(&name).components().all(|component| matches!(component, std::path::Component::Normal(_))) { return Err("unsafe fixture path".into()); }
        }
        if case.size.contains(&0) || !matches!(case.locale.as_str(), "C" | "en_US.UTF-8" | "C.UTF-8") {
            return Err("invalid geometry or locale".into());
        }
        if !case.source.starts_with("pty") && (!case.events.is_empty() || !case.env.is_empty() || case.locale != "C" || !matches!(case.trace_check, TraceCheck::Captured)) {
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

struct Execution { observation: Value, resources: Option<resources::Record> }

fn execute(binary: &Path, case: &Case) -> Result<Execution> {
    if case.source.starts_with("pty") {
        let (observation, resources) = interactive::execute_recorded(binary, case)?;
        Ok(Execution { observation, resources: Some(resources) })
    } else {
        let observation = stdio::redirected(binary, &case.fixture, &case.source, &case.args.iter().map(Argument::os_string).collect::<Vec<_>>(), &case.fixture_name, &case.files)?;
        Ok(Execution { observation, resources: None })
    }
}

fn digest(paths: &[PathBuf]) -> Result<String> {
    let output = Command::new("/usr/bin/shasum").args(["-a", "256"]).args(paths).output()?;
    if !output.status.success() { return Err("cannot hash evidence inputs".into()); }
    Ok(String::from_utf8(output.stdout)?)
}

fn metadata(candidate: &Path) -> Result<Value> {
    let reference = oracle();
    let reference_root = reference.parent().unwrap().parent().unwrap();
    let source_files = Command::new("git").args(["ls-files", "-co", "--exclude-standard", "-z", "src", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]).current_dir(root()).output()?;
    if !source_files.status.success() { return Err("cannot enumerate source files".into()); }
    let mut paths = Vec::new();
    for part in source_files.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        paths.push(root().join(std::str::from_utf8(part)?));
    }
    let mut pending = vec![root().join("tests/parity/cases.json"), root().join("tests/parity/inventory.json")];
    for entry in fs::read_dir(root().join("tests/parity"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "rs" || e == "py") { pending.push(entry.path()); }
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
    if let Some(object) = value.as_object_mut() {
        object.remove("raw_input"); object.remove("raw_output");
        if let Some(stderr) = object.get_mut("stderr")
            && let Ok(bytes) = serde_json::from_value::<Vec<u8>>(stderr.clone()) {
            let mut normalized = Vec::new();
            for line in bytes.split_inclusive(|byte| *byte == b'\n') {
                let mut replacement = None;
                for program in [b"less".as_slice(), b"lz".as_slice()] {
                    let suffix = [b" (\"".as_slice(), program, b" --help\" for help)\n".as_slice()].concat();
                    if let Some(prefix) = line.strip_suffix(suffix.as_slice())
                        && ((prefix.starts_with(b"There is no ") && prefix.ends_with(b" option"))
                            || prefix.ends_with(b" is an ambiguous abbreviation")) {
                        replacement = Some([prefix, b" (\"<program> --help\" for help)\n".as_slice()].concat());
                        break;
                    }
                }
                normalized.extend_from_slice(replacement.as_deref().unwrap_or(line));
            }
            *stderr = json!(normalized);
        }
    }
    value
}

fn execution_error(case: &Case, error: &(dyn std::error::Error + 'static), side: &str) -> Value {
    let oracle_leak = side == "oracle" && (error.is::<interactive::KernelViolation>() || error.downcast_ref::<ptytest::PtyTestError>().is_some_and(|error| matches!(error, ptytest::PtyTestError::SnapshotMismatch { path, .. } if path == Path::new("terminal-baseline"))));
    json!({"case":case,"outcome":if error.is::<NeedsEnvironment>() { "needs-environment" } else if oracle_leak { "mismatch" } else { "harness-error" },
        "side":side,"error":error.to_string(),"error_detail":format!("{error:#?}"),
        "difference":if oracle_leak { Some("oracle does not restore represented terminal modes; preserve as explicit divergence, never reproduce the leak") } else { None }})
}

fn observe_case(reference: &Path, candidate: &Path, case: &Case) -> Value {
    let expected = match execute(reference, case) {
        Ok(expected) => expected,
        Err(error) => return execution_error(case, error.as_ref(), "oracle"),
    };
    let actual = match execute(candidate, case) {
        Ok(actual) => actual,
        Err(error) if error.is::<TraceViolation>() || error.is::<interactive::CheckpointTimeout>() || error.is::<resources::Violation>() || error.is::<interactive::KernelViolation>() || error.downcast_ref::<ptytest::PtyTestError>().is_some_and(|error| matches!(error, ptytest::PtyTestError::Timeout { .. } | ptytest::PtyTestError::SnapshotMismatch { .. })) => {
            return json!({"case":case,"outcome":"mismatch","expected":expected.observation,"resources":{"oracle":expected.resources},
                "actual_error":error.to_string(),"actual_error_detail":format!("{error:#?}"),
                "difference":"candidate failed a readiness, lifecycle, trace or resource assertion after the oracle completed"});
        }
        Err(error) => return execution_error(case, error.as_ref(), "candidate"),
    };
    let difference = first_difference(&comparable(&expected.observation), &comparable(&actual.observation), "observation");
    json!({"case":case,"outcome":if difference.is_some() { "mismatch" } else { "pass" },
        "expected":expected.observation,"actual":actual.observation,
        "resources":{"oracle":expected.resources,"candidate":actual.resources},"difference":difference})
}

fn collect_cases(cases: &[&Case], workers: usize, observe: impl Fn(&Case) -> Value + Sync) -> Vec<Value> {
    use std::sync::{atomic::{AtomicUsize, Ordering}, mpsc::sync_channel};
    let workers = workers.min(cases.len()).max(1);
    if workers == 1 { return cases.iter().map(|case| observe(case)).collect(); }
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let (sender, receiver) = sync_channel(workers);
        let mut handles = Vec::new();
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            let observe = &observe;
            handles.push(scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(index) else { break; };
                    if sender.send((index, observe(case))).is_err() { break; }
                }
            }));
        }
        drop(sender);
        let mut results = vec![Value::Null; cases.len()];
        for (index, result) in receiver { results[index] = result; }
        for handle in handles { handle.join().expect("parity worker panicked"); }
        results
    })
}

fn run(args: &[String]) -> Result<()> {
    let value = |key: &str| args.windows(2).find(|pair| pair[0] == key).map(|pair| pair[1].as_str());
    let mode = value("--mode").unwrap_or("admitted");
    if args.iter().any(|s| s == "--inventory-check" || s == "--report") { return inventory::check_and_report(); }
    if args.iter().any(|s| s == "--fuzz") { return fuzz::run(args, &oracle(), Path::new(env!("CARGO_BIN_EXE_lz"))); }
    if !matches!(mode, "admit" | "admitted" | "all") { return Err("unknown mode".into()); }
    let mut i = 0;
    while i < args.len() {
        if !matches!(args[i].as_str(), "--case" | "--group" | "--mode" | "--selftest" | "--baseline") { return Err(format!("unknown argument {}", args[i]).into()); }
        if matches!(args[i].as_str(), "--selftest" | "--baseline") { i += 1; } else {
            if i + 1 == args.len() { return Err("missing argument value".into()); }
            i += 2;
        }
    }
    if args.iter().any(|s| s == "--selftest") { interactive::selftest()?; interactive::checkpoint_selftest()?; fuzz::selftest()?; return resources::selftest(); }
    let reference = oracle();
    let baseline = args.iter().any(|s| s == "--baseline");
    let candidate_path = if baseline { root().join(".work/tools/baseline-target/debug/lz") } else { PathBuf::from(env!("CARGO_BIN_EXE_lz")) };
    let candidate = candidate_path.as_path();
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
    let results = collect_cases(&selected, if baseline { 4 } else { 1 }, |case| observe_case(&reference, candidate, case));
    let mut failed = false;
    let mut harness_error = false;
    let full_corpus = selected.len() == cases.len();
    for (case, result) in selected.iter().zip(&results) {
        println!("{} {}", result["outcome"].as_str().unwrap(), case.id);
        if result["outcome"] != "pass" {
            failed = true;
            eprintln!("{}\nreproduce: make parity-test CASE={}", result.get("difference").filter(|value| !value.is_null()).unwrap_or(&result["error"]), case.id);
        }
        harness_error |= result["outcome"] == "harness-error";
    }
    let report = json!({"metadata": meta, "results": results, "candidate_kind": if baseline { "baseline-2ae5660" } else { "current" }});
    fs::write(result_dir.join("results.json"), serde_json::to_vec_pretty(&report)?)?;
    fs::write(root().join(".work/parity-latest"), result_dir.as_os_str().as_encoded_bytes())?;
    if full_corpus { fs::write(root().join(if baseline { ".work/parity-baseline" } else { ".work/parity-current" }), result_dir.as_os_str().as_encoded_bytes())?; }
    println!("evidence: {}", result_dir.display());
    if failed && (!baseline || harness_error) { return Err("differential suite failed; retained observations and reproduction commands".into()); }
    Ok(())
}

fn main() -> Result<()> {
    let raw_args: Vec<_> = std::env::args_os().skip(1).collect();
    if raw_args.first().is_some_and(|arg| arg == "--pipe-supervisor") {
        if raw_args.len() < 6 { return Err("missing supervisor arguments".into()); }
        let limit = raw_args[4].to_str().ok_or("non-UTF-8 RSS limit")?.parse::<u64>()?.try_into()?;
        let spill = raw_args[5].to_str().ok_or("non-UTF-8 spill limit")?.parse::<u64>()?.try_into()?;
        return supervisor::run(Path::new(&raw_args[1]), Some(Path::new(&raw_args[2])), Path::new(&raw_args[3]), &raw_args[6..], limit, spill);
    }
    if raw_args.first().is_some_and(|arg| arg == "--file-supervisor") {
        if raw_args.len() < 5 { return Err("missing file supervisor arguments".into()); }
        let limit = raw_args[3].to_str().ok_or("non-UTF-8 RSS limit")?.parse::<u64>()?.try_into()?;
        let spill = raw_args[4].to_str().ok_or("non-UTF-8 spill limit")?.parse::<u64>()?.try_into()?;
        return supervisor::run(Path::new(&raw_args[1]), None, Path::new(&raw_args[2]), &raw_args[5..], limit, spill);
    }
    if raw_args.first().is_some_and(|arg| arg == "--pipe-producer") { return supervisor::producer(Path::new(raw_args.get(1).ok_or("missing producer fixture")?)); }
    if raw_args.first().is_some_and(|arg| arg == "--pipe-probe") { return supervisor::probe(&raw_args[1..]); }
    let args: Vec<_> = raw_args.into_iter().map(|arg| arg.into_string().map_err(|_| "non-UTF-8 runner option")).collect::<std::result::Result<_, _>>()?;
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
