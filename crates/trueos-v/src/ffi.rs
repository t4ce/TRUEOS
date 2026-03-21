pub use core::ffi::{
    CStr, c_char, c_int, c_long, c_longlong, c_schar, c_short, c_uchar, c_uint, c_ulong,
    c_ulonglong, c_ushort, c_void,
};

pub use alloc::ffi::CString;
pub use alloc::string::String as OsString;
pub type OsStr = str;
