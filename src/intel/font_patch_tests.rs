//! Standalone host tests for the fail-closed font patch artifact handoff.
use super::font_patch::*;

#[test]
fn source_only_artifact_cannot_be_enabled() {
    assert!(!AVAILABLE);
    assert_eq!(COMPILED_DEVICE_ID, 0);
    assert_eq!(SOURCE_SHA256, "0".repeat(64));
    assert!(VERTEX.is_empty());
    assert!(TESS_CONTROL.is_empty());
    assert!(TESS_EVAL.is_empty());
    assert!(FRAGMENT.is_empty());
    assert!(super::font_patch_pipeline().is_none());
    assert_eq!(
        super::font_patch_upload_layout(0, usize::MAX),
        Err("font-tessellation-artifact-unavailable")
    );
    assert_eq!(
        super::font_tessellation_stage_packets(Some([64, 128])),
        Err("font-tessellation-artifact-unavailable")
    );
}

#[test]
fn disabled_packets_clear_every_stage() {
    let packets = super::font_tessellation_stage_packets(None).unwrap();
    assert_eq!(packets[0], 0x781b0007);
    assert_eq!(packets[9], 0x781c0003);
    assert_eq!(packets[14], 0x781d0009);
    for (index, word) in packets.into_iter().enumerate() {
        if ![0, 9, 14].contains(&index) {
            assert_eq!(word, 0);
        }
    }
}

#[test]
fn font_contract_does_not_alias_cube_contract() {
    assert_eq!(CONTRACT_VERSION, 1);
    assert_ne!(CONTRACT_VERSION, super::patch_cube::CONTRACT_VERSION);
    assert!(!matches(&PIPELINE));
    assert!(!matches(&super::patch_cube::PIPELINE));
    assert_eq!(URB, [(0, 0, 0); 4]);
}
