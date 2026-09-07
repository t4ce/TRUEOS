//! Standalone host tests for the built-in shader/packet handoff.
use super::patch_cube::*;

#[test]
fn upload_reserves_descriptor_page_after_native_code() {
    let gs = super::triangle_adjacency_geometry_shader();
    let after = gs.meta.kernel.code_offset_bytes as usize + gs.code.len() * 4;
    assert!(super::patch_cube_upload_layout(after, 16 * 1024).is_err());
    assert!(super::patch_cube_upload_layout(after, 24 * 1024).is_err());
    let ([hs, ds], end) = super::patch_cube_upload_layout(after, 28 * 1024).unwrap();
    assert!(hs >= after && ds >= hs + TESS_CONTROL.len() * 4);
    assert_eq!((hs | ds) & 63, 0);
    assert!(end + 4096 <= 28 * 1024);
    assert!(super::patch_cube_upload_layout(usize::MAX, usize::MAX).is_err());
}

#[test]
fn relocate_and_disable_all_stages() {
    let enabled = super::tessellation_stage_packets(Some([0x1000, 0x4000])).unwrap();
    assert_eq!(enabled[3], 0x1000);
    assert_eq!(enabled[15], 0x4000);
    assert_eq!(enabled[10] & 1, 1);
    let disabled = super::tessellation_stage_packets(None).unwrap();
    for i in 0..25 {
        if [0, 9, 14].contains(&i) {
            assert_eq!(disabled[i], enabled[i]);
        } else {
            assert_eq!(disabled[i], 0);
        }
    }
    for ksp in [[1, 64], [64, 1], [0, 64], [64, 0]] {
        assert!(super::tessellation_stage_packets(Some(ksp)).is_err());
    }
}

#[test]
fn packet_stage_contract() {
    assert_eq!(HS_PACKET[0], 0x781b0007);
    assert_eq!(HS_PACKET[3], 0); // relocation required
    assert_eq!(DS_PACKET[0], 0x781d0009);
    assert_eq!(DS_PACKET[1], 0);
    assert_ne!(DS_PACKET[7] & (1 << 2), 0); // triangle W coordinate
    assert_eq!((DS_PACKET[3] >> 18) & 0xff, 3); // reserved, camera, instances
    assert_eq!(TE_PACKET[0], 0x781c0003);
    assert_eq!(TE_PACKET[1] & 1, 1);
    assert_eq!((TE_PACKET[1] >> 4) & 3, 1); // triangle
    assert_eq!((TE_PACKET[1] >> 8) & 3, 3); // CCW upper-left
    assert_eq!((TE_PACKET[1] >> 22) & 3, 3); // patch header layout
    assert_eq!((TE_PACKET[1] >> 12) & 3, 0); // integer factors
}

#[test]
fn urb_entries_do_not_overlap() {
    let mut end = 32 * 1024;
    for (size, start, entries) in URB {
        if entries == 0 {
            continue;
        }
        assert!(start * 8192 >= end);
        end = start * 8192 + size * 64 * entries;
    }
    assert!(end <= 512 * 1024);
}

#[test]
fn pipeline_owns_matching_code_and_metadata() {
    assert!(matches(&PIPELINE));
    assert!(!matches(super::triangle_pipeline()));
    assert_eq!(PIPELINE.vs.meta.kernel.code_size_bytes as usize, VERTEX.len() * 4);
    assert_eq!(PIPELINE.ps.meta.kernel.code_size_bytes as usize, FRAGMENT.len() * 4);
    assert_eq!(PIPELINE.vs.meta.kernel.binding_table_entry_count, 4);
    assert_eq!(PIPELINE.vs.meta.urb_entry_output_length, 1); // three VUE slots fit 64 bytes
    assert_eq!(CONTRACT_VERSION, 6);
    assert_eq!(PIPELINE.ps.meta.num_varying_inputs, 1);
    assert!(TESS_CONTROL.len() > 0 && TESS_EVAL.len() > 0);
}
