extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ffi::{c_int, c_void};
use core::slice;
use core::sync::atomic::Ordering;
use spin::Mutex;

use crate::std_abi_shim::{
    BytePipe, OPEN_FILES, OpenFile, TRUEOS_EAGAIN, TRUEOS_EBADF, TRUEOS_EINVAL, TRUEOS_ENOTTY,
    TRUEOS_ERRNO, abi_write_bytes, copy_to_abi_out, next_file_fd, open_file_read_ready,
    open_file_write_ready,
};

const TRUEOS_AF_UNIX: c_int = 1;
const TRUEOS_SOCK_STREAM: c_int = 1;
const TRUEOS_POLLIN: i16 = 0x0001;
const TRUEOS_POLLOUT: i16 = 0x0004;
const TRUEOS_POLLNVAL: i16 = 0x0020;
const TRUEOS_TCGETS: usize = 0x5401;
const TRUEOS_TIOCGWINSZ: usize = 0x5413;
const TRUEOS_FIONREAD: usize = 0x541b;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrueosWinsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pipe(fds: *mut c_int) -> c_int {
    if fds.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let read_fd = next_file_fd();
    let write_fd = next_file_fd();
    let pipe = Arc::new(Mutex::new(BytePipe {
        bytes: Vec::new(),
        read_open: true,
        write_open: true,
    }));
    {
        let mut table = OPEN_FILES.lock();
        if table
            .insert(
                read_fd,
                OpenFile::PipeRead {
                    pipe: Arc::clone(&pipe),
                },
            )
            .is_err()
            || table
                .insert(
                    write_fd,
                    OpenFile::PipeWrite {
                        pipe: Arc::clone(&pipe),
                    },
                )
                .is_err()
        {
            let _ = table.remove(read_fd);
            let _ = table.remove(write_fd);
            TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
            return -1;
        }
    }
    if !copy_to_abi_out(fds.cast::<u8>(), &read_fd.to_ne_bytes())
        || !copy_to_abi_out(unsafe { fds.add(1) }.cast::<u8>(), &write_fd.to_ne_bytes())
    {
        let mut table = OPEN_FILES.lock();
        let _ = table.remove(read_fd);
        let _ = table.remove(write_fd);
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn socketpair(
    domain: c_int,
    socket_type: c_int,
    protocol: c_int,
    sv: *mut c_int,
) -> c_int {
    if sv.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if domain != TRUEOS_AF_UNIX || socket_type != TRUEOS_SOCK_STREAM || protocol != 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let left_fd = next_file_fd();
    let right_fd = next_file_fd();
    let left_to_right = Arc::new(Mutex::new(BytePipe {
        bytes: Vec::new(),
        read_open: true,
        write_open: true,
    }));
    let right_to_left = Arc::new(Mutex::new(BytePipe {
        bytes: Vec::new(),
        read_open: true,
        write_open: true,
    }));
    {
        let mut table = OPEN_FILES.lock();
        if table
            .insert(
                left_fd,
                OpenFile::UnixSocket {
                    rx: Arc::clone(&right_to_left),
                    tx: Arc::clone(&left_to_right),
                },
            )
            .is_err()
            || table
                .insert(
                    right_fd,
                    OpenFile::UnixSocket {
                        rx: Arc::clone(&left_to_right),
                        tx: Arc::clone(&right_to_left),
                    },
                )
                .is_err()
        {
            let _ = table.remove(left_fd);
            let _ = table.remove(right_fd);
            TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
            return -1;
        }
    }
    if !copy_to_abi_out(sv.cast::<u8>(), &left_fd.to_ne_bytes())
        || !copy_to_abi_out(unsafe { sv.add(1) }.cast::<u8>(), &right_fd.to_ne_bytes())
    {
        let mut table = OPEN_FILES.lock();
        let _ = table.remove(left_fd);
        let _ = table.remove(right_fd);
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn poll(fds: *mut PollFd, nfds: usize, _timeout: c_int) -> c_int {
    if nfds != 0 && fds.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    let Some(pollfds) =
        abi_write_bytes(fds.cast::<u8>(), nfds.saturating_mul(core::mem::size_of::<PollFd>()))
    else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let pollfds = unsafe { slice::from_raw_parts_mut(pollfds.as_mut_ptr().cast::<PollFd>(), nfds) };

    let table = OPEN_FILES.lock();
    let mut ready = 0;
    for pollfd in pollfds.iter_mut() {
        pollfd.revents = 0;
        if pollfd.fd < 0 {
            continue;
        }
        let mut revents = 0;
        if let Some(file) = table.get(pollfd.fd) {
            if pollfd.events & TRUEOS_POLLIN != 0 && open_file_read_ready(file) {
                revents |= TRUEOS_POLLIN;
            }
            if pollfd.events & TRUEOS_POLLOUT != 0 && open_file_write_ready(file) {
                revents |= TRUEOS_POLLOUT;
            }
        } else if (0..=2).contains(&pollfd.fd) {
            if pollfd.events & TRUEOS_POLLOUT != 0 {
                revents |= TRUEOS_POLLOUT;
            }
        } else {
            revents |= TRUEOS_POLLNVAL;
        }
        pollfd.revents = revents;
        if revents != 0 {
            ready += 1;
        }
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    ready
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn isatty(fd: c_int) -> c_int {
    if (0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        1
    } else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioctl(fd: c_int, request: usize, argp: *mut c_void) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    match request {
        TRUEOS_TIOCGWINSZ => {
            if argp.is_null() {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            let winsize = TrueosWinsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if !copy_to_abi_out(argp.cast::<u8>(), unsafe {
                slice::from_raw_parts(
                    (&winsize as *const TrueosWinsize).cast::<u8>(),
                    core::mem::size_of::<TrueosWinsize>(),
                )
            }) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        TRUEOS_FIONREAD => {
            if argp.is_null() {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            if !copy_to_abi_out(argp.cast::<u8>(), &0i32.to_ne_bytes()) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        TRUEOS_TCGETS => unsafe { tcgetattr(fd, argp) },
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_ENOTTY, Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tcgetattr(fd: c_int, termios_p: *mut c_void) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    if termios_p.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let zeros = [0u8; 64];
    if !copy_to_abi_out(termios_p.cast::<u8>(), &zeros) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tcsetattr(
    fd: c_int,
    _optional_actions: c_int,
    termios_p: *const c_void,
) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    if termios_p.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}
