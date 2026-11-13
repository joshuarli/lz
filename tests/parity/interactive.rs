use super::{Argument, Case, Event, Result, comparable, first_difference, json};
use ptytest::{CommandSpec, Deadline, ExitStatus, PtyTest, Scenario, ScreenSnapshot, Size, TestEnv};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct KernelViolation { state: Value, input: Vec<u8>, output: Vec<u8> }
impl std::fmt::Display for KernelViolation {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "kernel terminal state was not restored before supervisor repair; {} input bytes, {} output bytes; state={}", self.input.len(), self.output.len(), self.state)
    }
}
impl std::error::Error for KernelViolation {}

#[derive(Debug)]
pub(super) enum CheckpointTimeout { Readiness { artifact_dir: std::path::PathBuf }, Quiescence { artifact_dir: std::path::PathBuf }, JobControl { phase: &'static str, artifact_dir: std::path::PathBuf } }
impl std::fmt::Display for CheckpointTimeout {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (operation, directory) = match self {
            Self::Readiness { artifact_dir } => ("checkpoint readiness", artifact_dir),
            Self::Quiescence { artifact_dir } => ("checkpoint output quiescence", artifact_dir),
            Self::JobControl { phase, artifact_dir } => (*phase, artifact_dir),
        };
        write!(output, "{operation} timed out; retained evidence: {}", directory.display())
    }
}
impl std::error::Error for CheckpointTimeout {}

#[derive(Debug)]
pub(super) struct ClosedInput {
    exit: ExitStatus,
    cause: ptytest::PtyTestError,
    artifact_dir: std::path::PathBuf,
}
impl std::fmt::Display for ClosedInput {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "PTY input closed and child exited {:?}: {}; retained evidence: {}", self.exit, self.cause, self.artifact_dir.display())
    }
}
impl std::error::Error for ClosedInput {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(&self.cause) }
}

#[derive(Clone, Copy)]
enum BellScan { Ground, Escape, Csi, Osc, OscEscape, String, StringEscape }

fn audible_bells(bytes: &[u8]) -> usize {
    let mut state = BellScan::Ground;
    let mut bells = 0;
    for &byte in bytes {
        state = match state {
            BellScan::Ground => match byte { 7 => { bells += 1; BellScan::Ground }, 27 => BellScan::Escape, _ => BellScan::Ground },
            BellScan::Escape => match byte { b']' => BellScan::Osc, b'[' => BellScan::Csi, b'P' | b'^' | b'_' => BellScan::String,
                7 => { bells += 1; BellScan::Escape }, 27 => BellScan::Escape, 0x20..=0x2f => BellScan::Escape, _ => BellScan::Ground },
            BellScan::Csi => match byte { 7 => { bells += 1; BellScan::Csi }, 27 => BellScan::Escape, 0x40..=0x7e => BellScan::Ground, _ => BellScan::Csi },
            BellScan::Osc => match byte { 7 => BellScan::Ground, 27 => BellScan::OscEscape, _ => BellScan::Osc },
            BellScan::OscEscape => match byte { b'\\' | 7 => BellScan::Ground, 27 => BellScan::OscEscape, _ => BellScan::Osc },
            BellScan::String => if byte == 27 { BellScan::StringEscape } else { BellScan::String },
            BellScan::StringEscape => match byte { b'\\' => BellScan::Ground, 27 => BellScan::StringEscape, _ => BellScan::String },
        };
    }
    bells
}

fn retain_checkpoint(terminal: &PtyTest, id: &str) -> Result<std::path::PathBuf> {
    let directory = super::stdio::world()?.keep();
    fs::write(directory.join("input.bin"), terminal.raw_input())?;
    fs::write(directory.join("output.bin"), terminal.raw_output())?;
    fs::write(directory.join("screen.json"), serde_json::to_vec_pretty(&screen(&terminal.screen()))?)?;
    fs::write(directory.join("context.json"), serde_json::to_vec_pretty(&json!({"case":id,"input_bytes":terminal.raw_input().len(),"output_bytes":terminal.raw_output().len()}))?)?;
    Ok(directory)
}

fn bell_window_second() -> Result<std::time::Duration> {
    Ok(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?)
}

