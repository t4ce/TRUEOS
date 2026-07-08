#![allow(dead_code)]

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
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 0o4000;
const POLLIN: i16 = 0x0001;
const POLLOUT: i16 = 0x0004;
const TCSANOW: i32 = 0;
const TCGETS: usize = 0x5401;
const TIOCGWINSZ: usize = 0x5413;
const FIONREAD: usize = 0x541b;

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

fn log_api_stage(stage: &str) {
    crate::log!("unix-api-probe: stage {}\n", stage);
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
    let mut ok = true;

    log_api_stage("isatty.stdin");
    ok &= log_api_rc_nonnegative("isatty.stdin", unsafe { crate::unix_abi_shim::isatty(0) });

    log_api_stage("isatty.stdout");
    ok &= log_api_rc_nonnegative("isatty.stdout", unsafe { crate::unix_abi_shim::isatty(1) });

    let mut winsize = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    log_api_stage("ioctl.TIOCGWINSZ.stdout");
    let winsize_ok = log_api_rc_zero("ioctl.TIOCGWINSZ.stdout", unsafe {
        crate::unix_abi_shim::ioctl(1, TIOCGWINSZ, (&mut winsize as *mut Winsize).cast())
    });
    ok &= winsize_ok;
    if winsize_ok {
        crate::log!(
            "unix-api-probe: winsize rows={} cols={} xpixel={} ypixel={}\n",
            winsize.ws_row,
            winsize.ws_col,
            winsize.ws_xpixel,
            winsize.ws_ypixel
        );
    }

    let mut available = 0i32;
    log_api_stage("ioctl.FIONREAD.stdin");
    ok &= log_api_rc_zero("ioctl.FIONREAD.stdin", unsafe {
        crate::unix_abi_shim::ioctl(0, FIONREAD, (&mut available as *mut i32).cast())
    });

    let mut termios = [0u8; 64];
    log_api_stage("tcgetattr.stdin");
    let tcgetattr_ok = log_api_rc_zero("tcgetattr.stdin", unsafe {
        crate::unix_abi_shim::tcgetattr(0, termios.as_mut_ptr().cast())
    });
    ok &= tcgetattr_ok;
    if tcgetattr_ok {
        log_api_stage("tcsetattr.stdin.restore");
        ok &= log_api_rc_zero("tcsetattr.stdin.restore", unsafe {
            crate::unix_abi_shim::tcsetattr(0, TCSANOW, termios.as_ptr().cast())
        });
    }

    let mut raw_termios = [0u8; 64];
    log_api_stage("ioctl.TCGETS.stdin");
    ok &= log_api_rc_zero("ioctl.TCGETS.stdin", unsafe {
        crate::unix_abi_shim::ioctl(0, TCGETS, raw_termios.as_mut_ptr().cast())
    });

    let mut pipe_fds = [-1, -1];
    log_api_stage("pipe");
    if log_api_rc_zero("pipe", unsafe { crate::unix_abi_shim::pipe(pipe_fds.as_mut_ptr()) })
        && unix_api_fds_valid("pipe", pipe_fds)
    {
        log_api_stage("fcntl.pipe.read.F_GETFL");
        let read_flags = unsafe { crate::std_abi_shim::fcntl(pipe_fds[0], F_GETFL, 0) };
        ok &= log_api_rc_nonnegative("fcntl.pipe.read.F_GETFL", read_flags);
        if read_flags >= 0 {
            log_api_stage("fcntl.pipe.read.F_SETFL_NONBLOCK");
            ok &= log_api_rc_zero("fcntl.pipe.read.F_SETFL_NONBLOCK", unsafe {
                crate::std_abi_shim::fcntl(pipe_fds[0], F_SETFL, read_flags | O_NONBLOCK)
            });
        }

        let payload = b"pipe-ok";
        log_api_stage("write.pipe");
        ok &= log_api_io(
            "write.pipe",
            unsafe {
                crate::std_abi_shim::write(pipe_fds[1], payload.as_ptr().cast(), payload.len())
            },
            payload.len(),
        );

        let mut pollfds = [crate::unix_abi_shim::PollFd {
            fd: pipe_fds[0],
            events: POLLIN,
            revents: 0,
        }];
        log_api_stage("poll.pipe.readable");
        ok &= log_api_rc_nonnegative("poll.pipe.readable", unsafe {
            crate::unix_abi_shim::poll(pollfds.as_mut_ptr(), pollfds.len(), 0)
        });
        crate::log!("unix-api-probe: poll.pipe revents={}\n", pollfds[0].revents);

        let mut buf = [0u8; 16];
        log_api_stage("read.pipe");
        ok &= log_api_io(
            "read.pipe",
            unsafe {
                crate::std_abi_shim::read(pipe_fds[0], buf.as_mut_ptr().cast(), payload.len())
            },
            payload.len(),
        );

        log_api_stage("close.pipe.read");
        ok &=
            log_api_rc_zero("close.pipe.read", unsafe { crate::std_abi_shim::close(pipe_fds[0]) });
        log_api_stage("close.pipe.write");
        ok &=
            log_api_rc_zero("close.pipe.write", unsafe { crate::std_abi_shim::close(pipe_fds[1]) });
    } else {
        ok = false;
    }

    let mut socket_fds = [-1, -1];
    log_api_stage("socketpair.AF_UNIX_STREAM");
    if log_api_rc_zero("socketpair.AF_UNIX_STREAM", unsafe {
        crate::unix_abi_shim::socketpair(AF_UNIX, SOCK_STREAM, 0, socket_fds.as_mut_ptr())
    }) && unix_api_fds_valid("socketpair.AF_UNIX_STREAM", socket_fds)
    {
        let payload = b"unix-stream-ok";
        log_api_stage("write.socketpair.right");
        ok &= log_api_io(
            "write.socketpair.right",
            unsafe {
                crate::std_abi_shim::write(socket_fds[1], payload.as_ptr().cast(), payload.len())
            },
            payload.len(),
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
        log_api_stage("poll.socketpair");
        ok &= log_api_rc_nonnegative("poll.socketpair", unsafe {
            crate::unix_abi_shim::poll(pollfds.as_mut_ptr(), pollfds.len(), 0)
        });
        crate::log!(
            "unix-api-probe: poll.socketpair left_revents={} right_revents={}\n",
            pollfds[0].revents,
            pollfds[1].revents
        );

        let mut buf = [0u8; 32];
        log_api_stage("read.socketpair.left");
        ok &= log_api_io(
            "read.socketpair.left",
            unsafe {
                crate::std_abi_shim::read(socket_fds[0], buf.as_mut_ptr().cast(), payload.len())
            },
            payload.len(),
        );

        log_api_stage("close.socket.left");
        ok &= log_api_rc_zero("close.socket.left", unsafe {
            crate::std_abi_shim::close(socket_fds[0])
        });
        log_api_stage("close.socket.right");
        ok &= log_api_rc_zero("close.socket.right", unsafe {
            crate::std_abi_shim::close(socket_fds[1])
        });
    } else {
        ok = false;
    }

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
