//! Minimal pattern abstraction for `surface` string APIs.
//!
//! This is deliberately tiny to stay `no_std`-friendly while still allowing
//! callers to plug in their own matchers. When the optional
//! `surface-pattern-accel` feature is enabled we lean on `memchr`/`twoway`
//! for tighter single-byte and substring searches.

use core::str;

#[cfg(feature = "surface-pattern-accel")]
use memchr::memchr;
#[cfg(feature = "surface-pattern-accel")]
use twoway::find_str;

/// Lightweight string search primitive mirroring a subset of `std::str::pattern`.
pub trait Pattern<'a> {
    /// Returns the byte index of the first match within `haystack`, if any.
    fn find_in(&mut self, haystack: &'a str) -> Option<usize>;

    /// Convenience helper that reports whether the pattern matches at the start.
    fn is_prefix_of(&mut self, haystack: &'a str) -> bool {
        self.find_in(haystack) == Some(0)
    }
}

impl<'a, 'b> Pattern<'a> for &'b str {
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        if self.is_empty() {
            return Some(0);
        }

        #[cfg(feature = "surface-pattern-accel")]
        {
            if let Some(idx) = find_str(haystack, *self) {
                return Some(idx);
            }
        }

        haystack.find(*self)
    }
}

impl<'a> Pattern<'a> for char {
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        #[cfg(feature = "surface-pattern-accel")]
        {
            if (*self as u32) < 0x80 {
                if let Some(idx) = memchr(*self as u8, haystack.as_bytes()) {
                    return Some(idx);
                }
            }
        }

        haystack.find(*self)
    }
}

impl<'a, 'b> Pattern<'a> for &'b [char] {
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
