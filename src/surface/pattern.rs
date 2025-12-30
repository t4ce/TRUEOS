use memchr::memchr;
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

        if let Some(idx) = find_str(haystack, *self) {
            return Some(idx);
        }

        haystack.find(*self)
    }
}

impl<'a> Pattern<'a> for char {
    fn find_in(&mut self, haystack: &'a str) -> Option<usize> {
        if (*self as u32) < 0x80 {
            if let Some(idx) = memchr(*self as u8, haystack.as_bytes()) {
                return Some(idx);
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

/// Ensures the Pattern implementations keep working for fundamental cases.
pub fn smoke_test() {
    let haystack = "abc123xyz";

    let mut str_pat = "123";
    assert_eq!(Pattern::find_in(&mut str_pat, haystack), Some(3));

    let mut char_pat = 'x';
    assert_eq!(Pattern::find_in(&mut char_pat, haystack), Some(6));

    let slice_storage = ['x', 'y'];
    let mut slice_pat: &[char] = &slice_storage;
    assert_eq!(Pattern::find_in(&mut slice_pat, haystack), Some(6));

    let mut predicate = |ch: char| ch == 'z';
    assert_eq!(Pattern::find_in(&mut predicate, haystack), Some(8));
}
