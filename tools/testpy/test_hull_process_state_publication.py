#!/usr/bin/env python3
"""Exercise production Hull template publication patches against host storage."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    memory = (ROOT / "src/hv/memory.rs").read_text()
    start = memory.index("fn patch_guest_hull_rw_dynamic_state(")
    end = memory.index("fn guest_hull_rw_page_uses_kernel_backing(", start)
    production = memory[start:end]
    harness = r'''
use std::sync::atomic::{AtomicUsize, Ordering};
const GUEST_BASE: u64 = 0x100000;
const READY_OFFSET: usize = 31;
static CANONICAL_STATE: AtomicUsize = AtomicUsize::new(0xa5);
fn hvlogf(_: std::fmt::Arguments) {}
mod allocators {
    pub fn hv_guest_allocator_state_span(_: u8) -> Option<(u64, usize)> { None }
}
mod std_abi_shim {
    pub fn blueprint_process_state_span(_: u8) -> Option<(u64, usize)> {
        assert_eq!(super::CANONICAL_STATE.load(super::Ordering::Acquire), 0xa5);
        Some((super::GUEST_BASE + 64, 64))
    }
    pub fn blueprint_process_state_ready_flag(_: u8) -> Option<(u64, u8)> {
        Some((super::GUEST_BASE + super::READY_OFFSET as u64, 2))
    }
}
mod hv {
    pub fn current_vm_lapic_low_tag_addr() -> u64 { super::GUEST_BASE + 7 }
    pub fn blueprint_launch_active(_: u8) -> bool { false }
    pub fn blueprint_launch_states_span() -> (u64, usize) { (0, 0) }
    pub fn blueprint_process_contexts_span() -> (u64, usize) { (0, 0) }
}
#[test]
fn private_template_observes_published_shared_state_on_every_refresh() {
    for _ in 0..3 {
        let mut private = [0u8; 128];
        patch_guest_hull_rw_dynamic_state(4, private.as_mut_ptr() as usize, GUEST_BASE, private.len());
        assert_eq!(private[READY_OFFSET], 2, "private BSS must not reinitialize the shared slot");
        assert_eq!(private[7], 5);
        assert!(private[64..].iter().all(|byte| *byte == 0));
        assert_eq!(CANONICAL_STATE.load(Ordering::Acquire), 0xa5);
    }
}
#[test]
fn publication_patch_rejects_addresses_outside_the_private_image() {
    let mut private = [0x33u8; 16];
    let start = private.as_mut_ptr() as usize;
    patch_guest_hull_rw_u8(start, GUEST_BASE, 16, GUEST_BASE - 1, 2);
    patch_guest_hull_rw_u8(start, GUEST_BASE, 16, GUEST_BASE + 16, 2);
    assert_eq!(private, [0x33; 16]);
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-hull-publication-") as directory:
        path = Path(directory)
        source = path / "publication.rs"
        source.write_text(harness + production)
        binary = path / "publication"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu", str(source), "-o", str(binary)],
            check=True, cwd=ROOT,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
