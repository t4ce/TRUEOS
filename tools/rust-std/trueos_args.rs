// Reference source for library/std/src/sys/args/trueos.rs.
// A Blueprint receives native launch arguments rather than a C startup argv.

pub use super::common::Args;
use crate::ffi::OsString;
use crate::os::unix::ffi::OsStringExt;

unsafe extern "C" {
    fn trueos_cabi_env_args_count() -> usize;
    fn trueos_cabi_env_arg(index: usize, out: *mut u8, capacity: usize) -> isize;
}

pub unsafe fn init(_argc: isize, _argv: *const *const u8) {}

pub fn args() -> Args {
    let count = unsafe { trueos_cabi_env_args_count() };
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let length = unsafe { trueos_cabi_env_arg(index, crate::ptr::null_mut(), 0) };
        let length = usize::try_from(length).expect("native launch argument query failed");
        let mut bytes = vec![0; length];
        let copied = unsafe { trueos_cabi_env_arg(index, bytes.as_mut_ptr(), bytes.len()) };
        assert_eq!(
            copied, length as isize,
            "native launch argument changed or failed to copy"
        );
        values.push(OsString::from_vec(bytes));
    }
    Args::new(values)
}
