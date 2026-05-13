#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use core::cell::RefCell;
use core::fmt::{self, Write};
use core::str;
use core::time::Duration;
use crate::vec::Vec;
use crate::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "http2")]
use http::header::HeaderValue;
use httpdate::HttpDate;

// "Sun, 06 Nov 1994 08:49:37 GMT".len()
pub(crate) const DATE_VALUE_LENGTH: usize = 29;

#[cfg(feature = "http1")]
pub(crate) fn extend(dst: &mut Vec<u8>) {
    with_cache(|cache| dst.extend_from_slice(cache.buffer()))
}

#[cfg(feature = "http1")]
pub(crate) fn update() {
    with_cache(|cache| cache.check())
}

#[cfg(feature = "http2")]
pub(crate) fn update_and_header_value() -> HeaderValue {
    with_cache(|cache| {
        cache.check();
        cache.header_value.clone()
    })
}

struct CachedDate {
    bytes: [u8; DATE_VALUE_LENGTH],
    pos: usize,
    #[cfg(feature = "http2")]
    header_value: HeaderValue,
    next_update: SystemTime,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
static CACHED: crate::sync::Mutex<Option<CachedDate>> = crate::sync::Mutex::new(None);

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
thread_local!(static CACHED: RefCell<CachedDate> = RefCell::new(CachedDate::new()));

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn with_cache<R>(f: impl FnOnce(&mut CachedDate) -> R) -> R {
    let mut guard = CACHED.lock().unwrap();
    let cache = guard.get_or_insert_with(CachedDate::new);
    f(cache)
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn with_cache<R>(f: impl FnOnce(&mut CachedDate) -> R) -> R {
    CACHED.with(|cache| {
        let mut cache = cache.borrow_mut();
        f(&mut cache)
    })
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn system_time_now() -> SystemTime {
    crate::platform::system_time_now()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn system_time_now() -> SystemTime {
    SystemTime::now()
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn http_date_from_system_time(now: SystemTime) -> HttpDate {
    let duration_since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    HttpDate::from(httpdate::time::UNIX_EPOCH + duration_since_epoch)
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn http_date_from_system_time(now: SystemTime) -> HttpDate {
    HttpDate::from(now)
}

impl CachedDate {
    fn new() -> Self {
        let mut cache = CachedDate {
            bytes: [0; DATE_VALUE_LENGTH],
            pos: 0,
            #[cfg(feature = "http2")]
            header_value: HeaderValue::from_static(""),
            next_update: system_time_now(),
        };
        cache.update(cache.next_update);
        cache
    }

    fn buffer(&self) -> &[u8] {
        &self.bytes[..]
    }

    fn check(&mut self) {
        let now = system_time_now();
        if now > self.next_update {
            self.update(now);
        }
    }

    fn update(&mut self, now: SystemTime) {
        let nanos = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();

        self.render(now);
        self.next_update = now + Duration::new(1, 0) - Duration::from_nanos(nanos as u64);
    }

    fn render(&mut self, now: SystemTime) {
        self.pos = 0;
        let _ = write!(self, "{}", http_date_from_system_time(now));
        debug_assert!(self.pos == DATE_VALUE_LENGTH);
        self.render_http2();
    }

    #[cfg(feature = "http2")]
    fn render_http2(&mut self) {
        self.header_value = HeaderValue::from_bytes(self.buffer())
            .expect("Date format should be valid HeaderValue");
    }

    #[cfg(not(feature = "http2"))]
    fn render_http2(&mut self) {}
}

impl fmt::Write for CachedDate {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let len = s.len();
        self.bytes[self.pos..self.pos + len].copy_from_slice(s.as_bytes());
        self.pos += len;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "nightly")]
    use test::Bencher;

    #[test]
    fn test_date_len() {
        assert_eq!(DATE_VALUE_LENGTH, "Sun, 06 Nov 1994 08:49:37 GMT".len());
    }

    #[cfg(feature = "nightly")]
    #[bench]
    fn bench_date_check(b: &mut Bencher) {
        let mut date = CachedDate::new();
        // cache the first update
        date.check();

        b.iter(|| {
            date.check();
        });
    }

    #[cfg(feature = "nightly")]
    #[bench]
    fn bench_date_render(b: &mut Bencher) {
        let mut date = CachedDate::new();
        let now = system_time_now();
        date.render(now);
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        static CACHED: crate::sync::Mutex<Option<CachedDate>> = crate::sync::Mutex::new(None);

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        thread_local!(static CACHED: RefCell<CachedDate> = RefCell::new(CachedDate::new()));

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        fn with_cache<R>(f: impl FnOnce(&mut CachedDate) -> R) -> R {
            let mut guard = CACHED.lock().unwrap();
            let cache = guard.get_or_insert_with(CachedDate::new);
            f(cache)
        }

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        fn with_cache<R>(f: impl FnOnce(&mut CachedDate) -> R) -> R {
            CACHED.with(|cache| {
                let mut cache = cache.borrow_mut();
                f(&mut cache)
            })
        }

        b.iter(|| {
            date.render(now);
            test::black_box(&date);
        });
    }
}
