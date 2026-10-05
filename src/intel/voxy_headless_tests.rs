use super::voxy_headless_tgl;

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
