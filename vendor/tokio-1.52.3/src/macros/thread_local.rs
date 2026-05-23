#[cfg(all(loom, test))]
macro_rules! tokio_thread_local {
    ($(#[$attrs:meta])* $vis:vis static $name:ident: $ty:ty = const { $expr:expr } $(;)?) => {
        loom::thread_local! {
            $(#[$attrs])*
            $vis static $name: $ty = $expr;
        }
    };

    ($($tts:tt)+) => { loom::thread_local!{ $($tts)+ } }
}

#[cfg(all(not(all(loom, test)), any(target_os = "trueos", target_os = "zkvm")))]
pub(crate) mod trueos_tls {
    use alloc::boxed::Box;
    use core::sync::atomic::{AtomicUsize, Ordering};

    const TRUEOS_TLS_SLOT_COUNT: usize = 64;

    unsafe extern "Rust" {
        fn trueos_tokio_tls_current_slot() -> u32;
    }

    pub(crate) struct LocalKey<T: 'static> {
        init: fn() -> T,
        slots: [AtomicUsize; TRUEOS_TLS_SLOT_COUNT],
    }

    unsafe impl<T: 'static> Sync for LocalKey<T> {}

    impl<T: 'static> LocalKey<T> {
        pub(crate) const fn new(init: fn() -> T) -> Self {
            Self {
                init,
                slots: [const { AtomicUsize::new(0) }; TRUEOS_TLS_SLOT_COUNT],
            }
        }

        pub(crate) fn with<F, R>(&'static self, f: F) -> R
        where
            F: FnOnce(&T) -> R,
        {
            self.try_with(f)
                .unwrap_or_else(|_| unreachable!("TRUEOS Tokio TLS is never destroyed"))
        }

        pub(crate) fn try_with<F, R>(&'static self, f: F) -> Result<R, std::thread::AccessError>
        where
            F: FnOnce(&T) -> R,
        {
            let ptr = self.get_or_init_ptr();
            Ok(f(unsafe { &*(ptr as *const T) }))
        }

        fn get_or_init_ptr(&'static self) -> usize {
            let slot = unsafe { trueos_tokio_tls_current_slot() } as usize;
            let slot = if slot < TRUEOS_TLS_SLOT_COUNT {
                slot
            } else {
                0
            };
            let cell = &self.slots[slot];

            let existing = cell.load(Ordering::Acquire);
            if existing != 0 {
                return existing;
            }

            let ptr = Box::leak(Box::new((self.init)())) as *mut T as usize;
            match cell.compare_exchange(0, ptr, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => ptr,
                Err(existing) => existing,
            }
        }
    }
}

#[cfg(all(not(all(loom, test)), any(target_os = "trueos", target_os = "zkvm")))]
macro_rules! tokio_thread_local {
    ($(#[$attrs:meta])* $vis:vis static $name:ident: $ty:ty = const { $expr:expr } $(;)?) => {
        $(#[$attrs])*
        $vis static $name: crate::macros::thread_local::trueos_tls::LocalKey<$ty> = {
            fn __trueos_tokio_tls_init() -> $ty {
                $expr
            }
            crate::macros::thread_local::trueos_tls::LocalKey::new(__trueos_tokio_tls_init)
        };
    };

    ($(#[$attrs:meta])* $vis:vis static $name:ident: $ty:ty = $expr:expr $(;)?) => {
        $(#[$attrs])*
        $vis static $name: crate::macros::thread_local::trueos_tls::LocalKey<$ty> = {
            fn __trueos_tokio_tls_init() -> $ty {
                $expr
            }
            crate::macros::thread_local::trueos_tls::LocalKey::new(__trueos_tokio_tls_init)
        };
    };
}

#[cfg(all(
    not(all(loom, test)),
    not(any(target_os = "trueos", target_os = "zkvm"))
))]
macro_rules! tokio_thread_local {
    ($($tts:tt)+) => {
        ::std::thread_local!{ $($tts)+ }
    }
}
