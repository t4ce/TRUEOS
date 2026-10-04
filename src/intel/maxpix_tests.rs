use super::*;

#[test]
fn maxpix_native_shader_matches_the_retained_point_payload() {
    let pipeline = maxpix_tornado::pipeline();
    assert_eq!(pipeline.vs.code.len() * 4, pipeline.vs.meta.kernel.code_size_bytes as usize);
    assert_eq!(pipeline.ps.code.len() * 4, pipeline.ps.meta.kernel.code_size_bytes as usize);
    assert_eq!(pipeline.ps.meta.kernel.code_offset_bytes % 64, 0);
    assert!(pipeline.ps.meta.kernel.code_offset_bytes >= pipeline.vs.meta.kernel.code_size_bytes);
    assert_eq!(pipeline.vs.meta.kernel.binding_table_entry_count, 4);
    assert_eq!(pipeline.ps.meta.kernel.binding_table_entry_count, 1);
    assert_eq!(pipeline.vs.meta.kernel.sampler_count, 0);
    assert_eq!(pipeline.ps.meta.kernel.sampler_count, 0);
    assert_eq!(pipeline.ps.meta.num_varying_inputs, 0);
    assert_eq!(maxpix_tornado::VF_COMPONENT_PACKING, 0xa77);
    assert_eq!(maxpix_tornado::VF_SGVS_DW1, 0xe002_4002);
    assert_eq!(maxpix_tornado::VF_SGVS_2_DW1, 0xb002_0002);
    assert_eq!(maxpix_tornado::VS_URB_READ_LENGTH, 1);
    assert_eq!(pipeline.vs.meta.urb_entry_output_length, 1);
    assert_eq!(pipeline.vs.meta.kernel.push_constant_bytes, 0);
    assert_eq!(pipeline.ps.meta.kernel.push_constant_bytes, 0);
}
