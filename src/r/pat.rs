#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{AtomicU8, Ordering};
use memchr::memchr;
#[cfg(target_arch = "x86_64")]
use raw_cpuid::CpuId;
use twoway::find_str as twoway_find_str;

#[cfg(target_arch = "x86_64")]
static SSE42_SUPPORTED: AtomicU8 = AtomicU8::new(0);

/// Lightweight string search primitive mirroring a subset of `std::str::pattern`.
pub trait Pattern<'a> {
    /// Returns the byte index of the first match within `haystack`, if any.
    fn find_in(&mut self, haystack: &'a str) -> Option<usize>;

    /// Convenience helper that reports whether the pattern matches at the start.
    fn is_prefix_of(&mut self, haystack: &'a str) -> bool {
        self.find_in(haystack) == Some(0)
    }
}

impl<'a> Pattern<'a> for &str {
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        find_str(haystack, self)
    }
}

#[inline]
pub fn find_str(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    #[cfg(target_arch = "x86_64")]
    if let Some(idx) = find_str_sse42(haystack, needle) {
        return Some(idx);
    }

    if let Some(idx) = twoway_find_str(haystack, needle) {
        return Some(idx);
    }

    haystack.find(needle)
}

#[cfg(target_arch = "x86_64")]
#[inline]
pub fn sse42_available() -> bool {
    sse42_supported()
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub fn sse42_available() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_str_sse42(haystack: &str, needle: &str) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if !(2..=8).contains(&needle.len()) || haystack.len() < needle.len() || !sse42_supported() {
        return None;
    }

    unsafe { find_bytes_sse42(haystack, needle) }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn sse42_supported() -> bool {
    match SSE42_SUPPORTED.load(Ordering::Acquire) {
        1 => false,
        2 => true,
        _ => {
            let supported = CpuId::new()
                .get_feature_info()
                .map(|features| features.has_sse42())
                .unwrap_or(false);
            SSE42_SUPPORTED.store(if supported { 2 } else { 1 }, Ordering::Release);
            supported
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
unsafe fn find_bytes_sse42(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    use core::arch::x86_64::{
        __m128i, _SIDD_CMP_EQUAL_ORDERED, _mm_cmpestri, _mm_loadu_si128, _mm_setzero_si128,
    };

    #[inline(always)]
    unsafe fn load_prefix(bytes: &[u8]) -> __m128i {
        let mut block = _mm_setzero_si128();
        let len = bytes.len().min(16);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), &mut block as *mut __m128i as *mut u8, len);
        block
    }

    #[inline(always)]
    unsafe fn load_haystack_block(ptr: *const u8, len: usize) -> __m128i {
        if len >= 16 {
            _mm_loadu_si128(ptr as *const __m128i)
        } else {
            let mut block = _mm_setzero_si128();
            core::ptr::copy_nonoverlapping(ptr, &mut block as *mut __m128i as *mut u8, len);
            block
        }
    }

    let prefix_len = needle.len().min(16);
    let needle_prefix = load_prefix(&needle[..prefix_len]);
    let mut pos = 0usize;

    while pos + needle.len() <= haystack.len() {
        let remaining = haystack.len() - pos;
        let text_len = remaining.min(16);
        let text = load_haystack_block(haystack.as_ptr().add(pos), text_len);
        let rel = _mm_cmpestri(
            needle_prefix,
            prefix_len as i32,
            text,
            text_len as i32,
            _SIDD_CMP_EQUAL_ORDERED,
        ) as usize;

        if rel == 16 {
            pos = pos.saturating_add(16 - prefix_len + 1);
            continue;
        }

        let candidate = pos + rel;
        if candidate + needle.len() > haystack.len() {
            return None;
        }
        if &haystack[candidate..candidate + needle.len()] == needle {
            return Some(candidate);
        }
        pos = candidate + 1;
    }

    None
}

impl<'a> Pattern<'a> for char {
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        if (*self as u32) < 0x80
            && let Some(idx) = memchr(*self as u8, haystack.as_bytes())
        {
            return Some(idx);
        }

        haystack.find(*self)
    }
}

impl<'a> Pattern<'a> for &[char] {
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        let pat = *self;
        if pat.is_empty() {
            return Some(0);
        }
        if pat.len() == 1 {
            let mut single = pat[0];
            return Pattern::find_in(&mut single, haystack);
        }

        let first = pat[0];
        for (idx, ch) in haystack.char_indices() {
            if ch != first {
                continue;
            }

            let mut h_iter = haystack[idx + ch.len_utf8()..].chars();
            let mut matched = true;
            for expected in pat.iter().skip(1).copied() {
                match h_iter.next() {
                    Some(actual) if actual == expected => {}
                    _ => {
                        matched = false;
                        break;
                    }
                }
            }

            if matched {
                return Some(idx);
            }
        }

        None
    }
}

impl<'a, F> Pattern<'a> for F
where
    F: FnMut(char) -> bool,
{
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        for (idx, ch) in haystack.char_indices() {
            if (self)(ch) {
                return Some(idx);
            }
        }
        None
    }
}
