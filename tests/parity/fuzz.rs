use super::{Argument, Case, Event, Result, TraceCheck, collect_cases, json, metadata, observe_case, stdio};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};

struct Random(u64);
impl Random {
    fn choose(&mut self, upper: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((value ^ (value >> 31)) % upper as u64) as usize
    }
}

fn checkpoint(line: usize) -> Event { Event::Checkpoint { ready: format!("{line:06}"), row: Some(0) } }

fn generate(seed: u64) -> Case {
    let mut random = Random(seed);
    let size = if random.choose(2) == 0 { [40, 12] } else { [80, 24] };
    let lines = 48 + random.choose(80);
    let height = usize::from(size[1] - 1);
    let half = usize::from(size[1]).div_ceil(2);
    let last = lines - height + 1;
    let fixture: Vec<_> = (1..=lines).flat_map(|line| format!("{line:06} seed{seed:016X}\n").into_bytes()).collect();
    let mut top = 1usize;
    let mut marks = [None; 2];
    let mut events = vec![checkpoint(top)];
    for _ in 0..8 {
        let action = random.choose(8);
        let count = 1 + random.choose(4);
        let (bytes, target) = match action {
            0 if top + count <= last => {
                let key = [b'j', b'e', 5, 14][random.choose(4)];
                let prefix = if random.choose(2) == 0 { format!("{count}") } else { format!("000{count}") };
                ([prefix.into_bytes(), vec![key]].concat(), top + count)
            }
            1 if top > count => {
                let key = [b'k', b'y', 11, 16, 25][random.choose(5)];
                ([format!("{count}").into_bytes(), vec![key]].concat(), top - count)
            }
            2 if top + height <= last => (vec![[b' ', b'f', 6, 22][random.choose(4)]], top + height),
            3 if top > height => (vec![[b'b', 2][random.choose(2)]], top - height),
            4 if top + half <= last => (vec![[b'd', 4][random.choose(2)]], top + half),
            5 if top > half => (vec![[b'u', 21][random.choose(2)]], top - half),
            7 => {
                let slot = random.choose(2);
                let name = b'a' + slot as u8;
                if let Some(line) = marks[slot].filter(|line| *line != top) { (vec![b'\'', name], line) }
                else { marks[slot] = Some(top); (vec![b'm', name], top) }
            }
            _ => {
                let mut target = 1 + random.choose(last);
                if target == top { target = if top == last { 1 } else { top + 1 }; }
                (format!("{target}g").into_bytes(), target)
            }
        };
        events.push(Event::Send { bytes });
        top = target;
        events.push(checkpoint(top));
    }
    events.push(Event::Send { bytes: vec![b'q'] });
    Case {
        id: format!("fuzz-navigation-{seed:016x}"), family: "navigation".into(), admitted: false,
        source: "pty".into(), pty_rss_limit: Default::default(), fixture: fixture.into(), args: vec![Argument::from("input")],
        fixture_name: b"input".to_vec(), files: BTreeMap::new(), env: BTreeMap::new(),
        size, locale: "C".into(), events, trace_check: TraceCheck::Captured,
    }
}

fn signature(result: &Value) -> Option<(String, String)> {
    if result["outcome"] != "mismatch" || !result["side"].is_null() { return None; }
    let difference = result["difference"].as_str()?.split(": expected ").next()?.to_owned();
    let kind = result["actual_error_detail"].as_str().unwrap_or("").split(['{', '(']).next().unwrap_or("").trim().to_owned();
    Some((difference, kind))
}

fn reduce_events(case: &Case, original: &Value, mut observe: impl FnMut(&Case) -> Value) -> (Case, Value, usize) {
    let Some(wanted) = signature(original) else { return (case.clone(), original.clone(), 0); };
    let mut reduced = case.clone();
    let mut result = original.clone();
    let mut attempts = 0;
    let mut divisions = 2;
    while attempts < 8 && reduced.events.len() > 2 {
        let length = reduced.events.len() - 1;
        let chunk = length.div_ceil(divisions);
        let mut accepted = false;
        for start in (0..length).step_by(chunk) {
            if attempts == 8 { break; }
            let mut candidate = reduced.clone();
            candidate.events.drain(start..(start + chunk).min(length));
            attempts += 1;
            let observation = observe(&candidate);
            if signature(&observation).as_ref() == Some(&wanted) {
                reduced = candidate;
                result = observation;
                accepted = true;
                break;
            }
        }
        if accepted { divisions = 2; }
        else if divisions >= length { break; }
        else { divisions = (divisions * 2).min(length); }
    }
    (reduced, result, attempts)
}

