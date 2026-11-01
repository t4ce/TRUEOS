//! BCS-owned page tables: persistent topology and sparse transient leaf updates.
//!
//! The caller must prove exact retirement before mutating this table and must
//! publish the returned byte ranges before submitting commands. Translation
//! invalidation remains a command-stream responsibility.

use alloc::vec::Vec;

const PAGE_BYTES: usize = 4096;
const TABLE_PAGES: usize = 3 + 512;
const TABLE_BYTES: usize = TABLE_PAGES * PAGE_BYTES;
const TABLE_ENTRIES: usize = TABLE_BYTES / core::mem::size_of::<u64>();
const LEAF_OFFSET_BYTES: usize = 3 * PAGE_BYTES;
const VA_LIMIT: u64 = 512 * 512 * PAGE_BYTES as u64;
const DIRECTORY_FLAGS: u64 = 0x1b;
const CACHE_LINE_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BcsMapping {
    pub(super) gpu: u64,
    pub(super) phys: u64,
    pub(super) bytes: usize,
    pub(super) flags: u64,
}

impl BcsMapping {
    fn rounded_bytes(self) -> Option<u64> {
        let bytes = u64::try_from(self.bytes).ok()?;
        bytes
            .checked_add(PAGE_BYTES as u64 - 1)
            .map(|end| end & !(PAGE_BYTES as u64 - 1))
    }

    fn valid(self) -> bool {
        let Some(bytes) = self.rounded_bytes() else {
            return false;
        };
        bytes != 0
            && self.gpu.is_multiple_of(PAGE_BYTES as u64)
            && self.phys != 0
            && self.phys.is_multiple_of(PAGE_BYTES as u64)
            && self.flags & 1 != 0
            && self.flags & !(PAGE_BYTES as u64 - 1) == 0
            && self
                .gpu
                .checked_add(bytes)
                .is_some_and(|end| end <= VA_LIMIT)
            && self.phys.checked_add(bytes).is_some()
    }

    fn gpu_end(self) -> u64 {
        // All users first validate their complete mapping set.
        self.gpu + self.rounded_bytes().unwrap()
    }

    fn phys_end(self) -> u64 {
        self.phys + self.rounded_bytes().unwrap()
    }

    fn leaf_range(self) -> (usize, usize) {
        let first = LEAF_OFFSET_BYTES + (self.gpu / PAGE_BYTES as u64) as usize * 8;
        let bytes = (self.rounded_bytes().unwrap() / PAGE_BYTES as u64) as usize * 8;
        (first, bytes)
    }
}

pub(super) struct BcsMappings {
    root_phys: u64,
    controls: Vec<BcsMapping>,
    transient: Vec<BcsMapping>,
}

impl BcsMappings {
    pub(super) const fn new() -> Self {
        Self {
            root_phys: 0,
            controls: Vec::new(),
            transient: Vec::new(),
        }
    }

    /// Any external whole-table initialization makes the remembered leaves
    /// untrustworthy. The next prepare reconstructs and publishes the table.
    pub(super) fn invalidate(&mut self) {
        self.root_phys = 0;
        self.controls.clear();
        self.transient.clear();
    }

