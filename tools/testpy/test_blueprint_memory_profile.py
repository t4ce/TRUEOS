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
        "struct BlueprintVmMemoryProfile", "fn ceil_mib", "fn clamp_mib", "fn round_pow2_mib",
        "fn import_name_has", "fn imports_libc_tcp_listener", "fn archive_has",
        "fn blueprint_has_large_image", "fn classify_blueprint_memory",
        "fn blueprint_memory_profile_for_image(", "fn blueprint_memory_profile_for(",
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
    #[derive(Clone, Copy, Default)] pub struct ElfAllocStats { pub alloc_bytes: usize }
} }
@PRODUCTION@

fn heap_bounds(profile: BlueprintVmMemoryProfile) -> (usize, usize, usize) {
    (profile.heap_lower_mib, profile.heap_recommended_mib, profile.heap_upper_mib)
}

#[test]
fn audio_playback_imports_do_not_override_role_classification() {
    let audio = [hv::blueprint::ElfImport { name: "trueos_cabi_audio_open_playback" }];
    for archive in ["audio-client.bp", "scope-tui.bp", "aud-player-scope-tui.bp", "player.bp"] {
        let class = classify_blueprint_memory(archive, MIB,
            hv::blueprint::ElfAllocStats { alloc_bytes: MIB }, &audio);
        assert!(matches!(class, BlueprintMemoryClass::Unknown));
    }
    let network = [hv::blueprint::ElfImport { name: "trueos_cabi_audio_open_playback" },
        hv::blueprint::ElfImport { name: "trueos_mio_tcp_listener_bind" }];
    let class = classify_blueprint_memory("audio-client.bp", 133_254_144,
        hv::blueprint::ElfAllocStats { alloc_bytes: 132_067_536 }, &network);
    assert!(matches!(class, BlueprintMemoryClass::NetworkServer));
}

#[test]
fn ordinary_servers_keep_their_existing_budgets() {
    let network = [hv::blueprint::ElfImport { name: "trueos_mio_tcp_listener_bind" }];
    let class = classify_blueprint_memory("sqlite.bp", MIB,
        hv::blueprint::ElfAllocStats { alloc_bytes: MIB }, &network);
    assert!(matches!(class, BlueprintMemoryClass::NetworkServer));
    let profile = blueprint_memory_profile_for_image("sqlite.bp", MIB,
        hv::blueprint::ElfAllocStats { alloc_bytes: MIB }, &network);
    assert_eq!(heap_bounds(profile), (128, 512, 1024));
}

#[test]
fn ram_asset_admission_keeps_its_required_peak_budget() {
    let imports = [hv::blueprint::ElfImport { name: "trueos_cabi_archive_lz4_decode_start_v1" }];
    for size in [MIB, 238_280_704] {
        let profile = blueprint_memory_profile_for_image("assets.bp", size,
            hv::blueprint::ElfAllocStats { alloc_bytes: size }, &imports);
        assert!(matches!(profile.class, BlueprintMemoryClass::RamAssets));
        assert_eq!(heap_bounds(profile), (3072, 3072, 3072));
    }
}

#[test]
fn adding_a_listener_cannot_reduce_large_image_heap_admission() {
    let raw_payload_len = 238_280_704;
    let stats = hv::blueprint::ElfAllocStats { alloc_bytes: 226_207_236 };
    let standalone = blueprint_memory_profile_for_image("embedded-model.bp",
        raw_payload_len, stats, &[]);
    assert!(matches!(standalone.class, BlueprintMemoryClass::HeavyGraphics));
    assert_eq!(heap_bounds(standalone), (1024, 2048, 2048));

    let mio = [hv::blueprint::ElfImport { name: "trueos_mio_tcp_listener_bind" }];
    let libc = [hv::blueprint::ElfImport { name: "bind" },
        hv::blueprint::ElfImport { name: "listen" },
        hv::blueprint::ElfImport { name: "accept4" }];
    for imports in [mio.as_slice(), libc.as_slice()] {
        let service = blueprint_memory_profile_for_image("embedded-model.bp",
            raw_payload_len, stats, imports);
        assert!(matches!(service.class, BlueprintMemoryClass::NetworkServer));
        assert_eq!(heap_bounds(service), heap_bounds(standalone));
        assert_eq!((service.stack_lower_mib, service.stack_recommended_mib,
            service.stack_upper_mib), (16, 32, 64));
    }
}

#[test]
fn network_clients_also_preserve_large_image_requirements() {
    let imports = [hv::blueprint::ElfImport { name: "tcp_stream" }];
    let stats = hv::blueprint::ElfAllocStats { alloc_bytes: 226_207_236 };
    let profile = blueprint_memory_profile_for_image("embedded-model.bp",
        238_280_704, stats, &imports);
    assert!(matches!(profile.class, BlueprintMemoryClass::NetworkClient));
    assert_eq!(heap_bounds(profile), (1024, 2048, 2048));
    assert_eq!((profile.stack_lower_mib, profile.stack_recommended_mib,
        profile.stack_upper_mib), (8, 16, 64));
}

#[test]
fn image_admission_uses_existing_payload_and_alloc_section_thresholds() {
    let imports = [hv::blueprint::ElfImport { name: "trueos_mio_tcp_listener_bind" }];
    let boundary = blueprint_memory_profile_for_image("service.bp", 8 * MIB,
        hv::blueprint::ElfAllocStats { alloc_bytes: 4 * MIB }, &imports);
    assert_eq!(heap_bounds(boundary), (128, 1024, 1024));
    for (payload, alloc) in [(8 * MIB + 1, 4 * MIB), (MIB, 4 * MIB + 1)] {
        let stats = hv::blueprint::ElfAllocStats { alloc_bytes: alloc };
        let standalone = blueprint_memory_profile_for_image("embedded-model.bp",
            payload, stats, &[]);
        let service = blueprint_memory_profile_for_image("embedded-model.bp",
            payload, stats, &imports);
        assert_eq!(service.heap_lower_mib, standalone.heap_lower_mib);
        assert_eq!(service.heap_upper_mib, standalone.heap_upper_mib);
        assert!(service.heap_recommended_mib >= standalone.heap_recommended_mib);
    }
}

#[test]
fn large_images_do_not_reduce_larger_runtime_budgets() {
    let imports = [hv::blueprint::ElfImport { name: "pthread_create" }];
    let profile = blueprint_memory_profile_for_image("embedded-model.bp",
        238_280_704, hv::blueprint::ElfAllocStats { alloc_bytes: 226_207_236 }, &imports);
    assert!(matches!(profile.class, BlueprintMemoryClass::ServerRuntime));
    assert_eq!(heap_bounds(profile), (1024, 4096, 4096));
    assert_eq!((profile.stack_lower_mib, profile.stack_recommended_mib,
        profile.stack_upper_mib), (16, 64, 128));
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
