use super::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_RSS: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub(super) struct RssLimit(u64);

impl Default for RssLimit { fn default() -> Self { Self(MAX_RSS) } }
impl TryFrom<u64> for RssLimit {
    type Error = String;
    fn try_from(value: u64) -> std::result::Result<Self, Self::Error> {
        if value == 0 || value > MAX_RSS { return Err(format!("RSS limit must be in 1..={MAX_RSS} bytes")); }
        Ok(Self(value))
    }
}
impl From<RssLimit> for u64 { fn from(limit: RssLimit) -> Self { limit.0 } }

#[derive(Debug)]
pub(super) enum Violation { Resident { peak: u64, limit: u64 }, Spill { peak: u64, limit: u64 } }
impl std::fmt::Display for Violation {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resident {peak,limit} => write!(output, "resident memory budget exceeded: {peak} bytes > {limit} bytes"),
            Self::Spill {peak,limit} => write!(output, "owned spill budget exceeded: {peak} bytes > {limit} bytes"),
        }
    }
}
impl std::error::Error for Violation {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    limit: RssLimit,
    live_peak: u64,
    spill_limit: super::spill::Limit,
    spill_peak: u64,
    kernel_peak: Option<u64>,
    error: Option<String>,
}

fn path(manifest: &Path) -> PathBuf { manifest.with_extension("resources.json") }

impl Record {
    pub(super) fn new(limit: RssLimit, spill_limit: super::spill::Limit) -> Self { Self { limit, spill_limit, spill_peak: 0, live_peak: 0, kernel_peak: None, error: None } }
    pub(super) fn note_live(&mut self, bytes: u64) { self.live_peak = self.live_peak.max(bytes); }
    pub(super) fn note_spill(&mut self, bytes: u64) { self.spill_peak = self.spill_peak.max(bytes); }
    fn peak(&self) -> u64 { self.live_peak.max(self.kernel_peak.unwrap_or(0)) }
    pub(super) fn exceeded(&self) -> bool { self.peak() > self.limit.0 || self.spill_peak > u64::from(self.spill_limit) }
    pub(super) fn finish(&mut self, kernel_peak: u64) { self.kernel_peak = Some(kernel_peak); }
    pub(super) fn fail(&mut self, error: &io::Error) { self.error = Some(error.to_string()); }
    pub(super) fn save(&self, manifest: &Path) -> Result<()> {
        let destination = path(manifest);
        let temporary = destination.with_extension("new");
        fs::write(&temporary, serde_json::to_vec(self)?)?;
        fs::rename(temporary, destination)?;
        Ok(())
    }
}

pub(super) fn check(manifest: &Path, successful: bool) -> Result<Option<Record>> {
    let bytes = match fs::read(path(manifest)) {
        Ok(bytes) => bytes,
        Err(error) if !successful && error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let record: Record = serde_json::from_slice(&bytes)?;
    if let Some(error) = &record.error { return Err(format!("harness-error: resource observation failed: {error}").into()); }
    if record.peak() > record.limit.0 { return Err(Box::new(Violation::Resident { peak: record.peak(), limit: record.limit.0 })); }
    if record.spill_peak > u64::from(record.spill_limit) { return Err(Box::new(Violation::Spill { peak: record.spill_peak, limit: u64::from(record.spill_limit) })); }
    if successful && (record.kernel_peak.is_none_or(|peak| peak == 0)) { return Err("harness-error: missing completed kernel RSS accounting".into()); }
    Ok(Some(record))
}

#[cfg(target_os = "macos")]
pub(super) fn resident(pid: u32) -> io::Result<u64> {
    // SAFETY: libc supplies the platform ABI; the zeroed record is writable for RUSAGE_INFO_V0.
    let mut usage: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
    // SAFETY: pid is an owned unreaped child; the API retains no pointer.
    if unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V0, (&mut usage as *mut libc::rusage_info_v0).cast()) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) { return Ok(0); }
        return Err(error);
    }
    Ok(usage.ri_resident_size)
}

