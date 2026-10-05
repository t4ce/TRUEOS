use super::voxy_headless_tgl;

#[test]
fn admits_only_sealed_physical_targets() {
    use super::voxy_headless_supports_physical_device as supports;
    assert!(supports(0x8086, 0x9A49, 0x01));
    assert!(supports(0x8086, 0x4680, 0x0C));
    for (vendor, device, revision) in [
        (0x10DE, 0x4680, 0x0C),
        (0x8086, 0x4680, 0x01),
        (0x8086, 0x4680, 0x0D),
        (0x8086, 0x9A49, 0x0C),
        (0x8086, 0x9A49, 0x02),
        (0x8086, 0x4682, 0x0C),
        (0x8086, 0x9A40, 0x01),
        (0xFFFF, 0xFFFF, 0xFF),
    ] {
        assert!(!supports(vendor, device, revision));
    }
}

#[test]
fn atlas_composition_admits_only_the_sampled_ps_compiler_target() {
    use super::voxy_headless_texture_supports_physical_device as supports;
    assert!(supports(0x8086, 0x4680, 0x0C));
    for (vendor, device, revision) in [
        (0x8086, 0x9A49, 0x01),
        (0x8086, 0x4680, 0x01),
        (0x8086, 0x4680, 0x0D),
        (0x8086, 0x4682, 0x0C),
        (0xFFFF, 0x4680, 0x0C),
    ] {
        assert!(!supports(vendor, device, revision));
    }
}

#[test]
fn atlas_composition_reuses_exact_programs_and_compatible_vue_payloads() {
    let atlas = super::voxy_headless_texture_pipeline();
    let camera = super::voxy_headless_pipeline();
    let sampled = super::clip_position3_uv_texture_pipeline();
    assert!(core::ptr::eq(atlas.vs.code, camera.vs.code));
    assert!(core::ptr::eq(atlas.ps.code, sampled.ps.code));
    assert_eq!(atlas.vs.meta.kernel.code_size_bytes, 528);
    assert_eq!(atlas.vs.meta.kernel.push_constant_bytes, 96);
    assert_eq!(atlas.vs.meta.kernel.binding_table_entry_count, 2);
    assert_eq!(atlas.vs.meta.urb_entry_output_length, 1);
    assert_eq!(atlas.ps.meta.kernel.code_offset_bytes, 576);
    assert_eq!(atlas.ps.meta.kernel.code_offset_bytes % 64, 0);
    assert!(atlas.ps.meta.kernel.code_offset_bytes >= atlas.vs.meta.kernel.code_size_bytes);
    assert_eq!(atlas.ps.meta.kernel.code_size_bytes, 160);
    assert_eq!(atlas.ps.meta.kernel.grf_start_register, 6);
    assert_eq!(atlas.ps.meta.kernel.binding_table_entry_count, 3);
    assert_eq!(atlas.ps.meta.kernel.sampler_count, 1);
    assert_eq!(atlas.ps.meta.num_varying_inputs, 1);
    assert_eq!(atlas.ps.meta.flat_inputs, 0);
    // Both stages export location0 at VUE slot2 using the same final URB
    // SEND. The sampled PS reads XY; Voxy's extra ZW do not shift that slot.
    assert_eq!(&atlas.vs.code[atlas.vs.code.len()-4..], &sampled.vs.code[sampled.vs.code.len()-4..]);
    let camera_capture = include_str!("../../../veloren-voxygen/src/headless/shaders/tgl/vs.state.txt");
    let sampled_capture = include_str!("../../crates/trueos-shader/clip_position3_uv_texture/vertex_TRUEOS_VS_state_v1.txt");
    for capture in [camera_capture, sampled_capture] {
        assert!(capture.contains("urb_entry_64b=1"));
        assert!(capture.contains("position_slot=1 uv_slot=2"));
    }
}

#[test]
fn native_code_and_identity_match_the_blueprint_package() {
    let source = include_bytes!("../../../veloren-voxygen/src/headless/render.wgsl");
    let mut digest = 0xcbf29ce484222325u64;
    for byte in source { digest = (digest ^ *byte as u64).wrapping_mul(0x100000001b3); }
    assert_eq!(digest, voxy_headless_tgl::SOURCE_FNV1A64);
    for (words, bytes) in [
        (voxy_headless_tgl::PIPELINE.vs.code, &include_bytes!("../../../veloren-voxygen/src/headless/shaders/tgl/vs.bin")[..]),
        (voxy_headless_tgl::PIPELINE.ps.code, &include_bytes!("../../../veloren-voxygen/src/headless/shaders/tgl/ps.bin")[..]),
    ] {
        let native: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(native, bytes);
    }
}
