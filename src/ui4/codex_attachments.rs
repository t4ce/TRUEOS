//! Path-only drag attachments; no eager file reads while dragging.
use alloc::{string::String, vec::Vec};

pub(super) const LIMIT: usize = 16;
pub(super) const INLINE_BUDGET: usize = 1024;
const PATH_BYTES: usize = 6 * 1024;

pub(super) fn decode(mut bytes: &[u8]) -> Option<Vec<String>> {
    let mut paths = Vec::new();
    while !bytes.is_empty() {
        let len = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let end = 4usize.checked_add(len)?;
        let path = core::str::from_utf8(bytes.get(4..end)?).ok()?;
        // Paths are references, never terminal input or shell commands.
        if path.is_empty() || path.contains('\0') || paths.len() == LIMIT {
            return None;
        }
        paths.push(String::from(path));
        bytes = &bytes[end..];
    }
    (!paths.is_empty()).then_some(paths)
}

/// Reject the whole drop on overflow; do not silently lose part of a group.
pub(super) fn accumulate(paths: &mut Vec<String>, incoming: &[String]) -> bool {
    let mut combined = paths.clone();
    for path in incoming {
        if !combined.contains(path) {
            combined.push(path.clone());
        }
    }
    if combined.len() > LIMIT || combined.iter().map(String::len).sum::<usize>() > PATH_BYTES {
        return false;
    }
    *paths = combined;
    true
}

pub(super) fn question(paths: &[String]) -> &'static str {
    if paths.len() == 1 { "What to do with this file/folder? " }
    else { "What to do with these attachments? " }
}

pub(super) fn inline_text(bytes: &[u8], remaining: usize) -> Option<&str> {
    if bytes.len() > remaining { return None; }
    let text = core::str::from_utf8(bytes).ok()?;
    (!text.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t'))
        .then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_keep_unicode_and_newlines_and_reject_partial_groups() {
        let path = "4:/§hello\nworld";
        let mut bytes = (path.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(path.as_bytes());
        assert_eq!(decode(&bytes).unwrap(), [path]);
        bytes.push(1);
        assert!(decode(&bytes).is_none());
        assert!(decode(&[1,0,0,0,255]).is_none());
        assert!(decode(&[1,0,0,0,0]).is_none());
        assert!(decode(&[]).is_none());
    }
    #[test]
    fn repeated_drops_deduplicate_and_overflow_is_atomic() {
        let mut paths = Vec::new();
        for n in 0..LIMIT {
            assert!(accumulate(&mut paths, &[alloc::format!("/{n}")]));
        }
        assert!(accumulate(&mut paths, &[String::from("/0")]));
        assert!(!accumulate(&mut paths, &[String::from("/extra")]));
        assert_eq!(paths.len(), LIMIT);
        assert!(!accumulate(&mut paths, &["x".repeat(PATH_BYTES + 1)]));
        assert_eq!(paths.len(), LIMIT);
    }
    #[test]
    fn inline_is_whole_utf8_text_with_one_shared_budget() {
        assert_eq!(inline_text("§".as_bytes(), 2), Some("§"));
        assert_eq!(inline_text("§".as_bytes(), 1), None);
        assert_eq!(inline_text(b"\0binary", INLINE_BUDGET), None);
        assert_eq!(inline_text(&[255], INLINE_BUDGET), None);
        assert_eq!(inline_text(b"line\n\t", 6), Some("line\n\t"));
    }
}
