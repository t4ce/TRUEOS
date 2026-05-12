// Copyright 2016 Amanieu d'Antras
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use core::time::Duration;
use std::time::Instant;

// Option::unchecked_unwrap
pub trait UncheckedOptionExt<T> {
    unsafe fn unchecked_unwrap(self) -> T;
}

impl<T> UncheckedOptionExt<T> for Option<T> {
    #[inline]
    unsafe fn unchecked_unwrap(self) -> T {
        match self {
            Some(x) => x,
            None => unreachable(),
        }
    }
}

// hint::unreachable_unchecked() in release mode
#[inline]
unsafe fn unreachable() -> ! {
    if cfg!(debug_assertions) {
        unreachable!();
    } else {
        core::hint::unreachable_unchecked()
    }
}

#[inline]
pub fn to_deadline(timeout: Duration) -> Option<Instant> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let nanos = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
        return crate::zkvm_time::instant_now().checked_add(embassy_time::Duration::from_nanos(nanos));
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    crate::zkvm_time::instant_now().checked_add(timeout)
}
