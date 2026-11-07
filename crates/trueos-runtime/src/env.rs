//! UTF-8 launch environment held by the native process state.
use alloc::{string::String, vec};
#[derive(Debug)]
pub enum VarError {
    NotPresent,
    NotUnicode(alloc::vec::Vec<u8>),
}
impl core::fmt::Display for VarError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for VarError {}
unsafe extern "C" {
    fn trueos_cabi_env_var(key: *const u8, len: usize, out: *mut u8, capacity: usize) -> isize;
}
pub fn var(key: impl AsRef<str>) -> Result<String, VarError> {
    let key = key.as_ref();
    // Environment can change between size and copy; retry if it grew.
    loop {
        let len = unsafe { trueos_cabi_env_var(key.as_ptr(), key.len(), core::ptr::null_mut(), 0) };
        if len < 0 {
            return Err(VarError::NotPresent);
        }
        let mut bytes = vec![0; len as usize];
        let got = unsafe {
            trueos_cabi_env_var(key.as_ptr(), key.len(), bytes.as_mut_ptr(), bytes.len())
        };
        if got < 0 {
            return Err(VarError::NotPresent);
        }
        if got as usize > bytes.len() {
            continue;
        }
        bytes.truncate(got as usize);
        return String::from_utf8(bytes).map_err(|e| VarError::NotUnicode(e.into_bytes()));
    }
}
pub fn var_os(key: impl AsRef<str>) -> Option<String> {
    var(key).ok()
}
