//! Retained scene ownership shared by TRUEOS game simulations and renderers.
//!
//! This is deliberately a data seam, not a graphics API.  The game owns all
//! writes; a renderer consumes the published rows, liveness and generation
//! mirrors read-only and is free to derive cull lists and indirect commands.
//! That is the same ownership split as Helio's `helio-scenedb` integration,
//! without importing WGPU or making the operating-system runtime depend on a
//! hosted renderer.

use alloc::vec::Vec;

/// Stable slot + generation identity.  Reusing a retired row never makes an
/// old handle valid again.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SceneHandle {
    pub slot: u32,
    pub generation: u32,
}

/// One contiguous row range changed since the preceding publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirtyRows {
    pub first: u32,
    pub count: u32,
}

impl DirtyRows {
    pub const EMPTY: Self = Self { first: 0, count: 0 };

    pub const fn end(self) -> u32 {
        self.first.saturating_add(self.count)
    }
}

/// Read-only frame-boundary view.  `rows`, `alive`, and `generations` have
/// identical lengths and therefore map directly to three persistent GPU
/// buffers.  Renderer-owned visibility and indirect buffers are intentionally
/// absent from this type.
pub struct PublishedScene<'a, T> {
    pub epoch: u64,
    pub rows: &'a [T],
    pub alive: &'a [u32],
    pub generations: &'a [u32],
    pub dirty: DirtyRows,
    pub live_count: usize,
}

/// Handle-addressed retained rows with SceneDB-shaped publication semantics.
///
/// Removed row bytes remain unspecified and are never read while `alive == 0`.
/// This lets removal upload only the liveness/generation mirrors and allows a
/// later insertion to reuse the allocation without moving any other object.
pub struct SceneStore<T> {
    rows: Vec<T>,
    alive: Vec<u32>,
    generations: Vec<u32>,
    free: Vec<u32>,
    dirty: Option<(usize, usize)>,
    live_count: usize,
    epoch: u64,
}

impl<T> SceneStore<T> {
    pub const fn new() -> Self {
        Self {
            rows: Vec::new(),
            alive: Vec::new(),
            generations: Vec::new(),
            free: Vec::new(),
            dirty: None,
            live_count: 0,
            epoch: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            rows: Vec::with_capacity(capacity),
            alive: Vec::with_capacity(capacity),
            generations: Vec::with_capacity(capacity),
            free: Vec::new(),
            dirty: None,
            live_count: 0,
            epoch: 0,
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Number of allocated rows, including retired rows available for reuse.
    pub fn len(&self) -> usize {
        self.row_count()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub const fn live_count(&self) -> usize {
        self.live_count
    }

    pub fn insert(&mut self, value: T) -> SceneHandle {
        let slot = if let Some(slot) = self.free.pop() {
            let index = slot as usize;
            self.rows[index] = value;
            self.alive[index] = 1;
            slot
        } else {
            let slot = u32::try_from(self.rows.len()).expect("scene row count exceeds u32");
            self.rows.push(value);
            self.alive.push(1);
            self.generations.push(1);
            slot
        };
        self.live_count += 1;
        self.mark_dirty(slot as usize);
        SceneHandle {
            slot,
            generation: self.generations[slot as usize],
        }
    }

    pub fn contains(&self, handle: SceneHandle) -> bool {
        let index = handle.slot as usize;
        self.alive.get(index) == Some(&1) && self.generations.get(index) == Some(&handle.generation)
    }

    pub fn get(&self, handle: SceneHandle) -> Option<&T> {
        self.contains(handle)
            .then(|| &self.rows[handle.slot as usize])
    }

    /// Marks the row dirty before returning mutable access.  A caller that
    /// chooses not to alter the value may cause a harmless extra upload.
    pub fn get_mut(&mut self, handle: SceneHandle) -> Option<&mut T> {
        if !self.contains(handle) {
            return None;
        }
        let index = handle.slot as usize;
        self.mark_dirty(index);
        self.rows.get_mut(index)
    }

    pub fn update(&mut self, handle: SceneHandle, value: T) -> Result<(), T> {
        if !self.contains(handle) {
            return Err(value);
        }
        let index = handle.slot as usize;
        self.rows[index] = value;
        self.mark_dirty(index);
        Ok(())
    }

    pub fn remove(&mut self, handle: SceneHandle) -> bool {
        if !self.contains(handle) {
            return false;
        }
        let index = handle.slot as usize;
        self.alive[index] = 0;
        self.generations[index] = self.generations[index].wrapping_add(1).max(1);
        self.free.push(handle.slot);
        self.live_count -= 1;
        self.mark_dirty(index);
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(index, _)| self.alive[*index] != 0)
            .map(|(_, row)| row)
    }

    pub fn iter_with_handles(&self) -> impl Iterator<Item = (SceneHandle, &T)> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(index, _)| self.alive[*index] != 0)
            .map(|(index, row)| {
                (
                    SceneHandle {
                        slot: index as u32,
                        generation: self.generations[index],
                    },
                    row,
                )
            })
    }

