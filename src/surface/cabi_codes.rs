pub const FS_ERR_BAD_UTF8: i32 = -1;
pub const FS_ERR_IO: i32 = -2;
pub const FS_ERR_NO_SPACE: i32 = -3;
pub const FS_ERR_BAD_PARAM: i32 = -4;
pub const FS_ERR_USBMS_NOT_FOUND: i32 = -5;
pub const FS_ERR_BAD_PATH: i32 = -6;
pub const FS_ERR_TOO_LARGE: i32 = -7;
pub const FS_ERR_NOT_FOUND: i32 = -8;
pub const FS_ERR_ALREADY_EXISTS: i32 = -9;
#[allow(dead_code)]
pub const FS_ERR_TIMEOUT: i32 = -14;

#[allow(dead_code)]
// Contract limit for C ABI FS path parameters used by kernel + QJS.
pub const QJS_ASYNC_FS_MAX_PATH: usize = 1024;

pub const NET_ERR_BAD_URL: i32 = -10;
pub const NET_ERR_TIMEOUT: i32 = -11;
pub const NET_ERR_HTTP: i32 = -12;
pub const NET_ERR_TLS: i32 = -13;

pub const NET_ERR_TIMEOUT_DNS: i32 = -111;
pub const NET_ERR_TIMEOUT_CONNECT: i32 = -112;
pub const NET_ERR_TIMEOUT_TLS: i32 = -113;
pub const NET_ERR_TIMEOUT_BODY: i32 = -114;

#[inline]
pub fn cabi_rc_name(rc: i32) -> &'static [u8] {
    match rc {
        0 => b"OK",
        FS_ERR_BAD_UTF8 => b"FS_ERR_BAD_UTF8",
        FS_ERR_IO => b"FS_ERR_IO",
        FS_ERR_NO_SPACE => b"FS_ERR_NO_SPACE",
        FS_ERR_BAD_PARAM => b"FS_ERR_BAD_PARAM",
        FS_ERR_USBMS_NOT_FOUND => b"FS_ERR_USBMS_NOT_FOUND",
        FS_ERR_BAD_PATH => b"FS_ERR_BAD_PATH",
        FS_ERR_TOO_LARGE => b"FS_ERR_TOO_LARGE",
        FS_ERR_NOT_FOUND => b"FS_ERR_NOT_FOUND",
        FS_ERR_ALREADY_EXISTS => b"FS_ERR_ALREADY_EXISTS",
        FS_ERR_TIMEOUT => b"FS_ERR_TIMEOUT",
        NET_ERR_BAD_URL => b"NET_ERR_BAD_URL",
        NET_ERR_TIMEOUT => b"NET_ERR_TIMEOUT",
        NET_ERR_HTTP => b"NET_ERR_HTTP",
        NET_ERR_TLS => b"NET_ERR_TLS",
        NET_ERR_TIMEOUT_DNS => b"NET_ERR_TIMEOUT_DNS",
        NET_ERR_TIMEOUT_CONNECT => b"NET_ERR_TIMEOUT_CONNECT",
        NET_ERR_TIMEOUT_TLS => b"NET_ERR_TIMEOUT_TLS",
        NET_ERR_TIMEOUT_BODY => b"NET_ERR_TIMEOUT_BODY",
        _ => b"UNKNOWN",
    }
}
