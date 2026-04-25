use std::time::{Duration, Instant};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "C" {
    fn trueos_tokio_time_now_nanos() -> u64;
}

#[inline]
pub(crate) fn instant_now() -> Instant {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let duration = Duration::from_nanos(unsafe { trueos_tokio_time_now_nanos() });
        unsafe { core::mem::transmute::<Duration, Instant>(duration) }
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        Instant::now()
    }
}
