use core::arch::x86_64::_rdtsc;
use core::ffi::{c_char, c_void};

use embassy_time::{Duration as EmbassyDuration, Timer};

const PROBE_PATH: &[u8] = b"unix-fd-probe.bin\0";
const PROBE_PATH_STR: &str = "unix-fd-probe.bin";
const BLOCK_SEQ: &[u8] = b"TRUEOS kernel unix fd probe\n";
const BLOCK_ZERO: &[u8] = b"TRUEOS kernel pwrite block zero\n";
const BLOCK_SPARSE: &[u8] = b"TRUEOS kernel offset 4096";

const O_RDWR: i32 = 0o2;
const O_CREAT: i32 = 0o100;
const O_TRUNC: i32 = 0o1000;
const SEEK_SET: i32 = 0;
const F_GETLK: i32 = 5;
const F_SETLK: i32 = 6;
const AF_UNIX: i32 = 1;
const SOCK_STREAM: i32 = 1;
const SOCK_NONBLOCK: i32 = 0o4000;
const SOCK_CLOEXEC: i32 = 0o2000000;
const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 0o4000;
const POLLIN: i16 = 0x0001;
const POLLOUT: i16 = 0x0004;
const TCSANOW: i32 = 0;
const TCGETS: usize = 0x5401;
const TIOCGWINSZ: usize = 0x5413;
const FIONREAD: usize = 0x541b;
const EAGAIN: i32 = 11;
const EBADF: i32 = 9;
const ENOTTY: i32 = 25;

#[repr(C)]
#[derive(Clone, Copy)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[inline]
fn errno() -> i32 {
    unsafe { *crate::std_abi_shim::__errno_location() }
}

fn log_stage(stage: &str) {
    crate::log!("unix-fd-probe: stage {}\n", stage);
}

fn log_fd(stage: &str, fd: i32) -> bool {
    if fd >= 0 {
        crate::log!("unix-fd-probe: success {} fd={}\n", stage, fd);
        true
    } else {
        crate::log!("unix-fd-probe: failed {} fd={} errno={}\n", stage, fd, errno());
        false
    }
}

fn log_rc(stage: &str, rc: i32) -> bool {
    if rc == 0 {
        crate::log!("unix-fd-probe: success {} rc={}\n", stage, rc);
        true
    } else {
        crate::log!("unix-fd-probe: failed {} rc={} errno={}\n", stage, rc, errno());
        false
    }
}

fn log_io(stage: &str, got: isize, expected: usize) -> bool {
    if got == expected as isize {
        crate::log!("unix-fd-probe: success {} bytes={} expected={}\n", stage, got, expected);
        true
    } else {
        crate::log!(
            "unix-fd-probe: failed {} bytes={} expected={} errno={}\n",
            stage,
            got,
            expected,
            errno()
        );
        false
    }
}

fn log_seek(stage: &str, got: isize, expected: isize) -> bool {
    if got == expected {
        crate::log!("unix-fd-probe: success {} offset={} expected={}\n", stage, got, expected);
        true
    } else {
        crate::log!(
            "unix-fd-probe: failed {} offset={} expected={} errno={}\n",
            stage,
            got,
            expected,
            errno()
        );
        false
    }
}

fn probe_cycles() -> u64 {
    unsafe { _rdtsc() }
}

fn probe_cycles_to_us(cycles: u64) -> u64 {
    let hz = crate::time::tsc_hz().max(1);
    ((cycles as u128) * 1_000_000u128 / (hz as u128)).min(u64::MAX as u128) as u64
}

fn log_api_stage(stage: &str) -> u64 {
    let started_cycles = probe_cycles();
    crate::log!("unix-api-probe: stage {} start_cycles={}\n", stage, started_cycles);
    started_cycles
}

