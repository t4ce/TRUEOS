extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ffi::{c_int, c_void};
use core::slice;
use core::sync::atomic::Ordering;
use spin::Mutex;

use crate::std_abi_shim::{
    BytePipe, OPEN_FILES, OpenFile, TRUEOS_EAGAIN, TRUEOS_EBADF, TRUEOS_EINVAL, TRUEOS_ENOTTY,
    TRUEOS_ERRNO, abi_read_bytes, abi_write_bytes, copy_to_abi_out, next_file_fd,
    open_file_read_ready, open_file_write_ready,
};

const TRUEOS_AF_UNIX: c_int = 1;
const TRUEOS_SOCK_STREAM: c_int = 1;
const TRUEOS_SOCK_NONBLOCK: c_int = 0o4000;
const TRUEOS_SOCK_CLOEXEC: c_int = 0o2000000;
const TRUEOS_POLLIN: i16 = 0x0001;
const TRUEOS_POLLOUT: i16 = 0x0004;
const TRUEOS_POLLNVAL: i16 = 0x0020;
const TRUEOS_TCGETS: usize = 0x5401;
const TRUEOS_TCSETS: usize = 0x5402;
const TRUEOS_TCSETSW: usize = 0x5403;
const TRUEOS_TCSETSF: usize = 0x5404;
const TRUEOS_TIOCGWINSZ: usize = 0x5413;
const TRUEOS_FIONREAD: usize = 0x541b;
const TRUEOS_TERMIOS_BYTES: usize = 64;
const TRUEOS_TERMIOS_IFLAG_OFFSET: usize = 0;
const TRUEOS_TERMIOS_OFLAG_OFFSET: usize = 4;
const TRUEOS_TERMIOS_CFLAG_OFFSET: usize = 8;
const TRUEOS_TERMIOS_LFLAG_OFFSET: usize = 12;
const TRUEOS_TERMIOS_CC_OFFSET: usize = 17;
const TRUEOS_VTIME: usize = 5;
const TRUEOS_VMIN: usize = 6;
const TRUEOS_IGNBRK: u32 = 0o000001;
const TRUEOS_BRKINT: u32 = 0o000002;
const TRUEOS_PARMRK: u32 = 0o000010;
const TRUEOS_ISTRIP: u32 = 0o000040;
const TRUEOS_INLCR: u32 = 0o000100;
const TRUEOS_IGNCR: u32 = 0o000200;
const TRUEOS_ICRNL: u32 = 0o000400;
const TRUEOS_IXON: u32 = 0o002000;
const TRUEOS_OPOST: u32 = 0o000001;
const TRUEOS_ISIG: u32 = 0o000001;
const TRUEOS_ICANON: u32 = 0o000002;
const TRUEOS_ECHO: u32 = 0o000010;
const TRUEOS_ECHONL: u32 = 0o000100;
const TRUEOS_IEXTEN: u32 = 0o100000;
const TRUEOS_CSIZE: u32 = 0o000060;
const TRUEOS_CS8: u32 = 0o000060;
const TRUEOS_PARENB: u32 = 0o000400;

static STD_TERMIOS: Mutex<[[u8; TRUEOS_TERMIOS_BYTES]; 3]> =
    Mutex::new([[0; TRUEOS_TERMIOS_BYTES]; 3]);

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

fn read_termios_word(termios: &[u8; TRUEOS_TERMIOS_BYTES], offset: usize) -> u32 {
    u32::from_ne_bytes([
        termios[offset],
        termios[offset + 1],
        termios[offset + 2],
        termios[offset + 3],
    ])
}

