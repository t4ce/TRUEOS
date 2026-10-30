#!/usr/bin/env python3
"""Run the production free list on host-owned arenas, without a kernel boot."""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    source = (ROOT / "src/allocators.rs").read_text()
    # Preserve the entire free-list implementation. Only external platform
    # diagnostics, domain dispatch and backing-arena admission are replaced.
    start = source.index("#[repr(C)]\nstruct FreeBlock")
    end = source.index("\n#[cfg(test)]\nmod tests;", start)
    helpers = source[source.index("const fn minimum_block_size()") :]
    harness = r'''
#![allow(dead_code, unsafe_op_in_unsafe_fn)]
use core::alloc::Layout;
use core::mem::{align_of, size_of};
use core::ptr::{NonNull, null_mut};
extern crate alloc;
#[macro_export]
macro_rules! log { ($($args:tt)*) => {}; }
mod hv { pub fn current_guest_execution_context_vm_id() -> Option<u8> { None } }
fn trace_alloc_entry(_: bool, _: Layout, _: Option<NonNull<FreeBlock>>) {}
fn trace_alloc_block(_: bool, _: &FreeBlock, _: usize, _: usize, _: usize) {}
fn trace_alloc_compare(_: bool) {}
fn trace_alloc_success(_: bool) {}
fn trace_alloc_invalid_ptr(_: bool, _: usize) {}
fn alloc_domain_tag(domain: AllocDomain) -> u8 {
    match domain { AllocDomain::Host => 0, AllocDomain::HvGuest(vm) => vm + 1 }
}
fn alloc_domain_from_tag(tag: &AllocTag) -> Option<AllocDomain> {
    Some(if tag.domain == 0 { AllocDomain::Host } else { AllocDomain::HvGuest(tag.domain - 1) })
}
fn log_alloc_domain_mismatch(_: *mut u8, _: Option<AllocDomain>, _: Option<u8>) {
    panic!("allocation domain mismatch")
}
fn release_hv_guest_dma_pin(_: HvGuestDmaPin) -> bool { true }
'''
    harness += source[start:end] + "\n" + helpers
    harness += '\n#[path = "' + str(ROOT / "src/allocators/tests.rs") + '"] mod tests;\n'
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    with tempfile.TemporaryDirectory(prefix="trueos-allocator-search-") as directory:
        work = Path(directory)
        (work / "tests.rs").write_text(harness)
        subprocess.run([
            "rustc", "+" + channel.group(1), "--edition=2024", "--test", "-O",
            "--target", "x86_64-unknown-linux-gnu", str(work / "tests.rs"),
            "-o", str(work / "tests"),
        ], cwd=work, check=True)
        subprocess.run([str(work / "tests"), "--nocapture"], cwd=work, check=True)


if __name__ == "__main__":
    main()
