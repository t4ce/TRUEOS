//! Search for a byte in a byte array using libc.
//!
//! When nothing pulls in libc, then just use a trivial implementation. Note
//! that we only depend on libc on unix.

#[cfg(not(all(unix, feature = "libc")))]
fn memchr_inner(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|val| needle == *val)
}

#[cfg(all(unix, feature = "libc"))]
fn memchr_inner(needle: u8, haystack: &[u8]) -> Option<usize> {
    let start = haystack.as_ptr();

    // SAFETY: `start` is valid for `haystack.len()` bytes.
    let ptr = (unsafe { libc::memchr(start.cast(), needle as _, haystack.len()) })
        .cast::<u8>()
        .cast_const();

    if ptr.is_null() {
        None
    } else {
        // SAFETY: `ptr` will always be in bounds, since libc guarantees that the ptr will either
        //          be to an element inside the array or the ptr will be null
        //          since the ptr is in bounds the offset must also always be non null
        //          and there can't be more than isize::MAX elements inside an array
        //          as rust guarantees that the maximum number of bytes a allocation
        //          may occupy is isize::MAX
        unsafe {
            // TODO(MSRV 1.87): When bumping MSRV, switch to `ptr.byte_offset_from_unsigned(start)`.
            Some(usize::try_from(ptr.offset_from(start)).unwrap_unchecked())
        }
    }
}

pub(crate) fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    let index = memchr_inner(needle, haystack)?;

    // SAFETY: `memchr_inner` returns Some(index) and in that case index must point to an element in haystack
    //         or `memchr_inner` None which is guarded by the `?` operator above
    //         therefore the index must **always** point to an element in the array
    //         and so this indexing operation is safe
    // TODO(MSRV 1.81): When bumping MSRV, switch to `core::hint::assert_unchecked(haystack.get(..=index).is_some());`
    unsafe {
        if haystack.get(..=index).is_none() {
            core::hint::unreachable_unchecked()
        }
    }

    Some(index)
}
