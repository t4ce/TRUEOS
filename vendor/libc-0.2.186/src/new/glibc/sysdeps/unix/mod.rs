//! Source directory: `sysdeps/unix/`
//!
//! <https://github.com/bminor/glibc/tree/master/sysdeps/unix>

#[cfg(any(target_os = "linux", target_os = "trueos"))]
pub(crate) mod linux;