fn wait_bell_window(deadline: Instant) -> Result<u64> {
    loop {
        let now = bell_window_second()?;
        if now.subsec_millis() < 100 { return Ok(now.as_secs()); }
        if Instant::now() >= deadline { return Err("bell window could not start before deadline".into()); }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn finish_bell_window(start: Option<u64>, now: u64) -> Result<()> {
    match start {
        Some(start) if start == now => Ok(()),
        Some(_) => Err("bell window crossed a clock second".into()),
        None => Err("bell window ended without a start".into()),
    }
}

fn wait_checkpoint(terminal: &mut PtyTest, id: &str, deadline: Instant, ready: &str, row: Option<usize>) -> Result<()> {
    const QUIET: Duration = Duration::from_millis(20);
    loop {
        let screen = terminal.screen();
        if row.map_or_else(|| screen.contains(ready), |row| screen.row(row).is_some_and(|text| text.contains(ready))) { break; }
        if Instant::now() >= deadline {
            return Err(Box::new(CheckpointTimeout::Readiness { artifact_dir:retain_checkpoint(terminal, id)? }));
        }
        terminal.wait_for_output(Deadline::from_instant(deadline.min(Instant::now() + QUIET)))?;
    }
    let settle = Deadline::from_instant(deadline.min(Instant::now() + Duration::from_millis(250)));
    if !terminal.wait_for_quiescence(settle, QUIET)? {
        return Err(Box::new(CheckpointTimeout::Quiescence { artifact_dir:retain_checkpoint(terminal, id)? }));
    }
    Ok(())
}

fn wait_job(terminal: &mut PtyTest, id: &str, manifest: &Path, count: usize, resumed: bool, deadline: Instant) -> Result<Value> {
    loop {
        match fs::read(manifest.with_extension("job")) {
            Ok(bytes) => {
                let state: Value = serde_json::from_slice(&bytes)?;
                if state["stops"] == count && state["continues"] == count - usize::from(!resumed) { return Ok(state); }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err(Box::new(CheckpointTimeout::JobControl { phase:if resumed { "kernel continue acknowledgment" } else { "kernel stop acknowledgment" }, artifact_dir:retain_checkpoint(terminal, id)? }));
        }
        terminal.wait_for_output(Deadline::from_instant(deadline.min(Instant::now() + Duration::from_millis(20))))?;
    }
}

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
                let metadata = entry.metadata()?;
                let length = metadata.len();
                let mode = metadata.permissions().mode() & 0o7777;
                if length > 64 * 1024 {
                    let output = super::sha256_command().stdin(fs::File::open(path)?).output()?;
                    if !output.status.success() { return Err("file-effect hash failed".into()); }
                    let digest = std::str::from_utf8(&output.stdout)?.split_whitespace().next().ok_or("missing file-effect hash")?;
                    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("invalid file-effect hash".into()); }
                    result.insert(name, json!({"sha256": digest, "length": length, "mode":mode}));
                } else { result.insert(name, json!({"bytes": fs::read(path)?, "mode":mode})); }
            } else { return Err("unrepresented side-effect file type".into()); }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result)?;
    Ok(json!(result))
}

pub fn execute(binary: &Path, case: &Case) -> Result<Value> {
    execute_recorded(binary, case).map(|(observation, _)| observation)
}

pub(super) fn execute_recorded(binary: &Path, case: &Case) -> Result<(Value, super::resources::Record)> {
    let supervisor_world = super::stdio::world()?;
    execute_recorded_with_manifest(binary, case, &supervisor_world.path().join("groups"))
}

fn execute_with_manifest(binary: &Path, case: &Case, manifest: &Path) -> Result<Value> {
    execute_recorded_with_manifest(binary, case, manifest).map(|(observation, _)| observation)
}

fn execute_recorded_with_manifest(binary: &Path, case: &Case, manifest: &Path) -> Result<(Value, super::resources::Record)> {
    let result = execute_supervised(binary, case, manifest);
    let record = super::resources::check(manifest, result.is_ok())?;
    let observation = result?;
    Ok((observation, record.ok_or("missing successful RSS accounting")?))
}

