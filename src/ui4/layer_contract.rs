//! Weighted hardware leases for indivisible one- or two-surface windows.
//!
//! Slot 0 composes windows without a lease. Slots 1..=3 hold whole groups in
//! visual order. A group is never split between hardware and composition;
//! slot 4 remains exclusively owned by interaction chrome.

pub(super) const REQUIRED_PLANE_MASK: u8 = 0b1111;
const LEASE_SLOTS: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LeaseGroup {
    pub(super) window: u32,
    pub(super) slots: u8,
    pub(super) last_hot_ms: u64,
}

pub(super) type GroupPlan = [Option<LeaseGroup>; 3];

/// Plan first, mutate later. Insufficient revocable capacity changes nothing.
/// Input order is physical bottom-to-top order. A newly admitted window goes
/// on top; an existing dragged window stays put unless focus explicitly raises it.
pub(super) fn claim_group(
    current: GroupPlan, requested: LeaseGroup, now_ms: u64, grace_ms: u64, raise: bool,
) -> Option<GroupPlan> {
    if !(1..=2).contains(&requested.slots) { return None; }
    let mut plan = current;
    if let Some(index) = plan.iter().position(|group| group.is_some_and(|g| g.window == requested.window)) {
        if plan[index]?.slots != requested.slots { return None; }
        plan[index] = Some(requested);
        if raise {
            plan[index] = None;
            compact(&mut plan);
            let free = plan.iter().position(Option::is_none)?;
            plan[free] = Some(requested);
        }
        return Some(plan);
    }
    while used(plan) + requested.slots > LEASE_SLOTS {
        let victim = plan.iter().enumerate()
            .filter_map(|(index, group)| group.map(|group| (index, group)))
            .filter(|(_, group)| now_ms.saturating_sub(group.last_hot_ms) >= grace_ms)
            .min_by_key(|(_, group)| group.last_hot_ms)
            .map(|(index, _)| index)?;
        plan[victim] = None;
    }
    compact(&mut plan);
    let free = plan.iter().position(Option::is_none)?;
    plan[free] = Some(requested);
    Some(plan)
}

pub(super) fn used(plan: GroupPlan) -> u8 {
    plan.into_iter().flatten().map(|group| group.slots).sum()
}

fn compact(plan: &mut GroupPlan) {
    let mut output = [None; 3];
    for (index, group) in plan.iter().flatten().copied().enumerate() {
        output[index] = Some(group);
    }
    *plan = output;
}

/// First slot for a right-aligned group layout. Whole groups remain adjacent;
/// any spare capacity is below them, never between their member surfaces.
pub(super) fn first_slot(plan: GroupPlan) -> usize {
    4 - usize::from(used(plan))
}

/// Render-target tokens occupy a namespace separate from WindowRecord slots
/// (whose low words are 1..=256). The complete 16-bit generation is preserved.
/// These tokens identify producer state, never an independently movable window.
pub(super) const BACKGROUND_TARGET_BIT: u32 = 1 << 15;
pub(super) const fn background_target(window: u32) -> u32 { window | BACKGROUND_TARGET_BIT }

#[cfg(test)]
mod tests {
    use super::*;
    fn g(window: u32, slots: u8, hot: u64) -> LeaseGroup { LeaseGroup { window, slots, last_hot_ms: hot } }
    #[test]
    fn dual_promotion_demotes_two_idle_singles_atomically() {
        let initial = [Some(g(1,1,0)), Some(g(2,1,100)), Some(g(3,1,900))];
        let result = claim_group(initial,g(4,2,1000),1000,500,true).unwrap();
        assert_eq!(result,[Some(g(3,1,900)),Some(g(4,2,1000)),None]);
        assert_eq!(first_slot(result),1);
    }
    #[test]
    fn dual_and_single_can_be_dragged_without_trading_slots() {
        let initial = [Some(g(1,2,900)),Some(g(2,1,900)),None];
        let result = claim_group(initial,g(1,2,1000),1000,500,false).unwrap();
        assert_eq!(result,[Some(g(1,2,1000)),Some(g(2,1,900)),None]);
        assert!(claim_group(result,g(3,1,1001),1001,500,true).is_none());
    }
    #[test]
    fn not_enough_idle_capacity_never_revokes_a_prefix() {
        let initial = [Some(g(1,1,0)),Some(g(2,1,900)),Some(g(3,1,900))];
        assert!(claim_group(initial,g(4,2,1000),1000,500,true).is_none());
        assert_eq!(used(initial),3);
    }
    #[test]
    fn second_dual_waits_for_first_dual_to_be_revocable() {
        let initial=[Some(g(1,2,900)),None,None];
        assert!(claim_group(initial,g(2,2,1000),1000,500,true).is_none());
        assert_eq!(claim_group(initial,g(2,2,1400),1400,500,true),Some([Some(g(2,2,1400)),None,None]));
    }
    #[test]
    fn focus_raises_both_members_over_the_other_dragged_window() {
        let initial=[Some(g(1,2,900)),Some(g(2,1,900)),None];
        assert_eq!(claim_group(initial,g(1,2,1000),1000,500,true),Some([Some(g(2,1,900)),Some(g(1,2,1000)),None]));
    }
    #[test]
    fn one_free_slot_admits_a_single_but_not_half_a_dual() {
        let initial=[Some(g(1,2,900)),None,None];
        assert!(claim_group(initial,g(2,1,1000),1000,500,true).is_some());
        assert!(claim_group(initial,g(2,2,1000),1000,500,true).is_none());
    }
    #[test]
    fn stale_time_or_invalid_width_cannot_steal_a_live_lease() {
        let initial=[Some(g(1,2,1000)),Some(g(2,1,1000)),None];
        assert!(claim_group(initial,g(3,2,1),1,500,true).is_none());
        assert!(claim_group(initial,g(3,0,2000),2000,500,true).is_none());
        assert!(claim_group(initial,g(3,3,2000),2000,500,true).is_none());
    }
    #[test]
    fn surface_tokens_preserve_generations_without_aliasing_window_slots() {
        for generation in [1u32,2,32768,65535] {
            for slot in 1..=256 {
                let window=generation<<16|slot;
                let target=background_target(window);
                assert_eq!(target>>16,generation);
                assert!(target as u16>256);
                assert_eq!(target & !BACKGROUND_TARGET_BIT,window);
            }
        }
    }
}