fn log_api_check(stage: &str, ok: bool, started_cycles: u64) -> bool {
    let elapsed_cycles = probe_cycles().wrapping_sub(started_cycles);
    let elapsed_us = probe_cycles_to_us(elapsed_cycles);
    if ok {
        crate::log!(
            "unix-api-probe: success {} elapsed_cycles={} elapsed_us={} errno={}\n",
            stage,
            elapsed_cycles,
            elapsed_us,
            errno()
        );
    } else {
        crate::log!(
            "unix-api-probe: failed {} elapsed_cycles={} elapsed_us={} errno={}\n",
            stage,
            elapsed_cycles,
            elapsed_us,
            errno()
        );
    }
    ok
}

fn log_api_rc_zero(stage: &str, rc: i32) -> bool {
    if rc == 0 {
        crate::log!("unix-api-probe: success {} rc={} errno={}\n", stage, rc, errno());
        true
    } else {
        crate::log!("unix-api-probe: failed {} rc={} errno={}\n", stage, rc, errno());
        false
    }
}

fn log_api_rc_nonnegative(stage: &str, rc: i32) -> bool {
    if rc >= 0 {
        crate::log!("unix-api-probe: success {} rc={} errno={}\n", stage, rc, errno());
        true
    } else {
        crate::log!("unix-api-probe: failed {} rc={} errno={}\n", stage, rc, errno());
        false
    }
}

fn log_api_io(stage: &str, got: isize, expected: usize) -> bool {
    if got == expected as isize {
        crate::log!("unix-api-probe: success {} bytes={} expected={}\n", stage, got, expected);
        true
    } else {
        crate::log!(
            "unix-api-probe: failed {} bytes={} expected={} errno={}\n",
            stage,
            got,
            expected,
            errno()
        );
        false
    }
}

fn unix_api_fds_valid(stage: &str, fds: [i32; 2]) -> bool {
    let valid = fds[0] >= 0 && fds[1] >= 0;
    crate::log!(
        "unix-api-probe: {} fds=[{},{}] valid={} errno={}\n",
        stage,
        fds[0],
        fds[1],
        valid,
        errno()
    );
    valid
}