fn execute_supervised(binary: &Path, case: &Case, manifest: &Path) -> Result<Value> {
    let environment = if case.locale == "C" { TestEnv::hermetic_ascii()? } else { TestEnv::hermetic_utf8(&case.locale)? };
    let mut environment = environment.env("LESSHISTFILE", "-").env("LESSECHO", super::oracle().with_file_name("lessecho"));
    for (key, value) in &case.env { environment = environment.env(key, value); }
    let world = environment.paths().root().to_owned();
    super::write_fixture(&world.join(std::ffi::OsString::from_vec(case.fixture_name.clone())), &case.fixture)?;
    for (name, fixture) in &case.files {
        let path = world.join(name);
        if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
        super::write_auxiliary_fixture(&path, fixture)?;
    }
    let command = if case.source == "pty-pipe" {
        CommandSpec::new(std::env::current_exe()?).arg("--pipe-supervisor").arg(binary)
            .arg(std::ffi::OsString::from_vec(case.fixture_name.clone())).arg(manifest).arg(u64::from(case.pty_rss_limit).to_string()).arg(u64::from(case.pty_spill_limit).to_string())
            .args(case.args.iter().map(Argument::os_string)).current_dir(&world)
    } else {
        CommandSpec::new(std::env::current_exe()?).arg("--file-supervisor").arg(binary).arg(manifest)
            .arg(u64::from(case.pty_rss_limit).to_string()).arg(u64::from(case.pty_spill_limit).to_string()).args(case.args.iter().map(Argument::os_string)).current_dir(&world)
    };
    let scenario = Scenario::new(&case.id)?.command(command)
        .size(Size::new(case.size[0], case.size[1])?).environment(environment).trace_limit(64 * 1024 * 1024);
    let mut terminal = PtyTest::spawn(scenario)?;
    let mut cleanup = super::supervisor::Cleanup::new(manifest.to_owned());
    let baseline = terminal.terminal_baseline();
    let total = Instant::now() + Duration::from_secs(10);
    let checkpoint_deadline = || Deadline::from_instant(total.min(Instant::now() + Duration::from_secs(3)));
    let mut checkpoints = Vec::new();
    let mut suspensions = 0;
    let mut bell_window = None;
    for event in &case.events {
        match event {
            Event::Send { bytes } => {
                if let Err(error) = terminal.send_bytes(checkpoint_deadline(), bytes) {
                    if matches!(&error, ptytest::PtyTestError::System { errno, .. } if *errno == libc::EIO) {
                        let exit = terminal.wait_for_exit(checkpoint_deadline())?;
                        let artifact_dir = retain_checkpoint(&terminal, &case.id)?;
                        return Err(Box::new(ClosedInput { exit, cause: error, artifact_dir }));
                    }
                    return Err(error.into());
                }
            }
            Event::Checkpoint { ready, row } => {
                let deadline = total.min(Instant::now() + Duration::from_secs(3));
                wait_checkpoint(&mut terminal, &case.id, deadline, ready, *row)?;
                checkpoints.push(json!({"screen": screen(&terminal.screen()), "files": files(&world)?, "bells": audible_bells(terminal.raw_output())}));
            }
            Event::BellWindowStart {} => {
                if bell_window.is_some() { return Err("bell window already started".into()); }
                bell_window = Some(wait_bell_window(total)?);
            }
            Event::BellWindowEnd {} => {
                finish_bell_window(bell_window.take(), bell_window_second()?.as_secs())?;
            }
            Event::Resize { columns, rows } => terminal.resize(Size::new(*columns, *rows)?)?,
            Event::Signal { number } => terminal.signal(*number)?,
            Event::Continue {} => terminal.signal(libc::SIGCONT)?,
            Event::SuspendSignal {} => terminal.signal(libc::SIGTSTP)?,
            Event::Suspend {} | Event::WaitSuspended {} => {
                suspensions += 1;
                if matches!(event, Event::Suspend {}) { terminal.signal(libc::SIGTSTP)?; }
                let state = wait_job(&mut terminal, &case.id, manifest, suspensions, false, total.min(Instant::now() + Duration::from_secs(3)))?;
                if !terminal.wait_for_quiescence(checkpoint_deadline(), Duration::from_millis(20))? {
                    return Err(Box::new(CheckpointTimeout::Quiescence { artifact_dir:retain_checkpoint(&terminal, &case.id)? }));
                }
                checkpoints.push(json!({"suspended":state,"screen":screen(&terminal.screen())}));
                terminal.signal(libc::SIGCONT)?;
                wait_job(&mut terminal, &case.id, manifest, suspensions, true, total.min(Instant::now() + Duration::from_secs(3)))?;
            }
            Event::Append { bytes } => {
                OpenOptions::new().append(true).open(world.join(std::ffi::OsString::from_vec(case.fixture_name.clone())))?.write_all(bytes)?;
            }
            Event::Rewrite { bytes } => {
                OpenOptions::new().write(true).truncate(true).open(world.join(std::ffi::OsString::from_vec(case.fixture_name.clone())))?.write_all(bytes)?;
            }
        }
    }
    if bell_window.is_some() { return Err("bell window was not ended".into()); }
    let status = terminal.wait_for_exit(checkpoint_deadline())?;
    let state_path = if case.source == "pty-pipe" { world.join("supervisor-state") } else { manifest.with_extension("state") };
    let state: Value = serde_json::from_slice(&fs::read(state_path)?)?;
    if !state["kernel_before"].is_object() || !state["kernel_after"].is_object() { return Err("harness-error: missing kernel terminal observations".into()); }
    if state["kernel_restored"] != true {
        return Err(Box::new(KernelViolation { state, input:terminal.raw_input().to_vec(), output:terminal.raw_output().to_vec() }));
    }
    let kernel = json!({"before":state["kernel_before"],"after":state["kernel_after"],"restored":state["kernel_restored"]});
    terminal.assert_terminal_restored(&baseline)?;
    let effects = files(&world)?;
    let final_screen = screen(&terminal.screen());
    let input = terminal.raw_input().to_vec();
    let output = terminal.raw_output().to_vec();
    if matches!(case.trace_check, super::TraceCheck::PlainText) {
        let text = std::str::from_utf8(&output).map_err(|_| super::TraceViolation("plain-text trace is not valid UTF-8"))?;
        if text.chars().any(|character| character.is_control() && !matches!(character, '\r' | '\n')) {
            return Err(super::TraceViolation("plain-text trace contains a terminal control").into());
        }
    }
    super::supervisor::assert_cleanup(manifest, if case.source == "pty-pipe" { 2 } else { 1 })?;
    super::supervisor::assert_session_empty(manifest)?;
    cleanup.disarm();
    let finished = terminal.finish(Deadline::from_instant(total.min(Instant::now() + Duration::from_secs(1))))?;
    if status != finished { return Err("harness-error: wait and finish exit status differ".into()); }
    let final_bells = audible_bells(&output);
    Ok(json!({"checkpoints": checkpoints, "exit": format!("{status:?}"), "final_screen": final_screen,
        "final_bells": final_bells, "files": effects, "raw_input": input, "raw_output": output, "restored": true, "kernel_terminal":kernel}))
}

