use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Mutex;

unsafe extern "C" {
    fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, termios: *const Termios) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

unsafe extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
    fn raise(sig: i32) -> i32;
}

const STDOUT_FD: i32 = 1;
const TCSADRAIN: i32 = 1;

#[cfg(target_os = "macos")]
const TIOCGWINSZ: u64 = 0x40087468;
#[cfg(target_os = "linux")]
const TIOCGWINSZ: u64 = 0x5413;

const SIGWINCH: i32 = 28;
#[cfg(target_os = "macos")]
const SIGTSTP: i32 = 18;
#[cfg(target_os = "linux")]
const SIGTSTP: i32 = 20;
#[cfg(target_os = "macos")]
const SIGCONT: i32 = 19;
#[cfg(target_os = "linux")]
const SIGCONT: i32 = 18;
const SIGTERM: i32 = 15;
const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGHUP: i32 = 1;
#[cfg(target_os = "macos")]
const SIGSTOP: i32 = 17;
#[cfg(target_os = "linux")]
const SIGSTOP: i32 = 19;

const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;

pub static WINCH_FLAG: AtomicBool = AtomicBool::new(false);
pub static TSTP_FLAG: AtomicBool = AtomicBool::new(false);
pub static CONT_FLAG: AtomicBool = AtomicBool::new(false);
pub static TERMINATION_SIGNAL: AtomicI32 = AtomicI32::new(0);
static INTERRUPT_FLAG: AtomicBool = AtomicBool::new(false);

/// Consume a pending SIGINT from the active terminal session.
pub fn take_interrupt() -> bool { INTERRUPT_FLAG.swap(false, Ordering::SeqCst) }

pub(crate) fn interrupt_pending() -> bool { INTERRUPT_FLAG.load(Ordering::SeqCst) }
pub(crate) fn note_interrupt() { INTERRUPT_FLAG.store(true, Ordering::SeqCst); }

/// The fd we set raw mode on (the /dev/tty fd).
static ALT_SCREEN: AtomicBool = AtomicBool::new(false);
static APPLICATION_MODES: AtomicBool = AtomicBool::new(false);
static INTERACTIVE_ACTIVE: AtomicBool = AtomicBool::new(false);
static SUSPENDED: AtomicBool = AtomicBool::new(false);
static SUSPENDED_ALT_SCREEN: AtomicBool = AtomicBool::new(false);
static SUSPENDED_APPLICATION_MODES: AtomicBool = AtomicBool::new(false);
static EOF_BELL: BellRate = BellRate(AtomicU64::new(0));
static TTY_FD: AtomicI32 = AtomicI32::new(-1);

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u64,
    pub c_oflag: u64,
    pub c_cflag: u64,
    pub c_lflag: u64,
    pub c_cc: [u8; 20],
    pub c_ispeed: u64,
    pub c_ospeed: u64,
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 32],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

static ORIG_TERMIOS: Mutex<Option<Termios>> = Mutex::new(None);
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

struct BellRate(AtomicU64);

impl BellRate {
    fn request(&self, at: std::time::SystemTime) -> bool {
        let second = at.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        self.0.swap(second, Ordering::SeqCst) != second
    }
}

pub(crate) fn request_eof_bell(at: std::time::SystemTime) -> bool { EOF_BELL.request(at) }

/// Select audible or visual feedback for boundary and other errors.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum BellPolicy { #[default] Audible, QuietBoundary, Visual, QuietBoundarySilent, Silent }

/// Distinguish boundary feedback from other errors.
#[derive(Clone, Copy)]
pub enum BellKind { Boundary, General }

impl BellPolicy {
    pub fn without_visual(self) -> Self {
        match self { Self::QuietBoundary => Self::QuietBoundarySilent, Self::Visual => Self::Silent, other => other }
    }

    pub fn is_visual(self, kind: BellKind) -> bool {
        self == Self::Visual || (self == Self::QuietBoundary && matches!(kind, BellKind::Boundary))
    }

    pub fn is_audible(self, kind: BellKind) -> bool {
        self == Self::Audible || (matches!(self, Self::QuietBoundary | Self::QuietBoundarySilent) && matches!(kind, BellKind::General))
    }