    /// Return merged, cache-line-aligned byte ranges requiring publication.
    /// No table entries or metadata change if validation fails.
    pub(super) fn prepare(
        &mut self,
        table: &mut [u64],
        root_phys: u64,
        controls: &[BcsMapping],
        transient: &[BcsMapping],
    ) -> Option<Vec<(usize, usize)>> {
        if table.len() < TABLE_ENTRIES
            || root_phys == 0
            || !root_phys.is_multiple_of(PAGE_BYTES as u64)
            || root_phys.checked_add(TABLE_BYTES as u64).is_none()
            || !valid_mappings(controls, transient)
        {
            return None;
        }

        let initialize = self.root_phys != root_phys || !same_mappings(&self.controls, controls);
        if !initialize && same_mappings(&self.transient, transient) {
            return Some(Vec::new());
        }

        let next_transient = transient.to_vec();
        let mut dirty = Vec::new();
        if initialize {
            let next_controls = controls.to_vec();
            table[..TABLE_ENTRIES].fill(0);
            write_entry(table, 0, (root_phys + PAGE_BYTES as u64) | DIRECTORY_FLAGS);
            write_entry(
                table,
                PAGE_BYTES / 8,
                (root_phys + 2 * PAGE_BYTES as u64) | DIRECTORY_FLAGS,
            );
            for index in 0..512 {
                write_entry(
                    table,
                    2 * PAGE_BYTES / 8 + index,
                    (root_phys + LEAF_OFFSET_BYTES as u64 + (index * PAGE_BYTES) as u64)
                        | DIRECTORY_FLAGS,
                );
            }
            for &mapping in controls {
                write_mapping(table, mapping, false);
            }
            self.controls = next_controls;
            dirty.push((0, TABLE_BYTES));
        } else {
            // Clear the complete old transient set before installing the new
            // one. Compatible overlapping descriptors cannot erase a retained
            // descriptor's leaves after they have been reinstalled this way.
            for &mapping in &self.transient {
                write_mapping(table, mapping, true);
                dirty.push(mapping.leaf_range());
            }
        }
        for &mapping in transient {
            write_mapping(table, mapping, false);
            if !initialize {
                dirty.push(mapping.leaf_range());
            }
        }
        self.root_phys = root_phys;
        self.transient = next_transient;
        merge_cache_lines(&mut dirty);
        Some(dirty)
    }
}

fn same_mappings(first: &[BcsMapping], second: &[BcsMapping]) -> bool {
    first.len() == second.len() && first.iter().all(|mapping| second.contains(mapping))
}

fn gpu_overlap(first: BcsMapping, second: BcsMapping) -> bool {
    first.gpu < second.gpu_end() && second.gpu < first.gpu_end()
}

fn mappings_compatible(first: BcsMapping, second: BcsMapping) -> bool {
    let physical_overlap = first.phys < second.phys_end() && second.phys < first.phys_end();
    if physical_overlap && first.flags != second.flags {
        return false;
    }
    if !gpu_overlap(first, second) {
        return true;
    }
    let same_backing = if first.gpu <= second.gpu {
        first.phys.checked_add(second.gpu - first.gpu) == Some(second.phys)
    } else {
        second.phys.checked_add(first.gpu - second.gpu) == Some(first.phys)
    };
    same_backing && first.flags == second.flags
}

fn valid_mappings(controls: &[BcsMapping], transient: &[BcsMapping]) -> bool {
    if !controls
        .iter()
        .chain(transient)
        .all(|mapping| mapping.valid())
    {
        return false;
    }
    for (index, &control) in controls.iter().enumerate() {
        if controls[..index]
            .iter()
            .any(|&other| gpu_overlap(control, other))
            || transient.iter().any(|&mapping| {
                gpu_overlap(control, mapping) || !mappings_compatible(control, mapping)
            })
        {
            return false;
        }
    }
    for (index, &mapping) in transient.iter().enumerate() {
        if transient[..index]
            .iter()
            .any(|&other| !mappings_compatible(mapping, other))
        {
            return false;
        }
    }
    true
}

fn write_entry(table: &mut [u64], index: usize, value: u64) {
    // The table has ordinary CPU backing, shared with a non-coherent GPU
    // walker after publication. The caller currently owns it exclusively.
    unsafe { core::ptr::write_volatile(table.as_mut_ptr().add(index), value) };
}

fn write_mapping(table: &mut [u64], mapping: BcsMapping, clear: bool) {
    let (offset, bytes) = mapping.leaf_range();
    for page in 0..bytes / 8 {
        let value = if clear {
            0
        } else {
            (mapping.phys + (page * PAGE_BYTES) as u64) | mapping.flags
        };
        write_entry(table, offset / 8 + page, value);
    }
}