pub fn fixture(fault: &str) -> Result<()> {
    if matches!(fault, "kernel-restored" | "kernel-leak") {
        let mut session = lz::terminal::Session::start(lz::terminal::Screen::Primary, lz::terminal::SuspendPolicy::Allow)?;
        print!("probe");
        std::io::stdout().flush()?;
        session.keys_mut().read_key()?;
        if fault == "kernel-leak" { std::process::exit(0); }
        session.finish()?;
        return Ok(());
    }

    let mut stdout = std::io::stdout().lock();
    write!(stdout, "\x1b[2J\x1b[H")?;
    if fault == "attribute" { write!(stdout, "\x1b[31m")?; }
    write!(stdout, "{}", if fault == "output" { "proBe" } else { "probe" })?;
    if fault == "cursor" { write!(stdout, "\x1b[2;2H")?; }
    if fault != "effect" { fs::write("effect", b"expected")?; }
    if fault == "file-mode" {
        let mode = fs::metadata("effect")?.permissions().mode() ^ 0o200;
        fs::set_permissions("effect", fs::Permissions::from_mode(mode))?;
    }
    stdout.flush()?;
    if fault == "timeout" { loop { std::thread::park(); } }
    if fault == "flood" { loop { stdout.write_all(b"\rprobe")?; stdout.flush()?; } }
    let mut byte = [0];
    while std::io::stdin().read(&mut byte)? != 0 { if byte[0] == b'\n' { break; } }
    if fault == "exit" { std::process::exit(7); }
    if fault == "bell-after" { stdout.write_all(b"\x07")?; stdout.flush()?; }
    if fault == "osc-after" { stdout.write_all(b"\x1b]8;;https://example.test\x07\x1b]8;;\x07")?; stdout.flush()?; }
    Ok(())
}

