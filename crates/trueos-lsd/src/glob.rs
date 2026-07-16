extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

const UNKNOWN: u8 = 0;
const NO_MATCH: u8 = 1;
const MATCH: u8 = 2;

/// Match the basename-oriented glob syntax used by `--ignore-glob`.
///
/// TRUEOS paths are filtered one component at a time, matching upstream lsd's
/// use of `GlobSet::is_match` on each directory entry's file name. Supported
/// metacharacters are `*`, `?`, character classes/ranges, class negation, and
/// backslash escaping.
pub(crate) fn matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let stride = text.len().saturating_add(1);
    let cells = pattern.len().saturating_add(1).saturating_mul(stride);
    let mut memo = vec![UNKNOWN; cells];
    matches_from(&pattern, &text, 0, 0, stride, &mut memo)
}

fn matches_from(
    pattern: &[char],
    text: &[char],
    pattern_index: usize,
    text_index: usize,
    stride: usize,
    memo: &mut [u8],
) -> bool {
    let cell = pattern_index
        .saturating_mul(stride)
        .saturating_add(text_index);
    if let Some(cached) = memo.get(cell).copied() {
        match cached {
            MATCH => return true,
            NO_MATCH => return false,
            _ => {}
        }
    }

    let result = match pattern.get(pattern_index).copied() {
        None => text_index == text.len(),
        Some('*') => {
            let mut next_pattern = pattern_index + 1;
            while pattern.get(next_pattern) == Some(&'*') {
                next_pattern += 1;
            }
            matches_from(pattern, text, next_pattern, text_index, stride, memo)
                || (text_index < text.len()
                    && matches_from(pattern, text, pattern_index, text_index + 1, stride, memo))
        }
        Some('?') => {
            text_index < text.len()
                && matches_from(pattern, text, pattern_index + 1, text_index + 1, stride, memo)
        }
        Some('[') => match character_class(pattern, pattern_index, text.get(text_index).copied()) {
            Some((class_matches, next_pattern)) => {
                class_matches
                    && matches_from(pattern, text, next_pattern, text_index + 1, stride, memo)
            }
            None => {
                text.get(text_index) == Some(&'[')
                    && matches_from(pattern, text, pattern_index + 1, text_index + 1, stride, memo)
            }
        },
        Some('\\') => {
            let (literal, next_pattern) = match pattern.get(pattern_index + 1).copied() {
                Some(literal) => (literal, pattern_index + 2),
                None => ('\\', pattern_index + 1),
            };
            text.get(text_index) == Some(&literal)
                && matches_from(pattern, text, next_pattern, text_index + 1, stride, memo)
        }
        Some(literal) => {
            text.get(text_index) == Some(&literal)
                && matches_from(pattern, text, pattern_index + 1, text_index + 1, stride, memo)
        }
    };

    if let Some(cached) = memo.get_mut(cell) {
        *cached = if result { MATCH } else { NO_MATCH };
    }
    result
}

fn character_class(
    pattern: &[char],
    opening_index: usize,
    candidate: Option<char>,
) -> Option<(bool, usize)> {
    let mut index = opening_index + 1;
    let negated = matches!(pattern.get(index), Some('!') | Some('^'));
    if negated {
        index += 1;
    }

    let mut class_matches = false;
    let mut has_item = false;
    while index < pattern.len() {
        if pattern[index] == ']' && has_item {
            return Some((candidate.is_some() && (class_matches != negated), index + 1));
        }

        let (start, after_start) = class_character(pattern, index)?;
        has_item = true;
        index = after_start;

        if pattern.get(index) == Some(&'-')
            && pattern.get(index + 1).is_some()
            && pattern.get(index + 1) != Some(&']')
        {
            let (end, after_end) = class_character(pattern, index + 1)?;
            if let Some(candidate) = candidate
                && start <= candidate
                && candidate <= end
            {
                class_matches = true;
            }
            index = after_end;
        } else if candidate == Some(start) {
            class_matches = true;
        }
    }

    None
}

fn class_character(pattern: &[char], index: usize) -> Option<(char, usize)> {
    match pattern.get(index).copied()? {
        '\\' => pattern
            .get(index + 1)
            .copied()
            .map(|literal| (literal, index + 2)),
        literal => Some((literal, index + 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn matches_wildcards_and_literals() {
        assert!(matches("*.tmp", "draft.tmp"));
        assert!(matches("file-??.log", "file-01.log"));
        assert!(matches("literal\\*", "literal*"));
        assert!(!matches("*.tmp", "draft.rs"));
    }

    #[test]
    fn matches_character_classes() {
        assert!(matches("log-[0-9].txt", "log-7.txt"));
        assert!(matches("[!a-c]*", "draft"));
        assert!(!matches("[!a-c]*", "beta"));
        assert!(matches("broken[", "broken["));
    }

    #[test]
    fn handles_unicode_by_character() {
        assert!(matches("?.txt", "§.txt"));
        assert!(matches("[äöü].txt", "ö.txt"));
    }
}