fn merge_cache_lines(ranges: &mut Vec<(usize, usize)>) {
    for (offset, bytes) in ranges.iter_mut() {
        let start = *offset & !(CACHE_LINE_BYTES - 1);
        let end = (*offset + *bytes + CACHE_LINE_BYTES - 1) & !(CACHE_LINE_BYTES - 1);
        *offset = start;
        *bytes = end - start;
    }
    ranges.sort_unstable_by_key(|&(offset, _)| offset);
    let mut count = 0;
    for index in 0..ranges.len() {
        let (offset, bytes) = ranges[index];
        if count != 0 && offset <= ranges[count - 1].0 + ranges[count - 1].1 {
            let previous = &mut ranges[count - 1];
            previous.1 = (offset + bytes).max(previous.0 + previous.1) - previous.0;
        } else {
            ranges[count] = (offset, bytes);
            count += 1;
        }
    }
    ranges.truncate(count);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const ROOT: u64 = 0x1000_0000;

    fn mapping(gpu: u64, phys: u64, bytes: usize) -> BcsMapping {
        BcsMapping {
            gpu,
            phys,
            bytes,
            flags: 3,
        }
    }

    fn leaf(table: &[u64], gpu: u64) -> u64 {
        table[LEAF_OFFSET_BYTES / 8 + (gpu / PAGE_BYTES as u64) as usize]
    }

    #[test]
    fn first_prepare_publishes_topology_then_identical_or_reordered_maps_need_no_writes() {
        let mut state = BcsMappings::new();
        let mut table = vec![u64::MAX; TABLE_ENTRIES];
        let controls = [mapping(0x10000, 0x100000, PAGE_BYTES)];
        let maps = [
            mapping(0x400000, 0x200000, PAGE_BYTES),
            mapping(0x800000, 0x300000, 1),
        ];
        assert_eq!(
            state.prepare(&mut table, ROOT, &controls, &maps),
            Some(vec![(0, TABLE_BYTES)])
        );
        assert_eq!(table[0], (ROOT + 4096) | DIRECTORY_FLAGS);
        assert_eq!(table[512], (ROOT + 8192) | DIRECTORY_FLAGS);
        assert_eq!(
            table[1024 + 511],
            (ROOT + LEAF_OFFSET_BYTES as u64 + 511 * 4096) | DIRECTORY_FLAGS
        );
        assert_eq!(leaf(&table, maps[1].gpu), maps[1].phys | 3);
        let before = table.clone();
        assert_eq!(
            state.prepare(&mut table, ROOT, &controls, &maps),
            Some(vec![])
        );
        assert_eq!(
            state.prepare(&mut table, ROOT, &controls, &[maps[1], maps[0]]),
            Some(vec![])
        );
        assert_eq!(table, before);
    }

    #[test]
    fn recycled_va_updates_physical_backing_and_cache_policy() {
        let mut state = BcsMappings::new();
        let mut table = vec![0; TABLE_ENTRIES];
        let old = mapping(0x400000, 0x200000, PAGE_BYTES);
        state.prepare(&mut table, ROOT, &[], &[old]).unwrap();
        let next = BcsMapping {
            phys: 0x300000,
            flags: 0x1b,
            ..old
        };
        let dirty = state.prepare(&mut table, ROOT, &[], &[next]).unwrap();
        assert_eq!(dirty, vec![(next.leaf_range().0, 64)]);
        assert_eq!(leaf(&table, next.gpu), next.phys | next.flags);
    }

    #[test]
    fn shrinking_and_removing_ranges_clear_stale_leaves_without_touching_controls() {
        let mut state = BcsMappings::new();
        let mut table = vec![0; TABLE_ENTRIES];
        let controls = [mapping(0x10000, 0x100000, PAGE_BYTES)];
        let old = mapping(0x400000, 0x200000, 10 * PAGE_BYTES);
        state.prepare(&mut table, ROOT, &controls, &[old]).unwrap();
        let next = BcsMapping { bytes: 1, ..old };
        let dirty = state.prepare(&mut table, ROOT, &controls, &[next]).unwrap();
        assert_eq!(dirty, vec![(old.leaf_range().0, 128)]);
        assert_eq!(leaf(&table, old.gpu), old.phys | 3);
        for page in 1..10 {
            assert_eq!(leaf(&table, old.gpu + page * PAGE_BYTES as u64), 0);
        }
        state.prepare(&mut table, ROOT, &controls, &[]).unwrap();
        assert_eq!(leaf(&table, old.gpu), 0);
        assert_eq!(leaf(&table, controls[0].gpu), controls[0].phys | 3);
    }

    #[test]
    fn updates_across_pt_boundary_merge_aligned_publication_ranges() {
        let mut state = BcsMappings::new();
        let mut table = vec![0; TABLE_ENTRIES];
        state.prepare(&mut table, ROOT, &[], &[]).unwrap();
        let next = mapping(
            2 * 1024 * 1024 - PAGE_BYTES as u64,
            0x400000,
            PAGE_BYTES + 1,
        );
        let dirty = state.prepare(&mut table, ROOT, &[], &[next]).unwrap();
        assert_eq!(dirty, vec![(LEAF_OFFSET_BYTES + PAGE_BYTES - 64, 128)]);
        assert_eq!(leaf(&table, next.gpu), next.phys | 3);
        assert_eq!(
            leaf(&table, next.gpu + PAGE_BYTES as u64),
            (next.phys + PAGE_BYTES as u64) | 3
        );
    }

    #[test]
    fn invalid_ranges_and_aliases_leave_table_and_previous_metadata_untouched() {
        let mut state = BcsMappings::new();
        let mut table = vec![0; TABLE_ENTRIES];
        let controls = [mapping(0x10000, 0x100000, PAGE_BYTES)];
        let valid = mapping(0x400000, 0x200000, PAGE_BYTES);
        state
            .prepare(&mut table, ROOT, &controls, &[valid])
            .unwrap();
        let before = table.clone();
        for invalid in [
            mapping(VA_LIMIT, 0x200000, PAGE_BYTES),
            mapping(VA_LIMIT - PAGE_BYTES as u64, 0x200000, PAGE_BYTES + 1),
            mapping(u64::MAX - 4095, 0x200000, PAGE_BYTES),
            mapping(0x400000, u64::MAX - 4095, PAGE_BYTES),
            mapping(0x400000, 0x200000, usize::MAX),
            mapping(0x400001, 0x200000, PAGE_BYTES),
            mapping(0x400000, 0x200001, PAGE_BYTES),
            mapping(0x400000, 0x200000, 0),
            mapping(controls[0].gpu, 0x200000, PAGE_BYTES),
        ] {
            assert!(
                state
                    .prepare(&mut table, ROOT, &controls, &[invalid])
                    .is_none()
            );
            assert_eq!(table, before);
        }
        let conflict = BcsMapping {
            phys: 0x300000,
            ..valid
        };
        assert!(
            state
                .prepare(&mut table, ROOT, &controls, &[valid, conflict])
                .is_none()
        );
        let cache_alias = BcsMapping {
            gpu: 0x800000,
            flags: 0x1b,
            ..valid
        };
        assert!(
            state
                .prepare(&mut table, ROOT, &controls, &[valid, cache_alias])
                .is_none()
        );
        assert_eq!(table, before);
        assert_eq!(
            state.prepare(&mut table, ROOT, &controls, &[valid]),
            Some(vec![])
        );
    }

    #[test]
    fn compatible_overlapping_ranges_are_reinstalled_after_old_ranges_are_cleared() {
        let mut state = BcsMappings::new();
        let mut table = vec![0; TABLE_ENTRIES];
        let first = mapping(0x400000, 0x200000, 3 * PAGE_BYTES);
        let second = mapping(0x401000, 0x201000, 3 * PAGE_BYTES);
        state
            .prepare(&mut table, ROOT, &[], &[first, second])
            .unwrap();
        state.prepare(&mut table, ROOT, &[], &[second]).unwrap();
        assert_eq!(leaf(&table, first.gpu), 0);
        for page in 0..3 {
            assert_eq!(
                leaf(&table, second.gpu + page * PAGE_BYTES as u64),
                (second.phys + page * PAGE_BYTES as u64) | 3
            );
        }
    }

    #[test]
    fn invalidation_rebuilds_after_external_initializer_changed_table() {
        let mut state = BcsMappings::new();
        let mut table = vec![0; TABLE_ENTRIES];
        let mapping = mapping(0x400000, 0x200000, PAGE_BYTES);
        state.prepare(&mut table, ROOT, &[], &[mapping]).unwrap();
        state.invalidate();
        table.fill(u64::MAX);
        assert_eq!(
            state.prepare(&mut table, ROOT, &[], &[mapping]),
            Some(vec![(0, TABLE_BYTES)])
        );
        assert_eq!(leaf(&table, 0x800000), 0);
        assert_eq!(leaf(&table, mapping.gpu), mapping.phys | 3);
    }
}
