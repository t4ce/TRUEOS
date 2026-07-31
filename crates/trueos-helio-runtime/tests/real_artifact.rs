use trueos_helio_runtime::{Camera, decode_artifact};

const SIMPLE_GRAPH: &[u8] = include_bytes!("../../../assets/helio/simple-cube.trueos.intel.helio");

#[test]
fn build_artifact_reaches_the_runtime_draw_contract() {
    let scene = decode_artifact(SIMPLE_GRAPH, 16.0 / 9.0, Camera::helio_simple_graph()).unwrap();
    assert_eq!(scene.triangles.len(), 12);
    assert_eq!(scene.clear_rgba[3], u8::MAX);
    assert!(
        scene
            .triangles
            .iter()
            .all(|triangle| triangle.rgba[3] == u8::MAX)
    );
}
