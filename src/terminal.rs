use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

extern "C" {
    fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, termios: *const Termios) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
    fn raise(sig: i32) -> i32;
}

const STDOUT_FD: i32 = 1;
const TCSAFLUSH: i32 = 2;

#[cfg(target_os = "macos")]
const TIOCGWINSZ: u64 = 0x40087468;
#[cfg(target_os = "linux")]
const TIOCGWINSZ: u64 = 0x5413;

const SIGWINCH: i32 = 28;
const SIGTSTP: i32 = 18;
const SIGCONT: i32 = 19;
const SIGTERM: i32 = 15;
#[cfg(target_os = "macos")]
const SIGSTOP: i32 = 17;
#[cfg(target_os = "linux")]
const SIGSTOP: i32 = 19;

const SIG_DFL: usize = 0;

pub static WINCH_FLAG: AtomicBool = AtomicBool::new(false);
pub static TSTP_FLAG: AtomicBool = AtomicBool::new(false);
pub static CONT_FLAG: AtomicBool = AtomicBool::new(false);
pub static TERM_FLAG: AtomicBool = AtomicBool::new(false);

/// The fd we set raw mode on (the /dev/tty fd).
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

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

static mut ORIG_TERMIOS: Option<Termios> = None;

extern "C" fn handle_winch(_sig: i32) {
    WINCH_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn handle_tstp(_sig: i32) {
    TSTP_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn handle_cont(_sig: i32) {
    CONT_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn handle_term(_sig: i32) {
    TERM_FLAG.store(true, Ordering::SeqCst);
}

pub fn install_signal_handlers() {
    unsafe {
        signal(SIGWINCH, handle_winch as *const () as usize);
        signal(SIGTSTP, handle_tstp as *const () as usize);
        signal(SIGCONT, handle_cont as *const () as usize);
        signal(SIGTERM, handle_term as *const () as usize);
    }
}

/// Enable raw mode on the given fd (should be the /dev/tty fd).
pub fn enable_raw_mode(fd: i32) -> io::Result<()> {
    TTY_FD.store(fd, Ordering::SeqCst);
    unsafe {
        let mut raw: Termios = std::mem::zeroed();
        if tcgetattr(fd, &mut raw) != 0 {
            return Err(io::Error::last_os_error());
        }
        ORIG_TERMIOS = Some(raw);

        // Input flags: disable BRKINT, ICRNL, INPCK, ISTRIP, IXON
        raw.c_iflag &= !(0x02 | 0x100 | 0x10 | 0x20 | 0x200);
        // Output flags: disable OPOST
        raw.c_oflag &= !0x01;
        // Control flags: set CS8
        raw.c_cflag |= 0x300;
        // Local flags: disable ECHO, ICANON, IEXTEN, ISIG
        raw.c_lflag &= !(0x08 | 0x100 | 0x400 | 0x80);
        // VMIN = 1, VTIME = 0
        raw.c_cc[16] = 1;
        raw.c_cc[17] = 0;

        if tcsetattr(fd, TCSAFLUSH, &raw) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn disable_raw_mode() {
    let fd = TTY_FD.load(Ordering::SeqCst);
    if fd < 0 {
        return;
    }
    unsafe {
        if let Some(ref orig) = ORIG_TERMIOS {
            tcsetattr(fd, TCSAFLUSH, orig);
        }
    }
}

pub fn enter_alt_screen(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[?1049h")?;
    w.flush()
}

pub fn leave_alt_screen(w: &mut impl Write) -> io::Result<()> {
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

pub fn handle_suspend() {
    disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = leave_alt_screen(&mut stdout);
    let _ = show_cursor(&mut stdout);

    unsafe {
        signal(SIGTSTP, SIG_DFL);
        raise(SIGSTOP);
    }
}

pub fn handle_resume() {
    let fd = TTY_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let _ = enable_raw_mode(fd);
    }
    let mut stdout = io::stdout();
    let _ = enter_alt_screen(&mut stdout);
    let _ = hide_cursor(&mut stdout);

    unsafe {
        signal(SIGTSTP, handle_tstp as *const () as usize);
    }
}

pub fn restore_terminal() {
    disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = leave_alt_screen(&mut stdout);
    let _ = show_cursor(&mut stdout);
}
