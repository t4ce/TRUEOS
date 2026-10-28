//! Generation-scoped launch policy and Hull spawn scopes. Native spawn scopes
//! live in each pinned Thread instead; parallel callers never share a hint.
//! Each VM currently has one initial Hull execution context. Concurrent Hull
//! vthreads would need authenticated per-vthread scope keys; serializing a
//! communication page alone would not serialize an entire spawn scope.
use crate::workers::application_policy::{ThreadPlacement, ThreadRole};

#[derive(Clone, Copy)]
struct State {
    generation: u64,
    policy: ThreadPlacement,
    admitted: bool,
    ordinal: usize,
    background: [u32; 4],
    background_count: usize,
    hull_child_role: ThreadRole,
    hull_activity: u32,
}
impl State {
    const EMPTY: Self = Self { generation: 0, policy: ThreadPlacement::BASELINE, admitted: false, ordinal: 0, background: [0; 4], background_count: 0, hull_child_role: ThreadRole::Unclassified, hull_activity: 0 };
}
static STATES: [spin::Mutex<State>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { spin::Mutex::new(State::EMPTY) }; crate::allcaps::hv::VM_ID_LIMIT];

fn with_state<T>(vm: u8, generation: u64, f: impl FnOnce(&mut State) -> T) -> Result<T, i32> {
    let slot = STATES.get(vm as usize).filter(|_| generation != 0).ok_or(22)?;
    let mut state = slot.lock();
    if state.generation != generation {
        *state = State { generation, ..State::EMPTY };
    }
    Ok(f(&mut state))
}

pub(super) fn configure(vm: u8, generation: u64, policy: ThreadPlacement, background: &[u32]) -> Result<(), i32> {
    with_state(vm, generation, |state| {
        if state.admitted { return Err(16); }
        state.policy = policy;
        state.background_count = 0;
        for slot in background.iter().copied().filter(|slot| *slot >= 2) {
            if state.background_count >= policy.background_cap || state.background_count == state.background.len() { break; }
            if state.background[..state.background_count].contains(&slot) { continue; }
            state.background[state.background_count] = slot;
            state.background_count += 1;
        }
        Ok(())
    })?
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AdmissionPlan {
    pub policy: ThreadPlacement,
    pub ordinal: usize,
    background: [u32; 4],
    background_count: usize,
}
impl AdmissionPlan {
    pub fn background(&self) -> &[u32] { &self.background[..self.background_count] }
}

pub(super) fn admit(vm: u8, generation: u64) -> Result<AdmissionPlan, i32> {
    with_state(vm, generation, |state| {
        state.admitted = true;
        let ordinal = state.ordinal;
        state.ordinal = state.ordinal.wrapping_add(1);
        AdmissionPlan { policy: state.policy, ordinal, background: state.background, background_count: state.background_count }
    })
}

pub(super) fn hull_role(vm: u8, generation: u64) -> Result<ThreadRole, i32> {
    with_state(vm, generation, |state| state.hull_child_role)
}

pub(super) fn swap_hull_scope(vm: u8, generation: u64, activity: bool, value: u32) -> Result<u32, i32> {
    if activity {
        if value > 3 { return Err(22); }
        with_state(vm, generation, |state| core::mem::replace(&mut state.hull_activity, value))
    } else {
        let role = ThreadRole::from_raw(value).ok_or(22)?;
        with_state(vm, generation, |state| core::mem::replace(&mut state.hull_child_role, role) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_policy_is_immutable_after_admission_and_resets_with_generation() {
        let isolated = ThreadPlacement::from_raw(1, 2).unwrap();
        configure(43, 100, isolated, &[2, 3, 4]).unwrap();
        assert_eq!(swap_hull_scope(43, 100, false, 1), Ok(0));
        assert_eq!(hull_role(43, 100), Ok(ThreadRole::Foreground));
        let first = admit(43, 100).unwrap();
        assert_eq!(first.policy, isolated);
        assert_eq!(first.ordinal, 0);
        assert_eq!(first.background(), &[2, 3]);
        assert_eq!(configure(43, 100, ThreadPlacement::BASELINE, &[4, 5]), Err(16));
        let second = admit(43, 100).unwrap();
        assert_eq!(second.ordinal, 1);
        assert_eq!(second.background(), first.background());
        assert_eq!(hull_role(43, 101), Ok(ThreadRole::Unclassified));
        let reset = admit(43, 101).unwrap();
        assert_eq!(reset.policy, ThreadPlacement::BASELINE);
        assert_eq!(reset.ordinal, 0);
        assert!(reset.background().is_empty());
    }

    #[test]
    fn hull_scopes_are_nested_validated_and_independent_between_vms() {
        assert_eq!(swap_hull_scope(44, 100, false, 1), Ok(0));
        assert_eq!(swap_hull_scope(44, 100, false, 2), Ok(1));
        assert_eq!(swap_hull_scope(45, 100, false, 3), Ok(0));
        assert_eq!(swap_hull_scope(44, 100, false, 99), Err(22));
        assert_eq!(hull_role(44, 100), Ok(ThreadRole::TokioPool));
        assert_eq!(swap_hull_scope(44, 100, false, 1), Ok(2));
        assert_eq!(swap_hull_scope(44, 100, false, 0), Ok(1));
        assert_eq!(hull_role(45, 100), Ok(ThreadRole::DedicatedBlocking));
        assert_eq!(swap_hull_scope(44, 100, true, 3), Ok(0));
        assert_eq!(swap_hull_scope(44, 100, true, 2), Ok(3));
        assert_eq!(swap_hull_scope(44, 100, true, 4), Err(22));
        assert!(admit(0, 0).is_err());
    }
}
