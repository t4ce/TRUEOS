// Reference source for library/std/src/sys/io/is_terminal/trueos.rs.
// Descriptor-shaped std wrappers identify only the three native standard
// streams. This query never allocates a descriptor or claims a terminal lease.

use crate::os::fd::{AsFd, AsRawFd};

unsafe extern "C" {
    fn trueos_cabi_stdio_is_terminal_v1(stream: u32) -> i32;
}

pub fn is_terminal(stream: &impl AsFd) -> bool {
    match stream.as_fd().as_raw_fd() {
        number @ 0..=2 => unsafe { trueos_cabi_stdio_is_terminal_v1(number as u32) == 1 },
        _ => false,
    }
}
