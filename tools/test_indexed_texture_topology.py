#!/usr/bin/env python3
"""Exercise the production sampled-draw ABI and native topology admission."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def topology_impl() -> str:
    source = (ROOT / "src/intel/render/primary.rs").read_text()
    start = source.index("impl ResidentScenePrimitiveTopology {")
    end = re.search(r"^}\s*\n", source[start:], re.MULTILINE)
    if end is None:
        raise ValueError("missing topology implementation terminator")
    return source[start:start + end.end()]


def main() -> None:
    sdk = "crates/trueos-v/src/vgpu.rs"
    cabi = "src/r/io/vgpu_cabi.rs"
    resources = "src/intel/render/resources.rs"
    topology = item("src/intel/render/primary.rs", "ResidentScenePrimitiveTopology")
    sdk_source = item(sdk, "IndexedDraw")
    sdk_source += "\n".join(constant(sdk, name) for name in (
        "PRIMITIVE_TOPOLOGY_TRIANGLE_LIST", "PRIMITIVE_TOPOLOGY_QUAD_LIST",
    ))
    source = (
        "#![allow(dead_code)]\n"
        "mod intel { pub(crate) mod render {\n" + topology + topology_impl() + "}}\n"
        "mod v { pub(crate) mod vgpu {\n" + sdk_source + "}}\n"
        "mod blueprint_sdk {\n"
        + item("../TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs", "IndexedDraw") + "}\n"
        "use intel::render::ResidentScenePrimitiveTopology;\n"
        + item(cabi, "broker_indexed_draw_topology")
        + item(cabi, "indexed_draw_topology_tests")
        + item("src/gpu/vgpu.rs", "ui4_single_indexed_topology_valid")
        + item("src/gpu/vgpu.rs", "canonicalize_ui4_single_indexed_winding")
        + item("src/gpu/vgpu.rs", "ui4_single_indexed_topology_tests")
        + item(resources, "validate_resident_textured_mesh_shape")
        + item(resources, "resident_textured_mesh_shape_tests")
        + r'''
#[test]
fn both_sdks_keep_the_legacy_wire_layout() {
    macro_rules! check_layout {
        ($draw:ty) => {
            assert_eq!(core::mem::size_of::<$draw>(), 104);
            assert_eq!(core::mem::align_of::<$draw>(), 8);
            assert_eq!(core::mem::offset_of!($draw, topology), 64);
            assert_eq!(core::mem::offset_of!($draw, sampled_texture), 72);
            assert_eq!(core::mem::offset_of!($draw, texture_reserved), 96);
        };
    }
    check_layout!(v::vgpu::IndexedDraw);
    check_layout!(blueprint_sdk::IndexedDraw);
}

#[test]
fn legacy_serialized_triangle_and_new_quad_decode_without_moving_texture() {
    // Serialized independently of either SDK struct: old clients leave the
    // word at byte 64 zero. Explicit topology occupies that same word.
    let mut wire = [0u8; 104];
    wire[48..52].copy_from_slice(&6u32.to_le_bytes());
    wire[72..80].copy_from_slice(&0x100000009u64.to_le_bytes());
    for (topology, count, expected) in [
        (0u32, 6u32, ResidentScenePrimitiveTopology::TriangleList),
        (7, 4, ResidentScenePrimitiveTopology::QuadList),
    ] {
        wire[48..52].copy_from_slice(&count.to_le_bytes());
        wire[64..68].copy_from_slice(&topology.to_le_bytes());
        let draw = unsafe { core::ptr::read_unaligned(wire.as_ptr().cast::<v::vgpu::IndexedDraw>()) };
        let decoded = broker_indexed_draw_topology(draw.topology).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(draw.sampled_texture, 0x100000009);
        assert_eq!(draw.index_count, count);
        assert!(ui4_single_indexed_topology_valid(decoded, draw.index_count));
    }
}
'''
    )
    with tempfile.TemporaryDirectory(prefix="trueos-textured-topology-tests-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        rust_source.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
