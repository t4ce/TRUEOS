//! Pure topology policy. Leave at least one general AP2+ carrier available.
pub(super) fn reserved_slot(topology: usize, limit: usize) -> Option<u32> {
    // Never substitute another AP for LastAP, and retain one ordinary AP2+.
    (topology >= 4 && topology <= limit).then(|| (topology - 1) as u32)
}

pub(crate) fn is_codex_archive(archive: &str) -> bool {
    matches!(
        archive.rsplit('/').next().unwrap_or(archive),
        "codex" | "codex.bp" | "codex-check" | "codex-check.bp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_uses_only_lastap_and_preserves_general_capacity() {
        assert_eq!(reserved_slot(14, 64), Some(13));
        assert_eq!(reserved_slot(3, 64), None);
        assert_eq!(reserved_slot(4, 64), Some(3));
        assert_eq!(reserved_slot(100, 64), None);
        assert!(is_codex_archive("/apps/codex.bp"));
        assert!(!is_codex_archive("codex-other.bp"));
    }
}
