use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::Duration;

unsafe extern "C" {
    fn signal(number: i32, handler: usize) -> usize;
    fn tcsetpgrp(fd: i32, group: i32) -> i32;
    fn getpgrp() -> i32;
    fn kill(pid: i32, number: i32) -> i32;
}

const SIGINT: i32 = 2;
const SIGTTOU: i32 = 22;
const ESRCH: i32 = 3;

struct ForegroundChild {
    child: Child,
    reaped: bool,
    tty: i32,
    parent_group: i32,
    previous_ttou: usize,
    restored: bool,
}

impl ForegroundChild {
    fn restore(&mut self) -> io::Result<()> {
        if self.restored { return Ok(()); }
        // SAFETY: the borrowed session tty remains open, and SIGTTOU is ignored until foreground ownership is reclaimed.
        let result = unsafe { tcsetpgrp(self.tty, self.parent_group) };
        let mut error = (result < 0).then(io::Error::last_os_error);
        // SAFETY: this is the disposition saved before launching the owned foreground helper.
        if unsafe { signal(SIGTTOU, self.previous_ttou) } == usize::MAX { error.get_or_insert_with(io::Error::last_os_error); }
        self.restored = true;
        error.map_or(Ok(()), Err)
    }
}

impl Drop for ForegroundChild {
    fn drop(&mut self) {
        if !self.reaped {
            // SAFETY: the unreaped child reserves its isolated process group; the pager is never a member.
            unsafe { kill(-(self.child.id() as i32), 9); }
            if let Err(error) = self.child.wait() { eprintln!("lz: helper cleanup: {error}"); }
        }
        if let Err(error) = self.restore() { eprintln!("lz: helper terminal restoration: {error}"); }
    }
}

enum Output { FirstLine, Closed }

pub(crate) fn read_line(command: crate::terminal::ShellHelper, tty: i32) -> io::Result<Vec<u8>> {
    run(command.into_command(), tty, Output::FirstLine)
}

pub(crate) fn close_output(command: crate::terminal::ShellHelper, tty: i32) -> io::Result<()> {
    run(command.into_command(), tty, Output::Closed).map(|_| ())
}

fn run(mut command: Command, tty: i32, output_mode: Output) -> io::Result<Vec<u8>> {
    // SAFETY: these calls query the current group and temporarily suppress background terminal-control stops.
    let parent_group = unsafe { getpgrp() };
    let previous_ttou = unsafe { signal(SIGTTOU, 1) };
    if previous_ttou == usize::MAX { return Err(io::Error::last_os_error()); }
    command.stdout(Stdio::piped()).process_group(0);
    // SAFETY: pre_exec uses only async-signal-safe terminal and signal operations on inherited descriptors.
    unsafe {
        command.pre_exec(move || {
            if tcsetpgrp(tty, getpgrp()) < 0 { return Err(io::Error::last_os_error()); }
            if signal(SIGTTOU, previous_ttou) == usize::MAX { return Err(io::Error::last_os_error()); }
            Ok(())
        });
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            // SAFETY: failed spawn must restore the parent's controlling group before restoring SIGTTOU.
            let terminal = unsafe { tcsetpgrp(tty, parent_group) };
            let cleanup = (terminal < 0).then(io::Error::last_os_error);
            // SAFETY: previous_ttou was saved before spawn and belongs to this process.
            let restored = unsafe { signal(SIGTTOU, previous_ttou) };
            let cleanup = cleanup.or_else(|| (restored == usize::MAX).then(io::Error::last_os_error));
            if let Some(cleanup) = cleanup { return Err(io::Error::other(format!("{error}; helper terminal restoration: {cleanup}"))); }
            return Err(error);
        }
    };
    let mut child = ForegroundChild { child, reaped:false, tty, parent_group, previous_ttou, restored:false };
    let result = (|| {
        let mut output = child.child.stdout.take().ok_or_else(|| io::Error::other("missing helper output"))?;
        let mut line = Vec::new();
        let mut bytes = [0; 4096];
        let mut interrupt_forwarded = false;
        if matches!(output_mode, Output::FirstLine) {
            loop {
                forward_interrupt(&child, &mut interrupt_forwarded)?;
                if cancelled() { return Err(io::Error::new(io::ErrorKind::Interrupted, "helper interrupted")); }
                if !crate::readiness::readable(output.as_raw_fd(), 20)? { continue; }
                let count = match output.read(&mut bytes) {
                    Ok(count) => count,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                };
                let newline = bytes[..count].iter().position(|byte| *byte == b'\n');
                line.extend_from_slice(&bytes[..newline.unwrap_or(count)]);
                if count == 0 || newline.is_some() { break; }
            }
        }
        drop(output);
        loop {
            if let Some(status) = child.child.try_wait()? {
                child.reaped = true;
                if status.code() == Some(crate::terminal::HELPER_INTERRUPT_STATUS) || status.signal() == Some(SIGINT) { crate::terminal::note_interrupt(); }
                break;
            }
            forward_interrupt(&child, &mut interrupt_forwarded)?;
            if cancelled() { return Err(io::Error::new(io::ErrorKind::Interrupted, "helper interrupted")); }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(line)
    })();
    let restoration = child.restore();
    match (result, restoration) {
        (Ok(line), Ok(())) => Ok(line),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!("{error}; helper terminal restoration: {cleanup}"))),
    }
}

fn forward_interrupt(child: &ForegroundChild, forwarded: &mut bool) -> io::Result<()> {
    if !*forwarded && crate::terminal::interrupt_pending() {
        // SAFETY: the unreaped child reserves its isolated process group.
        if unsafe { kill(-(child.child.id() as i32), SIGINT) } < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ESRCH) { return Err(error); }
        }
        *forwarded = true;
    }
    Ok(())
}

fn cancelled() -> bool {
    crate::terminal::TERMINATION_SIGNAL.load(Ordering::SeqCst) != 0
}