fn run_unix_api_probe_once() -> bool {
    let probe_started_cycles = probe_cycles();
    let mut ok = true;

    let started = log_api_stage("isatty.stdin.true");
    ok &= log_api_check(
        "isatty.stdin.true",
        unsafe { crate::unix_abi_shim::isatty(0) } == 1 && errno() == 0,
        started,
    );

    let started = log_api_stage("isatty.invalid.ebadf");
    ok &= log_api_check(
        "isatty.invalid.ebadf",
        unsafe { crate::unix_abi_shim::isatty(9999) } == 0 && errno() == EBADF,
        started,
    );

    let mut winsize = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let started = log_api_stage("ioctl.TIOCGWINSZ.stdout.nonzero");
    let winsize_rc = unsafe {
        crate::unix_abi_shim::ioctl(1, TIOCGWINSZ, (&mut winsize as *mut Winsize).cast())
    };
    let winsize_ok = winsize_rc == 0 && winsize.ws_row != 0 && winsize.ws_col != 0;
    ok &= log_api_check("ioctl.TIOCGWINSZ.stdout.nonzero", winsize_ok, started);
    ok &= winsize_ok;
    if winsize_ok {
        crate::log!(
            "unix-api-probe: proof winsize rows={} cols={} xpixel={} ypixel={}\n",
            winsize.ws_row,
            winsize.ws_col,
            winsize.ws_xpixel,
            winsize.ws_ypixel
        );
    }

    let mut termios = [0u8; 64];
    let started = log_api_stage("tcgetattr.stdin.snapshot");
    let tcgetattr_ok =
        unsafe { crate::unix_abi_shim::tcgetattr(0, termios.as_mut_ptr().cast()) } == 0;
    ok &= log_api_check("tcgetattr.stdin.snapshot", tcgetattr_ok, started);
    ok &= tcgetattr_ok;
    if tcgetattr_ok {
        let original = termios;
        let mut changed = original;
        changed[0] ^= 0x5a;
        changed[7] ^= 0xa5;

        let started = log_api_stage("tcsetattr.stdin.roundtrip.write");
        ok &= log_api_check(
            "tcsetattr.stdin.roundtrip.write",
            unsafe { crate::unix_abi_shim::tcsetattr(0, TCSANOW, changed.as_ptr().cast()) } == 0,
            started,
        );

        let mut observed = [0u8; 64];
        let started = log_api_stage("tcgetattr.stdin.roundtrip.readback");
        let readback_ok =
            unsafe { crate::unix_abi_shim::tcgetattr(0, observed.as_mut_ptr().cast()) } == 0
                && observed == changed;
        ok &= log_api_check("tcgetattr.stdin.roundtrip.readback", readback_ok, started);

        let started = log_api_stage("tcsetattr.stdin.restore");
        ok &= log_api_check(
            "tcsetattr.stdin.restore",
            unsafe { crate::unix_abi_shim::tcsetattr(0, TCSANOW, original.as_ptr().cast()) } == 0,
            started,
        );
    }

    let mut raw_termios = [0u8; 64];
    let started = log_api_stage("ioctl.TCGETS.stdin.snapshot");
    ok &= log_api_check(
        "ioctl.TCGETS.stdin.snapshot",
        unsafe { crate::unix_abi_shim::ioctl(0, TCGETS, raw_termios.as_mut_ptr().cast()) } == 0,
        started,
    );

    let mut pipe_fds = [-1, -1];
    let started = log_api_stage("pipe.create");
    if log_api_check(
        "pipe.create",
        unsafe { crate::unix_abi_shim::pipe(pipe_fds.as_mut_ptr()) } == 0
            && unix_api_fds_valid("pipe.create", pipe_fds),
        started,
    ) {
        let started = log_api_stage("isatty.pipe.enotty");
        ok &= log_api_check(
            "isatty.pipe.enotty",
            unsafe { crate::unix_abi_shim::isatty(pipe_fds[0]) } == 0 && errno() == ENOTTY,
            started,
        );

        let mut available = -1i32;
        let started = log_api_stage("ioctl.FIONREAD.pipe.empty");
        ok &= log_api_check(
            "ioctl.FIONREAD.pipe.empty",
            unsafe {
                crate::unix_abi_shim::ioctl(
                    pipe_fds[0],
                    FIONREAD,
                    (&mut available as *mut i32).cast(),
                )
            } == 0
                && available == 0,
            started,
        );

        let mut pollfds = [crate::unix_abi_shim::PollFd {
            fd: pipe_fds[0],
            events: POLLIN,
            revents: -1,
        }];
        let started = log_api_stage("poll.pipe.empty.not_ready");
        ok &= log_api_check(
            "poll.pipe.empty.not_ready",
            unsafe { crate::unix_abi_shim::poll(pollfds.as_mut_ptr(), pollfds.len(), 0) } == 0
                && pollfds[0].revents == 0,
            started,
        );

        let started = log_api_stage("fcntl.pipe.read.F_GETFL");
        let read_flags = unsafe { crate::std_abi_shim::fcntl(pipe_fds[0], F_GETFL, 0) };
        ok &= log_api_check("fcntl.pipe.read.F_GETFL", read_flags >= 0, started);
        if read_flags >= 0 {
            let started = log_api_stage("fcntl.pipe.read.F_SETFL_NONBLOCK");
            ok &= log_api_check(
                "fcntl.pipe.read.F_SETFL_NONBLOCK",
                unsafe {
                    crate::std_abi_shim::fcntl(pipe_fds[0], F_SETFL, read_flags | O_NONBLOCK)
                } == 0,
                started,
            );

            let started = log_api_stage("fcntl.pipe.read.F_GETFL_NONBLOCK");
            let flags_after = unsafe { crate::std_abi_shim::fcntl(pipe_fds[0], F_GETFL, 0) };
            ok &= log_api_check(
                "fcntl.pipe.read.F_GETFL_NONBLOCK",
                flags_after & O_NONBLOCK != 0,
                started,
            );
        }

        let mut empty_buf = [0u8; 1];
        let started = log_api_stage("read.pipe.empty.nonblock.eagain");
        ok &= log_api_check(
            "read.pipe.empty.nonblock.eagain",
            unsafe { crate::std_abi_shim::read(pipe_fds[0], empty_buf.as_mut_ptr().cast(), 1) }
                == -1
                && errno() == EAGAIN,
            started,
        );

        let payload = b"pipe-ok";
        let started = log_api_stage("write.pipe.payload");
        ok &= log_api_check(
            "write.pipe.payload",
            unsafe {
                crate::std_abi_shim::write(pipe_fds[1], payload.as_ptr().cast(), payload.len())
            } == payload.len() as isize,
            started,
        );

        available = -1;
        let started = log_api_stage("ioctl.FIONREAD.pipe.payload");
        ok &= log_api_check(
            "ioctl.FIONREAD.pipe.payload",
            unsafe {
                crate::unix_abi_shim::ioctl(
                    pipe_fds[0],
                    FIONREAD,
                    (&mut available as *mut i32).cast(),
                )
            } == 0
                && available == payload.len() as i32,
            started,
        );

        let mut pollfds = [crate::unix_abi_shim::PollFd {
            fd: pipe_fds[0],
            events: POLLIN,
            revents: 0,
        }];
        let started = log_api_stage("poll.pipe.readable");
        ok &= log_api_check(
            "poll.pipe.readable",
            unsafe { crate::unix_abi_shim::poll(pollfds.as_mut_ptr(), pollfds.len(), 0) } == 1
                && pollfds[0].revents & POLLIN != 0,
            started,
        );

        let mut buf = [0u8; 16];
        let started = log_api_stage("read.pipe.payload.content");
        ok &= log_api_check(
            "read.pipe.payload.content",
            unsafe {
                crate::std_abi_shim::read(pipe_fds[0], buf.as_mut_ptr().cast(), payload.len())
            } == payload.len() as isize
                && &buf[..payload.len()] == payload,
            started,
        );

        available = -1;
        let started = log_api_stage("ioctl.FIONREAD.pipe.drained");
        ok &= log_api_check(
            "ioctl.FIONREAD.pipe.drained",
            unsafe {
                crate::unix_abi_shim::ioctl(
                    pipe_fds[0],
                    FIONREAD,
                    (&mut available as *mut i32).cast(),
                )
            } == 0
                && available == 0,
            started,
        );

        let started = log_api_stage("close.pipe.write");
        ok &= log_api_check(
            "close.pipe.write",
            unsafe { crate::std_abi_shim::close(pipe_fds[1]) } == 0,
            started,
        );
        let started = log_api_stage("read.pipe.after_writer_close.eof");
        ok &= log_api_check(
            "read.pipe.after_writer_close.eof",
            unsafe { crate::std_abi_shim::read(pipe_fds[0], empty_buf.as_mut_ptr().cast(), 1) }
                == 0,
            started,
        );
        let started = log_api_stage("close.pipe.read");
        ok &= log_api_check(
            "close.pipe.read",
            unsafe { crate::std_abi_shim::close(pipe_fds[0]) } == 0,
            started,
        );
    } else {
        ok = false;
    }

    let mut socket_fds = [-1, -1];
    let started = log_api_stage("socketpair.AF_UNIX_STREAM.create");
    if log_api_check(
        "socketpair.AF_UNIX_STREAM.create",
        unsafe {
            crate::unix_abi_shim::socketpair(AF_UNIX, SOCK_STREAM, 0, socket_fds.as_mut_ptr())
        } == 0
            && unix_api_fds_valid("socketpair.AF_UNIX_STREAM.create", socket_fds),
        started,
    ) {
        let payload = b"unix-stream-ok";
        let started = log_api_stage("write.socketpair.right.payload");
        ok &= log_api_check(
            "write.socketpair.right.payload",
            unsafe {
                crate::std_abi_shim::write(socket_fds[1], payload.as_ptr().cast(), payload.len())
            } == payload.len() as isize,
            started,
        );

        let mut available = -1i32;
        let started = log_api_stage("ioctl.FIONREAD.socket.left.payload");
        ok &= log_api_check(
            "ioctl.FIONREAD.socket.left.payload",
            unsafe {
                crate::unix_abi_shim::ioctl(
                    socket_fds[0],
                    FIONREAD,
                    (&mut available as *mut i32).cast(),
                )
            } == 0
                && available == payload.len() as i32,
            started,
        );

        let mut pollfds = [
            crate::unix_abi_shim::PollFd {
                fd: socket_fds[0],
                events: POLLIN,
                revents: 0,
            },
            crate::unix_abi_shim::PollFd {
                fd: socket_fds[1],
                events: POLLOUT,
                revents: 0,
            },
        ];
        let started = log_api_stage("poll.socketpair.read_write_ready");
        ok &= log_api_check(
            "poll.socketpair.read_write_ready",
            unsafe { crate::unix_abi_shim::poll(pollfds.as_mut_ptr(), pollfds.len(), 0) } == 2
                && pollfds[0].revents & POLLIN != 0
                && pollfds[1].revents & POLLOUT != 0,
            started,
        );

        let mut buf = [0u8; 32];
        let started = log_api_stage("read.socketpair.left.payload.content");
        ok &= log_api_check(
            "read.socketpair.left.payload.content",
            unsafe {
                crate::std_abi_shim::read(socket_fds[0], buf.as_mut_ptr().cast(), payload.len())
            } == payload.len() as isize
                && &buf[..payload.len()] == payload,
            started,
        );

        let started = log_api_stage("close.socket.left");
        ok &= log_api_check(
            "close.socket.left",
            unsafe { crate::std_abi_shim::close(socket_fds[0]) } == 0,
            started,
        );
        let started = log_api_stage("write.socketpair.after_peer_close.ebadf");
        ok &= log_api_check(
            "write.socketpair.after_peer_close.ebadf",
            unsafe {
                crate::std_abi_shim::write(socket_fds[1], payload.as_ptr().cast(), payload.len())
            } == -1
                && errno() == EBADF,
            started,
        );
        let started = log_api_stage("close.socket.right");
        ok &= log_api_check(
            "close.socket.right",
            unsafe { crate::std_abi_shim::close(socket_fds[1]) } == 0,
            started,
        );
    } else {
        ok = false;
    }

    let mut flagged_socket_fds = [-1, -1];
    let started = log_api_stage("socketpair.AF_UNIX_STREAM.flags.create");
    if log_api_check(
        "socketpair.AF_UNIX_STREAM.flags.create",
        unsafe {
            crate::unix_abi_shim::socketpair(
                AF_UNIX,
                SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
                0,
                flagged_socket_fds.as_mut_ptr(),
            )
        } == 0
            && unix_api_fds_valid("socketpair.AF_UNIX_STREAM.flags.create", flagged_socket_fds),
        started,
    ) {
        let started = log_api_stage("fcntl.socket.flags.F_GETFL.nonblock");
        let flags_after = unsafe { crate::std_abi_shim::fcntl(flagged_socket_fds[0], F_GETFL, 0) };
        ok &= log_api_check(
            "fcntl.socket.flags.F_GETFL.nonblock",
            flags_after >= 0 && flags_after & O_NONBLOCK != 0,
            started,
        );

        let started = log_api_stage("fcntl.socket.flags.F_SETFD_CLOEXEC");
        ok &= log_api_check(
            "fcntl.socket.flags.F_SETFD_CLOEXEC",
            unsafe { crate::std_abi_shim::fcntl(flagged_socket_fds[0], F_SETFD, FD_CLOEXEC) } == 0,
            started,
        );

        let started = log_api_stage("fcntl.socket.flags.F_GETFD_CLOEXEC");
        let fd_flags = unsafe { crate::std_abi_shim::fcntl(flagged_socket_fds[0], F_GETFD, 0) };
        ok &= log_api_check(
            "fcntl.socket.flags.F_GETFD_CLOEXEC",
            fd_flags >= 0 && fd_flags & FD_CLOEXEC != 0,
            started,
        );

        let mut empty_buf = [0u8; 1];
        let started = log_api_stage("read.socket.flags.empty.nonblock.eagain");
        ok &= log_api_check(
            "read.socket.flags.empty.nonblock.eagain",
            unsafe {
                crate::std_abi_shim::read(
                    flagged_socket_fds[0],
                    empty_buf.as_mut_ptr().cast(),
                    empty_buf.len(),
                )
            } == -1
                && errno() == EAGAIN,
            started,
        );

        let started = log_api_stage("close.socket.flags.left");
        ok &= log_api_check(
            "close.socket.flags.left",
            unsafe { crate::std_abi_shim::close(flagged_socket_fds[0]) } == 0,
            started,
        );
        let started = log_api_stage("close.socket.flags.right");
        ok &= log_api_check(
            "close.socket.flags.right",
            unsafe { crate::std_abi_shim::close(flagged_socket_fds[1]) } == 0,
            started,
        );
    } else {
        ok = false;
    }

    let probe_elapsed_cycles = probe_cycles().wrapping_sub(probe_started_cycles);
    crate::log!(
        "unix-api-probe: summary elapsed_cycles={} elapsed_us={} ok={}\n",
        probe_elapsed_cycles,
        probe_cycles_to_us(probe_elapsed_cycles),
        ok
    );
    ok
}

