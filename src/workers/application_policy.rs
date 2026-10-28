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

/// Admission roles describe a thread's contract, not the work it is polling.
/// Tokio pool threads can execute both scheduler loops and blocking closures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum ThreadRole {
    Unclassified = 0,
    Foreground = 1,
    TokioPool = 2,
    DedicatedBlocking = 3,
}

impl ThreadRole {
    pub(crate) const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Unclassified), 1 => Some(Self::Foreground),
            2 => Some(Self::TokioPool), 3 => Some(Self::DedicatedBlocking), _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum PlacementMode {
    SingleCarrier = 0,
    ForegroundIsolated = 1,
    Spread = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ThreadPlacement {
    pub mode: PlacementMode,
    pub background_cap: usize,
}

impl ThreadPlacement {
    pub(crate) const BASELINE: Self = Self { mode: PlacementMode::SingleCarrier, background_cap: 2 };

    pub(crate) fn from_raw(mode: u32, background_cap: u32) -> Option<Self> {
        let mode = match mode {
            0 => PlacementMode::SingleCarrier, 1 => PlacementMode::ForegroundIsolated,
            2 => PlacementMode::Spread, _ => return None,
        };
        (1..=4).contains(&background_cap).then_some(Self { mode, background_cap: background_cap as usize })
    }

    /// Select once, before the continuation starts. Neither activity changes
    /// nor work stealing may migrate its stack, TLS, or address space.
    pub(crate) fn carrier(self, role: ThreadRole, foreground: Option<u32>, background: &[u32], ordinal: usize) -> Option<u32> {
        let foreground = foreground.filter(|slot| *slot >= 2)?;
        let eligible = || background.iter().copied().filter(|slot| *slot >= 2 && *slot != foreground).take(self.background_cap);
        match self.mode {
            PlacementMode::SingleCarrier => Some(foreground),
            PlacementMode::ForegroundIsolated => match role {
                ThreadRole::Unclassified | ThreadRole::Foreground => Some(foreground),
                ThreadRole::TokioPool | ThreadRole::DedicatedBlocking => {
                    let count = eligible().count();
                    (count != 0).then(|| eligible().nth(ordinal % count)).flatten()
                }
            },
            PlacementMode::Spread => {
                let count = eligible().count();
                if count == 0 { return None; }
                let index = ordinal % (count + 1);
                if index == 0 { Some(foreground) } else { eligible().nth(index - 1) }
            }
        }
    }
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

    #[test]
    fn isolated_pool_uses_bounded_additional_aps_and_retains_foreground() {
        let policy = ThreadPlacement::from_raw(1, 2).unwrap();
        let slots = [0, 1, 2, 3, 4, 7];
        for role in [ThreadRole::Unclassified, ThreadRole::Foreground] {
            assert_eq!(policy.carrier(role, Some(7), &slots, 19), Some(7));
        }
        for (ordinal, expected) in [2, 3, 2, 3].into_iter().enumerate() {
            assert_eq!(policy.carrier(ThreadRole::TokioPool, Some(7), &slots, ordinal), Some(expected));
            assert_eq!(policy.carrier(ThreadRole::DedicatedBlocking, Some(7), &slots, ordinal), Some(expected));
        }
    }

    #[test]
    fn modes_preserve_baseline_and_never_substitute_ui_or_bsp() {
        for role in [ThreadRole::Unclassified, ThreadRole::Foreground, ThreadRole::TokioPool, ThreadRole::DedicatedBlocking] {
            assert_eq!(ThreadPlacement::BASELINE.carrier(role, Some(7), &[2, 3], 9), Some(7));
        }
        let spread = ThreadPlacement::from_raw(2, 1).unwrap();
        assert_eq!((0..4).map(|i| spread.carrier(ThreadRole::Foreground, Some(7), &[0, 1, 2, 3], i)).collect::<alloc::vec::Vec<_>>(), [Some(7), Some(2), Some(7), Some(2)]);
        assert_eq!(spread.carrier(ThreadRole::TokioPool, None, &[2], 0), None);
        assert_eq!(spread.carrier(ThreadRole::TokioPool, Some(1), &[2], 0), None);
        assert_eq!(spread.carrier(ThreadRole::TokioPool, Some(7), &[0, 1, 7], 0), None);
        assert!(ThreadPlacement::from_raw(3, 2).is_none());
        assert!(ThreadPlacement::from_raw(1, 0).is_none());
        assert!(ThreadPlacement::from_raw(1, 5).is_none());
        assert!(ThreadRole::from_raw(4).is_none());
    }
}