fn save(path: &Path, result: &Value) -> Result<()> {
    let mut compressor = stdio::OwnedChild(Command::new("gzip").arg("-c")
        .stdin(Stdio::piped()).stdout(File::create(path)?).spawn()?);
    let mut input = BufWriter::with_capacity(64 * 1024, compressor.0.stdin.take().ok_or("missing compression input")?);
    serde_json::to_writer(&mut input, result)?;
    input.flush()?;
    drop(input);
    if !compressor.0.wait()?.success() { return Err("observation compression failed".into()); }
    Ok(())
}

pub(super) fn run(args: &[String], reference: &Path, candidate: &Path) -> Result<()> {
    let mut seed = 0u64;
    let mut count = 50usize;
    let mut oracle_only = false;
    let mut seen = std::collections::BTreeSet::new();
    let mut options = args.iter();
    while let Some(option) = options.next() {
        if !seen.insert(option) { return Err("duplicate fuzz option".into()); }
        match option.as_str() {
            "--fuzz" => {}
            "--oracle-only" => oracle_only = true,
            "--seed" => seed = options.next().ok_or("missing seed")?.parse()?,
            "--cases" => count = options.next().ok_or("missing case count")?.parse()?,
            _ => return Err(format!("unknown fuzz option {option}").into()),
        }
    }
    if !(1..=1000).contains(&count) { return Err("fuzz case count must be in 1..=1000".into()); }
    seed.checked_add(count as u64 - 1).ok_or("fuzz seed range overflow")?;
    let candidate = if oracle_only { reference } else { candidate };
    let meta = metadata(candidate)?;
    let directory = stdio::world()?.keep();
    fs::write(directory.join("metadata.json"), serde_json::to_vec_pretty(&meta)?)?;
    fs::write(directory.join("request.json"), serde_json::to_vec_pretty(&json!({"seed":seed,"requested":count,"oracle_only":oracle_only}))?)?;
    let cases: Vec<_> = (0..count).map(|offset| generate(seed + offset as u64)).collect();
    let mut summaries = Vec::new();
    let mut failed = false;
    for batch in cases.chunks(4) {
        let selected: Vec<_> = batch.iter().collect();
        let results = collect_cases(&selected, 4, |case| {
            let result = observe_case(reference, candidate, case);
            let name = format!("{}.json.gz", case.id);
            match save(&directory.join(&name), &result) {
                Ok(()) => json!({"case":case.id,"outcome":result["outcome"],"observation":name}),
                Err(error) => json!({"case":case.id,"outcome":"harness-error","error":error.to_string()}),
            }
        });
        for (case, result) in batch.iter().zip(results) {
            println!("{} {}", result["outcome"].as_str().unwrap(), case.id);
            failed |= result["outcome"] != "pass";
            summaries.push(result);
        }
        if failed {
            if let Some(case) = batch.iter().find(|case| summaries.iter().any(|item| item["case"] == case.id && item["outcome"] == "mismatch")) {
                let name = format!("{}.json.gz", case.id);
                let decoded = Command::new("gzip").arg("-dc").arg(directory.join(name)).output()?;
                if !decoded.status.success() { return Err("cannot read retained fuzz failure".into()); }
                let original: Value = serde_json::from_slice(&decoded.stdout)?;
                let reproduced = observe_case(reference, candidate, case);
                let stable = signature(&original).is_some() && signature(&original) == signature(&reproduced);
                if stable {
                    let (minimized, observation, attempts) = reduce_events(case, &reproduced, |case| observe_case(reference, candidate, case));
                    fs::write(directory.join("minimized-case.json"), serde_json::to_vec_pretty(&minimized)?)?;
                    save(&directory.join("minimized-observation.json.gz"), &observation)?;
                    fs::write(directory.join("reduction.json"), serde_json::to_vec_pretty(&json!({"reproduced":true,"attempts":attempts,"original_events":case.events.len(),"minimized_events":minimized.events.len()}))?)?;
                } else {
                    save(&directory.join("reproduction-observation.json.gz"), &reproduced)?;
                    fs::write(directory.join("reduction.json"), serde_json::to_vec_pretty(&json!({"reproduced":false,"attempts":0,"reason":"failure signature changed or belongs to the oracle; original failure remains strict"}))?)?;
                }
            }
            break;
        }
        std::io::stdout().flush()?;
    }
    fs::write(directory.join("summary.json"), serde_json::to_vec_pretty(&json!({
        "metadata":meta,"seed":seed,"requested":count,"executed":summaries.len(),"oracle_only":oracle_only,"results":summaries,
        "grammar":"ASCII logical-line movement and ordinary in-file marks; eight actions; 40x12 or 80x24",
        "limits":{"case_seconds":10,"trace_bytes":64*1024*1024,"pty_rss_bytes":u64::from(super::resources::RssLimit::default()),"actions":8,"workers":4,"reduction_attempts":8},
        "open_limits":["stdio/helper/producer RSS enforcement","owned-spill accounting","adversarial/Unicode/stream/resize/helper grammar"]
    }))?)?;
    fs::write(stdio::root().join(".work/parity-fuzz-latest"), directory.as_os_str().as_encoded_bytes())?;
    fs::write(stdio::root().join(if oracle_only { ".work/parity-fuzz-oracle" } else { ".work/parity-fuzz-current" }), directory.as_os_str().as_encoded_bytes())?;
    println!("evidence: {}", directory.display());
    if failed { return Err(format!("generated differential failed; reproduce: make parity-fuzz SEED={seed} CASES={count}").into()); }
    Ok(())
}