async fn verify_persisted_close() -> bool {
    log_stage("verify.persisted.close_async");
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("unix-fd-probe: failed verify.persisted.close_async err=no-root\n");
        return false;
    };
    let mut readback = [0u8; 64];
    match crate::r::fs::trueosfs::file_read_range_async(
        disk,
        PROBE_PATH_STR,
        0,
        &mut readback[..BLOCK_ZERO.len()],
    )
    .await
    {
        Ok(Some(got)) if got == BLOCK_ZERO.len() && &readback[..got] == BLOCK_ZERO => {
            crate::log!(
                "unix-fd-probe: success verify.persisted.close_async bytes={} expected={}\n",
                got,
                BLOCK_ZERO.len()
            );
            true
        }
        Ok(Some(got)) => {
            crate::log!(
                "unix-fd-probe: failed verify.persisted.close_async got={} expected={} match={}\n",
                got,
                BLOCK_ZERO.len(),
                got == BLOCK_ZERO.len() && &readback[..got] == BLOCK_ZERO
            );
            false
        }
        Ok(None) => {
            crate::log!("unix-fd-probe: failed verify.persisted.close_async err=missing\n");
            false
        }
        Err(err) => {
            crate::log!("unix-fd-probe: failed verify.persisted.close_async err={:?}\n", err);
            false
        }
    }
}

