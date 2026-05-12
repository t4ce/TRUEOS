use crate::sys::windows::Event;

use core::cell::UnsafeCell;
use core::fmt;

use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_ENTRY};

#[repr(C)]
pub(crate) struct Overlapped {
    inner: UnsafeCell<OVERLAPPED>,
    pub(crate) callback: fn(&OVERLAPPED_ENTRY, Option<&mut Vec<Event>>),
}

#[cfg(feature = "os-ext")]
impl Overlapped {
    pub(crate) fn new(cb: fn(&OVERLAPPED_ENTRY, Option<&mut Vec<Event>>)) -> Overlapped {
        Overlapped {
            inner: UnsafeCell::new(unsafe { core::mem::zeroed() }),
            callback: cb,
        }
    }

    pub(crate) fn as_ptr(&self) -> *const OVERLAPPED {
        self.inner.get()
    }
}

impl fmt::Debug for Overlapped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Overlapped").finish()
    }
}

unsafe impl Send for Overlapped {}
unsafe impl Sync for Overlapped {}
