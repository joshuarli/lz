use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io;

const MAX_SPILL: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub(super) struct Limit(u64);
impl Default for Limit { fn default() -> Self { Self(MAX_SPILL) } }
impl TryFrom<u64> for Limit {
    type Error = String;
    fn try_from(bytes: u64) -> Result<Self, Self::Error> {
        if bytes == 0 || bytes > MAX_SPILL { return Err(format!("spill limit must be in 1..={MAX_SPILL} bytes")); }
        Ok(Self(bytes))
    }
}
impl From<Limit> for u64 { fn from(limit: Limit) -> Self { limit.0 } }

#[cfg(target_os = "linux")]
pub(super) fn retained_bytes(pid: u32) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    let directory = format!("/proc/{pid}/fd");
    let descriptors = match std::fs::read_dir(&directory) {
        Ok(descriptors) => descriptors,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for descriptor in descriptors {
        let descriptor = descriptor?;
        let path = descriptor.path();
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !metadata.is_file() || metadata.nlink() != 0 { continue; }
        let information = match std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{}", descriptor.file_name().to_string_lossy())) {
            Ok(information) => information,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let flags = information.lines().find_map(|line| line.strip_prefix("flags:")).ok_or_else(|| io::Error::other("missing fd access flags"))?;
        let flags = u64::from_str_radix(flags.trim(), 8).map_err(io::Error::other)?;
        if flags & libc::O_ACCMODE as u64 == libc::O_RDONLY as u64 { continue; }
        let current = match std::fs::metadata(&path) {
            Ok(current) => current,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !current.is_file() || current.nlink() != 0 || (metadata.dev(), metadata.ino()) != (current.dev(), current.ino()) { continue; }
        if seen.insert((current.dev(), current.ino())) {
            total = total.checked_add(current.len()).ok_or_else(|| io::Error::other("spill byte count overflow"))?;
        }
    }
    Ok(total)
}

// Darwin PROC_PIDFDVNODEINFO uses the LP64 sys/proc_info.h vnode_fdinfo layout.
#[cfg(target_os = "macos")]
#[repr(C)]
struct VnodeFdInfo {
    open_flags: u32,
    status: u32,
    offset: i64,
    file_type: i32,
    guard_flags: u32,
    device: u32,
    mode: u16,
    links: u16,
    inode: u64,
    uid: u32,
    gid: u32,
    times: [i64; 8],
    size: i64,
    blocks: i64,
    block_size: i32,
    node_flags: u32,
    generation: u32,
    rdev: u32,
    spare: [i64; 2],
    vnode_type: i32,
    padding: i32,
    fsid: [i32; 2],
}

#[cfg(target_os = "macos")]
pub(super) fn retained_bytes(pid: u32) -> io::Result<u64> {
    let mut descriptors = Vec::new();
    for capacity in [256, 1024, 8192, 65536] {
        descriptors.resize(capacity, libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 });
        let bytes = std::mem::size_of_val(descriptors.as_slice());
        // SAFETY: __error returns this thread's writable errno cell; zero distinguishes an empty fd table from an API error.
        unsafe { *libc::__error() = 0; }
        // SAFETY: pid is an owned unreaped child; descriptors owns the complete writable byte capacity.
        let count = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, descriptors.as_mut_ptr().cast(), bytes as i32) };
        if count <= 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(0 | libc::ESRCH)) { return Ok(0); }
            return Err(error);
        }
        if count as usize == bytes { continue; }
        if count as usize > bytes || !(count as usize).is_multiple_of(std::mem::size_of::<libc::proc_fdinfo>()) {
            return Err(io::Error::other("invalid Darwin descriptor-list byte count"));
        }
        descriptors.truncate(count as usize / std::mem::size_of::<libc::proc_fdinfo>());
        let mut seen = BTreeSet::new();
        let mut total = 0u64;
        for descriptor in &descriptors {
            if descriptor.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 { continue; }
            // SAFETY: the integer-only repr(C) record is valid when zeroed and has the SDK's vnode_fdinfo layout.
            let mut information: VnodeFdInfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<VnodeFdInfo>();
            // SAFETY: flavor 1 is PROC_PIDFDVNODEINFO; the API receives the entire writable record and retains no pointer.
            let count = unsafe { libc::proc_pidfdinfo(pid as i32, descriptor.proc_fd, 1, (&mut information as *mut VnodeFdInfo).cast(), size as i32) };
            if count <= 0 {
                let error = io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(libc::EBADF | libc::ESRCH)) { continue; }
                return Err(error);
            }
            if count as usize != size { return Err(io::Error::other("Darwin vnode record size differs from the SDK ABI")); }
            // Darwin's kernel FWRITE flag is 2; it differs from user-facing O_WRONLY.
            if information.open_flags & 2 == 0 || information.links != 0 || information.mode & libc::S_IFMT != libc::S_IFREG { continue; }
            let length = u64::try_from(information.size).map_err(io::Error::other)?;
            if seen.insert((information.device, information.inode)) {
                total = total.checked_add(length).ok_or_else(|| io::Error::other("spill byte count overflow"))?;
            }
        }
        return Ok(total);
    }
    Err(io::Error::other("owned child exceeded the bounded Darwin descriptor scan"))
}
