#!/usr/bin/env python3
"""Compile the real Blueprint memory policy with minimal ELF metadata fixtures."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "src/hv/mod.rs").read_text()


def item(signature):
    start = source.index(signature)
    brace = source.index("{", start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


harness = r'''
#![allow(dead_code)]
const MIB: usize = 1024 * 1024;
mod allcaps { pub mod blueprint { pub const HEAVY_GRAPHICS_HEAP_UPPER_MIB: usize = 1024; } }
mod hv { pub mod blueprint {
    pub struct ElfImport<'a> { pub name: &'a str }
    #[derive(Default, Copy, Clone)]
    pub struct ElfAllocStats { pub alloc_bytes: usize }
    pub struct BlueprintModule<'a> { pub raw_payload_len: usize, pub _data: &'a [u8] }
    pub fn elf_alloc_stats(_: &[u8]) -> Option<ElfAllocStats> { Some(ElfAllocStats::default()) }
} }
'''
for signature in [
    "enum BlueprintMemoryClass", "impl BlueprintMemoryClass",
    "struct BlueprintVmMemoryProfile", "fn ceil_mib", "fn clamp_mib",
    "fn round_pow2_mib", "fn import_name_has", "fn imports_libc_tcp_listener",
    "fn archive_has", "fn classify_blueprint_memory", "fn estimate_blueprint_memory_profile",
]:
    if signature.startswith("enum "):
        harness += "#[derive(Copy, Clone)]\n"
    harness += item(signature) + "\n"
harness += r'''
#[test]
fn x86_session_reserves_asset_memory_before_generic_classification() {
    use hv::blueprint::*;
    let imports = [
        ElfImport { name: "trueos_cabi_x86_address_space_create_v1" },
        ElfImport { name: "trueos_tokio_spawn" },
        ElfImport { name: "pthread_create" },
    ];
    // Capability detection also works for renamed archives and tiny launchers.
    for archive in ["wc3.bp", "renamed.bp"] {
        let module = BlueprintModule { raw_payload_len: 1024, _data: &[] };
        let p = estimate_blueprint_memory_profile(archive, &module, &[], &imports);
        assert_eq!(p.class.label(), "x86-session");
        assert_eq!(p.heap_lower_mib, 2048);
        assert_eq!(p.heap_recommended_mib, 2048);
        assert_eq!(p.heap_upper_mib, 2048);
        assert!(p.heap_lower_mib * MIB > 1024 * MIB + 128 * MIB);
    }
}
#[test]
fn ordinary_blueprints_keep_their_existing_budget() {
    use hv::blueprint::*;
    let module = BlueprintModule { raw_payload_len: 1024, _data: &[] };
    let p = estimate_blueprint_memory_profile("small.bp", &module, &[], &[]);
    assert_eq!(p.class.label(), "unknown");
    assert_eq!((p.heap_lower_mib, p.heap_recommended_mib), (64, 128));
}
'''
with tempfile.TemporaryDirectory(prefix="trueos-x86-memory-") as directory:
    path = Path(directory)
    (path / "test.rs").write_text(harness)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test")], check=True)
    subprocess.run([str(path / "test")], check=True)
