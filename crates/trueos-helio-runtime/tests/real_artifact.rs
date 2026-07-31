use trueos_helio_runtime::{
    Camera, DrawCommandSource, DrawIndexedIndirectArgs, decode_artifact_with_replay,
};

const SIMPLE_GRAPH: &[u8] = include_bytes!("../../../assets/helio/simple-cube.trueos.intel.helio");

#[test]
fn build_artifact_reaches_the_runtime_draw_contract() {
    let scene = decode_artifact_with_replay(SIMPLE_GRAPH, 16.0 / 9.0, Camera::helio_simple_graph())
        .unwrap();
    assert_eq!(scene.triangles.len(), 12);
    assert_eq!(scene.clear_rgba[3], u8::MAX);
    assert_eq!(scene.draw_source, DrawCommandSource::ArtifactReplayV1);
    assert_eq!(
        scene.source_draw_indexed_indirect,
        DrawIndexedIndirectArgs {
            index_count: 36,
            instance_count: 1,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0,
        }
    );
    assert_eq!(
        scene.resident_triangle_draw_indexed_indirect(0).unwrap(),
        DrawIndexedIndirectArgs::new(3)
    );
    assert!(
        scene
            .triangles
            .iter()
            .all(|triangle| triangle.rgba[3] == u8::MAX)
    );
}
