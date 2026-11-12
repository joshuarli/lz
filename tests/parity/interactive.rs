use super::{Argument, Case, Event, Result, comparable, first_difference, json};
use ptytest::{CommandSpec, Deadline, ExitStatus, PtyTest, Scenario, ScreenSnapshot, Size, TestEnv};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::os::unix::ffi::OsStringExt;
use std::time::{Duration, Instant};

fn screen(snapshot: &ScreenSnapshot) -> Value {
    let rows: Vec<_> = (0..snapshot.row_count()).map(|row| {
        let cells: Vec<_> = (0..snapshot.size().columns() as usize).map(|column| {
            let cell = snapshot.cell(row, column).unwrap();
            json!({"text": if cell.contents().is_empty() { " " } else { cell.contents() },
                "wide": cell.is_wide(), "continuation": cell.is_wide_continuation(),
                "attributes": format!("{:?}", cell.attributes())})
        }).collect();
        json!({"cells": cells, "wrapped": snapshot.row_wrapped(row)})
    }).collect();
    json!({"size": [snapshot.size().columns(), snapshot.size().rows()],
        "cursor": format!("{:?}", snapshot.cursor()), "modes": format!("{:?}", snapshot.modes()), "rows": rows})
}

fn files(root: &Path) -> Result<Value> {
    fn walk(base: &Path, dir: &Path, result: &mut BTreeMap<String, Value>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            let name = path.strip_prefix(base)?.to_str().ok_or("non-UTF-8 effect path not represented yet")?.to_owned();
            if kind.is_symlink() {
                result.insert(name, json!({"symlink": fs::read_link(path)?.as_os_str().as_encoded_bytes()}));
            } else if kind.is_dir() { walk(base, &path, result)?; }
            else if kind.is_file() {
                if entry.metadata()?.len() > 64 * 1024 * 1024 { return Err("side-effect capture limit".into()); }
                result.insert(name, json!({"bytes": fs::read(path)?}));
            } else { return Err("unrepresented side-effect file type".into()); }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result)?;
    Ok(json!(result))
}

pub fn execute(binary: &Path, case: &Case) -> Result<Value> {
    let environment = if case.locale == "C" { TestEnv::hermetic_ascii()? } else { TestEnv::hermetic_utf8(&case.locale)? };
    let mut environment = environment.env("LESSHISTFILE", "-");
    for (key, value) in &case.env { environment = environment.env(key, value); }
    let world = environment.paths().root().to_owned();
    super::write_fixture(&world.join(std::ffi::OsString::from_vec(case.fixture_name.clone())), &case.fixture)?;
    for (name, bytes) in &case.files { fs::write(world.join(name), bytes)?; }
    let scenario = Scenario::new(&case.id)?.command(CommandSpec::new(binary).args(case.args.iter().map(Argument::os_string)).current_dir(&world))
        .size(Size::new(case.size[0], case.size[1])?).environment(environment).trace_limit(64 * 1024 * 1024);
    let mut terminal = PtyTest::spawn(scenario)?;
    let baseline = terminal.terminal_baseline();
    let total = Instant::now() + Duration::from_secs(10);
    let checkpoint_deadline = || Deadline::from_instant(total.min(Instant::now() + Duration::from_secs(3)));
    let mut checkpoints = Vec::new();
    for event in &case.events {
        match event {
            Event::Send { bytes } => terminal.send_bytes(checkpoint_deadline(), bytes)?,
            Event::Checkpoint { ready, row } => {
                let deadline = checkpoint_deadline();
                terminal.wait_for_screen(deadline, ready, |screen| row.map_or_else(|| screen.contains(ready), |row| screen.row(row).is_some_and(|text| text.contains(ready))))?;
                if !terminal.wait_for_quiescence(deadline, Duration::from_millis(20))? { return Err("harness-error: no output quiescence".into()); }
                checkpoints.push(json!({"screen": screen(&terminal.screen()), "files": files(&world)?, "bells": terminal.raw_output().iter().filter(|b| **b == 7).count()}));
            }
            Event::Resize { columns, rows } => terminal.resize(Size::new(*columns, *rows)?)?,
            Event::Signal { number } => terminal.signal(*number)?,
            Event::Append { bytes } => {
                OpenOptions::new().append(true).open(world.join(std::ffi::OsString::from_vec(case.fixture_name.clone())))?.write_all(bytes)?;
            }
        }
    }
    let status = terminal.wait_for_exit(checkpoint_deadline())?;
    terminal.assert_terminal_restored(&baseline)?;
    let effects = files(&world)?;
    let final_screen = screen(&terminal.screen());
    let input = terminal.raw_input().to_vec();
    let output = terminal.raw_output().to_vec();
    let finished = terminal.finish(Deadline::from_instant(total.min(Instant::now() + Duration::from_secs(1))))?;
    if status != finished { return Err("harness-error: wait and finish exit status differ".into()); }
    Ok(json!({"checkpoints": checkpoints, "exit": format!("{status:?}"), "final_screen": final_screen,
        "files": effects, "raw_input": input, "raw_output": output, "restored": true}))
}