pub(super) fn selftest() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let control = stdio::world()?;
    let executable = std::env::current_exe()?;
    let quoted = executable.to_str().ok_or("non-UTF-8 fixture executable")?.replace('\'', "'\\''");
    for (name, fault) in [("good", "none"), ("bad", "attribute")] {
        let path = control.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nexec '{quoted}' --fixture {fault}\n"))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let mut case = generate(0);
    let again = generate(0);
    if serde_json::to_vec(&case)? != serde_json::to_vec(&again)? || serde_json::to_value(&case.fixture)? == serde_json::to_value(&generate(1).fixture)? {
        return Err("generated cases are not deterministic and seed-distinguishing".into());
    }
    case.fixture = Vec::new().into();
    case.events = vec![
        Event::Checkpoint { ready:"probe".into(), row:Some(0) },
        Event::Checkpoint { ready:"probe".into(), row:Some(0) },
        Event::Checkpoint { ready:"probe".into(), row:Some(0) },
        Event::Send { bytes:vec![b'\n'] },
    ];
    let good = control.path().join("good");
    let bad = control.path().join("bad");
    for (arguments, expected) in [
        (vec!["--fuzz", "--cases", "0"], "case count"),
        (vec!["--fuzz", "--cases", "1001"], "case count"),
        (vec!["--fuzz", "--seed", "18446744073709551615", "--cases", "2"], "seed range overflow"),
    ] {
        let error = run(&arguments.iter().map(|argument| (*argument).to_owned()).collect::<Vec<_>>(), &good, &bad).expect_err("invalid fuzz budget accepted");
        if !error.to_string().contains(expected) { return Err("fuzz budget was rejected at the wrong boundary".into()); }
    }
    let original = observe_case(&good, &bad, &case);
    if signature(&original).is_none() { return Err("fuzz reduction control did not detect the attribute fault".into()); }
    let (reduced, result, attempts) = reduce_events(&case, &original, |case| observe_case(&good, &bad, case));
    if reduced.events.len() >= case.events.len() || signature(&result) != signature(&original) {
        return Err("fuzz reduction did not preserve and shrink the real attribute mismatch".into());
    }
    let path = control.path().join("observation.json.gz");
    save(&path, &result)?;
    let decoded = Command::new("gzip").arg("-dc").arg(path).output()?;
    if !decoded.status.success() || serde_json::from_slice::<Value>(&decoded.stdout)? != result {
        return Err("compressed observations do not roundtrip".into());
    }
    println!("deterministic generated cases distinguish seeds; attribute mismatch shrinks from {} to {} events in {attempts} attempts and compressed observations roundtrip", case.events.len(), reduced.events.len());
    Ok(())
}
