use crate::vcabi;

#[inline]
pub fn poll_once() {
    unsafe { vcabi::trueos_cabi_poll_once() }
}

#[inline]
pub fn write_stream(stream: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    //unsafe { vcabi::trueos_cabi_write(stream, bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn write_log_stream(stream: u32, s: &str) {
    write_stream(stream, s.as_bytes());
}

#[inline]
pub fn log_info(s: &str) {
    write_log_stream(1, s);
}

#[inline]
pub fn log_error(s: &str) {
    write_log_stream(2, s);
}