pub fn fixture(fault: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "\x1b[2J\x1b[H")?;
    if fault == "attribute" { write!(stdout, "\x1b[31m")?; }
    write!(stdout, "{}", if fault == "output" { "proBe" } else { "probe" })?;
    if fault == "cursor" { write!(stdout, "\x1b[2;2H")?; }
    if fault != "effect" { fs::write("effect", b"expected")?; }
    stdout.flush()?;
    if fault == "timeout" { loop { std::thread::park(); } }
    let mut byte = [0];
    while std::io::stdin().read(&mut byte)? != 0 { if byte[0] == b'\n' { break; } }
    if fault == "exit" { std::process::exit(7); }
    Ok(())
}

pub fn selftest() -> Result<()> {
    let candidate = std::env::current_exe()?;
    let mut case = Case { id: "selftest".into(), family: "harness".into(), admitted: false, source: "pty".into(),
        fixture: vec![], fixture_name: b"input".to_vec(), files: BTreeMap::new(), args: vec!["--fixture".into(), "none".into()], env: BTreeMap::new(), size: [40, 8], locale: "C".into(),
        events: vec![Event::Checkpoint { ready: "pro".into(), row: None }, Event::Send { bytes: b"q\n".to_vec() }] };
    let mut invalid = serde_json::to_value(&case)?;
    invalid.as_object_mut().unwrap().insert("typo".into(), json!(true));
    if serde_json::from_value::<Case>(invalid).is_ok() { return Err("unknown scenario field accepted".into()); }
    if serde_json::from_value::<Event>(json!({"type": "typo"})).is_ok() { return Err("unknown event accepted".into()); }
    let expected_diagnostic = super::stdio::redirected(&candidate, b"", "file", &["--stdio-fixture".into(), "none".into()], b"input", &BTreeMap::new())?;
    let dropped = super::stdio::redirected(&candidate, b"", "file", &["--stdio-fixture".into(), "drop-error".into()], b"input", &BTreeMap::new())?;
    let difference = first_difference(&expected_diagnostic, &dropped, "observation").ok_or("dropped error missed")?;
    if !difference.contains("stderr") { return Err("dropped error detected at wrong observation".into()); }
    println!("detected dropped stderr diagnostic; unknown fields/events rejected");
    let expected = execute(&candidate, &case)?;
    if comparable(&expected) != comparable(&execute(&candidate, &case)?) { return Err("independent fixture self-comparison failed".into()); }
    for (fault, observation) in [("output", "text"), ("attribute", "attributes"), ("cursor", "cursor"), ("exit", "exit"), ("effect", "files")] {
        case.args[1] = fault.into();
        let actual = execute(&candidate, &case)?;
        let difference = first_difference(&comparable(&expected), &comparable(&actual), "observation").ok_or("mutation was missed")?;
        if !difference.contains(observation) { return Err(format!("{fault} detected at wrong observation: {difference}").into()); }
        println!("detected {fault} at {difference}");
    }
    case.args[1] = "timeout".into();
    if !execute(&candidate, &case).is_err_and(|error| error.downcast_ref::<ptytest::PtyTestError>().is_some_and(|error| matches!(error, ptytest::PtyTestError::Timeout { .. }))) { return Err("timeout mutation was missed or failed for another reason".into()); }
    println!("detected bounded timeout; PTY owner cleaned up child");
    case.id = "oracle-selftest".into();
    case.fixture = (1..81).map(|n| format!("{n:06} reference parity\n")).collect::<String>().into_bytes();
    case.args = vec!["input".into()];
    case.events = vec![Event::Checkpoint { ready: "000001".into(), row: None }, Event::Send { bytes: b"q".to_vec() }];
    let reference = super::oracle();
    let a = execute(&reference, &case)?;
    let b = execute(&reference, &case)?;
    if let Some(diff) = first_difference(&comparable(&a), &comparable(&b), "observation") { return Err(format!("oracle self-comparison: {diff}").into()); }
    if a["exit"] != format!("{:?}", ExitStatus::Code(0)) { return Err("oracle quit exit differs".into()); }
    let mut hangup = case.clone();
    hangup.id = "candidate-hangup-safety".into();
    hangup.fixture = b"x".to_vec();
    hangup.events = vec![Event::Checkpoint { ready: "x".into(), row: Some(0) }, Event::Signal { number: 1 }];
    let hangup = execute(Path::new(env!("CARGO_BIN_EXE_lz")), &hangup)?;
    if hangup["exit"] != format!("{:?}", ExitStatus::Signal(1)) || hangup["restored"] != true {
        return Err("candidate did not restore its terminal and preserve HUP signal death".into());
    }
    println!("candidate HUP safety: represented modes restored, signal death preserved; oracle leak remains a parked divergence");
    let report = json!({"candidate_hangup_safety": hangup, "runner_executable": super::digest(&[candidate])?, "metadata": super::metadata(Path::new(env!("CARGO_BIN_EXE_lz")))?, "faults_detected": ["output", "attribute", "cursor", "exit", "effect", "timeout", "stderr"], "oracle_a": a, "oracle_b": b});
    let path = super::stdio::world()?.keep().join("selftest.json");
    fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    println!("oracle independent-world startup/quit/lifecycle self-comparison passes; {}", path.display());
    Ok(())
}