    /// Close the simulation write phase and expose one coherent renderer
    /// publication.  Calling it again without writes advances the epoch but
    /// reports an empty upload range.
    pub fn publish(&mut self) -> PublishedScene<'_, T> {
        self.epoch = self.epoch.wrapping_add(1).max(1);
        let dirty = self
            .dirty
            .take()
            .map_or(DirtyRows::EMPTY, |(first, end)| DirtyRows {
                first: first as u32,
                count: (end - first) as u32,
            });
        PublishedScene {
            epoch: self.epoch,
            rows: &self.rows,
            alive: &self.alive,
            generations: &self.generations,
            dirty,
            live_count: self.live_count,
        }
    }

    fn mark_dirty(&mut self, slot: usize) {
        self.dirty = Some(match self.dirty {
            Some((first, end)) => (first.min(slot), end.max(slot + 1)),
            None => (slot, slot + 1),
        });
    }
}

impl<T> Default for SceneStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_is_parallel_and_delta_tracked() {
        let mut scene = SceneStore::new();
        let a = scene.insert(10u32);
        let b = scene.insert(20u32);
        let first = scene.publish();
        assert_eq!(first.epoch, 1);
        assert_eq!(first.rows, &[10, 20]);
        assert_eq!(first.alive, &[1, 1]);
        assert_eq!(first.generations, &[1, 1]);
        assert_eq!(first.dirty, DirtyRows { first: 0, count: 2 });
        assert_eq!(first.live_count, 2);

        assert_eq!(scene.update(b, 21), Ok(()));
        let second = scene.publish();
        assert_eq!(second.dirty, DirtyRows { first: 1, count: 1 });
        assert_eq!(second.rows, &[10, 21]);
        assert!(scene.contains(a));
    }

    #[test]
    fn retired_handle_stays_stale_when_slot_is_reused() {
        let mut scene = SceneStore::new();
        let old = scene.insert(7u32);
        let _ = scene.publish();
        assert!(scene.remove(old));
        assert!(!scene.contains(old));
        let replacement = scene.insert(9u32);
        assert_eq!(replacement.slot, old.slot);
        assert_ne!(replacement.generation, old.generation);
        assert_eq!(scene.get(replacement), Some(&9));
        assert_eq!(scene.get(old), None);
        let publication = scene.publish();
        assert_eq!(publication.dirty, DirtyRows { first: 0, count: 1 });
        assert_eq!(publication.live_count, 1);
    }

    #[test]
    fn clean_publication_has_no_upload_rows() {
        let mut scene = SceneStore::new();
        let _ = scene.insert(1u8);
        let _ = scene.publish();
        let publication = scene.publish();
        assert_eq!(publication.epoch, 2);
        assert_eq!(publication.dirty, DirtyRows::EMPTY);
    }
}
