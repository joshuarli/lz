use super::{Result, json};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn signal(number: i32, handler: usize) -> usize;
    fn kill(pid: i32, number: i32) -> i32;
    fn getpgrp() -> i32;
    fn tcsetpgrp(fd: i32, group: i32) -> i32;
    fn tcgetpgrp(fd: i32) -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn raise(number: i32) -> i32;
    fn tcgetattr(fd: i32, state: *mut lz::terminal::Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, state: *const lz::terminal::Termios) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_listallpids(buffer: *mut std::ffi::c_void, bytes: i32) -> i32;
    fn getpgid(pid: i32) -> i32;
}

#[cfg(target_os = "macos")]
fn signal_darwin_members(group: i32, number: i32) -> io::Result<()> {
    for capacity in [8192, 32768, 131072] {
        let mut pids = vec![0i32; capacity];
        // SAFETY: pids owns capacity initialized pid_t entries; libproc receives their byte capacity and retains no pointer.
        let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (capacity * std::mem::size_of::<i32>()) as i32) };
        if count < 0 { return Err(io::Error::last_os_error()); }
        if count as usize >= capacity { continue; }
        for pid in pids.into_iter().take(count as usize).filter(|pid| *pid > 0) {
            // SAFETY: group membership is queried immediately before signalling. The owned unreaped leader reserves the group id.
            if unsafe { getpgid(pid) } == group && unsafe { kill(pid, number) } < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(3) { return Err(error); }
            }
        }
        return Ok(());
    }
    Err(io::Error::other("Darwin process table exceeded the bounded signal enumeration"))
}

#[cfg(target_os = "macos")]
const JOB_SIGNALS: [i32; 4] = [18, 19, 21, 22];
#[cfg(target_os = "linux")]
const JOB_SIGNALS: [i32; 4] = [20, 18, 21, 22];
#[cfg(target_os = "macos")]
const STOP: i32 = 17;
#[cfg(target_os = "linux")]
const STOP: i32 = 19;
#[cfg(target_os = "macos")]
const CONTINUED: i32 = 16;
#[cfg(target_os = "linux")]
const CONTINUED: i32 = 8;

static SIGNALS: AtomicU32 = AtomicU32::new(0);

extern "C" fn remember(number: i32) { SIGNALS.fetch_or(1 << number, Ordering::SeqCst); }

fn disposition(number: i32, handler: usize) -> io::Result<()> {
    // SAFETY: callers provide a C signal handler or the platform SIG_DFL/SIG_IGN sentinel.
    if unsafe { signal(number, handler) } == usize::MAX { return Err(io::Error::last_os_error()); }
    Ok(())
}

fn group_signal(pid: u32, number: i32) -> io::Result<()> {
    let group = i32::try_from(pid).map_err(|_| io::Error::other("child process group is out of range"))?;
    // SAFETY: the negative pid addresses an owned child process group, never the caller's group.
    if unsafe { kill(-group, number) } < 0 {
        let error = io::Error::last_os_error();
        // Darwin can reject killpg even when signalling each owned member is permitted.
        #[cfg(target_os = "macos")]
        if error.raw_os_error() == Some(1) { return signal_darwin_members(group, number); }
        if error.raw_os_error() != Some(3) { return Err(error); }
    }
    Ok(())
}

struct GroupChild { child: Child, reaped: bool }
impl Drop for GroupChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = group_signal(self.child.id(), 9);
            let _ = self.child.wait();
        }
    }
}

impl GroupChild {
    fn stop_and_reap(&mut self) -> io::Result<()> {
        group_signal(self.child.id(), 15)?;
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if self.child.try_wait()?.is_some() { self.reaped = true; return Ok(()); }
            std::thread::sleep(Duration::from_millis(2));
        }
        group_signal(self.child.id(), 9)?;
        self.child.wait()?;
        self.reaped = true;
        Ok(())
    }
}

pub(super) struct Cleanup { manifest: PathBuf }
impl Cleanup { pub fn new(manifest: PathBuf) -> Self { Self { manifest } } }
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(bytes) = fs::read(&self.manifest)
            && let Ok(groups) = serde_json::from_slice::<Vec<u32>>(&bytes) {
            for pid in groups { let _ = group_signal(pid, 9); }
        }
    }
}

