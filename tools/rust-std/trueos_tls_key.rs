//! Static identity keys for TRUEOS execution-local storage.
//!
//! Reuse std's existing Storage initialization, recursion and destruction
//! protocol, without creating a second runtime key registry.

pub type Key = &'static LazyKey;

pub struct LazyKey {
    destructor: Option<unsafe extern "C" fn(*mut u8)>,
}

unsafe extern "Rust" {
    fn trueos_platform_thread_local_get(key: usize) -> *mut u8;
    fn trueos_platform_thread_local_set(
        key: usize,
        value: *mut u8,
        destructor: Option<unsafe extern "C" fn(*mut u8)>,
    ) -> i32;
}

impl LazyKey {
    pub const fn new(destructor: Option<unsafe extern "C" fn(*mut u8)>) -> Self {
        Self { destructor }
    }

    #[inline]
    pub fn force(&'static self) -> Key { self }
}

#[inline]
pub unsafe fn get(key: Key) -> *mut u8 {
    unsafe { trueos_platform_thread_local_get(key as *const LazyKey as usize) }
}

#[inline]
pub unsafe fn set(key: Key, value: *mut u8) {
    let result = unsafe {
        trueos_platform_thread_local_set(key as *const LazyKey as usize, value, key.destructor)
    };
    rtassert!(result == 0);
}