async fn run_once() -> bool {
    let mut ok = true;

    log_stage("open.O_RDWR_O_CREAT_O_TRUNC");
    let fd = unsafe {
        crate::std_abi_shim::open(
            PROBE_PATH.as_ptr().cast::<c_char>(),
            O_RDWR | O_CREAT | O_TRUNC,
            0o644,
        )
    };
    if !log_fd("open.O_RDWR_O_CREAT_O_TRUNC", fd) {
        return false;
    }

    log_stage("fstat.initial");
    let mut statbuf = [0u8; 256];
    ok &= log_rc("fstat.initial", unsafe {
        crate::std_abi_shim::fstat(fd, statbuf.as_mut_ptr().cast::<c_void>())
    });

    log_stage("lseek.start0");
    ok &= log_seek("lseek.start0", unsafe { crate::std_abi_shim::lseek(fd, 0, SEEK_SET) }, 0);

    log_stage("write.sequential");
    ok &= log_io(
        "write.sequential",
        unsafe {
            crate::std_abi_shim::write(fd, BLOCK_SEQ.as_ptr().cast::<c_void>(), BLOCK_SEQ.len())
        },
        BLOCK_SEQ.len(),
    );

    log_stage("fsync.after_sequential_write");
    ok &= log_rc("fsync.after_sequential_write", unsafe { crate::std_abi_shim::fsync(fd) });

    log_stage("lseek.readback0");
    ok &= log_seek("lseek.readback0", unsafe { crate::std_abi_shim::lseek(fd, 0, SEEK_SET) }, 0);

    log_stage("read.sequential");
    let mut read_seq = [0u8; 64];
    ok &= log_io(
        "read.sequential",
        unsafe {
            crate::std_abi_shim::read(fd, read_seq.as_mut_ptr().cast::<c_void>(), BLOCK_SEQ.len())
        },
        BLOCK_SEQ.len(),
    );

    log_stage("pwrite.offset0");
    ok &= log_io(
        "pwrite.offset0",
        unsafe {
            crate::std_abi_shim::pwrite(
                fd,
                BLOCK_ZERO.as_ptr().cast::<c_void>(),
                BLOCK_ZERO.len(),
                0,
            )
        },
        BLOCK_ZERO.len(),
    );

    log_stage("pwrite.offset4096");
    ok &= log_io(
        "pwrite.offset4096",
        unsafe {
            crate::std_abi_shim::pwrite(
                fd,
                BLOCK_SPARSE.as_ptr().cast::<c_void>(),
                BLOCK_SPARSE.len(),
                4096,
            )
        },
        BLOCK_SPARSE.len(),
    );

    log_stage("pread.offset0");
    let mut read_zero = [0u8; 64];
    ok &= log_io(
        "pread.offset0",
        unsafe {
            crate::std_abi_shim::pread(
                fd,
                read_zero.as_mut_ptr().cast::<c_void>(),
                BLOCK_ZERO.len(),
                0,
            )
        },
        BLOCK_ZERO.len(),
    );

    log_stage("pread.offset4096");
    let mut read_sparse = [0u8; 64];
    ok &= log_io(
        "pread.offset4096",
        unsafe {
            crate::std_abi_shim::pread(
                fd,
                read_sparse.as_mut_ptr().cast::<c_void>(),
                BLOCK_SPARSE.len(),
                4096,
            )
        },
        BLOCK_SPARSE.len(),
    );

    log_stage("ftruncate.grow8192");
    ok &= log_rc("ftruncate.grow8192", unsafe { crate::std_abi_shim::ftruncate(fd, 8192) });

    log_stage("ftruncate.shrink4119");
    ok &= log_rc("ftruncate.shrink4119", unsafe { crate::std_abi_shim::ftruncate(fd, 4119) });

    log_stage("fdatasync");
    ok &= log_rc("fdatasync", unsafe { crate::std_abi_shim::fdatasync(fd) });

    log_stage("fcntl.F_GETLK");
    ok &= log_rc("fcntl.F_GETLK", unsafe { crate::std_abi_shim::fcntl(fd, F_GETLK, 0) });

    log_stage("fcntl.F_SETLK.write_lock");
    ok &= log_rc("fcntl.F_SETLK.write_lock", unsafe { crate::std_abi_shim::fcntl(fd, F_SETLK, 0) });

    log_stage("close.async");
    ok &= log_rc("close.async", crate::std_abi_shim::close_async(fd).await);

    ok &= verify_persisted_close().await;

    crate::log!("unix-fd-probe: cleanup path={}\n", PROBE_PATH_STR);
    ok
}

#[embassy_executor::task]
pub async fn unix_fd_probe_task() {
    Timer::after(EmbassyDuration::from_secs(15)).await;
    crate::log!("unix-api-probe: kernel deferred one-shot start delay_secs=15\n");
    let api_ok = run_unix_api_probe_once();
    if api_ok {
        crate::log!("unix-api-probe: result=ok\n");
    } else {
        crate::log!("unix-api-probe: result=failed stage=one_or_more_unix_api_stages\n");
    }
}