fn record_groups(path: &Path, groups: &[u32]) -> Result<()> {
    let temporary = path.with_extension("new");
    fs::write(&temporary, serde_json::to_vec(groups)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn termios(fd: i32) -> io::Result<lz::terminal::Termios> {
    // SAFETY: the initialized object has the existing platform termios layout and is valid for this call.
    let mut state = unsafe { std::mem::zeroed() };
    // SAFETY: fd remains owned by the caller and state is writable for a complete termios value.
    if unsafe { tcgetattr(fd, &mut state) } < 0 { return Err(io::Error::last_os_error()); }
    Ok(state)
}

fn same_termios(a: &lz::terminal::Termios, b: &lz::terminal::Termios) -> bool {
    // Darwin PENDIN is kernel-managed retyping state, not an acquired mode. Full flags remain in the differential observation.
    #[cfg(target_os = "macos")]
    let local_equal = (a.c_lflag & !0x20000000) == (b.c_lflag & !0x20000000);
    #[cfg(target_os = "linux")]
    let local_equal = a.c_lflag == b.c_lflag;
    let equal = a.c_iflag == b.c_iflag && a.c_oflag == b.c_oflag && a.c_cflag == b.c_cflag
        && local_equal && a.c_cc == b.c_cc && a.c_ispeed == b.c_ispeed && a.c_ospeed == b.c_ospeed;
    #[cfg(target_os = "linux")]
    { equal && a.c_line == b.c_line }
    #[cfg(target_os = "macos")]
    { equal }
}

pub fn producer(path: &Path) -> Result<()> {
    fs::write("producer-state", b"holding-open")?;
    io::copy(&mut File::open(path)?, &mut io::stdout().lock())?;
    loop { std::thread::park(); }
}

pub fn run(binary: &Path, fixture: &Path, manifest: &Path, args: &[std::ffi::OsString]) -> Result<()> {
    let forwarded = [1, 2, 3, 15, 28, JOB_SIGNALS[0], JOB_SIGNALS[1]];
    for number in forwarded { disposition(number, remember as *const () as usize)?; }
    for number in &JOB_SIGNALS[2..] { disposition(*number, 1)?; }
    let tty = File::options().read(true).write(true).open("/dev/tty")?;
    let fd = tty.as_raw_fd();
    let baseline = termios(fd)?;
    let producer = Command::new(std::env::current_exe()?).args([std::ffi::OsStr::new("--pipe-producer"), fixture.as_os_str()])
        .stdin(Stdio::null()).stdout(Stdio::piped()).process_group(0).spawn()?;
    let mut producer = GroupChild { child: producer, reaped: false };
    record_groups(manifest, &[producer.child.id()])?;
    let mut command = Command::new(binary);
    command.args(args).stdin(producer.child.stdout.take().ok_or("missing producer pipe")?).process_group(0);
    // SAFETY: tcsetpgrp/getpgrp/signal are async-signal-safe. fd is inherited until exec; all dispositions are reset before exec.
    unsafe {
        command.pre_exec(move || {
            if tcsetpgrp(fd, getpgrp()) < 0 { return Err(io::Error::last_os_error()); }
            for number in forwarded.into_iter().chain(JOB_SIGNALS[2..].iter().copied()) {
                if signal(number, 0) == usize::MAX { return Err(io::Error::last_os_error()); }
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let mut pager = GroupChild { child, reaped: false };
    record_groups(manifest, &[producer.child.id(), pager.child.id()])?;
    // SAFETY: fd is an owned controlling tty descriptor, and getpgrp returns the current process group.
    let foreground = unsafe { tcgetpgrp(fd) } == pager.child.id() as i32;
    let deadline = Instant::now() + Duration::from_secs(9);
    let mut cancellation_deadline = None;
    let mut stops = 0;
    let mut continues = 0;
    let status = loop {
        let pending = SIGNALS.swap(0, Ordering::SeqCst);
        for number in forwarded {
            if pending & (1 << number) != 0 {
                group_signal(pager.child.id(), number)?;
                if matches!(number, 1 | 3 | 15) && cancellation_deadline.is_none() { cancellation_deadline = Some(Instant::now() + Duration::from_millis(100)); }
            }
        }
        if cancellation_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            group_signal(pager.child.id(), 9)?;
        }
        let mut status = 0;
        // SAFETY: pager is an owned direct child, status is writable, and the flags only request nonblocking status observations.
        let result = unsafe { waitpid(pager.child.id() as i32, &mut status, 1 | 2 | CONTINUED) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted { return Err(error.into()); }
        } else if result > 0 {
            if status == 0xffff { continues += 1; }
            else if status & 0xff == 0x7f {
                stops += 1;
                // SAFETY: SIGSTOP deliberately mirrors the owned pager's stopped state; the harness can resume the supervisor group.
                if unsafe { raise(STOP) } != 0 { return Err(io::Error::last_os_error().into()); }
            } else { pager.reaped = true; break ExitStatus::from_raw(status); }
        }
        if Instant::now() >= deadline { return Err("supervised pager exceeded its 9-second deadline".into()); }
        std::thread::sleep(Duration::from_millis(2));
    };
    let producer_held_open = producer.child.try_wait()?.is_none();
    producer.stop_and_reap()?;
    fs::write("producer-state", b"stopped-and-reaped")?;
    let after = termios(fd)?;
    let restored = same_termios(&baseline, &after);
    let record = |state: &lz::terminal::Termios| json!({"flags":[state.c_iflag,state.c_oflag,state.c_cflag,state.c_lflag],
        "characters":state.c_cc.to_vec(),"speeds":[state.c_ispeed,state.c_ospeed]});
    fs::write("supervisor-state", serde_json::to_vec(&json!({"foreground":foreground,"producer_held_open":producer_held_open,
        "producer_reaped": producer.reaped,"pager_reaped":pager.reaped,"kernel_restored":restored,
        "kernel_before":record(&baseline),"kernel_after":record(&after),
        "code":status.code(),"signal":status.signal(),"stops":stops,"continues":continues}))?)?;
    // SAFETY: the supervisor owns the controlling tty, ignores SIGTTOU, and retakes its own process group after the pager has exited.
    if unsafe { tcsetpgrp(fd, getpgrp()) } < 0 { return Err(io::Error::last_os_error().into()); }
    if !restored {
        // SAFETY: baseline was returned by tcgetattr for this descriptor. Restore after recording the failure so it remains observable.
        if unsafe { tcsetattr(fd, 0, &baseline) } < 0 { return Err(io::Error::last_os_error().into()); }
    }
    record_groups(&manifest.with_extension("finished"), &[producer.child.id(), pager.child.id()])?;
    record_groups(manifest, &[])?;
    if let Some(number) = status.signal() {
        disposition(number, 0)?;
        // SAFETY: the default disposition was restored, preserving the supervised child's signal exit.
        if unsafe { raise(number) } != 0 { return Err(io::Error::last_os_error().into()); }
        return Err("default terminating signal unexpectedly returned".into());
    }
    std::process::exit(status.code().ok_or("missing supervised exit code")?);
}

pub fn probe(args: &[std::ffi::OsString]) -> Result<()> {
    let mut bytes = [0; 11];
    io::stdin().read_exact(&mut bytes)?;
    let tty = File::open("/dev/tty")?;
    // SAFETY: tty is a live controlling-terminal descriptor; both calls return kernel process-group identifiers.
    let foreground = unsafe { tcgetpgrp(tty.as_raw_fd()) == getpgrp() };
    let observation = json!({"input":bytes, "args": args.iter().map(|arg| arg.as_bytes()).collect::<Vec<_>>(),
        "environment":std::env::var_os("SUPERVISOR_PROBE").map(|value| value.as_bytes().to_vec()),"foreground":foreground});
    fs::write("pipe-probe", serde_json::to_vec(&observation)?)?;
    if args.first().is_some_and(|arg| arg == "hang") {
        disposition(15, 1)?;
        print!("probe");
        io::stdout().flush()?;
        loop { std::thread::park(); }
    }
    let mut session = lz::terminal::Session::start()?;
    print!("probe");
    io::stdout().flush()?;
    loop {
        if lz::terminal::TERMINATION_SIGNAL.load(Ordering::SeqCst) != 0 { break; }
        if session.keys_mut().read_key_timeout(100)?.is_some() { break; }
    }
    session.finish()?;
    if args.first().is_some_and(|arg| arg == "signal") || lz::terminal::TERMINATION_SIGNAL.load(Ordering::SeqCst) == 15 {
        disposition(15, 0)?;
        // SAFETY: terminate the owned probe using the reset default disposition.
        unsafe { raise(15); }
    }
    if args.first().is_some_and(|arg| arg == "exit") { std::process::exit(7); }
    Ok(())
}

pub fn assert_cleanup(manifest: &Path) -> Result<()> {
    let active: Vec<u32> = serde_json::from_slice(&fs::read(manifest)?)?;
    let finished: Vec<u32> = serde_json::from_slice(&fs::read(manifest.with_extension("finished"))?)?;
    if !active.is_empty() || finished.len() != 2 { return Err("supervisor did not finish reaping both children".into()); }
    for pid in finished {
        // SAFETY: signal zero only queries a previously owned group, without sending a signal.
        if unsafe { kill(-(pid as i32), 0) } == 0 { return Err("supervisor left a live process group".into()); }
        if io::Error::last_os_error().raw_os_error() != Some(3) { return Err(io::Error::last_os_error().into()); }
    }
    Ok(())
}
