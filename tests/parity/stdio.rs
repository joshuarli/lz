use std::fs::{self, File};
use std::io;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(10);
const TRACE_LIMIT: u64 = 64 * 1024 * 1024;

pub(super) struct OwnedChild(pub(super) Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn observed(stdout: Vec<u8>, stderr: Vec<u8>, status: ExitStatus) -> Value {
    json!({"stdout": stdout, "stderr": stderr, "code": status.code(), "signal": status.signal()})
}

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn world() -> io::Result<tempfile::TempDir> {
    let scratch = root().join(".work/tmp");
    fs::create_dir_all(&scratch)?;
    tempfile::tempdir_in(scratch)
}

fn command(binary: &Path, cwd: &Path) -> Command {
    let mut cmd = Command::new(binary);
    cmd.env_clear().env("HOME", cwd).env("XDG_STATE_HOME", cwd.join("state"))
        .env("XDG_CONFIG_HOME", cwd.join("config")).env("LC_ALL", "C")
        .env("TERM", "xterm-256color").env("PATH", "/usr/bin:/bin")
        .env("LESSHISTFILE", "-").current_dir(cwd);
    // SAFETY: setsid is async-signal-safe and uses no parent memory after fork.
    unsafe {
        cmd.pre_exec(|| {
            unsafe extern "C" { fn setsid() -> i32; }
            if setsid() < 0 { return Err(io::Error::last_os_error()); }
            Ok(())
        });
    }
    cmd
}

fn wait(child: &mut Child, paths: &[PathBuf]) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + LIMIT;
    loop {
        for path in paths {
            if fs::metadata(path)?.len() > TRACE_LIMIT {
                return Err(io::Error::other("harness-error: trace limit"));
            }
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "harness-error: scenario deadline"));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn redirected(binary: &Path, fixture_data: &super::Fixture, source: &str, args: &[OsString], fixture_name: &[u8], files: &BTreeMap<String, super::Fixture>) -> super::Result<Value> {
    let dir = world()?;
    let cwd = dir.path();
    let fixture = cwd.join(OsString::from_vec(fixture_name.to_vec()));
    for (name, data) in files {
        let path = cwd.join(name);
        if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
        super::write_auxiliary_fixture(&path, data)?;
    }
    super::write_fixture(&fixture, fixture_data)?;
    let out = cwd.join("stdout");
    let err = cwd.join("stderr");
    let mut cmd = command(binary, cwd);
    cmd.args(args).stdout(File::create(&out)?).stderr(File::create(&err)?);
    let producer = if source == "pipe" {
        let mut producer = Command::new("/bin/cat");
        producer.arg(&fixture).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut producer = OwnedChild(producer.spawn()?);
        cmd.stdin(producer.0.stdout.take().unwrap());
        Some(producer)
    } else {
        cmd.stdin(Stdio::null());
        None
    };
    let mut child = OwnedChild(cmd.spawn()?);
    let status = wait(&mut child.0, &[out.clone(), err.clone()])?;
    if let Some(mut producer) = producer {
        let status = wait(&mut producer.0, &[])?;
        if !status.success() {
            return Err(io::Error::other("harness-error: producer failed").into());
        }
    }
    Ok(observed(fs::read(out)?, fs::read(err)?, status))
}

