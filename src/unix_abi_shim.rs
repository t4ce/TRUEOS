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
    open_file_poll_events,
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
const TRUEOS_FIONBIO: usize = 0x5421;
const TRUEOS_FIONCLEX: usize = 0x5450;
const TRUEOS_FIOCLEX: usize = 0x5451;
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

const _: () = assert!(core::mem::size_of::<PollFd>() == 8);
const _: () = assert!(core::mem::align_of::<PollFd>() == 4);

fn terminal_event_wait_eligible(pollfds: &[PollFd]) -> bool {
    let table = OPEN_FILES.lock();
    let mut waits_for_stdin = false;
    for pollfd in pollfds {
        if pollfd.fd < 0 {
            continue;
        }
        if pollfd.fd == 0 {
            waits_for_stdin |= pollfd.events & TRUEOS_POLLIN != 0;
            continue;
        }
        if (1..=2).contains(&pollfd.fd) {
            continue;
        }
        if !matches!(
            table.get(pollfd.fd),
            Some(OpenFile::PipeRead { .. } | OpenFile::PipeWrite { .. })
        ) {
            return false;
        }
    }
    waits_for_stdin
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
pub unsafe extern "C" fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int {
    if nfds > crate::allcaps::io::DESCRIPTOR_SOFT_CAP {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if nfds != 0 && fds.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    let Some(pollfd_bytes) = nfds.checked_mul(core::mem::size_of::<PollFd>()) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let Some(pollfds) = abi_write_bytes(fds.cast::<u8>(), pollfd_bytes) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let pollfds = unsafe { slice::from_raw_parts_mut(pollfds.as_mut_ptr().cast::<PollFd>(), nfds) };

    // Crossterm's legacy blocking source retains its typed terminal wake. All
    // other Blueprint descriptor sets rendezvous on the VM-local I/O generation.
    let terminal_event_wait = terminal_event_wait_eligible(pollfds);
    if terminal_event_wait {
        crate::r::io::fs_cabi::claim_attached_console_for_terminal_io();
    }
    let blueprint_wait = !terminal_event_wait
        && (crate::hv::current_hull_guest_context_vm_id().is_some()
            || crate::hv::current_guest_execution_context_vm_id().is_some());
    let deadline_ns = (timeout >= 0).then(|| {
        crate::r::platform::trueos_platform_monotonic_nanos()
            .saturating_add((timeout as u64).saturating_mul(1_000_000))
    });
    let mut woke_without_ready = false;

    loop {
        // Observe before probing so a producer racing the scan cannot lose an edge.
        let observed = blueprint_wait.then(|| {
            crate::r::platform::trueos_tokio_platform_wait_observe(
                crate::wait::BLUEPRINT_IO_WAIT_KEY,
            )
        });

        let ready = {
            let mut ready = 0;
            for pollfd in pollfds.iter_mut() {
                pollfd.revents = 0;
                if pollfd.fd < 0 {
                    continue;
                }
                let mut revents = 0;
                let file_revents = {
                    let table = OPEN_FILES.lock();
                    table
                        .get(pollfd.fd)
                        .map(|file| open_file_poll_events(file, pollfd.events))
                };
                if let Some(file_revents) = file_revents {
                    revents = file_revents;
                } else if let Some(socket_revents) =
                    crate::std_abi_shim::socket_poll_events(pollfd.fd, pollfd.events)
                {
                    revents = socket_revents;
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
            ready
        };

        if ready != 0 {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            return ready;
        }

        if terminal_event_wait {
            let wait_ms = match deadline_ns {
                Some(deadline) => {
                    let now = crate::r::platform::trueos_platform_monotonic_nanos();
                    if now >= deadline {
                        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                        return 0;
                    }
                    deadline.saturating_sub(now).div_ceil(1_000_000).max(1)
                }
                None => 10_000,
            };
            let woke = crate::r::io::fs_cabi::wait_attached_console_readable(wait_ms);
            if woke && crate::r::io::fs_cabi::trueos_cabi_shell_attached_readable_len() != 0 {
                continue;
            }
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            return 0;
        }

        if timeout == 0 || woke_without_ready {
            // A readiness-generation wake with no fd event is intentionally a
            // spurious poll return. Typed terminal/control state can then be
            // observed by its userspace source without inventing a fake byte.
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            return 0;
        }

        if blueprint_wait {
            let wait_ms = match deadline_ns {
                Some(deadline) => {
                    let now = crate::r::platform::trueos_platform_monotonic_nanos();
                    if now >= deadline {
                        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                        return 0;
                    }
                    deadline.saturating_sub(now).div_ceil(1_000_000).max(1)
                }
                None => u64::MAX,
            };
            woke_without_ready = crate::r::platform::trueos_tokio_platform_wait_after(
                crate::wait::BLUEPRINT_IO_WAIT_KEY,
                observed.unwrap_or_default(),
                wait_ms,
            );
            if !woke_without_ready {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                return 0;
            }
            continue;
        }

        let sleep_ms = match deadline_ns {
            Some(deadline) => {
                let now = crate::r::platform::trueos_platform_monotonic_nanos();
                if now >= deadline {
                    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                    return 0;
                }
                deadline
                    .saturating_sub(now)
                    .div_ceil(1_000_000)
                    .min(10)
                    .max(1)
            }
            None => 10,
        };
        crate::r::io::fs_cabi::trueos_cabi_sleep_ms(sleep_ms);
    }
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
            let mut cols = 80u32;
            let mut rows = 25u32;
            let _ = crate::r::io::fs_cabi::trueos_cabi_konsole_size(&mut cols, &mut rows);
            let winsize = TrueosWinsize {
                ws_row: rows.max(1).min(u16::MAX as u32) as u16,
                ws_col: cols.max(1).min(u16::MAX as u32) as u16,
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
        TRUEOS_FIONBIO => {
            let Some(value) = abi_read_bytes(argp.cast::<u8>(), core::mem::size_of::<c_int>())
            else {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            };
            let nonblocking = c_int::from_ne_bytes([value[0], value[1], value[2], value[3]]) != 0;
            match crate::std_abi_shim::socket_set_nonblocking(fd, nonblocking) {
                Ok(()) => {
                    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                    0
                }
                Err(errno) => {
                    TRUEOS_ERRNO.store(errno, Ordering::Relaxed);
                    -1
                }
            }
        }
        TRUEOS_FIONCLEX | TRUEOS_FIOCLEX => {
            // Rust's Unix fd layer issues these argumentless ioctls when a
            // newly-created socket cannot request SOCK_CLOEXEC atomically.
            // Route them through the same descriptor-flag state as fcntl.
            let cloexec = c_int::from(request == TRUEOS_FIOCLEX);
            unsafe { crate::std_abi_shim::fcntl(fd, 2, cloexec) }
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
    if fd == 0 {
        // Raw-mode entry precedes alternate-screen rendering in Crossterm and
        // most native TUIs.  Claim here so their very first frame, not merely
        // their first key read, is routed through the direct Shell2 surface.
        crate::r::io::fs_cabi::claim_attached_console_for_terminal_io();
    }
    STD_TERMIOS.lock()[fd as usize].copy_from_slice(input);
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}