#[cfg(target_os = "linux")]
pub(super) fn resident(pid: u32) -> io::Result<u64> {
    let status = match fs::read_to_string(format!("/proc/{pid}/statm")) {
        Ok(status) => status,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let pages: u64 = status.split_whitespace().nth(1).ok_or_else(|| io::Error::other("missing resident pages"))?
        .parse().map_err(io::Error::other)?;
    // SAFETY: sysconf queries a process-wide kernel page-size constant and retains no pointer.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 { return Err(io::Error::last_os_error()); }
    pages.checked_mul(page_size as u64).ok_or_else(|| io::Error::other("resident byte count overflow"))
}

pub(super) fn reaped_peak(usage: &libc::rusage) -> io::Result<u64> {
    let peak = u64::try_from(usage.ru_maxrss).map_err(io::Error::other)?;
    #[cfg(target_os = "macos")]
    { Ok(peak) }
    #[cfg(target_os = "linux")]
    { peak.checked_mul(1024).ok_or_else(|| io::Error::other("kernel RSS byte count overflow")) }
}

pub(super) fn selftest() -> Result<()> {
    use super::{Argument, Case, Event, TraceCheck, json};
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    let control = super::stdio::world()?;
    let executable = std::env::current_exe()?;
    let quoted = executable.to_str().ok_or("non-UTF-8 resource probe executable")?.replace('\'', "'\\''");
    for (name, mode) in [("good", "exit"), ("over", "memory")] {
        let path = control.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nexec '{quoted}' --pipe-probe {mode}\n"))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let mut case = Case {
        id: "rss-control".into(), family:"harness".into(), admitted:false, source:"pty-pipe".into(),
        pty_rss_limit: RssLimit::default(), pty_spill_limit: Default::default(), fixture: b"owned data\n".to_vec().into(), args: Vec::<Argument>::new(),
        fixture_name:b"input".to_vec(), files:BTreeMap::new(), env:BTreeMap::new(), size:[40,8], locale:"C".into(),
        events:vec![Event::Checkpoint {ready:"probe".into(),row:None},Event::Send {bytes:vec![b'q']}],
        trace_check:TraceCheck::Captured,
    };
    let good = control.path().join("good");
    let over = control.path().join("over");
    let normal = super::observe_case(&good, &good, &case);
    if normal["outcome"] != "pass" { return Err(format!("resource supervisor self-comparison failed: {normal}").into()); }
    if !normal["resources"]["oracle"]["kernel_peak"].as_u64().is_some_and(|peak| peak > 0)
        || !normal["resources"]["candidate"]["kernel_peak"].as_u64().is_some_and(|peak| peak > 0) {
        return Err("completed kernel RSS evidence was not retained".into());
    }
    let unfinished = control.path().join("unfinished");
    Record::new(case.pty_rss_limit, case.pty_spill_limit).save(&unfinished)?;
    let error = check(&unfinished, true).err().ok_or("unfinished RSS evidence accepted")?;
    let classified = super::execution_error(&case, error.as_ref(), "candidate");
    if classified["outcome"] != "harness-error" || classified["side"] != "candidate" {
        return Err("candidate measurement failure was attributed to the oracle".into());
    }
    case.pty_rss_limit = (32 * 1024 * 1024).try_into()?;
    let rejected = super::observe_case(&good, &over, &case);
    if rejected["outcome"] != "mismatch" || !rejected["actual_error_detail"].as_str().is_some_and(|detail| detail.starts_with("Resident {")) {
        return Err(format!("candidate RSS violation did not retain its mismatch classification: {rejected}").into());
    }
    let oracle_rejected = super::observe_case(&over, &over, &case);
    if oracle_rejected["outcome"] != "harness-error" || oracle_rejected["side"] != "oracle"
        || !oracle_rejected["error"].as_str().is_some_and(|error| error.contains("resident memory budget exceeded")) {
        return Err(format!("oracle RSS violation was counted as parity: {oracle_rejected}").into());
    }
    for mode in ["spill", "read-only-spill", "write-only-spill"] {
        let path = control.path().join(mode);
        fs::write(&path, format!("#!/bin/sh\nexec '{quoted}' --pipe-probe {mode}\n"))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        let mut spill_case = case.clone();
        spill_case.id = format!("{mode}-control");
        spill_case.pty_rss_limit = RssLimit::default();
        spill_case.pty_spill_limit = (1536 * 1024).try_into()?;
        let measured = super::observe_case(&path, &path, &spill_case);
        let expected = if mode == "read-only-spill" { 0 } else { 1024 * 1024 };
        if measured["outcome"] != "pass" || measured["resources"]["candidate"]["spill_peak"] != expected {
            return Err(format!("spill accounting failed for {mode}: outcome={} peak={}", measured["outcome"], measured["resources"]["candidate"]["spill_peak"]).into());
        }
    }
    let mut spill_case = case.clone();
    spill_case.id = "spill-budget-control".into();
    spill_case.pty_rss_limit = RssLimit::default();
    spill_case.pty_spill_limit = (64 * 1024).try_into()?;
    let spilled = control.path().join("spill");
    let rejected = super::observe_case(&good, &spilled, &spill_case);
    if rejected["outcome"] != "mismatch" || !rejected["actual_error_detail"].as_str().is_some_and(|detail| detail.starts_with("Spill {")) {
        return Err(format!("candidate spill budget did not fail in its own category: {}", rejected["outcome"]).into());
    }
    let rejected = super::observe_case(&spilled, &spilled, &spill_case);
    if rejected["outcome"] != "harness-error" || rejected["side"] != "oracle"
        || !rejected["error"].as_str().is_some_and(|error| error.contains("owned spill budget exceeded")) {
        return Err("oracle spill budget was counted as candidate parity".into());
    }
    for bytes in [0, u64::from(super::spill::Limit::default()) + 1] {
        if super::spill::Limit::try_from(bytes).is_ok() || serde_json::from_value::<super::spill::Limit>(json!(bytes)).is_ok() {
            return Err("invalid spill budget accepted".into());
        }
    }
    println!("anonymous-spill accounting deduplicates inode aliases, excludes deleted read-only inputs, includes write-only files and rejects candidate/oracle budget breaches distinctly");
    for (name, fault) in [("kernel-good", "kernel-restored"), ("kernel-bad", "kernel-leak")] {
        let path = control.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nexec '{quoted}' --fixture {fault}\n"))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let mut kernel_case = case.clone();
    kernel_case.id = "kernel-restoration-control".into();
    kernel_case.source = "pty".into();
    kernel_case.pty_rss_limit = RssLimit::default();
    kernel_case.fixture = Vec::new().into();
    let kernel = super::observe_case(&control.path().join("kernel-good"), &control.path().join("kernel-bad"), &kernel_case);
    if kernel["outcome"] != "mismatch" || !kernel["actual_error_detail"].as_str().is_some_and(|detail| detail.contains("KernelViolation")) {
        return Err("the supervisor repair masked the child's raw-terminal leak".into());
    }
    for limit in [0, MAX_RSS + 1] {
        if RssLimit::try_from(limit).is_ok() || serde_json::from_value::<RssLimit>(json!(limit)).is_ok() {
            return Err("invalid RSS budget accepted".into());
        }
    }
    println!("owned supervisor records kernel peak RSS; candidate-only excess is a mismatch, oracle excess is a harness error, invalid limits are rejected, and pre-repair kernel leaks remain failures");
    Ok(())
}
