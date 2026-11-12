use std::fs::{self, File};
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(10);
const TRACE_LIMIT: u64 = 64 * 1024 * 1024;

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug, PartialEq)]
struct Observation {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
    signal: Option<i32>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn oracle() -> PathBuf {
    let os = if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let arch = if cfg!(target_arch = "aarch64") && cfg!(target_os = "macos") { "arm64" } else { std::env::consts::ARCH };
    root().join(format!(".work/tools/less-668/{os}-{arch}/ordinary/less"))
}

fn world() -> io::Result<tempfile::TempDir> {
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

fn redirected(binary: &Path, bytes: &[u8], source: &str) -> io::Result<Observation> {
    let dir = world()?;
    let cwd = dir.path();
    let fixture = cwd.join("input");
    fs::write(&fixture, bytes)?;
    let out = cwd.join("stdout");
    let err = cwd.join("stderr");
    let mut cmd = command(binary, cwd);
    cmd.stdout(File::create(&out)?).stderr(File::create(&err)?);
    let producer = if source == "pipe" {
        let mut producer = Command::new("/bin/cat");
        producer.arg(&fixture).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut producer = OwnedChild(producer.spawn()?);
        cmd.stdin(producer.0.stdout.take().unwrap());
        Some(producer)
    } else {
        cmd.stdin(Stdio::null()).arg(match source { "missing" => "missing", "directory" => ".", "special" => "/dev/null", _ => "input" });
        None
    };
    let mut child = OwnedChild(cmd.spawn()?);
    let status = wait(&mut child.0, &[out.clone(), err.clone()])?;
    if let Some(mut producer) = producer {
        let status = wait(&mut producer.0, &[])?;
        if !status.success() {
            return Err(io::Error::other("harness-error: producer failed"));
        }
    }
    Ok(Observation { stdout: fs::read(out)?, stderr: fs::read(err)?, code: status.code(), signal: status.signal() })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() == 1 {
        return Ok(());
    }
    let args: Vec<String> = std::env::args().collect();
    let value = |key: &str| args.windows(2).find(|pair| pair[0] == key).map(|pair| pair[1].as_str());
    let group = value("--group");
    if group.is_some_and(|g| g != "redirected") {
        return Err("unknown group".into());
    }
    if !matches!(value("--mode"), Some("admit" | "admitted" | "all")) {
        return Err("unknown mode".into());
    }
    let reference = oracle();
    if !reference.is_file() {
        return Err("run make parity-setup".into());
    }
    let mut count = 0;
    let mut failed = false;
    for (id, bytes) in [("text", &b"one\ntwo\nlast"[..]), ("empty", b""), ("binary", b"a\0b\n"), ("invalid", b"a\xff\xfe\n"), ("controls", b"a\r\n\x1b[31mb\x1b[0m\n")] {
        for source in ["file", "pipe"] {
            let case = format!("redirected-{source}-{id}");
            if value("--case").is_some_and(|selected| selected != case) {
                continue;
            }
            count += 1;
            let expected = redirected(&reference, bytes, source)?;
            let actual = redirected(Path::new(env!("CARGO_BIN_EXE_lz")), bytes, source)?;
            if expected != actual {
                failed = true;
                eprintln!("mismatch {case}: expected {expected:?}, actual {actual:?}\nreproduce: make parity-test CASE={case}");
            } else {
                println!("pass {case}");
            }
        }
    }
    for source in ["missing", "directory", "special"] {
        let case = format!("redirected-{source}");
        if value("--case").is_some_and(|selected| selected != case) { continue; }
        count += 1;
        let expected = redirected(&reference, b"", source)?;
        let actual = redirected(Path::new(env!("CARGO_BIN_EXE_lz")), b"", source)?;
        if expected != actual {
            failed = true;
            eprintln!("mismatch {case}: expected {expected:?}, actual {actual:?}\nreproduce: make parity-test CASE={case}");
        } else { println!("pass {case}"); }
    }
    if count == 0 {
        return Err("no selected cases".into());
    }
    if failed {
        return Err("differential mismatches".into());
    }
    println!("{count} redirected cases pass (bytes, stderr, exit)");
    Ok(())
}
