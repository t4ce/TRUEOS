#!/usr/bin/env python3
"""Exercise production Blueprint role classification and memory admission policy."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(source: str, marker: str) -> str:
    start = source.index(marker)
    opening = source.index("{", start)
    depth = 1
    cursor = opening + 1
    while depth:
        if source[cursor] == "{":
            depth += 1
        elif source[cursor] == "}":
            depth -= 1
        cursor += 1
    return source[start:cursor]


def main():
    source = (ROOT / "src/hv/mod.rs").read_text()
    selected = [
        "enum BlueprintMemoryClass", "impl BlueprintMemoryClass",
        "struct BlueprintVmMemoryProfile", "fn clamp_mib", "fn round_pow2_mib",
        "fn import_name_has", "fn imports_libc_tcp_listener", "fn archive_has",
        "fn classify_blueprint_memory", "fn blueprint_memory_profile_for",
    ]
    bodies = []
    for marker in selected:
        body = item(source, marker)
        if marker.startswith("enum ") or marker.startswith("struct "):
            body = "#[derive(Clone, Copy)]\n" + body
        bodies.append(body)
    harness = r'''
#![allow(dead_code)]
const MIB: usize = 1024 * 1024;
mod allcaps { pub mod blueprint { pub const HEAVY_GRAPHICS_HEAP_UPPER_MIB: usize = 2048; } }
mod hv { pub mod blueprint {
    pub struct ElfImport<'a> { pub name: &'a str }
    #[derive(Default)] pub struct ElfAllocStats { pub alloc_bytes: usize }
} }
@PRODUCTION@

#[test]
fn retired_audio_roles_receive_no_special_budget() {
    let audio = [hv::blueprint::ElfImport { name: "trueos_cabi_audio_open_playback" }];
    for archive in ["ttstt.bp", "ttstt", "scope-tui.bp", "aud-player-scope-tui.bp", "player.bp"] {
        let class = classify_blueprint_memory(archive, MIB,
            hv::blueprint::ElfAllocStats { alloc_bytes: MIB }, &audio);
        assert!(matches!(class, BlueprintMemoryClass::Unknown));
    }
    let network = [hv::blueprint::ElfImport { name: "trueos_cabi_audio_open_playback" },
        hv::blueprint::ElfImport { name: "trueos_mio_tcp_listener_bind" }];
    let class = classify_blueprint_memory("ttstt.bp", 133_254_144,
        hv::blueprint::ElfAllocStats { alloc_bytes: 132_067_536 }, &network);
    assert!(matches!(class, BlueprintMemoryClass::NetworkServer));
}

#[test]
fn ordinary_servers_keep_their_existing_budgets() {
    let network = [hv::blueprint::ElfImport { name: "trueos_mio_tcp_listener_bind" }];
    let class = classify_blueprint_memory("sqlite.bp", MIB,
        hv::blueprint::ElfAllocStats { alloc_bytes: MIB }, &network);
    assert!(matches!(class, BlueprintMemoryClass::NetworkServer));
    let profile = blueprint_memory_profile_for(class, 1);
    assert_eq!((profile.heap_lower_mib, profile.heap_recommended_mib, profile.heap_upper_mib), (128, 512, 1024));
}

#[test]
fn ram_asset_admission_keeps_its_required_peak_budget() {
    let imports = [hv::blueprint::ElfImport { name: "trueos_cabi_archive_lz4_decode_start_v1" }];
    let class = classify_blueprint_memory("ttstt.bp", MIB,
        hv::blueprint::ElfAllocStats::default(), &imports);
    assert!(matches!(class, BlueprintMemoryClass::RamAssets));
    let profile = blueprint_memory_profile_for(class, 1);
    assert_eq!(profile.heap_lower_mib, 3072);
}
'''.replace("@PRODUCTION@", "\n".join(bodies))
    with tempfile.TemporaryDirectory(prefix="trueos-blueprint-memory-") as work:
        root = Path(work)
        fixture = root / "profile.rs"
        fixture.write_text(harness)
        binary = root / "profile-tests"
        subprocess.run(["rustup", "run", "nightly-2026-07-10", "rustc", "--edition=2024", "--test", str(fixture), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
