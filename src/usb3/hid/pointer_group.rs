//! Shared relative pointer state for physical devices assigned to one persona.
//! Device samples and HUT records remain independent; only cursor presentation
//! uses this canonical identity, position and union of held buttons.

const MAX_MEMBERS: usize = 64;
const MAX_GROUPS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Identity {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Binding {
    pub source: Identity,
    pub combo_id: u32,
    pub canonical: Identity,
    pub buttons: u32,
    /// Previous device position, used only for a group with no earlier state.
    pub seed_x: f64,
    pub seed_y: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Motion {
    pub source: Identity,
    pub dx: f64,
    pub dy: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub combo_id: u32,
    pub canonical: Identity,
    pub x: f64,
    pub y: f64,
    pub buttons: u32,
    pub seq: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Update {
    pub snapshot: Snapshot,
    pub moved: bool,
    pub buttons_changed: bool,
}

pub(crate) struct Changes {
    /// Existing groups precede newly split groups. This lets a consumer retarget
    /// an old canonical route before its device ID is reused by another group.
    pub updates: [Option<Update>; MAX_GROUPS],
    pub retired: [Option<Identity>; MAX_GROUPS],
    pub active_combo: Option<u32>,
}

#[derive(Clone, Copy)]
struct Member {
    identity: Identity,
    combo_id: u32,
    buttons: u32,
}

#[derive(Clone, Copy)]
struct Group {
    snapshot: Snapshot,
    published: bool,
}

pub(crate) struct GroupState {
    members: [Option<Member>; MAX_MEMBERS],
    groups: [Option<Group>; MAX_GROUPS],
}

fn normalized(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

impl GroupState {
    pub(crate) const fn new() -> Self {
        Self {
            members: [None; MAX_MEMBERS],
            groups: [None; MAX_GROUPS],
        }
    }

    pub(crate) fn contains(&self, identity: Identity) -> bool {
        self.members
            .iter()
            .flatten()
            .any(|member| member.identity == identity)
    }

    /// Reconcile the current physical HUT inventory and apply at most one
    /// relative report. An idle member still contributes its own held buttons.
    /// Failed capacity/identity validation leaves the previous state untouched.
    pub(crate) fn apply(
        &mut self,
        bindings: &[Binding],
        motion: Option<Motion>,
    ) -> Option<Changes> {
        if bindings.len() > MAX_MEMBERS {
            return None;
        }
        let mut members: [Option<Member>; MAX_MEMBERS] = [None; MAX_MEMBERS];
        let mut groups: [Option<Group>; MAX_GROUPS] = [None; MAX_GROUPS];
        let mut member_count = 0;
        let mut group_count = 0;
        for binding in bindings {
            if binding.combo_id == 0 {
                return None;
            }
            if members[..member_count]
                .iter()
                .flatten()
                .any(|member| member.identity == binding.source)
            {
                return None;
            }
            members[member_count] = Some(Member {
                identity: binding.source,
                combo_id: binding.combo_id,
                buttons: binding.buttons,
            });
            member_count += 1;
            let existing = groups[..group_count].iter().position(|group| {
                group.is_some_and(|group| group.snapshot.combo_id == binding.combo_id)
            });
            if let Some(index) = existing {
                if groups[index]?.snapshot.canonical != binding.canonical {
                    return None;
                }
                continue;
            }
            if group_count == MAX_GROUPS {
                return None;
            }
            let seed = bindings.iter().find(|candidate| {
                candidate.combo_id == binding.combo_id && candidate.source == binding.canonical
            })?;
            let prior = self
                .groups
                .iter()
                .flatten()
                .find(|group| group.snapshot.combo_id == binding.combo_id)
                .copied()
                .or_else(|| {
                    let old_member = self
                        .members
                        .iter()
                        .flatten()
                        .find(|member| member.identity == seed.source)?;
                    self.groups
                        .iter()
                        .flatten()
                        .find(|group| group.snapshot.combo_id == old_member.combo_id)
                        .copied()
                });
            let (x, y, seq, published) = prior
                .map(|group| {
                    (
                        group.snapshot.x,
                        group.snapshot.y,
                        group.snapshot.seq,
                        group.published,
                    )
                })
                .unwrap_or((normalized(seed.seed_x), normalized(seed.seed_y), 0, false));
            groups[group_count] = Some(Group {
                snapshot: Snapshot {
                    combo_id: binding.combo_id,
                    canonical: binding.canonical,
                    x,
                    y,
                    seq,
                    buttons: 0,
                },
                published,
            });
            group_count += 1;
        }
        for group in groups[..group_count].iter_mut().flatten() {
            group.snapshot.buttons = members[..member_count]
                .iter()
                .flatten()
                .filter(|member| member.combo_id == group.snapshot.combo_id)
                .fold(0, |buttons, member| buttons | member.buttons);
        }
        let active_combo = motion.and_then(|motion| {
            let member = members[..member_count]
                .iter()
                .flatten()
                .find(|member| member.identity == motion.source)?;
            let group = groups[..group_count]
                .iter_mut()
                .flatten()
                .find(|group| group.snapshot.combo_id == member.combo_id)?;
            group.snapshot.x = normalized(group.snapshot.x + motion.dx);
            group.snapshot.y = normalized(group.snapshot.y + motion.dy);
            group.published = true;
            Some(member.combo_id)
        });
        let mut changes = Changes {
            updates: [None; MAX_GROUPS],
            retired: [None; MAX_GROUPS],
            active_combo,
        };
        let mut update_count = 0;
        let mut retire_count = 0;
        // Existing personas go first, including canonical promotions. Never
        // retire a surviving group merely because its leading device changed.
        for old in self.groups.iter().flatten() {
            if let Some(next) = groups[..group_count]
                .iter_mut()
                .flatten()
                .find(|next| next.snapshot.combo_id == old.snapshot.combo_id)
            {
                if next.published
                    && (active_combo == Some(next.snapshot.combo_id)
                        || next.snapshot.canonical != old.snapshot.canonical
                        || next.snapshot.buttons != old.snapshot.buttons
                        || next.snapshot.x != old.snapshot.x
                        || next.snapshot.y != old.snapshot.y)
                {
                    next.snapshot.seq = next.snapshot.seq.wrapping_add(1);
                    changes.updates[update_count] = Some(Update {
                        snapshot: next.snapshot,
                        moved: next.snapshot.x != old.snapshot.x
                            || next.snapshot.y != old.snapshot.y,
                        buttons_changed: next.snapshot.buttons != old.snapshot.buttons,
                    });
                    update_count += 1;
                }
            } else if old.published {
                changes.retired[retire_count] = Some(old.snapshot.canonical);
                retire_count += 1;
            }
        }
        for next in groups[..group_count].iter_mut().flatten() {
            if next.published
                && !self
                    .groups
                    .iter()
                    .flatten()
                    .any(|old| old.snapshot.combo_id == next.snapshot.combo_id)
            {
                next.snapshot.seq = next.snapshot.seq.wrapping_add(1);
                changes.updates[update_count] = Some(Update {
                    snapshot: next.snapshot,
                    moved: false,
                    buttons_changed: next.snapshot.buttons != 0,
                });
                update_count += 1;
            }
        }
        self.members = members;
        self.groups = groups;
        Some(changes)
    }

    /// Pointer lock rebases the persona, so switching devices cannot restore a
    /// stale pre-lock position. No button event is synthesized here.
    pub(crate) fn center(&mut self, identity: Identity) -> Option<Snapshot> {
        let combo = self
            .members
            .iter()
            .flatten()
            .find(|member| member.identity == identity)
            .map(|member| member.combo_id);
        let group = self.groups.iter_mut().flatten().find(|group| {
            Some(group.snapshot.combo_id) == combo || group.snapshot.canonical == identity
        })?;
        group.snapshot.x = 0.5;
        group.snapshot.y = 0.5;
        Some(group.snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MOUSE: Identity = Identity {
        controller_id: 4,
        slot_id: 1,
        ep_target: 3,
        hid_kind: 2,
    };
    const PAD: Identity = Identity {
        controller_id: 0x4932_4330,
        slot_id: 0x4932_0001,
        ep_target: 0,
        hid_kind: 2,
    };

    fn binding(source: Identity, combo_id: u32, canonical: Identity, buttons: u32) -> Binding {
        Binding {
            source,
            combo_id,
            canonical,
            buttons,
            seed_x: 0.5,
            seed_y: 0.5,
        }
    }
    fn motion(source: Identity, dx: f64, dy: f64) -> Option<Motion> {
        Some(Motion { source, dx, dy })
    }
    fn update(changes: &Changes, combo: u32) -> Snapshot {
        changes
            .updates
            .iter()
            .flatten()
            .find(|update| update.snapshot.combo_id == combo)
            .unwrap()
            .snapshot
    }

    #[test]
    fn alternating_devices_add_to_one_shared_position_and_sequence() {
        let mut state = GroupState::new();
        let devices = [binding(MOUSE, 1, MOUSE, 0), binding(PAD, 1, MOUSE, 0)];
        let first = state.apply(&devices, motion(MOUSE, 0.2, -0.1)).unwrap();
        let first = update(&first, 1);
        let second = state.apply(&devices, motion(PAD, 0.1, 0.2)).unwrap();
        let second = update(&second, 1);
        assert_eq!(first.canonical, MOUSE);
        assert_eq!(second.canonical, MOUSE);
        assert!((second.x - 0.8).abs() < 1e-12 && (second.y - 0.6).abs() < 1e-12);
        assert_eq!(second.seq, first.seq + 1);
    }

    #[test]
    fn buttons_are_per_device_and_idle_second_device_cannot_release_a_drag() {
        let mut state = GroupState::new();
        let mut devices = [binding(MOUSE, 1, MOUSE, 1), binding(PAD, 1, MOUSE, 0)];
        assert_eq!(
            update(&state.apply(&devices, motion(MOUSE, 0.0, 0.0)).unwrap(), 1).buttons,
            1
        );
        let idle = state.apply(&devices, motion(PAD, 0.1, 0.0)).unwrap();
        assert_eq!(update(&idle, 1).buttons, 1);
        assert!(
            !idle
                .updates
                .iter()
                .flatten()
                .next()
                .unwrap()
                .buttons_changed
        );
        devices[1].buttons = 2;
        assert_eq!(
            update(&state.apply(&devices, motion(PAD, 0.0, 0.0)).unwrap(), 1).buttons,
            3
        );
        devices[0].buttons = 0;
        assert_eq!(
            update(&state.apply(&devices, motion(MOUSE, 0.0, 0.0)).unwrap(), 1).buttons,
            2
        );
    }

    #[test]
    fn canonical_removal_promotes_peer_without_losing_group_or_position() {
        let mut state = GroupState::new();
        let devices = [binding(MOUSE, 1, MOUSE, 1), binding(PAD, 1, MOUSE, 2)];
        state.apply(&devices, motion(MOUSE, 0.3, 0.1)).unwrap();
        let changes = state.apply(&[binding(PAD, 1, PAD, 2)], None).unwrap();
        assert!(changes.retired.iter().all(Option::is_none));
        let remaining = update(&changes, 1);
        assert_eq!(remaining.canonical, PAD);
        assert_eq!(remaining.buttons, 2);
        assert_eq!((remaining.x, remaining.y), (0.8, 0.6));
        assert!(!state.contains(MOUSE));
        assert!(state.contains(PAD));
    }

    #[test]
    fn alias_removal_releases_only_its_buttons_and_last_member_retires_cursor() {
        let mut state = GroupState::new();
        state
            .apply(
                &[binding(MOUSE, 1, MOUSE, 1), binding(PAD, 1, MOUSE, 2)],
                motion(PAD, 0.0, 0.0),
            )
            .unwrap();
        let changes = state.apply(&[binding(MOUSE, 1, MOUSE, 1)], None).unwrap();
        assert_eq!(update(&changes, 1).buttons, 1);
        let changes = state.apply(&[], None).unwrap();
        assert_eq!(
            changes.retired.into_iter().flatten().collect::<Vec<_>>(),
            [MOUSE]
        );
        assert!(changes.updates.iter().all(Option::is_none));
    }

    #[test]
    fn manual_split_starts_at_shared_position_and_orders_old_persona_first() {
        let mut state = GroupState::new();
        state
            .apply(
                &[binding(MOUSE, 1, MOUSE, 1), binding(PAD, 1, MOUSE, 0)],
                motion(MOUSE, 0.3, -0.2),
            )
            .unwrap();
        // The canonical mouse moved to a new bucket. The old bucket's keyboard
        // stays with the promoted trackpad; retarget it before reusing MOUSE.
        let changes = state
            .apply(
                &[binding(MOUSE, 2, MOUSE, 1), binding(PAD, 1, PAD, 0)],
                None,
            )
            .unwrap();
        let updates: Vec<_> = changes.updates.into_iter().flatten().collect();
        assert_eq!(
            updates
                .iter()
                .map(|update| update.snapshot.combo_id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!((updates[0].snapshot.x, updates[0].snapshot.y), (0.8, 0.3));
        assert_eq!((updates[1].snapshot.x, updates[1].snapshot.y), (0.8, 0.3));
        assert_eq!(updates[0].snapshot.buttons, 0);
        assert_eq!(updates[1].snapshot.buttons, 1);
        let devices = [binding(MOUSE, 2, MOUSE, 1), binding(PAD, 1, PAD, 0)];
        let moved = state.apply(&devices, motion(PAD, -0.1, 0.0)).unwrap();
        assert!((update(&moved, 1).x - 0.7).abs() < 1e-12);
        assert!(
            moved
                .updates
                .iter()
                .flatten()
                .all(|update| update.snapshot.combo_id != 2)
        );
    }

    #[test]
    fn group_center_rebases_all_members_and_clamps_relative_motion() {
        let mut state = GroupState::new();
        let devices = [binding(MOUSE, 1, MOUSE, 0), binding(PAD, 1, MOUSE, 0)];
        state.apply(&devices, motion(MOUSE, 0.4, 0.3)).unwrap();
        assert_eq!(
            state.center(PAD).map(|snapshot| (snapshot.x, snapshot.y)),
            Some((0.5, 0.5))
        );
        let changes = state.apply(&devices, motion(PAD, 3.0, -2.0)).unwrap();
        assert_eq!((update(&changes, 1).x, update(&changes, 1).y), (1.0, 0.0));
    }

    #[test]
    fn invalid_inventory_cannot_discard_existing_held_state() {
        let mut state = GroupState::new();
        let devices = [binding(MOUSE, 1, MOUSE, 1)];
        state.apply(&devices, motion(MOUSE, 0.2, 0.0)).unwrap();
        assert!(state.apply(&[binding(PAD, 1, MOUSE, 0)], None).is_none());
        let changes = state.apply(&devices, motion(MOUSE, 0.0, 0.0)).unwrap();
        assert_eq!(update(&changes, 1).buttons, 1);
        assert_eq!(update(&changes, 1).x, 0.7);
    }
}
