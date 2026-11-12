use std::io;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct PollFd { fd: i32, events: i16, revents: i16 }

#[cfg(target_os = "linux")]
const POLLIN: i16 = 0x0001;

#[cfg(target_os = "macos")]
#[repr(C)]
struct FdSet { bits: [i32; 32] }

#[cfg(target_os = "macos")]
#[repr(C)]
struct Timeval { seconds: i64, microseconds: i32 }

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn select(nfds: i32, readfds: *mut FdSet, writefds: *mut FdSet, exceptfds: *mut FdSet, timeout: *mut Timeval) -> i32;
}

#[cfg(target_os = "linux")]
pub(crate) fn readable(fd: i32, timeout_ms: i32) -> io::Result<bool> {
        let mut descriptor = PollFd { fd, events: POLLIN, revents: 0 };
        // SAFETY: descriptor is initialized and valid for one pollfd; poll does not retain the pointer.
        let result = unsafe { poll(&mut descriptor, 1, timeout_ms) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted { return Ok(false); }
            return Err(error);
        }
        if descriptor.revents & 0x20 != 0 { return Err(io::Error::other("invalid input descriptor")); }
        Ok(result > 0 && descriptor.revents & (POLLIN | 0x18) != 0)
}

#[cfg(target_os = "macos")]
pub(crate) fn readable(fd: i32, timeout_ms: i32) -> io::Result<bool> {
        // Darwin poll reports POLLNVAL for /dev/tty; select supports that device.
        if !(0..1024).contains(&fd) { return Err(io::Error::other("input descriptor exceeds Darwin FD_SETSIZE")); }
        let mut readfds = FdSet { bits: [0; 32] };
        readfds.bits[fd as usize / 32] |= 1 << (fd % 32);
        let mut timeout = Timeval { seconds: i64::from(timeout_ms / 1000), microseconds: (timeout_ms % 1000) * 1000 };
        let timeout = if timeout_ms < 0 { std::ptr::null_mut() } else { &mut timeout };
        // SAFETY: fd fits the platform fd_set, optional fd_sets are null, and timeout has Darwin timeval layout.
        let result = unsafe { select(fd + 1, &mut readfds, std::ptr::null_mut(), std::ptr::null_mut(), timeout) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted { return Ok(false); }
            return Err(error);
        }
        Ok(result > 0)
}