pub fn selftest() -> Result<()> {
    super::fixtures::selftest()?;
    {
        use std::io::{Seek, SeekFrom};
        let world = super::stdio::world()?;
        let path = world.path().join("large-effect");
        let fixture = super::fixtures::Fixture::Repeated(super::fixtures::Repeat { repeat: vec![0, 255, 13, 10, 42], count: 20_003, suffix: b"tail".to_vec() });
        super::write_fixture(&path, &fixture)?;
        let before = files(world.path())?;
        if before["large-effect"].get("bytes").is_some() || before["large-effect"]["sha256"].as_str().is_none_or(|hash| hash.len() != 64) {
            return Err("large file effect was not captured with a bounded digest".into());
        }
        let mut file = OpenOptions::new().write(true).open(path)?;
        file.seek(SeekFrom::End(-1))?;
        file.write_all(&[0])?;
        if before == files(world.path())? { return Err("same-length large file mutation was missed".into()); }
        println!("bounded file-effect capture detects a same-length suffix mutation");
    }
    let candidate = std::env::current_exe()?;
    let mut case = Case { id: "selftest".into(), family: "harness".into(), admitted: false, source: "pty".into(), pty_rss_limit: Default::default(), pty_spill_limit: Default::default(),
        fixture: vec![].into(), fixture_name: b"input".to_vec(), files: BTreeMap::new(), args: vec!["--fixture".into(), "none".into()], env: BTreeMap::new(), size: [40, 8], locale: "C".into(),
        events: vec![Event::Checkpoint { ready: "pro".into(), row: None }, Event::Send { bytes: b"q\n".to_vec() }], trace_check:super::TraceCheck::Captured };
    let mut blocked = case.clone();
    blocked.id = "parallel-timeout".into();
    blocked.args[1] = "timeout".into();
    let mut controls = vec![blocked];
    for index in 0..super::BASELINE_WORKERS {
        let mut ready = case.clone();
        ready.id = format!("parallel-ready-{index}");
        controls.push(ready);
    }
    let references: Vec<_> = controls.iter().collect();
    let parallel = super::collect_cases(&references, super::BASELINE_WORKERS, |case| super::observe_case(&candidate, &candidate, case));
    for (index, control) in controls.iter().enumerate() {
        let outcome = if index == 0 { "harness-error" } else { "pass" };
        if parallel[index]["case"]["id"] != control.id || parallel[index]["outcome"] != outcome {
            return Err("parallel collection lost, reordered or reclassified a case".into());
        }
    }
    if !parallel[0]["error"].as_str().is_some_and(|error| error.contains("timed out")) { return Err("parallel control failed outside its timeout observation".into()); }
    println!("bounded workers preserve corpus order, independent fixture worlds and timeout classification while draining cases beyond the worker count");
    let mut pipe_case = case.clone();
    pipe_case.source = "pty-pipe".into();
    pipe_case.fixture = b"owned data\n".to_vec().into();
    pipe_case.env.insert("SUPERVISOR_PROBE".into(), "inherited-value".into());
    pipe_case.args = vec!["--pipe-probe".into(), "exit".into(), super::Argument::Bytes(super::ByteArgument { bytes: vec![255, 32, 39, 36] })];
    let pipe = execute(&candidate, &pipe_case)?;
    if pipe["exit"] != format!("{:?}", ExitStatus::Code(7)) { return Err("supervisor changed a nonzero child exit".into()); }
    let bytes: Vec<u8> = serde_json::from_value(pipe["files"]["pipe-probe"]["bytes"].clone())?;
    let probe: Value = serde_json::from_slice(&bytes)?;
    if probe["core_dump_limits"] != json!([0, 0]) { return Err("owned child did not inherit zero core-file limits".into()); }
    println!("owned child inherits zero soft/hard core-file limits");
    if probe["input"] != json!(b"owned data\n") || probe["args"] != json!([b"exit".to_vec(), vec![255,32,39,36]])
        || probe["environment"] != json!(b"inherited-value") || probe["foreground"] != true {
        return Err("supervisor changed stdin, argv, environment or foreground ownership".into());
    }
    let bytes: Vec<u8> = serde_json::from_value(pipe["files"]["supervisor-state"]["bytes"].clone())?;
    let state: Value = serde_json::from_slice(&bytes)?;
    for field in ["foreground", "producer_held_open", "producer_reaped", "pager_reaped", "kernel_restored"] {
        if state[field] != true { return Err(format!("supervisor control failed: {field}").into()); }
    }
    pipe_case.args[1] = "signal".into();
    let pipe = execute(&candidate, &pipe_case)?;
    if pipe["exit"] != format!("{:?}", ExitStatus::Signal(15)) { return Err("supervisor changed child signal death".into()); }
    pipe_case.args[1] = "wait".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }, Event::Signal { number:15 }];
    let pipe = execute(&candidate, &pipe_case)?;
    if pipe["exit"] != format!("{:?}", ExitStatus::Signal(15)) { return Err("supervisor failed to forward explicit termination".into()); }
    pipe_case.env.insert("LZ_PARITY_PRODUCER_CLOSE".into(), "1".into());
    pipe_case.args[1] = "exit".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }, Event::Send { bytes:b"q".to_vec() }];
    let closed = execute(&candidate, &pipe_case)?;
    let bytes: Vec<u8> = serde_json::from_value(closed["files"]["supervisor-state"]["bytes"].clone())?;
    let state: Value = serde_json::from_slice(&bytes)?;
    if state["producer_held_open"] != false || state["producer_reaped"] != true || closed["exit"] != format!("{:?}", ExitStatus::Code(7)) {
        return Err("supervisor failed to reap a producer that closed its pipe".into());
    }
    pipe_case.env.remove("LZ_PARITY_PRODUCER_CLOSE");
    pipe_case.env.insert("LZ_PARITY_PRODUCER_READY".into(), "producer-ready".into());
    pipe_case.events = vec![Event::Checkpoint { ready:"producer-ready".into(), row:None }, Event::Send { bytes:b"q".to_vec() }];
    execute(&candidate, &pipe_case)?;
    pipe_case.env.remove("LZ_PARITY_PRODUCER_READY");
    println!("supervisor argv/env/stdin/foreground, code 7, signal 15 and explicit TERM controls pass");
    pipe_case.args[1] = "job".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }, Event::Suspend {}, Event::Suspend {}, Event::Send { bytes:b"\n".to_vec() }];
    let job = execute(&candidate, &pipe_case)?;
    if job["exit"] != format!("{:?}", ExitStatus::Code(7)) || job["checkpoints"][1]["suspended"]["stops"] != 1
        || job["checkpoints"][2]["suspended"]["stops"] != 2 || job["checkpoints"][2]["suspended"]["continues"] != 1
        || job["checkpoints"][2]["suspended"]["kernel_restored"] != true {
        return Err("supervisor did not record two actual kernel stops and the intervening continue".into());
    }
    println!("owned foreground child stops and continues twice with portable kernel wait status and pre-resume termios records");
    pipe_case.args[1] = "primary".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }, Event::Send { bytes: b"n".to_vec() },
        Event::Checkpoint { ready:"alternate".into(), row:None }, Event::Send { bytes: b"q".to_vec() }];
    let primary_transition = execute(&candidate, &pipe_case)?;
    if primary_transition["checkpoints"].as_array().is_none_or(|frames| frames.len() != 2) { return Err("missing staged terminal checkpoints".into()); }
    for (index, enabled) in [(0, false), (1, true)] {
        let modes = primary_transition["checkpoints"][index]["screen"]["modes"].as_str().ok_or("missing staged terminal modes")?;
        for field in ["alternate_screen", "application_cursor", "application_keypad"] {
            if !modes.contains(&format!("{field}: {enabled}")) { return Err(format!("staged terminal acquisition failed: {field}").into()); }
        }
    }
    println!("primary screen acquires no alternate/application modes; explicit transition acquires them once and restores them");
    pipe_case.args[1] = "queued".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }, Event::Send { bytes:b"qretained\n".to_vec() }];
    let queued = execute(&candidate, &pipe_case)?;
    if queued["files"]["queued-input"]["bytes"] != json!(b"retained\n") { return Err("terminal restoration discarded queued input".into()); }
    println!("terminal restoration and reentry preserve queued tty input for the next reader");
    pipe_case.args[1] = "hang".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }];
    let supervisor_world = super::stdio::world()?;
    let supervisor_root = supervisor_world.keep();
    let manifest = supervisor_root.join("groups");
    let start = Instant::now();
    let timeout = execute_with_manifest(&candidate, &pipe_case, &manifest);
    if !timeout.is_err_and(|error| error.downcast_ref::<ptytest::PtyTestError>().is_some_and(|error| matches!(error, ptytest::PtyTestError::Timeout { .. }))) {
        return Err("supervisor timeout control did not time out at the PTY boundary".into());
    }
    super::supervisor::assert_cleanup(&manifest, 2)?;
    if start.elapsed() > Duration::from_secs(5) { return Err("supervisor cancellation exceeded checkpoint plus escalation bound".into()); }
    println!("pipe supervisor preserves raw argv, env, foreground group, stdin and exit/signal status; open producer and ignored-TERM child are reaped on cancellation");
    pipe_case.args[1] = "new-group".into();
    pipe_case.events = vec![Event::Checkpoint { ready:"probe".into(), row:None }, Event::Send { bytes:b"q".to_vec() }];
    let pid_path = supervisor_root.join("orphan-pid");
    pipe_case.env.insert("LZ_ORPHAN_PROBE_PID".into(), pid_path.to_string_lossy().into_owned());
    let leak = execute(&candidate, &pipe_case);
    if !leak.is_err_and(|error| error.is::<super::supervisor::SessionLeak>()) { return Err("independent helper group escaped the lifecycle comparator".into()); }
    let pid: i32 = fs::read_to_string(pid_path)?.parse()?;
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        // SAFETY: signal zero only queries the fixture child identified in the owned record.
        if unsafe { libc::kill(pid, 0) } != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) { break; }
        if Instant::now() >= deadline { return Err("session cleanup left the independent helper child alive".into()); }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!("session-wide lifecycle checks reject a leaked independent group and cleanup kills it before releasing the owned session identity");
    let mut invalid = serde_json::to_value(&case)?;
    invalid.as_object_mut().unwrap().insert("typo".into(), json!(true));
    if serde_json::from_value::<Case>(invalid).is_ok() { return Err("unknown scenario field accepted".into()); }
    if serde_json::from_value::<Event>(json!({"type": "typo"})).is_ok() { return Err("unknown event accepted".into()); }
    let oracle_diagnostic = json!({"stderr": b"There is no -Z option (\"less --help\" for help)\n".to_vec()});
    let candidate_diagnostic = json!({"stderr": b"There is no -Z option (\"lz --help\" for help)\n".to_vec()});
    if comparable(&oracle_diagnostic) != comparable(&candidate_diagnostic) { return Err("identified diagnostic identity field differs".into()); }
    let wrong_diagnostic = json!({"stderr": b"There is no -Y option (\"lz --help\" for help)\n".to_vec()});
    if comparable(&oracle_diagnostic) == comparable(&wrong_diagnostic) { return Err("diagnostic identity normalization hid a wrong option".into()); }
    if comparable(&json!({"stdout": b"less --help".to_vec()})) == comparable(&json!({"stdout": b"lz --help".to_vec()})) { return Err("identity normalization escaped its identified location".into()); }
    println!("diagnostic program token normalized only at its identified location; negative controls pass");
    let expected_diagnostic = super::stdio::redirected(&candidate, &Vec::new().into(), "file", &["--stdio-fixture".into(), "none".into()], b"input", &BTreeMap::new())?;
    let dropped = super::stdio::redirected(&candidate, &Vec::new().into(), "file", &["--stdio-fixture".into(), "drop-error".into()], b"input", &BTreeMap::new())?;
    let difference = first_difference(&expected_diagnostic, &dropped, "observation").ok_or("dropped error missed")?;
    if !difference.contains("stderr") { return Err("dropped error detected at wrong observation".into()); }
    println!("detected dropped stderr diagnostic; unknown fields/events rejected");
    let expected = execute(&candidate, &case)?;
    if comparable(&expected) != comparable(&execute(&candidate, &case)?) { return Err("independent fixture self-comparison failed".into()); }
    let mut plain = case.clone();
    plain.trace_check = super::TraceCheck::PlainText;
    if !execute(&candidate, &plain).is_err_and(|error| error.is::<super::TraceViolation>()) {
        return Err("plain-text trace check missed a known screen-clearing control".into());
    }
    println!("plain-text trace check rejects an otherwise successful fixture with hidden controls");
    case.args[1] = "osc-after".into();
    if comparable(&expected) != comparable(&execute(&candidate, &case)?) { return Err("OSC terminators were counted as audible bells".into()); }
    for (fault, observation) in [("output", "text"), ("attribute", "attributes"), ("cursor", "cursor"), ("exit", "exit"), ("effect", "files"), ("file-mode", "mode"), ("bell-after", "final_bells")] {
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
    case.fixture = (1..81).map(|n| format!("{n:06} reference parity\n")).collect::<String>().into_bytes().into();
    case.args = vec!["input".into()];
    case.events = vec![Event::Checkpoint { ready: "000001".into(), row: None }, Event::Send { bytes: b"q".to_vec() }];
    let reference = super::oracle();
    let a = execute(&reference, &case)?;
    let b = execute(&reference, &case)?;
    if let Some(diff) = first_difference(&comparable(&a), &comparable(&b), "observation") { return Err(format!("oracle self-comparison: {diff}").into()); }
    if a["exit"] != format!("{:?}", ExitStatus::Code(0)) { return Err("oracle quit exit differs".into()); }
    let mut large_navigation = case.clone();
    large_navigation.id = "candidate-large-navigation-safety".into();
    large_navigation.args = vec!["-S".into(), "+9223372036854775807g9223372036854775807j".into(), "input".into()];
    large_navigation.events = vec![Event::Checkpoint { ready:"(END)".into(), row:Some(7) }, Event::Send { bytes:b"q".to_vec() }];
    let large_navigation = execute(Path::new(env!("CARGO_BIN_EXE_lz")), &large_navigation)?;
    if large_navigation["exit"] != format!("{:?}", ExitStatus::Code(0)) { return Err("large legal navigation counts did not exit cleanly".into()); }
    println!("candidate large-position arithmetic remains bounded and restores the terminal; out-of-file seek diagnostics remain a separate parity item");
    let mut hangup = case.clone();
    hangup.id = "candidate-hangup-safety".into();
    hangup.fixture = b"x".to_vec().into();
    hangup.events = vec![Event::Checkpoint { ready: "input (END)".into(), row: Some(7) }, Event::Signal { number: 1 }];
    let hangup = execute(Path::new(env!("CARGO_BIN_EXE_lz")), &hangup)?;
    if hangup["exit"] != format!("{:?}", ExitStatus::Signal(1)) || hangup["restored"] != true {
        return Err("candidate did not restore its terminal and preserve HUP signal death".into());
    }
    println!("candidate HUP safety: represented modes restored, signal death preserved; oracle leak remains a parked divergence");
    let report = json!({"candidate_hangup_safety": hangup, "candidate_large_navigation_safety":large_navigation, "primary_transition":primary_transition, "runner_executable": super::digest(&[candidate])?, "metadata": super::metadata(Path::new(env!("CARGO_BIN_EXE_lz")))?, "faults_detected": ["output", "attribute", "cursor", "exit", "effect", "timeout", "stderr", "large-effect-suffix", "fixture-bounds", "identity-location", "pipe-argv-env-foreground", "pipe-code-signal", "pipe-timeout-reap", "queued-input-retention", "plain-text-hidden-control", "parallel-order-timeout-drain", "file-mode", "final-bell", "osc-bell-terminator"], "oracle_a": a, "oracle_b": b});
    let path = super::stdio::world()?.keep().join("selftest.json");
    fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    println!("oracle independent-world startup/quit/lifecycle self-comparison passes; {}", path.display());
    Ok(())
}

