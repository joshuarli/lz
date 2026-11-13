use super::{Argument, Case, Event, Result, TraceCheck, collect_cases, json, metadata, observe_case, stdio};
use super::fixtures::Fixture;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const GENERATOR_VERSION: u32 = 10;

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
    let mask = random.choose(8);
    let mut flags = String::from("-");
    for (bit, flag) in [(1, 'N'), (2, 'S'), (4, 'R')] {
        if mask & bit != 0 { flags.push(flag); }
    }
    let mut args = Vec::new();
    let mut env = BTreeMap::new();
    if flags.len() > 1 {
        if random.choose(2) == 0 { args.push(Argument::Text(flags)); }
        else { env.insert("LESS".into(), flags); }
    }
    args.push(Argument::from("input"));
    let text_kind = random.choose(10);
    let utf8 = text_kind < 7;
    let lines = 48 + random.choose(80);
    let mut current_size = size;
    let mut height = usize::from(size[1] - 1);
    let mut half = usize::from(size[1]).div_ceil(2);
    let mut last = lines - height + 1;
    let mut fixture = Vec::new();
    for line in 1..=lines {
        fixture.extend_from_slice(format!("{line:06} seed{seed:016X}").as_bytes());
        let malformed = text_kind < 3 && line == lines / 2;
        if utf8 && !malformed { fixture.extend_from_slice(" é界e\u{301}".as_bytes()); }
        if malformed { fixture.push(0xff); }
        fixture.push(b'\n');
    }
    let mut files = BTreeMap::new();
    files.insert("second".into(), fixture.clone().into());
    args.push(Argument::from("second"));
    let mut top = 1usize;
    let mut marks = [None; 2];
    let mut switched = false;
    let mut events = vec![checkpoint(top)];
    for _ in 0..8 {
        let action = if random.choose(10) < 3 { 12 } else { random.choose(12) };
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
            8 if top < last => {
                let target = top + 1 + random.choose(last - top);
                (format!("/{target:06} seed\n").into_bytes(), target)
            }
            9 => {
                let before_resize = if top == 1 { 2 } else { 1 };
                events.push(Event::Send { bytes: format!("{before_resize}g").into_bytes() });
                events.push(checkpoint(before_resize));
                current_size = if current_size == [40, 12] { [80, 24] } else { [40, 12] };
                events.push(Event::Resize { columns: current_size[0], rows: current_size[1] });
                events.push(Event::Checkpoint { ready: ":".into(), row: Some(usize::from(current_size[1] - 1)) });
                height = usize::from(current_size[1] - 1);
                half = usize::from(current_size[1]).div_ceil(2);
                last = lines - height + 1;
                let target = if before_resize == 1 { 2 } else { 1 };
                (format!("{target}g").into_bytes(), target)
            }
            10 if !switched => {
                let before_switch = if top == 2 { 3 } else { 2 };
                events.push(Event::Send { bytes: format!("{before_switch}g").into_bytes() });
                events.push(checkpoint(before_switch));
                marks = [None; 2];
                switched = true;
                (b":n".to_vec(), 1)
            }
            11 => (vec![b'-', 16, b'N'], top),
            12 => {
                let target = if top == 1 { 2 } else { 1 };
                (format!("a{target}g").into_bytes(), target)
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
    if random.choose(10) < 3 { events.push(Event::Signal { number: libc::SIGTERM }); }
    else { events.push(Event::Send { bytes: vec![b'q'] }); }
    Case {
        id: format!("fuzz-navigation-{seed:016x}"), family: "navigation".into(), admitted: false,
        source: "pty".into(), pty_rss_limit: Default::default(), pty_spill_limit: Default::default(), fixture: fixture.into(), args,
        fixture_name: b"input".to_vec(), files, env,
        size, locale: if utf8 { "C.UTF-8" } else { "C" }.into(), events, trace_check: TraceCheck::Captured,
    }
}

fn signature(result: &Value) -> Option<(String, String)> {
    if result["outcome"] != "mismatch" || !result["side"].is_null() { return None; }
    let difference = result["difference"].as_str()?.split(": expected ").next()?.to_owned();
    let kind = result["actual_error_detail"].as_str().unwrap_or("").split(['{', '(']).next().unwrap_or("").trim().to_owned();
    Some((difference, kind))
}

#[derive(Clone, Copy)]
enum ReductionPart { Events, Arguments, FixtureBytes, Environment, AuxiliaryFiles }

impl ReductionPart {
    fn len(self, case: &Case) -> usize {
        match self {
            Self::Events => case.events.len().saturating_sub(1),
            Self::Arguments => case.args.len(),
            Self::FixtureBytes => match &case.fixture { Fixture::Bytes(bytes) => bytes.len(), _ => 0 },
            Self::Environment => case.env.len(),
            Self::AuxiliaryFiles => case.files.len(),
        }
    }

    fn remove(self, case: &mut Case, range: std::ops::Range<usize>) {
        match self {
            Self::Events => { case.events.drain(range); }
            Self::Arguments => { case.args.drain(range); }
            Self::FixtureBytes => if let Fixture::Bytes(bytes) = &mut case.fixture { bytes.drain(range); },
            Self::Environment => {
                let keys: Vec<_> = case.env.keys().skip(range.start).take(range.len()).cloned().collect();
                for key in keys { case.env.remove(&key); }
            }
            Self::AuxiliaryFiles => {
                let keys: Vec<_> = case.files.keys().skip(range.start).take(range.len()).cloned().collect();
                for key in keys { case.files.remove(&key); }
            }
        }
    }
}

fn reduce_part(case: &Case, original: &Value, part: ReductionPart, mut observe: impl FnMut(&Case) -> Value) -> (Case, Value, usize) {
    let Some(wanted) = signature(original) else { return (case.clone(), original.clone(), 0); };
    let mut reduced = case.clone();
    let mut result = original.clone();
    let mut attempts = 0;
    let mut divisions = 2;
    while attempts < 8 && part.len(&reduced) > 0 {
        let length = part.len(&reduced);
        let chunk = length.div_ceil(divisions);
        let mut accepted = false;
        for start in (0..length).step_by(chunk) {
            if attempts == 8 { break; }
            let mut candidate = reduced.clone();
            part.remove(&mut candidate, start..(start + chunk).min(length));
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
    fs::write(directory.join("request.json"), serde_json::to_vec_pretty(&json!({"seed":seed,"generator_version":GENERATOR_VERSION,"requested":count,"oracle_only":oracle_only}))?)?;
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
                    let (events, observation, event_attempts) = reduce_part(case, &reproduced, ReductionPart::Events, |case| observe_case(reference, candidate, case));
                    let (arguments, observation, argument_attempts) = reduce_part(&events, &observation, ReductionPart::Arguments, |case| observe_case(reference, candidate, case));
                    let (fixture, observation, fixture_attempts) = reduce_part(&arguments, &observation, ReductionPart::FixtureBytes, |case| observe_case(reference, candidate, case));
                    let (environment, observation, environment_attempts) = reduce_part(&fixture, &observation, ReductionPart::Environment, |case| observe_case(reference, candidate, case));
                    let (minimized, observation, auxiliary_attempts) = reduce_part(&environment, &observation, ReductionPart::AuxiliaryFiles, |case| observe_case(reference, candidate, case));
                    fs::write(directory.join("minimized-case.json"), serde_json::to_vec_pretty(&minimized)?)?;
                    save(&directory.join("minimized-observation.json.gz"), &observation)?;
                    fs::write(directory.join("reduction.json"), serde_json::to_vec_pretty(&json!({"reproduced":true,"attempts":{"events":event_attempts,"arguments":argument_attempts,"fixture":fixture_attempts,"environment":environment_attempts,"auxiliary_files":auxiliary_attempts},"original_auxiliary_files":case.files.len(),"minimized_auxiliary_files":minimized.files.len(),"original_environment":case.env.len(),"minimized_environment":minimized.env.len(),"original_events":case.events.len(),"minimized_events":minimized.events.len(),"original_arguments":case.args.len(),"minimized_arguments":minimized.args.len(),"original_fixture_bytes":ReductionPart::FixtureBytes.len(case),"minimized_fixture_bytes":ReductionPart::FixtureBytes.len(&minimized)}))?)?;
                } else {
                    save(&directory.join("reproduction-observation.json.gz"), &reproduced)?;
                    fs::write(directory.join("reduction.json"), serde_json::to_vec_pretty(&json!({"reproduced":false,"attempts":{"events":0,"arguments":0,"fixture":0,"environment":0,"auxiliary_files":0},"reason":"failure signature changed or belongs to the oracle; original failure remains strict"}))?)?;
                }
            }
            break;
        }
        std::io::stdout().flush()?;
    }
    fs::write(directory.join("summary.json"), serde_json::to_vec_pretty(&json!({
        "metadata":meta,"seed":seed,"generator_version":GENERATOR_VERSION,"requested":count,"executed":summaries.len(),"oracle_only":oracle_only,"results":summaries,
        "grammar":"logical-line movement, ordinary in-file marks, forward literal searches, resize-and-jump, next-file transitions, quiet line-number toggles and quit or SIGTERM with CLI/LESS display options over ASCII, UTF-8 or isolated malformed bytes; 30% invalid-command actions; eight actions; 40x12 or 80x24",
        "limits":{"case_seconds":10,"trace_bytes":64*1024*1024,"pty_rss_bytes":u64::from(super::resources::RssLimit::default()),"pty_spill_bytes":u64::from(super::spill::Limit::default()),"actions":8,"workers":4,"reduction_attempts_per_dimension":8},
        "open_limits":["stdio/helper/producer RSS enforcement","stdio/helper/producer spill accounting","broader CLI/config/byte-text/file-transition/signal grammar","event boundary weighting","search variants and adversarial/stream/helper grammar"]
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
    let mut generated_toggles = 0;
    for seed in 0..1000 {
        let generated = generate(seed);
        generated_toggles += generated.events.iter().filter(|event| matches!(event, Event::Send { bytes } if bytes == &[b'-', 16, b'N'])).count();
        let Fixture::Bytes(bytes) = &generated.fixture else { return Err("generated fixture is not inline bytes".into()); };
        if bytes[..bytes.len().min(256)].contains(&0xff) { return Err("generated malformed byte enters the binary sample".into()); }
        for line in bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            let mut columns = line.iter().filter(|byte| **byte == 0xff).count() * 4;
            for valid in line.split(|byte| *byte == 0xff) {
                columns += unicode_width::UnicodeWidthStr::width(std::str::from_utf8(valid)?);
            }
            if columns + 8 > 40 { return Err("generated numbered line exceeds the narrow viewport".into()); }
        }
    }
    if generated_toggles == 0 { return Err("generated sessions never toggle line numbers".into()); }
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
    let (reduced, result, attempts) = reduce_part(&case, &original, ReductionPart::Events, |case| observe_case(&good, &bad, case));
    if reduced.events.len() >= case.events.len() || signature(&result) != signature(&original) {
        return Err("fuzz reduction did not preserve and shrink the real attribute mismatch".into());
    }
    let mut input_case = case.clone();
    input_case.args = vec![Argument::from("unused"), Argument::from("--required"), Argument::from("extra")];
    input_case.fixture = b"abXcd\n".to_vec().into();
    input_case.env = [("AAA_UNUSED".into(), "one".into()), ("LZ_REDUCE_REQUIRED".into(), "yes".into()), ("ZZZ_UNUSED".into(), "two".into())].into();
    input_case.files = [("aaa".into(), b"unused".to_vec().into()), ("needed".into(), b"required".to_vec().into()), ("zzz".into(), b"unused".to_vec().into())].into();
    let conditional = control.path().join("conditional");
    fs::write(&conditional, format!("#!/bin/sh\nrequired=0\nfor argument do [ \"$argument\" = --required ] && required=1; done\nIFS= read -r content < input || :\nfault=none\ncase \"$content\" in *X*) [ \"$required\" = 1 ] && [ \"$LZ_REDUCE_REQUIRED\" = yes ] && [ -f needed ] && fault=attribute;; esac\nexec '{quoted}' --fixture \"$fault\"\n"))?;
    fs::set_permissions(&conditional, fs::Permissions::from_mode(0o700))?;
    let observe_inputs = |candidate: &Case| observe_case(&good, &conditional, candidate);
    let input_original = observe_inputs(&input_case);
    if signature(&input_original).is_none() { return Err("conditional input control did not produce an attribute mismatch".into()); }
    let (arguments, result, _) = reduce_part(&input_case, &input_original, ReductionPart::Arguments, observe_inputs);
    let (fixture, result, _) = reduce_part(&arguments, &result, ReductionPart::FixtureBytes, observe_inputs);
    let (environment, result, _) = reduce_part(&fixture, &result, ReductionPart::Environment, observe_inputs);
    let (inputs, result, _) = reduce_part(&environment, &result, ReductionPart::AuxiliaryFiles, observe_inputs);
    if inputs.args.len() >= input_case.args.len() || ReductionPart::FixtureBytes.len(&inputs) >= ReductionPart::FixtureBytes.len(&input_case)
        || inputs.env.len() != 1 || inputs.env.get("LZ_REDUCE_REQUIRED").map(String::as_str) != Some("yes")
        || inputs.files.len() != 1 || !inputs.files.contains_key("needed")
        || signature(&result) != signature(&input_original) || signature(&observe_inputs(&inputs)) != signature(&input_original) {
        return Err("fuzz input reduction lost a required argument, byte, environment variable, file or mismatch category".into());
    }
    let path = control.path().join("observation.json.gz");
    save(&path, &result)?;
    let decoded = Command::new("gzip").arg("-dc").arg(path).output()?;
    if !decoded.status.success() || serde_json::from_slice::<Value>(&decoded.stdout)? != result {
        return Err("compressed observations do not roundtrip".into());
    }
    println!("deterministic generated cases distinguish seeds; attribute mismatch shrinks from {} to {} events in {attempts} attempts; input reduction retains required bytes, arguments, environment and file; compressed observations roundtrip", case.events.len(), reduced.events.len());
    Ok(())
}