fn write_termios_word(termios: &mut [u8; TRUEOS_TERMIOS_BYTES], offset: usize, value: u32) {
    termios[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
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
                    flags: 0,
                },
            )
            .is_err()
            || table
                .insert(
                    write_fd,
                    OpenFile::PipeWrite {
                        pipe: Arc::clone(&pipe),
                        flags: 0,
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
    let requested_flags = socket_type & (TRUEOS_SOCK_NONBLOCK | TRUEOS_SOCK_CLOEXEC);
    let base_socket_type = socket_type & !(TRUEOS_SOCK_NONBLOCK | TRUEOS_SOCK_CLOEXEC);
    if domain != TRUEOS_AF_UNIX || base_socket_type != TRUEOS_SOCK_STREAM || protocol != 0 {
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
                    flags: requested_flags & TRUEOS_SOCK_NONBLOCK,
                },
            )
            .is_err()
            || table
                .insert(
                    right_fd,
                    OpenFile::UnixSocket {
                        rx: Arc::clone(&left_to_right),
                        tx: Arc::clone(&right_to_left),
                        flags: requested_flags & TRUEOS_SOCK_NONBLOCK,
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
            if pollfd.fd == 0
                && pollfd.events & TRUEOS_POLLIN != 0
                && crate::r::io::fs_cabi::trueos_cabi_shell_attached_readable_len() != 0
            {
                revents |= TRUEOS_POLLIN;
            }
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
    } else if fd >= 0 && OPEN_FILES.lock().get(fd).is_some() {
        TRUEOS_ERRNO.store(TRUEOS_ENOTTY, Ordering::Relaxed);
        0
    } else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioctl(fd: c_int, request: usize, argp: *mut c_void) -> c_int {
    match request {
        TRUEOS_TIOCGWINSZ => {
            if !(0..=2).contains(&fd) {
                TRUEOS_ERRNO.store(
                    if fd >= 0 && OPEN_FILES.lock().get(fd).is_some() {
                        TRUEOS_ENOTTY
                    } else {
                        TRUEOS_EBADF
                    },
                    Ordering::Relaxed,
                );
                return -1;
            }
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
            let available = if fd == 0 {
                core::cmp::min(
                    crate::r::io::fs_cabi::trueos_cabi_shell_attached_readable_len(),
                    i32::MAX as usize,
                ) as i32
            } else if (1..=2).contains(&fd) {
                0
            } else {
                let table = OPEN_FILES.lock();
                let Some(file) = table.get(fd) else {
                    TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                    return -1;
                };
                core::cmp::min(file.readable_len(), i32::MAX as usize) as i32
            };
            if !copy_to_abi_out(argp.cast::<u8>(), &available.to_ne_bytes()) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        TRUEOS_TCGETS => unsafe { tcgetattr(fd, argp) },
        TRUEOS_TCSETS | TRUEOS_TCSETSW | TRUEOS_TCSETSF => unsafe {
            tcsetattr(fd, 0, argp.cast_const())
        },
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_ENOTTY, Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tcgetattr(fd: c_int, termios_p: *mut c_void) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(
            if fd >= 0 && OPEN_FILES.lock().get(fd).is_some() {
                TRUEOS_ENOTTY
            } else {
                TRUEOS_EBADF
            },
            Ordering::Relaxed,
        );
        return -1;
    }
    if termios_p.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let termios = STD_TERMIOS.lock();
    if !copy_to_abi_out(termios_p.cast::<u8>(), &termios[fd as usize]) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn cfmakeraw(termios_p: *mut c_void) {
    if termios_p.is_null() {
        return;
    }
    let Some(input) = abi_read_bytes(termios_p.cast::<u8>(), TRUEOS_TERMIOS_BYTES) else {
        return;
    };
    let mut termios = [0u8; TRUEOS_TERMIOS_BYTES];
    termios.copy_from_slice(input);

    let iflag = read_termios_word(&termios, TRUEOS_TERMIOS_IFLAG_OFFSET)
        & !(TRUEOS_IGNBRK
            | TRUEOS_BRKINT
            | TRUEOS_PARMRK
            | TRUEOS_ISTRIP
            | TRUEOS_INLCR
            | TRUEOS_IGNCR
            | TRUEOS_ICRNL
            | TRUEOS_IXON);
    let oflag = read_termios_word(&termios, TRUEOS_TERMIOS_OFLAG_OFFSET) & !TRUEOS_OPOST;
    let cflag = (read_termios_word(&termios, TRUEOS_TERMIOS_CFLAG_OFFSET)
        & !(TRUEOS_CSIZE | TRUEOS_PARENB))
        | TRUEOS_CS8;
    let lflag = read_termios_word(&termios, TRUEOS_TERMIOS_LFLAG_OFFSET)
        & !(TRUEOS_ECHO | TRUEOS_ECHONL | TRUEOS_ICANON | TRUEOS_ISIG | TRUEOS_IEXTEN);

    write_termios_word(&mut termios, TRUEOS_TERMIOS_IFLAG_OFFSET, iflag);
    write_termios_word(&mut termios, TRUEOS_TERMIOS_OFLAG_OFFSET, oflag);
    write_termios_word(&mut termios, TRUEOS_TERMIOS_CFLAG_OFFSET, cflag);
    write_termios_word(&mut termios, TRUEOS_TERMIOS_LFLAG_OFFSET, lflag);
    termios[TRUEOS_TERMIOS_CC_OFFSET + TRUEOS_VMIN] = 1;
    termios[TRUEOS_TERMIOS_CC_OFFSET + TRUEOS_VTIME] = 0;

    let _ = copy_to_abi_out(termios_p.cast::<u8>(), &termios);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tcsetattr(
    fd: c_int,
    _optional_actions: c_int,
    termios_p: *const c_void,
) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(
            if fd >= 0 && OPEN_FILES.lock().get(fd).is_some() {
                TRUEOS_ENOTTY
            } else {
                TRUEOS_EBADF
            },
            Ordering::Relaxed,
        );
        return -1;
    }
    if termios_p.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let Some(input) = abi_read_bytes(termios_p.cast::<u8>(), TRUEOS_TERMIOS_BYTES) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    STD_TERMIOS.lock()[fd as usize].copy_from_slice(input);
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}