pub(super) fn checkpoint_selftest() -> Result<()> {
    if finish_bell_window(Some(7), 7).is_err() || finish_bell_window(Some(7), 8).is_ok() || finish_bell_window(None, 7).is_ok() {
        return Err("bell window accepted an invalid interval".into());
    }
    let world = super::stdio::world()?;
    let quoted = std::env::current_exe()?.to_str().ok_or("non-UTF-8 checkpoint fixture path")?.replace('\'', "'\\''");
    for (name, fault) in [("quiet", "none"), ("noisy", "flood")] {
        let path = world.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nexec '{quoted}' --fixture {fault}\n"))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let case = Case {
        id:"checkpoint-control".into(),family:"harness".into(),admitted:false,source:"pty".into(),
        pty_rss_limit:Default::default(),pty_spill_limit:Default::default(),
        fixture:Vec::new().into(),args:Vec::new(),fixture_name:b"input".to_vec(),files:BTreeMap::new(),env:BTreeMap::new(),
        size:[40,8],locale:"C".into(),events:vec![Event::Checkpoint {ready:"probe".into(),row:Some(0)},Event::Send {bytes:b"q\n".to_vec()}],
        trace_check:super::TraceCheck::Captured,
    };
    let quiet = world.path().join("quiet");
    let noisy = world.path().join("noisy");
    let mut timed = case.clone();
    timed.events = vec![Event::Checkpoint { ready:"probe".into(), row:Some(0) }, Event::BellWindowStart {}, Event::BellWindowEnd {}, Event::Send { bytes:b"q\n".to_vec() }];
    if super::observe_case(&quiet, &quiet, &timed)["outcome"] != "pass" { return Err("valid bell window failed".into()); }
    for (events, message) in [
        (vec![Event::BellWindowEnd {}], "bell window ended without a start"),
        (vec![Event::BellWindowStart {}, Event::BellWindowStart {}], "bell window already started"),
        (vec![Event::BellWindowStart {}], "bell window was not ended"),
    ] {
        timed.events = events;
        if !execute(&quiet, &timed).is_err_and(|error| error.to_string().contains(message)) { return Err(format!("bell window accepted invalid sequence: {message}").into()); }
    }
    let result = super::observe_case(&quiet, &noisy, &case);
    if result["outcome"] != "mismatch" || !result["actual_error_detail"].as_str().is_some_and(|detail| detail.starts_with("Quiescence {")) {
        return Err(format!("continuous redraw did not retain its candidate-only quiescence failure: {} {}", result["outcome"], result["actual_error_detail"]).into());
    }
    let result = super::observe_case(&noisy, &noisy, &case);
    if result["outcome"] != "harness-error" || result["side"] != "oracle"
        || !result["error_detail"].as_str().is_some_and(|detail| detail.starts_with("Quiescence {")) {
        return Err("oracle nonquiescence was counted as parity".into());
    }
    println!("bell windows reject invalid and crossed intervals; bounded output slices preserve 20ms raw quiescence and detect continuous redraw");
    Ok(())
}
