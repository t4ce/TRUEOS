// Reference source for library/std/src/sys/stdio/trueos.rs.
// Blocking byte streams use native attached console or explicit child pipes.

use crate::io::{self, Read, Write};

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_shell_attached_read_v1(out: *mut u8, capacity: usize) -> isize;
    fn trueos_cabi_shell_attached_observe_v1() -> u32;
    fn trueos_cabi_shell_attached_wait_after_v1(observed: u32, timeout_ms: u64) -> i32;
    fn trueos_cabi_blueprint_terminal_lease_current_v1(ready: u64, epoch: *mut u64) -> i32;
    fn trueos_cabi_blueprint_child_read_v1(handle: u64, out: *mut u8, capacity: usize) -> isize;
    fn trueos_cabi_blueprint_child_send_v1(handle: u64, bytes: *const u8, len: usize) -> isize;
    fn trueos_cabi_blueprint_child_status_v1(handle: u64) -> i32;
}

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;

impl Stdin {
    pub const fn new() -> Self {
        Self
    }
}
impl Stdout {
    pub const fn new() -> Self {
        Self
    }
}
impl Stderr {
    pub const fn new() -> Self {
        Self
    }
}

fn child_stdio() -> bool {
    crate::env::var_os("TRUEOS_MCP_STDIO").as_deref() == Some(crate::ffi::OsStr::new("1"))
}

fn checked_count(count: usize, capacity: usize) -> io::Result<usize> {
    if count <= capacity {
        Ok(count)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native stream returned an invalid byte count",
        ))
    }
}

impl Read for Stdin {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if child_stdio() {
            loop {
                let count = unsafe {
                    trueos_cabi_blueprint_child_read_v1(0, bytes.as_mut_ptr(), bytes.len())
                };
                if count > 0 {
                    return checked_count(count as usize, bytes.len());
                }
                if count < 0 || unsafe { trueos_cabi_blueprint_child_status_v1(0) } >= 4 {
                    return Ok(0);
                }
                crate::thread::sleep(crate::time::Duration::from_millis(10));
            }
        }
        let mut epoch = 0;
        let status = unsafe { trueos_cabi_blueprint_terminal_lease_current_v1(0, &mut epoch) };
        if status != 0 || epoch == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "native stdin has no active terminal lease",
            ));
        }
        loop {
            // Observe before probing so arriving input cannot be lost between
            // the empty read and parking this logical thread.
            let observed = unsafe { trueos_cabi_shell_attached_observe_v1() };
            let count =
                unsafe { trueos_cabi_shell_attached_read_v1(bytes.as_mut_ptr(), bytes.len()) };
            if count > 0 {
                return checked_count(count as usize, bytes.len());
            }
            if count < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "native attached stdin read failed",
                ));
            }
            if unsafe { trueos_cabi_shell_attached_wait_after_v1(observed, u64::MAX) } < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "native attached stdin wait failed",
                ));
            }
        }
    }
}

impl Write for Stdout {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if child_stdio() {
            let bytes = &bytes[..bytes.len().min(64 * 1024)];
            loop {
                let count =
                    unsafe { trueos_cabi_blueprint_child_send_v1(0, bytes.as_ptr(), bytes.len()) };
                if count >= 0 {
                    return checked_count(count as usize, bytes.len());
                }
                if count != -3 {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "native child stdout is closed",
                    ));
                }
                crate::thread::sleep(crate::time::Duration::from_millis(10));
            }
        }
        unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Stderr {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !bytes.is_empty() {
            unsafe { trueos_cabi_write(2, bytes.as_ptr(), bytes.len()) };
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn is_ebadf(error: &io::Error) -> bool {
    error.raw_os_error() == Some(9)
}
pub const STDIN_BUF_SIZE: usize = crate::sys::io::DEFAULT_BUF_SIZE;
pub fn panic_output() -> Option<impl Write> {
    Some(Stderr::new())
}
