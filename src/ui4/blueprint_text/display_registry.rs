//! Owner-scoped display connections; tokens contain a slot and a generation.

#[derive(Clone, Copy)]
struct Connection<O> {
    owner: O,
    references: u32,
}

#[derive(Clone, Copy)]
struct Slot<O> {
    generation: u32,
    connection: Option<Connection<O>>,
}

pub(super) struct DisplayRegistry<O, const N: usize> {
    slots: [Slot<O>; N],
}

impl<O: Copy + Eq, const N: usize> DisplayRegistry<O, N> {
    pub(super) const fn new() -> Self {
        Self { slots: [Slot { generation: 0, connection: None }; N] }
    }

    pub(super) fn open(&mut self, owner: O) -> Option<u64> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some(connection) = &mut slot.connection
                && connection.owner == owner
            {
                connection.references = connection.references.checked_add(1)?;
                return Some((u64::from(slot.generation) << 32) | (index as u64 + 1));
            }
        }
        let (index, slot) = self.slots.iter_mut().enumerate()
            .find(|(_, slot)| slot.connection.is_none() && slot.generation != u32::MAX)?;
        slot.generation += 1;
        slot.connection = Some(Connection { owner, references: 1 });
        Some((u64::from(slot.generation) << 32) | (index as u64 + 1))
    }

    fn slot_mut(&mut self, owner: O, token: u64) -> Option<&mut Slot<O>> {
        let index = (token as u32).checked_sub(1)? as usize;
        let slot = self.slots.get_mut(index)?;
        if slot.generation != (token >> 32) as u32 || slot.connection?.owner != owner {
            return None;
        }
        Some(slot)
    }

    pub(super) fn valid(&mut self, owner: O, token: u64) -> bool {
        self.slot_mut(owner, token).is_some()
    }

    pub(super) fn retain(&mut self, owner: O, token: u64) -> bool {
        let Some(connection) = self.slot_mut(owner, token).and_then(|slot| slot.connection.as_mut()) else { return false; };
        let Some(references) = connection.references.checked_add(1) else { return false; };
        connection.references = references;
        true
    }

    pub(super) fn close(&mut self, owner: O, token: u64) -> bool {
        let Some(slot) = self.slot_mut(owner, token) else { return false; };
        let connection = slot.connection.as_mut().expect("validated connection");
        connection.references -= 1;
        if connection.references == 0 { slot.connection = None; }
        true
    }

    pub(super) fn release_owner(&mut self, owner: O) {
        for slot in &mut self.slots {
            if slot.connection.is_some_and(|connection| connection.owner == owner) {
                slot.connection = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DisplayRegistry;

    #[test]
    fn connections_cannot_cross_owners_or_outlive_the_last_reference() {
        let mut registry = DisplayRegistry::<u8, 2>::new();
        let token = registry.open(1).unwrap();
        assert!(!registry.valid(2, token));
        assert!(!registry.retain(2, token));
        assert!(!registry.close(2, token));
        assert!(registry.retain(1, token));
        assert!(registry.close(1, token));
        assert!(registry.valid(1, token));
        assert!(registry.close(1, token));
        assert!(!registry.valid(1, token));
        let next = registry.open(1).unwrap();
        assert_ne!(next, token);
        assert!(!registry.valid(1, token));
    }

    #[test]
    fn teardown_revokes_references_before_an_owner_id_is_reused() {
        let mut registry = DisplayRegistry::<u8, 2>::new();
        let old = registry.open(1).unwrap();
        assert_eq!(registry.open(1), Some(old));
        let other = registry.open(2).unwrap();
        assert!(registry.open(3).is_none());
        registry.release_owner(1);
        assert!(!registry.valid(1, old));
        assert!(registry.valid(2, other));
        assert_ne!(registry.open(1).unwrap(), old);
        assert!(!registry.valid(1, 0));
        assert!(!registry.valid(1, u64::MAX));
    }
}
