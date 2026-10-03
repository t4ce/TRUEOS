//! Construct the three owned carrier branches without changing host tables.

use super::super::GuestPage;

const ADDRESS: u64 = 0x000F_FFFF_FFFF_F000;
const PRESENT: u64 = 1;
const WRITABLE: u64 = 2;
const LARGE: u64 = 1 << 7;
const NO_EXECUTE: u64 = 1 << 63;
const PAGE_BYTES: u64 = 4096;
const PDE_BYTES: u64 = 2 * 1024 * 1024;

#[allow(clippy::too_many_arguments)]
pub(super) fn populate(
    pages: &mut [GuestPage; 3],
    physical: u64,
    parent: &[u64; 512],
    guest_low_pd: &[u64; 512],
    stack_start: u64,
    stack_bytes: usize,
    comm_start: u64,
    comm_bytes: usize,
    mut read: impl FnMut(u64) -> Result<[u64; 512], &'static str>,
) -> Result<(), &'static str> {
    let stack_end = stack_start
        .checked_add(stack_bytes as u64)
        .filter(|end| *end > stack_start)
        .ok_or("thread carrier stack span")?;
    let comm_end = comm_start
        .checked_add(comm_bytes as u64)
        .filter(|end| *end > comm_start)
        .ok_or("thread carrier comm span")?;
    // The fixed Hull stack and communication pages occupy one low 1 GiB
    // branch. Refuse geometry requiring further branch ownership.
    if stack_start >> 30 != 0
        || stack_end > (1 << 30)
        || comm_start >> 30 != 0
        || comm_end > (1 << 30)
        || physical & (PAGE_BYTES - 1) != 0
        || physical
            .checked_add(3 * PAGE_BYTES)
            .is_none_or(|end| end > (1 << 52))
    {
        return Err("thread carrier table geometry");
    }

    pages[0].0 = *parent;
    let root_entry = parent[0];
    pages[1].0 = if root_entry & PRESENT != 0 {
        read(root_entry & ADDRESS)?
    } else {
        [0; 512]
    };
    if root_entry & PRESENT != 0 {
        for entry in &mut pages[1].0 {
            inherit_permissions(entry, root_entry);
        }
    }
    let pdpt_entry = pages[1].0[0];
    pages[2].0 = if pdpt_entry & PRESENT == 0 {
        [0; 512]
    } else if pdpt_entry & LARGE != 0 {
        // Preserve an existing 1 GiB identity/HHDM branch as 512 2 MiB
        // leaves. Large-page PAT remains bit 12 at either level.
        let start = pdpt_entry & 0x000F_FFFF_C000_0000;
        let flags = pdpt_entry & !ADDRESS | (pdpt_entry & (1 << 12));
        core::array::from_fn(|index| start + index as u64 * PDE_BYTES | flags)
    } else {
        read(pdpt_entry & ADDRESS)?
    };
    if pdpt_entry & PRESENT != 0 {
        for entry in &mut pages[2].0 {
            inherit_permissions(entry, pdpt_entry);
        }
    }

    for (start, end) in [(stack_start, stack_end), (comm_start, comm_end)] {
        let first = (start / PDE_BYTES) as usize;
        let last = ((end - 1) / PDE_BYTES) as usize;
        for index in first..=last {
            let entry = guest_low_pd[index];
            if entry & PRESENT == 0 {
                return Err("thread carrier guest branch absent");
            }
            pages[2].0[index] = entry;
        }
    }

    // Restrictions of replaced host parents were pushed into their copied
    // children, retaining every unrelated mapping's effective permissions.
    // The selected guest leaves instead keep the Hull's own permissions.
    pages[0].0[0] = physical + PAGE_BYTES | PRESENT | WRITABLE;
    pages[1].0[0] = physical + 2 * PAGE_BYTES | PRESENT | WRITABLE;
    Ok(())
}

fn inherit_permissions(entry: &mut u64, parent: u64) {
    if parent & WRITABLE == 0 {
        *entry &= !WRITABLE;
    }
    *entry |= parent & NO_EXECUTE;
}
