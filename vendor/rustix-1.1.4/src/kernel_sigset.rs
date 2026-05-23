//! The [`KernelSigSet`] type.

#![allow(unsafe_code)]
#![allow(non_camel_case_types)]

use crate::backend::c;
use crate::signal::Signal;
use ::core::fmt;
use linux_raw_sys::general::{kernel_sigset_t, _NSIG};

/// `kernel_sigset_t`—A set of signal numbers, as used by some syscalls.
///
/// This is similar to `libc::sigset_t`, but with only enough space for the
/// signals currently known to be used by the kernel. libc implementations
/// reserve extra space so that if Linux defines new signals in the future
/// they can add support without breaking their dynamic linking ABI. Rustix
/// doesn't support a dynamic linking ABI, so if we need to increase the
/// size of `KernelSigSet` in the future, we can do so.
///
/// It's also the case that the last time Linux changed the size of its
/// `kernel_sigset_t` was when it added support for POSIX.1b signals in 1999.
///
/// `KernelSigSet` is guaranteed to have a subset of the layout of
/// `libc::sigset_t`.
///
/// libc implementations typically also reserve some signal values for internal
/// use. In a process that contains a libc, some unsafe functions invoke
/// undefined behavior if passed a `KernelSigSet` that contains one of the
/// signals that the libc reserves.
#[repr(transparent)]
#[derive(Clone)]
pub struct KernelSigSet(kernel_sigset_t);

impl KernelSigSet {
    /// Create a new empty `KernelSigSet`.
    pub const fn empty() -> Self {
        const fn zeros<const N: usize>() -> [c::c_ulong; N] {
            [0; N]
        }
        Self(kernel_sigset_t { sig: zeros() })
    }

    /// Create a new `KernelSigSet` with all signals set.
    ///
    /// This includes signals which are typically reserved for libc.
    pub const fn all() -> Self {
        const fn ones<const N: usize>() -> [c::c_ulong; N] {
            [!0; N]
        }
        Self(kernel_sigset_t { sig: ones() })
    }

    /// Remove all signals.
    pub fn clear(&mut self) {
        *self = Self(kernel_sigset_t {
            sig: Default::default(),
        });
    }

    /// Insert a signal.
    pub fn insert(&mut self, sig: Signal) {
        let sigs_per_elt = core::mem::size_of_val(&self.0.sig[0]) * 8;

        let raw = (sig.as_raw().wrapping_sub(1)) as usize;
        self.0.sig[raw / sigs_per_elt] |= 1 << (raw % sigs_per_elt);
    }

    /// Insert all signals.
    pub fn insert_all(&mut self) {
        self.0.sig.fill(!0);
    }

    /// Remove a signal.
    pub fn remove(&mut self, sig: Signal) {
        let sigs_per_elt = core::mem::size_of_val(&self.0.sig[0]) * 8;

        let raw = (sig.as_raw().wrapping_sub(1)) as usize;
        self.0.sig[raw / sigs_per_elt] &= !(1 << (raw % sigs_per_elt));
    }

    /// Test whether a given signal is present.
    pub fn contains(&self, sig: Signal) -> bool {
        let sigs_per_elt = core::mem::size_of_val(&self.0.sig[0]) * 8;

        let raw = (sig.as_raw().wrapping_sub(1)) as usize;
        (self.0.sig[raw / sigs_per_elt] & (1 << (raw % sigs_per_elt))) != 0
    }
}

impl Default for KernelSigSet {
    #[inline]
    fn default() -> Self {
        Self::empty()
    }
}

impl fmt::Debug for KernelSigSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_set();

        // Surprisingly, `_NSIG` is inclusive.
        for i in 1..=_NSIG {
            // SAFETY: This value is non-zero, in range, and only used for
            // debug output.
            let sig = unsafe { Signal::from_raw_unchecked(i as _) };

            if self.contains(sig) {
                d.entry(&sig);
            }
        }

        d.finish()
    }
}