    pub fn write_to(self, output: &mut impl io::Write, kind: BellKind) -> io::Result<()> {
        if self.is_visual(kind) {
            output.write_all(b"\x1b[?5h")?;
            output.flush()?;
            for _ in 0..10 {
                if TERMINATION_SIGNAL.load(Ordering::SeqCst) != 0 { break; }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            output.write_all(b"\x1b[?5l")
        } else if self.is_audible(kind) { output.write_all(b"\x07") } else { Ok(()) }
    }
}

extern "C" fn handle_winch(_sig: i32) {
    WINCH_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn handle_tstp(_sig: i32) {
    TSTP_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn handle_cont(_sig: i32) {
    CONT_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn handle_term(sig: i32) {
    let _ = TERMINATION_SIGNAL.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
}

extern "C" fn handle_interrupt(_sig: i32) {
    INTERRUPT_FLAG.store(true, Ordering::SeqCst);
}

fn install_signal_handlers(suspension: SuspendPolicy) -> io::Result<Vec<(i32, usize)>> {
    let mut previous = Vec::new();
    for (number, handler) in [(SIGWINCH, handle_winch as *const () as usize), (SIGTSTP, if suspension == SuspendPolicy::Allow { handle_tstp as *const () as usize } else { SIG_IGN }), (SIGCONT, handle_cont as *const () as usize), (SIGTERM, handle_term as *const () as usize), (SIGHUP, handle_term as *const () as usize), (SIGINT, handle_interrupt as *const () as usize), (SIGQUIT, SIG_IGN)] {
        // SAFETY: entries are C handlers which only update atomic flags, or the platform SIG_IGN sentinel.
        let saved = unsafe { signal(number, handler) };
        if saved == usize::MAX {
            let error = io::Error::last_os_error();
            for (number, handler) in previous {
                // SAFETY: these dispositions were returned by signal for the same signal numbers.
                unsafe { signal(number, handler); }
            }
            return Err(error);
        }
        previous.push((number, saved));
    }
    Ok(previous)
}

/// Enable raw mode on the given fd (should be the /dev/tty fd).
fn enable_raw_mode(fd: i32) -> io::Result<()> {
    TTY_FD.store(fd, Ordering::SeqCst);
    unsafe {
        let mut raw: Termios = std::mem::zeroed();
        if tcgetattr(fd, &mut raw) != 0 {
            return Err(io::Error::last_os_error());
        }
        *ORIG_TERMIOS.lock().map_err(|_| io::Error::other("terminal state lock poisoned"))? = Some(raw);

        #[cfg(target_os = "macos")]
        {
            // Input flags: disable BRKINT, ICRNL, INPCK, ISTRIP, IXON
            raw.c_iflag &= !(0x02 | 0x100 | 0x10 | 0x20 | 0x200);
            // Output flags: disable OPOST
            raw.c_oflag &= !0x01;
            // Control flags: set CS8
            raw.c_cflag |= 0x300;
            // Preserve inherited ISIG so the kernel handles configured signal characters.
            raw.c_lflag &= !(0x08 | 0x100 | 0x400);
            // VMIN = 1, VTIME = 0
            raw.c_cc[16] = 1;
            raw.c_cc[17] = 0;
        }

        #[cfg(target_os = "linux")]
        {
            // Input flags: disable BRKINT, ICRNL, INPCK, ISTRIP, IXON
            raw.c_iflag &= !(0x02 | 0x100 | 0x10 | 0x20 | 0x400);
            // Output flags: disable OPOST
            raw.c_oflag &= !0x01;
            // Control flags: set CS8
            raw.c_cflag |= 0x30;
            // Preserve inherited ISIG so the kernel handles configured signal characters.
            raw.c_lflag &= !(0x08 | 0x02 | 0x8000);
            // VMIN = 1, VTIME = 0
            raw.c_cc[6] = 1;
            raw.c_cc[5] = 0;
        }

        if tcsetattr(fd, TCSADRAIN, &raw) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn disable_raw_mode() -> io::Result<()> {
    let fd = TTY_FD.load(Ordering::SeqCst);
    let original = *ORIG_TERMIOS.lock().map_err(|_| io::Error::other("terminal state lock poisoned"))?;
    if fd >= 0 && let Some(original) = original {
        // SAFETY: the active session owns fd and original has the platform termios layout.
        if unsafe { tcsetattr(fd, TCSADRAIN, &original) } != 0 { return Err(io::Error::last_os_error()); }
    }
    Ok(())
}

pub fn enter_alt_screen(w: &mut impl Write) -> io::Result<()> {
    ALT_SCREEN.store(true, Ordering::SeqCst);
    w.write_all(b"\x1b[?1049h")?;
    w.flush()
}

pub fn leave_alt_screen(w: &mut impl Write) -> io::Result<()> {
    ALT_SCREEN.store(false, Ordering::SeqCst);
    w.write_all(b"\x1b[?1049l")?;
    w.flush()
}

pub fn hide_cursor(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[?25l")?;
    Ok(())
}

pub fn show_cursor(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[?25h")?;
    w.flush()
}

pub fn move_cursor(buf: &mut Vec<u8>, row: u16, col: u16) {
    let _ = write!(buf, "\x1b[{};{}H", row + 1, col + 1);
}

pub fn clear_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[K");
}

pub fn get_terminal_size() -> (u16, u16) {
    unsafe {
        let mut ws: Winsize = std::mem::zeroed();
        if ioctl(STDOUT_FD, TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
            (ws.ws_col, ws.ws_row)
        } else {
            (80, 24)
        }
    }
}

pub fn handle_suspend() -> io::Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(b"\r\x1b[K")?;
    let alternate = ALT_SCREEN.load(Ordering::SeqCst);
    let application = APPLICATION_MODES.swap(false, Ordering::SeqCst);
    SUSPENDED_ALT_SCREEN.store(alternate, Ordering::SeqCst);
    SUSPENDED_APPLICATION_MODES.store(application, Ordering::SeqCst);
    if application { stdout.write_all(b"\x1b[?1l\x1b>")?; }
    if alternate { leave_alt_screen(&mut stdout)?; }
    show_cursor(&mut stdout)?;
    SUSPENDED.store(true, Ordering::SeqCst);

    // SAFETY: the session owns this signal disposition; SIGSTOP targets only the current process.
    unsafe {
        if signal(SIGTSTP, SIG_DFL) == usize::MAX || raise(SIGSTOP) != 0 { return Err(io::Error::last_os_error()); }
    }
    Ok(())
}

/// Whether continuation restored modes released by an owned suspension.
#[derive(PartialEq, Eq)]
pub enum ResumeOutcome { Unchanged, Restored }

pub fn handle_resume() -> io::Result<ResumeOutcome> {
    if !SUSPENDED.swap(false, Ordering::SeqCst) { return Ok(ResumeOutcome::Unchanged); }
    let fd = TTY_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        enable_raw_mode(fd)?;
    }
    let mut stdout = io::stdout().lock();
    if SUSPENDED_ALT_SCREEN.swap(false, Ordering::SeqCst) { enter_alt_screen(&mut stdout)?; }
    if SUSPENDED_APPLICATION_MODES.swap(false, Ordering::SeqCst) {
        APPLICATION_MODES.store(true, Ordering::SeqCst);
        stdout.write_all(b"\x1b[?1h\x1b=")?;
    }
    stdout.flush()?;

    // SAFETY: the C handler updates only an atomic flag and replaces the session's saved disposition.
    unsafe {
        if signal(SIGTSTP, handle_tstp as *const () as usize) == usize::MAX { return Err(io::Error::last_os_error()); }
    }
    Ok(ResumeOutcome::Restored)
}

fn restore_acquired_terminal() -> io::Result<()> {
    let mut failure = None;
    let fd = TTY_FD.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        match ORIG_TERMIOS.lock() {
            Ok(saved) => {
                let original = *saved;
                drop(saved);
                if let Some(original) = original {
                    // SAFETY: the session owns fd until after restoration and saved its termios before changing it.
                    if unsafe { tcsetattr(fd, TCSADRAIN, &original) } != 0 { failure = Some(io::Error::last_os_error()); }
                }
            }
            Err(_) => { failure = Some(io::Error::other("terminal state lock poisoned")); }
        }
    }
    let mut stdout = io::stdout().lock();
    let interactive = INTERACTIVE_ACTIVE.swap(false, Ordering::SeqCst);
    let alternate = ALT_SCREEN.swap(false, Ordering::SeqCst);
    if interactive && !alternate && let Err(error) = stdout.write_all(b"\r\x1b[K") { failure.get_or_insert(error); }
    if APPLICATION_MODES.swap(false, Ordering::SeqCst)
        && let Err(error) = stdout.write_all(b"\x1b[?1l\x1b>") { failure.get_or_insert(error); }
    if alternate && let Err(error) = stdout.write_all(b"\x1b[?1049l") { failure.get_or_insert(error); }
    if interactive && let Err(error) = stdout.write_all(b"\x1b[?25h") { failure.get_or_insert(error); }
    if let Err(error) = stdout.flush() { failure.get_or_insert(error); }
    match failure { Some(error) => Err(error), None => Ok(()) }
}

pub fn restore_terminal() {
    if let Err(error) = restore_acquired_terminal() { eprintln!("lz: terminal restoration: {error}"); }
}

pub(crate) const HELPER_INTERRUPT_STATUS: i32 = 222;

/// A shell command whose interrupt outcome is reported to its terminal owner.
pub struct ShellHelper { command: std::ffi::OsString }

impl ShellHelper {
    pub fn new(command: std::ffi::OsString) -> Self { Self { command } }
    pub(crate) fn into_command(self) -> std::process::Command {
        let mut process = std::process::Command::new("/bin/sh");
        process.arg("-c").arg(format!("trap 'exit {HELPER_INTERRUPT_STATUS}' INT; \"$@\"; exit 0"))
            .arg("lz-helper").arg("/bin/sh").arg("-c").arg(self.command);
        process
    }
}

/// Owns keyboard input and restores every acquired terminal mode before closing the tty.
pub struct Session {
    keys: crate::input::KeyReader,
    previous_handlers: Vec<(i32, usize)>,
    deferred_deinitialization: Option<InteractiveOptions>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ScreenMode { #[default] Alternate, Preserve }

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum KeypadMode { #[default] Application, Normal }

#[derive(Clone, Copy, Default)]
pub struct InteractiveOptions {
    pub screen: ScreenMode,
    pub keypad: KeypadMode,
}

#[derive(Clone, Copy)]
pub enum Screen { Primary, Interactive(InteractiveOptions) }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SuspendPolicy { Allow, Ignore }

impl Session {
    pub fn start(screen: Screen, suspension: SuspendPolicy) -> io::Result<Self> {
        let keys = crate::input::KeyReader::new()?;
        SESSION_ACTIVE.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).map_err(|_| io::Error::other("a terminal session is already active"))?;
        TERMINATION_SIGNAL.store(0, Ordering::SeqCst);
        INTERRUPT_FLAG.store(false, Ordering::SeqCst);
        SUSPENDED.store(false, Ordering::SeqCst);
        SUSPENDED_ALT_SCREEN.store(false, Ordering::SeqCst);
        SUSPENDED_APPLICATION_MODES.store(false, Ordering::SeqCst);
        TSTP_FLAG.store(false, Ordering::SeqCst);
        CONT_FLAG.store(false, Ordering::SeqCst);
        let mut session = Self { keys, previous_handlers: Vec::new(), deferred_deinitialization: None };
        session.previous_handlers = install_signal_handlers(suspension)?;
        enable_raw_mode(session.keys.fd())?;
        if let Screen::Interactive(options) = screen { session.enter_interactive(options)?; }
        Ok(session)
    }

    pub fn enter_interactive(&mut self, options: InteractiveOptions) -> io::Result<()> {
        if INTERACTIVE_ACTIVE.swap(true, Ordering::SeqCst) { return Err(io::Error::new(io::ErrorKind::AlreadyExists, "interactive display already acquired")); }
        let mut stdout = io::stdout().lock();
        if options.screen == ScreenMode::Alternate { enter_alt_screen(&mut stdout)?; }
        if options.keypad == KeypadMode::Application {
            APPLICATION_MODES.store(true, Ordering::SeqCst);
            stdout.write_all(b"\x1b[?1h\x1b=")?;
        }
        stdout.flush()
    }

    pub fn keys_mut(&mut self) -> &mut crate::input::KeyReader { &mut self.keys }

    /// Run a foreground helper, read its first output line, and reclaim the tty after reaping it.
    pub fn read_helper_line(&mut self, command: ShellHelper) -> io::Result<Vec<u8>> {
        crate::helper::read_line(command, self.keys.fd())
    }

    /// Run a foreground helper with its output pipe closed and wait for it.
    pub fn run_helper_with_closed_output(&mut self, command: ShellHelper) -> io::Result<()> {
        crate::helper::close_output(command, self.keys.fd())
    }

    /// Deinitialize display modes on finish after a one-screen session grows beyond the viewport.
    pub fn defer_deinitialization(&mut self, options: InteractiveOptions) -> io::Result<()> {
        if INTERACTIVE_ACTIVE.load(Ordering::SeqCst) || self.deferred_deinitialization.is_some() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "terminal display already acquired or deferred"));
        }
        self.deferred_deinitialization = Some(options);
        Ok(())
    }

    pub fn finish(self) -> io::Result<()> {
        let restored = restore_acquired_terminal();
        let deinitialized = if let Some(options) = self.deferred_deinitialization {
            let mut output = io::stdout().lock();
            (|| {
                output.write_all(b"\r\x1b[K")?;
                if options.keypad == KeypadMode::Application { output.write_all(b"\x1b[?1l\x1b>")?; }
                if options.screen == ScreenMode::Alternate { output.write_all(b"\x1b[?1049l")?; }
                output.flush()
            })()
        } else { Ok(()) };
        match (restored, deinitialized) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(cleanup)) => Err(io::Error::other(format!("{error}; deferred terminal deinitialization: {cleanup}"))),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        restore_terminal();
        for &(number, handler) in &self.previous_handlers {
            // SAFETY: these dispositions were returned by signal for the same signal numbers.
            unsafe { signal(number, handler); }
        }
        SESSION_ACTIVE.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_bell_follows_wall_clock_seconds() {
        let rate = BellRate(AtomicU64::new(0));
        let at = |second| std::time::UNIX_EPOCH + std::time::Duration::from_secs(second);
        assert!(rate.request(at(10)));
        assert!(!rate.request(at(10)));
        assert!(rate.request(at(11)));
        assert!(rate.request(at(9)));
    }
}
