fn upload_triangle_shader_pipeline(
    warm: RenderWarmState,
    pipeline: &crate::intel::shader::TrianglePipeline,
    draw_rgba: Option<[u8; 4]>,
) -> Result<TriangleShaderLayout, &'static str> {
    upload_triangle_shader_pipeline_at(warm, pipeline, draw_rgba, GPU_VA_DRAW_STATE_BASE, true)
}

/// Upload the proven triangle shaders into a caller-owned state slot.
///
/// Probe callers retain the historical warm-state VA. A persistent scene
/// frame owner supplies a distinct VA per object, allowing all specialized
/// color shaders and fixed-function state to coexist until one batched frame
/// submission has retired.
fn upload_triangle_shader_pipeline_at(
    warm: RenderWarmState,
    pipeline: &crate::intel::shader::TrianglePipeline,
    draw_rgba: Option<[u8; 4]>,
    bo_gpu_base: u64,
    flush_upload: bool,
) -> Result<TriangleShaderLayout, &'static str> {
    let vs = stage_range("vs", pipeline.vs.meta.kernel, pipeline.vs.code)?;
    let ps = stage_range("ps", pipeline.ps.meta.kernel, pipeline.ps.code)?;
    let host_simd16_pipeline = crate::intel::shader::triangle_pipeline_simd16();
    let upload_host_ps_pair =
        pipeline.ps.code.as_ptr() == crate::intel::shader::triangle_pipeline().ps.code.as_ptr();
    let host_simd16 = if upload_host_ps_pair {
        Some(stage_range(
            "ps-simd16",
            host_simd16_pipeline.ps.meta.kernel,
            host_simd16_pipeline.ps.code,
        )?)
    } else {
        None
    };

    if pipeline.vs.meta.kernel.grf_used == 0 {
        return Err("vs-shader-grf-used-zero");
    }
    if pipeline.ps.meta.kernel.grf_used == 0 {
        return Err("ps-shader-grf-used-zero");
    }
    if pipeline.vs.meta.max_threads == 0 {
        return Err("vs-max-threads-zero");
    }

    if ranges_overlap(
        vs.code_offset_bytes,
        vs.code_size_bytes,
        ps.code_offset_bytes,
        ps.code_size_bytes,
    ) {
        return Err("shader-code-overlap");
    }

    let mut used_end = core::cmp::max(
        stage_end(vs.code_offset_bytes, vs.code_size_bytes).ok_or("shader-code-overflow")?,
        stage_end(ps.code_offset_bytes, ps.code_size_bytes).ok_or("shader-code-overflow")?,
    );
    if let Some(host_simd16) = host_simd16 {
        if ranges_overlap(
            ps.code_offset_bytes,
            ps.code_size_bytes,
            host_simd16.code_offset_bytes,
            host_simd16.code_size_bytes,
        ) || ranges_overlap(
            vs.code_offset_bytes,
            vs.code_size_bytes,
            host_simd16.code_offset_bytes,
            host_simd16.code_size_bytes,
        ) {
            return Err("host-ps-pair-code-overlap");
        }
        used_end = core::cmp::max(
            used_end,
            stage_end(host_simd16.code_offset_bytes, host_simd16.code_size_bytes)
                .ok_or("host-ps-pair-code-overflow")?,
        );
    }
    if used_end > warm.draw_state_len {
        return Err("shader-code-exceeds-state-bo");
    }

    upload_stage_code(warm.draw_state_virt, vs.code_offset_bytes, pipeline.vs.code)?;
    upload_stage_code(warm.draw_state_virt, ps.code_offset_bytes, pipeline.ps.code)?;
    if let Some(rgba) = draw_rgba {
        specialize_uploaded_triangle_ps_color(
            warm.draw_state_virt,
            ps.code_offset_bytes,
            pipeline,
            rgba,
        )?;
    }
    if let Some(host_simd16) = host_simd16 {
        upload_stage_code(
            warm.draw_state_virt,
            host_simd16.code_offset_bytes,
            host_simd16_pipeline.ps.code,
        )?;
        if let Some(rgba) = draw_rgba {
            // Both dispatch widths are enabled in 3DSTATE_PS. Specializing
            // only SIMD8 leaves the paired SIMD16 kernel at its baked
            // #0040FFFF color, so whichever fragments land on SIMD16 leak
            // isolated legacy-blue pixels into an otherwise colored draw.
            specialize_uploaded_triangle_ps_color(
                warm.draw_state_virt,
                host_simd16.code_offset_bytes,
                host_simd16_pipeline,
                rgba,
            )?;
        }
    }

    if flush_upload {
        crate::intel::dma_flush(warm.draw_state_virt, used_end);
    }

    let state_region_offset_bytes =
        crate::intel::align_up(used_end, crate::intel::WARM_ALIGN).ok_or("state-region-align")?;
    if state_region_offset_bytes > warm.draw_state_len {
        return Err("state-region-exceeds-state-bo");
    }

    let vs_gpu = bo_gpu_base + vs.code_offset_bytes as u64;
    let ps_gpu = bo_gpu_base + ps.code_offset_bytes as u64;

    Ok(TriangleShaderLayout {
        vs: TriangleShaderStageLayout {
            code_offset_bytes: vs.code_offset_bytes as u32,
            code_gpu_addr: vs_gpu,
            ksp_offset_bytes: pipeline.vs.meta.kernel.ksp_offset_bytes,
            ksp_gpu_addr: vs_gpu + pipeline.vs.meta.kernel.ksp_offset_bytes as u64,
            code_size_bytes: vs.code_size_bytes as u32,
        },
        ps: TriangleShaderStageLayout {
            code_offset_bytes: ps.code_offset_bytes as u32,
            code_gpu_addr: ps_gpu,
            ksp_offset_bytes: pipeline.ps.meta.kernel.ksp_offset_bytes,
            ksp_gpu_addr: ps_gpu + pipeline.ps.meta.kernel.ksp_offset_bytes as u64,
            code_size_bytes: ps.code_size_bytes as u32,
        },
        state_region_gpu_addr: bo_gpu_base + state_region_offset_bytes as u64,
        state_region_offset_bytes: state_region_offset_bytes as u32,
        used_bytes: used_end as u32,
    })
}

const TRIANGLE_PS_COLOR_WORDS: [usize; 4] = [1, 3, 5, 7];

fn encode_compacted_float_immediate(component: u8) -> u32 {
    let normalized = component as f32 / u8::MAX as f32;
    // A compacted gfx12 float immediate carries only IEEE-754 bits 31:19.
    // Bits 18:16 are the compacted source/instruction index and must remain
    // `001`; bits 15:0 are instruction encoding. The baked 0.0, 0.25 and 1.0
    // constants all happen to have zeroes in float bits 18:16, which hid this
    // boundary. Copying bits 18:16 from arbitrary RGBA values corrupts the
    // compacted MOV and can stall the pixel backend before PS invocation.
    (normalized.to_bits() & 0xFFF8_0000) | 0x0001_0000
}

#[cfg(test)]
mod compacted_float_immediate_tests {
    use super::{
        TRIANGLE_PS_COLOR_WORDS, encode_compacted_float_immediate,
        specialize_uploaded_triangle_ps_color,
    };

    #[test]
    fn every_rgba_byte_preserves_the_compacted_instruction_fields() {
        for component in u8::MIN..=u8::MAX {
            let normalized = component as f32 / u8::MAX as f32;
            let encoded = encode_compacted_float_immediate(component);
            assert_eq!(encoded & 0xFFF8_0000, normalized.to_bits() & 0xFFF8_0000);
            assert_eq!(encoded & 0x0007_0000, 0x0001_0000);
            assert_eq!(encoded & 0x0000_FFFF, 0);
        }
    }

    #[test]
    fn baked_shader_constants_remain_bit_exact() {
        assert_eq!(encode_compacted_float_immediate(0), 0x0001_0000);
        assert_eq!(encode_compacted_float_immediate(64), 0x3E81_0000);
        assert_eq!(encode_compacted_float_immediate(255), 0x3F81_0000);
    }

    #[test]
    fn both_enabled_pixel_dispatch_widths_receive_the_draw_color() {
        let rgba = [17, 91, 203, 149];
        for pipeline in [
            crate::intel::shader::triangle_pipeline(),
            crate::intel::shader::triangle_pipeline_simd16(),
        ] {
            let mut uploaded = [0u32; 12];
            uploaded.copy_from_slice(pipeline.ps.code);
            specialize_uploaded_triangle_ps_color(
                uploaded.as_mut_ptr() as *mut u8,
                0,
                pipeline,
                rgba,
            )
            .expect("constant-color shader contract");

            for (word_index, component) in TRIANGLE_PS_COLOR_WORDS.into_iter().zip(rgba) {
                assert_eq!(uploaded[word_index], encode_compacted_float_immediate(component));
            }
        }
    }
}

fn specialize_uploaded_triangle_ps_color(
    dst_base: *mut u8,
    ps_offset_bytes: usize,
    pipeline: &crate::intel::shader::TrianglePipeline,
    rgba: [u8; 4],
) -> Result<(), &'static str> {
    let constant_color_pipeline = crate::intel::shader::triangle_pipeline();
    let constant_color_simd16_pipeline = crate::intel::shader::triangle_pipeline_simd16();
    let simd8_contract = pipeline.ps.code.as_ptr() == constant_color_pipeline.ps.code.as_ptr()
        && pipeline.ps.code.len() == 12
        && pipeline.ps.code[0] == 0xA17F_0061
        && pipeline.ps.code[2] == 0xA17C_0061
        && pipeline.ps.code[4] == 0xA17D_0061
        && pipeline.ps.code[6] == 0xA17E_0061
        && pipeline.ps.code[8..] == [0x0003_0132, 0x0000_0004, 0x5800_7F0C, 0x00C4_7C1C];
    let simd16_contract = pipeline.ps.code.as_ptr()
        == constant_color_simd16_pipeline.ps.code.as_ptr()
        && pipeline.ps.code.len() == 12
        && pipeline.ps.code[0] == 0xA07E_0061
        && pipeline.ps.code[2] == 0xA078_0061
        && pipeline.ps.code[4] == 0xA07A_0061
        && pipeline.ps.code[6] == 0xA07C_0061
        && pipeline.ps.code[8..] == [0x0004_0132, 0x0000_0004, 0x5000_7E14, 0x00C4_7834];
    if (!simd8_contract && !simd16_contract)
        || encode_compacted_float_immediate(0) != pipeline.ps.code[1]
        || encode_compacted_float_immediate(64) != pipeline.ps.code[3]
        || encode_compacted_float_immediate(255) != pipeline.ps.code[5]
        || encode_compacted_float_immediate(255) != pipeline.ps.code[7]
    {
        return Err("ps-color-specialization-contract");
    }

    let uploaded = unsafe {
        core::slice::from_raw_parts_mut(
            dst_base.add(ps_offset_bytes) as *mut u32,
            pipeline.ps.code.len(),
        )
    };
    for (word_index, component) in TRIANGLE_PS_COLOR_WORDS.into_iter().zip(rgba) {
        uploaded[word_index] = encode_compacted_float_immediate(component);
    }
    Ok(())
}

#[derive(Copy, Clone)]
struct StageUploadRange {
    code_offset_bytes: usize,
    code_size_bytes: usize,
}

fn stage_range(
    stage_name: &'static str,
    meta: crate::intel::shader::ShaderKernelMetadata,
    code: &'static [u32],
) -> Result<StageUploadRange, &'static str> {
    if meta.code_size_bytes == 0 || code.is_empty() {
        return Err(stage_error(stage_name, "shader-empty"));
    }

    let code_len_bytes = code
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or(stage_error(stage_name, "shader-code-len-overflow"))?;
    let declared_size = usize::try_from(meta.code_size_bytes)
        .map_err(|_| stage_error(stage_name, "shader-size-convert"))?;
    if declared_size != code_len_bytes {
        return Err(stage_error(stage_name, "shader-size-mismatch"));
    }

    let code_offset = usize::try_from(meta.code_offset_bytes)
        .map_err(|_| stage_error(stage_name, "shader-offset-convert"))?;
    let code_alignment = usize::try_from(meta.code_alignment_bytes)
        .map_err(|_| stage_error(stage_name, "shader-align-convert"))?;
    if code_alignment == 0 || code_offset % code_alignment != 0 {
        return Err(stage_error(stage_name, "shader-offset-alignment"));
    }

    let ksp_offset = usize::try_from(meta.ksp_offset_bytes)
        .map_err(|_| stage_error(stage_name, "shader-ksp-convert"))?;
    if ksp_offset % 64 != 0 {
        return Err(stage_error(stage_name, "shader-ksp-alignment"));
    }
    if ksp_offset >= declared_size {
        return Err(stage_error(stage_name, "shader-ksp-range"));
    }

    Ok(StageUploadRange {
        code_offset_bytes: code_offset,
        code_size_bytes: declared_size,
    })
}

fn upload_stage_code(
    dst_base: *mut u8,
    offset_bytes: usize,
    code: &'static [u32],
) -> Result<(), &'static str> {
    let len_bytes = code
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("shader-copy-len-overflow")?;
    if len_bytes == 0 {
        return Ok(());
    }

    unsafe {
        core::ptr::copy_nonoverlapping(
            code.as_ptr() as *const u8,
            dst_base.add(offset_bytes),
            len_bytes,
        );
    }
    Ok(())
}

fn shader_word_signature(words: &[u32]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &word in words {
        hash ^= word as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn log_uploaded_triangle_shader_verification(
    warm: RenderWarmState,
    pipeline: &crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    submit_name: &'static str,
    draw_rgba: Option<[u8; 4]>,
) {
    let uploaded_vs = unsafe {
        core::slice::from_raw_parts(
            warm.draw_state_virt
                .add(shader_layout.vs.code_offset_bytes as usize) as *const u32,
            pipeline.vs.code.len(),
        )
    };
    let uploaded_ps = unsafe {
        core::slice::from_raw_parts(
            warm.draw_state_virt
                .add(shader_layout.ps.code_offset_bytes as usize) as *const u32,
            pipeline.ps.code.len(),
        )
    };
    let vs_baked_sig = shader_word_signature(pipeline.vs.code);
    let vs_uploaded_sig = shader_word_signature(uploaded_vs);
    let ps_baked_sig = shader_word_signature(pipeline.ps.code);
    let ps_uploaded_sig = shader_word_signature(uploaded_ps);
    let ps_expected_match = uploaded_ps.iter().enumerate().all(|(index, uploaded)| {
        let expected = draw_rgba
            .and_then(|rgba| {
                TRIANGLE_PS_COLOR_WORDS
                    .iter()
                    .position(|&word_index| word_index == index)
                    .map(|component| encode_compacted_float_immediate(rgba[component]))
            })
            .unwrap_or(pipeline.ps.code[index]);
        *uploaded == expected
    });
    let color_binding = if draw_rgba.is_some() {
        "transient-ps-immediate-specialization"
    } else {
        "baked-ps-constant"
    };
    let vs_first = pipeline.vs.code.first().copied().unwrap_or(0);
    let vs_uploaded_first = uploaded_vs.first().copied().unwrap_or(0);
    let vs_last = pipeline.vs.code.last().copied().unwrap_or(0);
    let vs_uploaded_last = uploaded_vs.last().copied().unwrap_or(0);
    if submit_name == "vs-draw-frontier" {
        intel_render_focus_log!(
            "{} shader-upload-verify note={} vs_match={} vs_baked_sig=0x{:016X} vs_uploaded_sig=0x{:016X} vs_first=0x{:08X}/0x{:08X} vs_last=0x{:08X}/0x{:08X} ps_expected_match={} ps_baked_match={} ps_baked_sig=0x{:016X} ps_uploaded_sig=0x{:016X} color_binding={} rgba={:?}\n",
            submit_name,
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.vs.code == uploaded_vs) as u8,
            vs_baked_sig,
            vs_uploaded_sig,
            vs_first,
            vs_uploaded_first,
            vs_last,
            vs_uploaded_last,
            ps_expected_match as u8,
            (pipeline.ps.code == uploaded_ps) as u8,
            ps_baked_sig,
            ps_uploaded_sig,
            color_binding,
            draw_rgba,
        );
    } else {
        intel_render_verbose_log!(
            "{} shader-upload-verify note={} vs_match={} vs_baked_sig=0x{:016X} vs_uploaded_sig=0x{:016X} vs_first=0x{:08X}/0x{:08X} vs_last=0x{:08X}/0x{:08X} ps_expected_match={} ps_baked_match={} ps_baked_sig=0x{:016X} ps_uploaded_sig=0x{:016X} color_binding={} rgba={:?}\n",
            submit_name,
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.vs.code == uploaded_vs) as u8,
            vs_baked_sig,
            vs_uploaded_sig,
            vs_first,
            vs_uploaded_first,
            vs_last,
            vs_uploaded_last,
            ps_expected_match as u8,
            (pipeline.ps.code == uploaded_ps) as u8,
            ps_baked_sig,
            ps_uploaded_sig,
            color_binding,
            draw_rgba,
        );
    }
}

fn stage_end(offset_bytes: usize, size_bytes: usize) -> Option<usize> {
    offset_bytes.checked_add(size_bytes)
}

fn ranges_overlap(a_offset: usize, a_size: usize, b_offset: usize, b_size: usize) -> bool {
    let Some(a_end) = stage_end(a_offset, a_size) else {
        return true;
    };
    let Some(b_end) = stage_end(b_offset, b_size) else {
        return true;
    };
    a_offset < b_end && b_offset < a_end
}

fn stage_error(stage_name: &'static str, reason: &'static str) -> &'static str {
    match (stage_name, reason) {
        ("vs", "shader-empty") => "vs-shader-empty",
        ("vs", "shader-code-len-overflow") => "vs-shader-code-len-overflow",
        ("vs", "shader-size-convert") => "vs-shader-size-convert",
        ("vs", "shader-size-mismatch") => "vs-shader-size-mismatch",
        ("vs", "shader-offset-convert") => "vs-shader-offset-convert",
        ("vs", "shader-align-convert") => "vs-shader-align-convert",
        ("vs", "shader-offset-alignment") => "vs-shader-offset-alignment",
        ("vs", "shader-ksp-convert") => "vs-shader-ksp-convert",
        ("vs", "shader-ksp-alignment") => "vs-shader-ksp-alignment",
        ("vs", "shader-ksp-range") => "vs-shader-ksp-range",
        ("ps", "shader-empty") => "ps-shader-empty",
        ("ps", "shader-code-len-overflow") => "ps-shader-code-len-overflow",
        ("ps", "shader-size-convert") => "ps-shader-size-convert",
        ("ps", "shader-size-mismatch") => "ps-shader-size-mismatch",
        ("ps", "shader-offset-convert") => "ps-shader-offset-convert",
        ("ps", "shader-align-convert") => "ps-shader-align-convert",
        ("ps", "shader-offset-alignment") => "ps-shader-offset-alignment",
        ("ps", "shader-ksp-convert") => "ps-shader-ksp-convert",
        ("ps", "shader-ksp-alignment") => "ps-shader-ksp-alignment",
        ("ps", "shader-ksp-range") => "ps-shader-ksp-range",
        _ => "shader-stage-error",
    }
}

fn prepare_triangle_draw_resources(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> Option<TriangleDrawPrep> {
    prepare_triangle_draw_resources_for_geometry(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        VfPrimitiveGeometry::Canonical,
    )
}

fn prepare_triangle_draw_resources_for_geometry(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    geometry: VfPrimitiveGeometry,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if warm.vertex_len < TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_STRIDE {
        return None;
    }
    if warm.draw_state_len == 0 {
        return None;
    }

    let vertex_proof = write_triangle_vertices_for_geometry(warm, geometry)?;

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: vertex_proof.vertex_count,
        vertex_stride: vertex_proof.vertex_stride,
        vertex_buffer_bytes: u32::try_from(vertex_proof.byte_len).ok()?,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: vertex_proof.gpu_addr,
        index_buffer: None,
        indirect_args_gpu_addr: None,
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn prepare_triangle_draw_resources_for_vertices(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    label: &'static str,
    triangle: [[f32; 3]; TRIANGLE_DRAW_VERTICES],
) -> Option<TriangleDrawPrep> {
    prepare_triangle_draw_resources_for_vertex_slice(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        label,
        &triangle,
    )
}

fn prepare_triangle_draw_resources_for_vertex_slice(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    label: &'static str,
    vertices: &[[f32; 3]],
) -> Option<TriangleDrawPrep> {
    prepare_triangle_draw_resources_for_vertex_slice_with_state_clear(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        label,
        vertices,
        true,
    )
}

/// Scene state is completely overwritten by shader/state staging immediately
/// after resource preparation, so clearing and publishing the whole 8 KiB
/// slot here would only duplicate that later visibility operation.
fn prepare_triangle_draw_resources_for_scene_vertex_slice(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    label: &'static str,
    vertices: &[[f32; 3]],
) -> Option<TriangleDrawPrep> {
    prepare_triangle_draw_resources_for_vertex_slice_with_state_clear(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        label,
        vertices,
        false,
    )
}

fn prepare_triangle_draw_resources_for_vertex_slice_with_state_clear(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    label: &'static str,
    vertices: &[[f32; 3]],
    clear_state: bool,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if vertices.is_empty() || vertices.len() % 3 != 0 {
        return None;
    }
    if warm.vertex_len < vertices.len().saturating_mul(TRIANGLE_DRAW_VERTEX_STRIDE) {
        return None;
    }
    if warm.draw_state_len == 0 {
        return None;
    }

    let vertex_proof = write_triangle_vertex_slice(warm, label, vertices)?;

    if clear_state {
        unsafe {
            core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
        }
        crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);
    }

    Some(TriangleDrawPrep {
        vertex_count: vertex_proof.vertex_count,
        vertex_stride: vertex_proof.vertex_stride,
        vertex_buffer_bytes: u32::try_from(vertex_proof.byte_len).ok()?,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: vertex_proof.gpu_addr,
        index_buffer: None,
        indirect_args_gpu_addr: None,
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn prepare_triangle_draw_resources_for_indexed_vertex_slice(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    label: &'static str,
    vertices: &[[f32; 3]],
    indices: &[u32],
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if vertices.len() < 3
        || indices.is_empty()
        || !indices.len().is_multiple_of(3)
        || indices
            .iter()
            .any(|index| *index as usize >= vertices.len())
    {
        return None;
    }

    let vertex_proof = write_triangle_vertex_slice(warm, label, vertices)?;
    let index_offset = crate::intel::align_up(vertex_proof.byte_len, 64)?;
    let index_byte_len = indices.len().checked_mul(core::mem::size_of::<u32>())?;
    let upload_end = index_offset.checked_add(index_byte_len)?;
    if upload_end > warm.vertex_len {
        return None;
    }

    let index_dst = unsafe { warm.vertex_virt.add(index_offset) as *mut u32 };
    let index_slice = unsafe { core::slice::from_raw_parts_mut(index_dst, indices.len()) };
    index_slice.copy_from_slice(indices);
    let cpu_readback_ok = index_slice == indices;
    crate::intel::dma_flush(unsafe { warm.vertex_virt.add(index_offset) }, index_byte_len);

    let index_gpu_addr = GPU_VA_VERTEX_BASE.checked_add(index_offset as u64)?;
    intel_render_focus_log!(
        "indexed-mesh-upload-proof accepted={} geometry={} unique_vertices={} indices={} triangles={} vertex_bytes={} index_bytes={} total_bytes={} vb_gpu=0x{:X} ib_gpu=0x{:X} index_format=u32 persistent_mapping=1 cpu_readback_ok={} does_not_prove=index_fetch\n",
        cpu_readback_ok as u8,
        label,
        vertices.len(),
        indices.len(),
        indices.len() / 3,
        vertex_proof.byte_len,
        index_byte_len,
        upload_end,
        vertex_proof.gpu_addr,
        index_gpu_addr,
        cpu_readback_ok as u8,
    );
    if !cpu_readback_ok {
        return None;
    }

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: u32::try_from(indices.len()).ok()?,
        vertex_stride: vertex_proof.vertex_stride,
        vertex_buffer_bytes: u32::try_from(vertex_proof.byte_len).ok()?,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: vertex_proof.gpu_addr,
        index_buffer: Some(TriangleIndexBufferPrep {
            index_count: u32::try_from(indices.len()).ok()?,
            byte_len: u32::try_from(index_byte_len).ok()?,
            gpu_addr: index_gpu_addr,
        }),
        indirect_args_gpu_addr: None,
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch,
        target_w,
        target_h,
    })
}

pub(crate) fn create_resident_font_mesh(
    vertices: &[[f32; 2]],
    indices: &[u32],
    bounds: (f32, f32, f32, f32),
) -> Result<ResidentFontMesh, &'static str> {
    if vertices.len() < 3
        || indices.is_empty()
        || !indices.len().is_multiple_of(3)
        || indices
            .iter()
            .any(|index| *index as usize >= vertices.len())
    {
        return Err("resident-font-shape");
    }
    if crate::intel::claimed_device().is_none() {
        return Err("no-device");
    }
    let warm = warm_state().ok_or("render-boot-not-ready")?;
    if render_ppgtt_pml4_phys() == 0 || warm.vertex_len == 0 {
        return Err("render-ppgtt");
    }

    let (min_x, min_y, max_x, max_y) = bounds;
    let width = (max_x - min_x).max(1.0);
    let height = (max_y - min_y).max(1.0);
    let scale = 1.8 / width.max(height);
    let center_x = (min_x + max_x) * 0.5;
    let center_y = (min_y + max_y) * 0.5;
    let mut draw_vertices = Vec::with_capacity(vertices.len());
    for source in vertices {
        draw_vertices.push([
            (source[0] - center_x) * scale,
            (center_y - source[1]) * scale,
            0.5,
        ]);
    }
    let mut draw_indices = Vec::with_capacity(indices.len());
    for triangle in indices.chunks_exact(3) {
        let v0 = draw_vertices[triangle[0] as usize];
        let v1 = draw_vertices[triangle[1] as usize];
        let v2 = draw_vertices[triangle[2] as usize];
        let area2 = (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]);
        if area2 < 0.0 {
            draw_indices.extend_from_slice(&[triangle[0], triangle[2], triangle[1]]);
        } else {
            draw_indices.extend_from_slice(triangle);
        }
    }

    create_resident_triangle_mesh(&draw_vertices, &draw_indices)
}

/// Upload an indexed clip-space triangle mesh into persistent render PPGTT
/// storage. Draw calls borrow its GPU addresses directly until release.
pub(crate) fn create_resident_triangle_mesh(
    draw_vertices: &[[f32; 3]],
    draw_indices: &[u32],
) -> Result<ResidentTriangleMesh, &'static str> {
    if draw_vertices.len() < 3
        || draw_indices.is_empty()
        || !draw_indices.len().is_multiple_of(3)
        || draw_vertices
            .iter()
            .flatten()
            .any(|component| !component.is_finite())
        || draw_indices
            .iter()
            .any(|index| *index as usize >= draw_vertices.len())
    {
        return Err("resident-triangle-shape");
    }
    if crate::intel::claimed_device().is_none() {
        return Err("no-device");
    }
    let warm = warm_state().ok_or("render-boot-not-ready")?;
    if render_ppgtt_pml4_phys() == 0 || warm.vertex_len == 0 {
        return Err("render-ppgtt");
    }

    let vertex_bytes = draw_vertices
        .len()
        .checked_mul(core::mem::size_of::<[f32; 3]>())
        .ok_or("resident-triangle-bytes")?;
    let index_offset = crate::intel::align_up(vertex_bytes, 64).ok_or("resident-triangle-align")?;
    let index_bytes = draw_indices
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("resident-triangle-bytes")?;
    let index_end = index_offset
        .checked_add(index_bytes)
        .ok_or("resident-triangle-bytes")?;
    // Keep Helio's standard 20-byte indexed-indirect record in the same
    // stable PPGTT allocation as the geometry it addresses. This makes the
    // record independently writable by a later cull/compaction pass without
    // introducing target GPU addresses into the .helio artifact.
    let indirect_args_offset =
        crate::intel::align_up(index_end, 64).ok_or("resident-triangle-align")?;
    let used_bytes = indirect_args_offset
        .checked_add(DRAW_INDEXED_INDIRECT_BYTES)
        .ok_or("resident-triangle-bytes")?;
    let storage_bytes =
        crate::intel::align_up(used_bytes, 4096).ok_or("resident-triangle-align")?;
    let gpu_base = reserve_persistent_render_gpu_va(storage_bytes).ok_or("resident-triangle-va")?;
    let vertex_count = u32::try_from(draw_vertices.len()).map_err(|_| "resident-triangle-count")?;
    let vertex_bytes_u32 = u32::try_from(vertex_bytes).map_err(|_| "resident-triangle-count")?;
    let index_count = u32::try_from(draw_indices.len()).map_err(|_| "resident-triangle-count")?;
    let index_bytes_u32 = u32::try_from(index_bytes).map_err(|_| "resident-triangle-count")?;
    let index_gpu_addr = gpu_base
        .checked_add(index_offset as u64)
        .ok_or("resident-triangle-address")?;
    let indirect_args_gpu_addr = gpu_base
        .checked_add(indirect_args_offset as u64)
        .ok_or("resident-triangle-address")?;
    let Some((storage_phys, storage_virt)) = crate::dma::alloc(storage_bytes, 4096) else {
        return Err("resident-triangle-alloc");
    };

    unsafe {
        core::ptr::write_bytes(storage_virt, 0, storage_bytes);
        core::ptr::copy_nonoverlapping(
            draw_vertices.as_ptr() as *const u8,
            storage_virt,
            vertex_bytes,
        );
        core::ptr::copy_nonoverlapping(
            draw_indices.as_ptr() as *const u8,
            storage_virt.add(index_offset),
            index_bytes,
        );
        for (index, value) in [index_count, 1, 0, 0, 0].into_iter().enumerate() {
            core::ptr::copy_nonoverlapping(
                value.to_le_bytes().as_ptr(),
                storage_virt.add(indirect_args_offset + index * core::mem::size_of::<u32>()),
                core::mem::size_of::<u32>(),
            );
        }
    }
    crate::intel::dma_flush(storage_virt, storage_bytes);
    if !map_render_ppgtt_range(gpu_base, storage_phys, storage_bytes) {
        crate::dma::dealloc(storage_virt, storage_bytes);
        return Err("resident-triangle-map");
    }

    let resident = ResidentTriangleMesh {
        storage_phys,
        storage_virt,
        storage_bytes,
        gpu_base,
        vertex_gpu_addr: gpu_base,
        vertex_count,
        vertex_bytes: vertex_bytes_u32,
        index_gpu_addr,
        index_count,
        index_bytes: index_bytes_u32,
        indirect_args_gpu_addr,
        indirect_args_offset,
    };
    intel_render_focus_log!(
        "resident-triangle upload authority=gpu-resident phys=0x{:X} gpu=0x{:X} bytes=0x{:X} vertices={} indices={} indirect_gpu=0x{:X} indirect_stride={} cpu_uploads=1 retained=1\n",
        resident.storage_phys,
        resident.gpu_base,
        resident.storage_bytes,
        resident.vertex_count,
        resident.index_count,
        resident.indirect_args_gpu_addr,
        DRAW_INDEXED_INDIRECT_BYTES,
    );
    Ok(resident)
}

pub(crate) fn release_resident_font_mesh(mesh: &ResidentFontMesh) -> bool {
    release_resident_triangle_mesh(mesh)
}

/// Replace a resident job's vertex/index payload without changing its PPGTT
/// mapping. Stable projected topology makes camera and transform updates cheap
/// while color changes require no geometry write at all.
pub(crate) fn update_resident_triangle_mesh(
    mesh: &ResidentTriangleMesh,
    vertices: &[[f32; 3]],
    indices: &[u32],
) -> Result<(), &'static str> {
    let vertex_bytes = vertices
        .len()
        .checked_mul(core::mem::size_of::<[f32; 3]>())
        .ok_or("resident-triangle-bytes")?;
    let index_bytes = indices
        .len()
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("resident-triangle-bytes")?;
    if vertices.len() != mesh.vertex_count as usize
        || indices.len() != mesh.index_count as usize
        || vertex_bytes != mesh.vertex_bytes as usize
        || index_bytes != mesh.index_bytes as usize
        || vertices
            .iter()
            .flatten()
            .any(|component| !component.is_finite())
        || indices
            .iter()
            .any(|index| *index as usize >= vertices.len())
    {
        return Err("resident-triangle-update-shape");
    }
    let index_offset = usize::try_from(mesh.index_gpu_addr.saturating_sub(mesh.gpu_base))
        .map_err(|_| "resident-triangle-address")?;
    if index_offset.saturating_add(index_bytes) > mesh.storage_bytes {
        return Err("resident-triangle-address");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            vertices.as_ptr() as *const u8,
            mesh.storage_virt,
            vertex_bytes,
        );
        core::ptr::copy_nonoverlapping(
            indices.as_ptr() as *const u8,
            mesh.storage_virt.add(index_offset),
            index_bytes,
        );
    }
    // Only the live vertex/index payload changed. Flushing the page-rounded
    // allocation made small streaming meshes pay for untouched tail space on
    // every frame.
    crate::intel::dma_flush(mesh.storage_virt, index_offset + index_bytes);
    Ok(())
}

/// Replace only the mutable vertex payload of a resident indexed mesh.
///
/// Helio keeps mesh topology resident while cameras and object transforms
/// change. Its indexed scene paths therefore must not copy or flush the
/// immutable index range again on every frame.
pub(crate) fn update_resident_triangle_vertices(
    mesh: &ResidentTriangleMesh,
    vertices: &[[f32; 3]],
) -> Result<(), &'static str> {
    let vertex_bytes = vertices
        .len()
        .checked_mul(core::mem::size_of::<[f32; 3]>())
        .ok_or("resident-triangle-bytes")?;
    if vertices.len() != mesh.vertex_count as usize
        || vertex_bytes != mesh.vertex_bytes as usize
        || vertices
            .iter()
            .flatten()
            .any(|component| !component.is_finite())
        || vertex_bytes > mesh.storage_bytes
    {
        return Err("resident-triangle-update-shape");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            vertices.as_ptr() as *const u8,
            mesh.storage_virt,
            vertex_bytes,
        );
    }
    crate::intel::dma_flush(mesh.storage_virt, vertex_bytes);
    Ok(())
}

/// Publish one exact WGPU/Helio DrawIndexedIndirectArgs record beside a
/// resident mesh. The RCS scene path consumes these bytes without translating
/// them into a CPU-authored 3DPRIMITIVE payload.
pub(crate) fn update_resident_triangle_draw_indexed_indirect(
    mesh: &ResidentTriangleMesh,
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
) -> Result<(), &'static str> {
    let draw_end = first_index
        .checked_add(index_count)
        .ok_or("resident-indirect-index-range")?;
    if index_count == 0
        // This first retained backend expands transforms into vertices and
        // has no instance storage binding yet. Zero remains valid for Helio's
        // GPU culling result; one is the only drawable instance contract.
        || instance_count > 1
        || base_vertex != 0
        || first_instance != 0
        || draw_end > mesh.index_count
        || mesh.indirect_args_offset.saturating_add(DRAW_INDEXED_INDIRECT_BYTES)
            > mesh.storage_bytes
        || mesh.indirect_args_gpu_addr
            != mesh.gpu_base.saturating_add(mesh.indirect_args_offset as u64)
    {
        return Err("resident-indirect-shape");
    }
    let words = [
        index_count,
        instance_count,
        first_index,
        base_vertex as u32,
        first_instance,
    ];
    unsafe {
        for (index, value) in words.into_iter().enumerate() {
            core::ptr::copy_nonoverlapping(
                value.to_le_bytes().as_ptr(),
                mesh.storage_virt
                    .add(mesh.indirect_args_offset + index * core::mem::size_of::<u32>()),
                core::mem::size_of::<u32>(),
            );
        }
        crate::intel::dma_flush(
            mesh.storage_virt.add(mesh.indirect_args_offset),
            DRAW_INDEXED_INDIRECT_BYTES,
        );
    }
    Ok(())
}

pub(crate) fn release_resident_triangle_mesh(mesh: &ResidentTriangleMesh) -> bool {
    if !unmap_render_ppgtt_range(mesh.gpu_base, mesh.storage_bytes) {
        return false;
    }
    crate::dma::dealloc(mesh.storage_virt, mesh.storage_bytes);
    recycle_persistent_render_gpu_va(mesh.gpu_base, mesh.storage_bytes);
    true
}

const CHURN_FORWARD_MESH_COUNT: usize = trueos_helio_runtime::churn::SHAPE_COUNT;
const CHURN_FORWARD_DRAW_COUNT: usize = trueos_helio_runtime::churn::DRAW_GROUP_COUNT;
const CHURN_FORWARD_VERTICES_PER_MESH: usize = 24;
const CHURN_FORWARD_INDICES_PER_MESH: usize = 36;
static CHURN_FORWARD_PIPELINE: spin::Mutex<Option<crate::intel::shader::TrianglePipeline>> =
    spin::Mutex::new(None);

fn churn_forward_stage_words(bytes: &[u8]) -> Result<&'static [u32], &'static str> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(core::mem::size_of::<u32>()) {
        return Err("churn-native-stage-shape");
    }
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    // The embedded artifact and this service are both boot-lifetime owners.
    // Keeping one aligned native-code copy avoids assuming that a packed
    // HELIOA section happens to have Rust `u32` alignment.
    Ok(alloc::boxed::Box::leak(words.into_boxed_slice()))
}

fn churn_forward_pipeline(
    artifact: trueos_helio_artifact::Artifact<'_>,
    program: trueos_helio_artifact::churn_forward::Program<'_>,
) -> Result<crate::intel::shader::TrianglePipeline, &'static str> {
    use crate::intel::shader::{
        DispatchMode, ShaderKernelMetadata, TrianglePipeline, TrianglePixelShader,
        TrianglePixelShaderMetadata, TriangleVertexShader, TriangleVertexShaderMetadata,
    };

    let vertex = program.vertex_stage();
    let fragment = program.fragment_stage();
    if vertex.simd_width != 8
        || fragment.simd_width != 8
        || vertex.code_offset_bytes != 0
        || fragment.code_offset_bytes != 0
        || vertex.code_alignment_bytes != 64
        || fragment.code_alignment_bytes != 64
        || vertex.ksp_offset_bytes != 0
        || fragment.ksp_offset_bytes != 0
        || vertex.grf_start_register != 2
        || fragment.grf_start_register != 2
        || vertex.grf_used != 128
        || fragment.grf_used != 128
        || vertex.max_threads != 64
        || fragment.max_threads != 64
        || vertex.binding_table_entry_count != 4
        || fragment.binding_table_entry_count != 1
        || vertex.sampler_count != 0
        || fragment.sampler_count != 0
        || vertex.push_constant_bytes != 0
        || fragment.push_constant_bytes != 0
        || vertex.urb_entry_output_length != 1
        || vertex.num_varying_inputs != 0
        || fragment.urb_entry_output_length != 0
        || fragment.num_varying_inputs != 2
        || !fragment.uses_vmask
        || fragment.computed_stencil
        || fragment.persample_dispatch
        || fragment.computed_depth_mode != 0
        || fragment.flat_inputs != 2
    {
        return Err("churn-native-stage-contract");
    }
    let mut cached = CHURN_FORWARD_PIPELINE.lock();
    if let Some(pipeline) = *cached {
        return Ok(pipeline);
    }
    let vs_section = artifact
        .section(vertex.section_name)
        .ok_or("churn-native-vs-section")?;
    let ps_section = artifact
        .section(fragment.section_name)
        .ok_or("churn-native-ps-section")?;
    let vs_code = churn_forward_stage_words(vs_section.data)?;
    let ps_code = churn_forward_stage_words(ps_section.data)?;
    let ps_offset =
        crate::intel::align_up(vs_section.data.len(), 64).ok_or("churn-native-shader-layout")?;
    let ps_offset = u32::try_from(ps_offset).map_err(|_| "churn-native-shader-layout")?;

    let kernel = |stage: trueos_helio_artifact::churn_forward::StageRef<'_>, offset| {
        Ok::<_, &'static str>(ShaderKernelMetadata {
            code_offset_bytes: offset,
            code_size_bytes: stage.code_size_bytes,
            code_alignment_bytes: stage.code_alignment_bytes,
            ksp_offset_bytes: stage.ksp_offset_bytes,
            dispatch_mode: DispatchMode::Simd8,
            grf_start_register: u8::try_from(stage.grf_start_register)
                .map_err(|_| "churn-native-stage-metadata")?,
            grf_used: u8::try_from(stage.grf_used).map_err(|_| "churn-native-stage-metadata")?,
            push_constant_bytes: stage.push_constant_bytes,
            binding_table_entry_count: u8::try_from(stage.binding_table_entry_count)
                .map_err(|_| "churn-native-stage-metadata")?,
            sampler_count: u8::try_from(stage.sampler_count)
                .map_err(|_| "churn-native-stage-metadata")?,
        })
    };
    let pipeline = TrianglePipeline {
        vs: TriangleVertexShader {
            meta: TriangleVertexShaderMetadata {
                kernel: kernel(vertex, 0)?,
                max_threads: vertex.max_threads,
                urb_entry_output_length: u8::try_from(vertex.urb_entry_output_length)
                    .map_err(|_| "churn-native-stage-metadata")?,
            },
            code: vs_code,
        },
        ps: TrianglePixelShader {
            meta: TrianglePixelShaderMetadata {
                kernel: kernel(fragment, ps_offset)?,
                num_varying_inputs: u8::try_from(fragment.num_varying_inputs)
                    .map_err(|_| "churn-native-stage-metadata")?,
                uses_vmask: fragment.uses_vmask,
                computed_stencil: fragment.computed_stencil,
                persample_dispatch: fragment.persample_dispatch,
                computed_depth_mode: fragment.computed_depth_mode,
                flat_inputs: fragment.flat_inputs,
            },
            code: ps_code,
        },
    };
    *cached = Some(pipeline);
    Ok(pipeline)
}

fn build_churn_forward_geometry(
    meshes: &[trueos_helio_runtime::churn::MeshDescriptor; CHURN_FORWARD_MESH_COUNT],
) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
    let mut vertices = Vec::with_capacity(
        CHURN_FORWARD_MESH_COUNT
            * CHURN_FORWARD_VERTICES_PER_MESH
            * trueos_helio_artifact::churn_forward::VERTEX_STRIDE as usize,
    );
    let mut indices = Vec::with_capacity(
        CHURN_FORWARD_MESH_COUNT * CHURN_FORWARD_INDICES_PER_MESH * core::mem::size_of::<u32>(),
    );
    const FACE_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        if mesh.mesh_id != mesh_index as u32
            || mesh.first_vertex != (mesh_index * CHURN_FORWARD_VERTICES_PER_MESH) as u32
            || mesh.vertex_count != CHURN_FORWARD_VERTICES_PER_MESH as u32
            || mesh.first_index != (mesh_index * CHURN_FORWARD_INDICES_PER_MESH) as u32
            || mesh.index_count != CHURN_FORWARD_INDICES_PER_MESH as u32
            || mesh.base_vertex != (mesh_index * CHURN_FORWARD_VERTICES_PER_MESH) as i32
            || mesh
                .half_extents
                .iter()
                .any(|extent| !extent.is_finite() || *extent <= 0.0)
        {
            return Err("churn-native-mesh-contract");
        }
        let [x, y, z] = mesh.half_extents;
        let faces = [
            ([0.0, 0.0, 1.0], [[-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]]),
            ([0.0, 0.0, -1.0], [[x, -y, -z], [-x, -y, -z], [-x, y, -z], [x, y, -z]]),
            ([1.0, 0.0, 0.0], [[x, -y, z], [x, -y, -z], [x, y, -z], [x, y, z]]),
            ([-1.0, 0.0, 0.0], [[-x, -y, -z], [-x, -y, z], [-x, y, z], [-x, y, -z]]),
            ([0.0, 1.0, 0.0], [[-x, y, z], [x, y, z], [x, y, -z], [-x, y, -z]]),
            ([0.0, -1.0, 0.0], [[-x, -y, -z], [x, -y, -z], [x, -y, z], [-x, -y, z]]),
        ];
        for (face_index, (normal, corners)) in faces.into_iter().enumerate() {
            for position in corners {
                for component in position.into_iter().chain(normal) {
                    vertices.extend_from_slice(&component.to_le_bytes());
                }
            }
            for index in FACE_INDICES {
                let index = (face_index as u32) * 4 + index;
                indices.extend_from_slice(&index.to_le_bytes());
            }
        }
    }
    Ok((vertices, indices))
}

/// Build one immutable indexed cube topology for every compacted output slot.
///
/// The transformer authors 24 unique Float32x3 positions per slot.  Repeating
/// the six-face index pattern lets its 20-byte output records remain exact
/// WGPU `DrawIndexedIndirectArgs`: a group selects a contiguous slot range via
/// `first_index = first_slot * 36` and `index_count = slot_count * 36`.
fn build_churn_expanded_indices(max_instances: usize) -> Result<Vec<u8>, &'static str> {
    const FACE_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
    let index_count = max_instances
        .checked_mul(CHURN_FORWARD_INDICES_PER_MESH)
        .ok_or("churn-expanded-index-capacity")?;
    let mut indices = Vec::with_capacity(
        index_count
            .checked_mul(core::mem::size_of::<u32>())
            .ok_or("churn-expanded-index-capacity")?,
    );
    for slot in 0..max_instances {
        let slot_vertex = slot
            .checked_mul(CHURN_FORWARD_VERTICES_PER_MESH)
            .ok_or("churn-expanded-index-capacity")?;
        let slot_vertex =
            u32::try_from(slot_vertex).map_err(|_| "churn-expanded-index-capacity")?;
        for face in 0..6u32 {
            let face_vertex = slot_vertex
                .checked_add(face * 4)
                .ok_or("churn-expanded-index-capacity")?;
            for index in FACE_INDICES {
                indices.extend_from_slice(
                    &face_vertex
                        .checked_add(index)
                        .ok_or("churn-expanded-index-capacity")?
                        .to_le_bytes(),
                );
            }
        }
    }
    Ok(indices)
}

fn churn_material_srgba8(material: trueos_helio_runtime::churn::GpuMaterial) -> Option<[u8; 4]> {
    let [r, g, b, alpha] = material.base_color;
    if [r, g, b, alpha]
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return None;
    }
    let encode = |linear: f32| {
        let srgb = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * libm::powf(linear, 1.0 / 2.4) - 0.055
        };
        ((srgb * alpha * 255.0) + 0.5).clamp(0.0, 255.0) as u8
    };
    Some([
        encode(r),
        encode(g),
        encode(b),
        ((alpha * 255.0) + 0.5).clamp(0.0, 255.0) as u8,
    ])
}

fn publish_churn_material_rgba(
    resident: &ResidentChurnForward,
    materials: &[trueos_helio_runtime::churn::GpuMaterial;
         trueos_helio_runtime::churn::MATERIAL_COUNT],
) -> Result<(), &'static str> {
    const FALLBACK_RGBA: [[u8; 4]; trueos_helio_runtime::churn::MATERIAL_COUNT] = [
        [64, 179, 255, 255],
        [255, 82, 158, 255],
        [82, 255, 148, 255],
        [255, 199, 64, 255],
    ];
    for (index, (target, material)) in resident.material_rgba.iter().zip(materials).enumerate() {
        // Some retained side demos intentionally carry no forward-material
        // payload because the old native shader supplied its own four-color
        // palette. Preserve that visible contract on the storage-free path.
        let rgba = if material.base_color == [0.0; 4] {
            FALLBACK_RGBA[index]
        } else {
            churn_material_srgba8(*material).ok_or("churn-expanded-material")?
        };
        target.store(u32::from_le_bytes(rgba), Ordering::Release);
    }
    Ok(())
}

const _: () = {
    assert!(
        core::mem::size_of::<trueos_helio_runtime::retained_transform::RetainedTransformNode>()
            == crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_NODE_BYTES
    );
    assert!(
        core::mem::size_of::<trueos_helio_runtime::retained_transform::Affine3x4>()
            == crate::intel::gpgpu::GPGPU_HELIO_AFFINE_BYTES
    );
};

fn encode_churn_hierarchy_nodes(
    nodes: &[trueos_helio_runtime::retained_transform::RetainedTransformNode],
) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(nodes.len() * crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_NODE_BYTES);
    for node in nodes {
        bytes.extend_from_slice(&node.to_le_bytes());
    }
    bytes
}

fn encode_churn_affines(
    affines: &[trueos_helio_runtime::retained_transform::Affine3x4],
) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(affines.len() * crate::intel::gpgpu::GPGPU_HELIO_AFFINE_BYTES);
    for affine in affines {
        bytes.extend_from_slice(&affine.to_le_bytes());
    }
    bytes
}

fn encode_churn_hierarchy_indices(indices: &[u32]) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(indices.len() * crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_INDEX_BYTES);
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

fn stage_resident_churn_hierarchy_inputs(
    resident: &ResidentChurnHierarchy,
    frame: &trueos_helio_runtime::retained_transform::TransformHierarchyFrame<'_>,
    row_count: u32,
) -> Result<crate::intel::gpgpu::GpgpuHelioRetainedHierarchyDispatch, &'static str> {
    use trueos_helio_runtime::retained_transform::{CONSTANT_NODE, NO_PARENT};

    let node_count = u32::try_from(frame.nodes.len()).map_err(|_| "churn-hierarchy-capacity")?;
    let dirty_local_count = u32::try_from(frame.dirty_local_nodes.len())
        .map_err(|_| "churn-hierarchy-dirty-capacity")?;
    let dirty_world_count = u32::try_from(frame.dirty_world_nodes.len())
        .map_err(|_| "churn-hierarchy-dirty-capacity")?;
    let dirty_row_count =
        u32::try_from(frame.dirty_rows.len()).map_err(|_| "churn-hierarchy-dirty-capacity")?;
    let max_depth = frame.report.max_depth;
    let level_node_count = frame
        .levels
        .iter()
        .try_fold(0u32, |total, level| total.checked_add(level.count));
    if node_count == 0
        || frame.nodes.len() > resident.node_capacity
        || frame.local_affines.len() != frame.nodes.len()
        || frame.dynamic_bindings.len() != frame.nodes.len()
        || frame.row_leaf_nodes.len() != row_count as usize
        || frame.level_indices.len() != frame.nodes.len()
        || level_node_count != Some(node_count)
        || frame.levels.len() != max_depth as usize
        || frame.report.runtime_nodes != node_count
        || frame.report.dirty_local != dirty_local_count
        || frame.report.dirty_world != dirty_world_count
        || max_depth == 0
        || max_depth > crate::intel::gpgpu::GPGPU_HELIO_MAX_HIERARCHY_DEPTH
        || dirty_row_count > row_count
        || frame.nodes.iter().any(|node| {
            (node.parent != NO_PARENT && node.parent >= node_count) || node.level >= max_depth
        })
        || frame
            .dynamic_bindings
            .iter()
            .any(|binding| *binding != CONSTANT_NODE && *binding >= row_count)
        || frame
            .level_indices
            .iter()
            .chain(frame.dirty_local_nodes)
            .chain(frame.dirty_world_nodes)
            .chain(frame.row_leaf_nodes)
            .any(|node| *node >= node_count)
        || frame.dirty_rows.iter().any(|row| *row >= row_count)
    {
        return Err("churn-hierarchy-frame-contract");
    }

    // These compact CPU-authored tables are exact frame inputs, not a cache
    // keyed only by node count. Uploading them each frame makes same-sized
    // reparenting, rebinding, and row remapping visible without a lossy hash.
    let node_bytes = encode_churn_hierarchy_nodes(frame.nodes);
    let binding_bytes = encode_churn_hierarchy_indices(frame.dynamic_bindings);
    let row_leaf_bytes = encode_churn_hierarchy_indices(frame.row_leaf_nodes);
    if !resident.nodes.write_and_flush(0, &node_bytes) {
        return Err("churn-hierarchy-node-upload");
    }
    if !resident.dynamic_bindings.write_and_flush(0, &binding_bytes) {
        return Err("churn-hierarchy-binding-upload");
    }
    if !resident.row_leaf_nodes.write_and_flush(0, &row_leaf_bytes) {
        return Err("churn-hierarchy-row-leaf-upload");
    }

    // Never overwrite a clean GPU-authored dynamic local with its CPU identity
    // placeholder. Folded constants are the only authoritative CPU locals, so
    // upload their contiguous runs exactly (normally one 48-byte root row).
    let mut first = 0usize;
    while first < frame.dynamic_bindings.len() {
        if frame.dynamic_bindings[first] != CONSTANT_NODE {
            first += 1;
            continue;
        }
        let mut end = first + 1;
        while end < frame.dynamic_bindings.len() && frame.dynamic_bindings[end] == CONSTANT_NODE {
            end += 1;
        }
        let local_bytes = encode_churn_affines(&frame.local_affines[first..end]);
        let local_offset = first
            .checked_mul(crate::intel::gpgpu::GPGPU_HELIO_AFFINE_BYTES)
            .ok_or("churn-hierarchy-local-upload")?;
        if !resident
            .local_affines
            .write_and_flush(local_offset, &local_bytes)
        {
            return Err("churn-hierarchy-local-upload");
        }
        first = end;
    }

    for (buffer, indices, error) in [
        (
            &resident.dirty_local_nodes,
            frame.dirty_local_nodes,
            "churn-hierarchy-dirty-local-upload",
        ),
        (
            &resident.dirty_world_nodes,
            frame.dirty_world_nodes,
            "churn-hierarchy-dirty-world-upload",
        ),
        (&resident.dirty_rows, frame.dirty_rows, "churn-hierarchy-dirty-row-upload"),
    ] {
        if !indices.is_empty() {
            let bytes = encode_churn_hierarchy_indices(indices);
            if !buffer.write_and_flush(0, &bytes) {
                return Err(error);
            }
        }
    }

    Ok(crate::intel::gpgpu::GpgpuHelioRetainedHierarchyDispatch {
        nodes: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.nodes.gpu_base(),
            resident.nodes.storage_bytes(),
        ),
        dynamic_bindings: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.dynamic_bindings.gpu_base(),
            resident.dynamic_bindings.storage_bytes(),
        ),
        local_affines: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.local_affines.gpu_base(),
            resident.local_affines.storage_bytes(),
        ),
        world_affines: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.world_affines.gpu_base(),
            resident.world_affines.storage_bytes(),
        ),
        dirty_local_nodes: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.dirty_local_nodes.gpu_base(),
            resident.dirty_local_nodes.storage_bytes(),
        ),
        dirty_world_nodes: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.dirty_world_nodes.gpu_base(),
            resident.dirty_world_nodes.storage_bytes(),
        ),
        dirty_rows: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.dirty_rows.gpu_base(),
            resident.dirty_rows.storage_bytes(),
        ),
        row_leaf_nodes: crate::intel::gpgpu::GpgpuHelioBufferSlice::new(
            resident.row_leaf_nodes.gpu_base(),
            resident.row_leaf_nodes.storage_bytes(),
        ),
        node_count,
        dirty_local_count,
        dirty_world_count,
        dirty_row_count,
        max_depth,
    })
}

fn publish_resident_churn_hierarchy_counts(
    resident: &ResidentChurnHierarchy,
    dispatch: crate::intel::gpgpu::GpgpuHelioRetainedHierarchyDispatch,
) {
    resident
        .dirty_local_count
        .store(dispatch.dirty_local_count, Ordering::Release);
    resident
        .dirty_world_count
        .store(dispatch.dirty_world_count, Ordering::Release);
    resident
        .dirty_row_count
        .store(dispatch.dirty_row_count, Ordering::Release);
    resident
        .max_depth
        .store(dispatch.max_depth, Ordering::Release);
    // Node count is the graph-present flag read by the Render encoder. Publish
    // it last so every preceding input write and metadata count is visible.
    resident
        .node_count
        .store(dispatch.node_count, Ordering::Release);
}

fn churn_hierarchy_node_capacity(max_instances: usize) -> Option<usize> {
    let max_rows = crate::intel::gpgpu::GPGPU_HELIO_MAX_ROWS as usize;
    let max_nodes = crate::intel::gpgpu::GPGPU_HELIO_MAX_HIERARCHY_NODES as usize;
    if max_instances == 0 || max_instances > max_rows {
        return None;
    }
    max_instances
        .checked_add(1)
        .filter(|node_capacity| *node_capacity <= max_nodes)
}

fn allocate_resident_churn_hierarchy(
    max_instances: usize,
) -> Result<ResidentChurnHierarchy, &'static str> {
    let node_capacity =
        churn_hierarchy_node_capacity(max_instances).ok_or("churn-hierarchy-node-capacity")?;
    let node_bytes = node_capacity
        .checked_mul(crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_NODE_BYTES)
        .ok_or("churn-hierarchy-buffer-capacity")?;
    let binding_bytes = node_capacity
        .checked_mul(crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_DYNAMIC_BINDING_BYTES)
        .ok_or("churn-hierarchy-buffer-capacity")?;
    let affine_bytes = node_capacity
        .checked_mul(crate::intel::gpgpu::GPGPU_HELIO_AFFINE_BYTES)
        .ok_or("churn-hierarchy-buffer-capacity")?;
    let dirty_node_bytes = node_capacity
        .checked_mul(crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_INDEX_BYTES)
        .ok_or("churn-hierarchy-buffer-capacity")?;
    let row_index_bytes = max_instances
        .checked_mul(crate::intel::gpgpu::GPGPU_HELIO_HIERARCHY_INDEX_BYTES)
        .ok_or("churn-hierarchy-buffer-capacity")?;
    let layouts = [
        (node_bytes, "churn-hierarchy-node-allocation"),
        (binding_bytes, "churn-hierarchy-binding-allocation"),
        (affine_bytes, "churn-hierarchy-local-allocation"),
        (affine_bytes, "churn-hierarchy-world-allocation"),
        (dirty_node_bytes, "churn-hierarchy-dirty-local-allocation"),
        (dirty_node_bytes, "churn-hierarchy-dirty-world-allocation"),
        (row_index_bytes, "churn-hierarchy-dirty-row-allocation"),
        (row_index_bytes, "churn-hierarchy-row-leaf-allocation"),
    ];
    let mut allocated = Vec::with_capacity(layouts.len());
    for (bytes, error) in layouts {
        match allocate_resident_render_buffer(bytes) {
            Ok(buffer) => allocated.push(buffer),
            Err(_) => {
                for buffer in allocated.iter().rev() {
                    let _ = release_resident_render_buffer(buffer);
                }
                return Err(error);
            }
        }
    }
    let buffers: [ResidentRenderBuffer; 8] = match allocated.try_into() {
        Ok(buffers) => buffers,
        Err(buffers) => {
            for buffer in buffers.iter().rev() {
                let _ = release_resident_render_buffer(buffer);
            }
            return Err("churn-hierarchy-allocation-count");
        }
    };
    let [
        nodes,
        dynamic_bindings,
        local_affines,
        world_affines,
        dirty_local_nodes,
        dirty_world_nodes,
        dirty_rows,
        row_leaf_nodes,
    ] = buffers;
    Ok(ResidentChurnHierarchy {
        nodes,
        dynamic_bindings,
        local_affines,
        world_affines,
        dirty_local_nodes,
        dirty_world_nodes,
        dirty_rows,
        row_leaf_nodes,
        node_capacity,
        node_count: AtomicU32::new(0),
        dirty_local_count: AtomicU32::new(0),
        dirty_world_count: AtomicU32::new(0),
        dirty_row_count: AtomicU32::new(0),
        max_depth: AtomicU32::new(0),
    })
}

fn create_resident_churn_transform(
    max_instances: usize,
    hardware_admission: ChurnHardwareAdmission,
) -> Result<ResidentChurnTransform, &'static str> {
    if max_instances == 0 || max_instances > crate::intel::gpgpu::GPGPU_HELIO_MAX_ROWS as usize {
        return Err("churn-transform-row-capacity");
    }
    let dev = crate::intel::claimed_device().ok_or("no-device")?;
    if !device_admits_churn_retained_transform(hardware_admission, dev.device_id, dev.revision_id) {
        return Err("churn-transform-hardware-unvalidated");
    }
    let artifact = resident_helio_transform_artifact()?;
    let seed_bytes = max_instances
        .checked_mul(trueos_helio_runtime::churn::GpuRetainedTransformSeed::BYTE_LEN)
        .ok_or("churn-transform-buffer-capacity")?;
    let draw_template_bytes = CHURN_FORWARD_DRAW_COUNT
        .checked_mul(trueos_helio_runtime::churn::GpuRetainedDrawTemplate::BYTE_LEN)
        .ok_or("churn-transform-buffer-capacity")?;
    let seeds = allocate_resident_render_buffer(seed_bytes)
        .map_err(|_| "churn-transform-seed-allocation")?;
    let draw_templates = match allocate_resident_render_buffer(draw_template_bytes) {
        Ok(buffer) => buffer,
        Err(_) => {
            let _ = release_resident_render_buffer(&seeds);
            return Err("churn-transform-template-allocation");
        }
    };
    let hierarchy = match allocate_resident_churn_hierarchy(max_instances) {
        Ok(hierarchy) => hierarchy,
        Err(error) => {
            let _ = release_resident_render_buffer(&draw_templates);
            let _ = release_resident_render_buffer(&seeds);
            return Err(error);
        }
    };
    Ok(ResidentChurnTransform {
        seeds,
        draw_templates,
        hierarchy,
        artifact,
        row_count: AtomicU32::new(0),
    })
}

/// Cold-path activation of the artifact-authenticated Helio Churn pipeline.
pub(crate) fn create_resident_churn_forward(
    artifact_bytes: &'static [u8],
    max_instances: usize,
    meshes: &[trueos_helio_runtime::churn::MeshDescriptor; CHURN_FORWARD_MESH_COUNT],
) -> Result<ResidentChurnForward, &'static str> {
    create_resident_churn_forward_with_admission(
        artifact_bytes,
        max_instances,
        meshes,
        ChurnHardwareAdmission::ValidatedProduction,
    )
}

pub(crate) fn create_resident_churn_forward_adls_retained_probe(
    artifact_bytes: &'static [u8],
    max_instances: usize,
    meshes: &[trueos_helio_runtime::churn::MeshDescriptor; CHURN_FORWARD_MESH_COUNT],
) -> Result<ResidentChurnForward, &'static str> {
    create_resident_churn_forward_with_admission(
        artifact_bytes,
        max_instances,
        meshes,
        ChurnHardwareAdmission::Adls4680Rev0cPhysicalProbe,
    )
}

fn create_resident_churn_forward_with_admission(
    artifact_bytes: &'static [u8],
    max_instances: usize,
    meshes: &[trueos_helio_runtime::churn::MeshDescriptor; CHURN_FORWARD_MESH_COUNT],
    hardware_admission: ChurnHardwareAdmission,
) -> Result<ResidentChurnForward, &'static str> {
    if max_instances == 0 {
        return Err("churn-native-instance-capacity");
    }
    let artifact = trueos_helio_artifact::Artifact::parse(artifact_bytes)
        .map_err(|_| "churn-native-artifact")?;
    let program = artifact
        .churn_forward_program()
        .map_err(|_| "churn-native-program")?;
    let fetch = program.vertex_fetch();
    let fixed = program.fixed_function();
    let sgvs = program.sgvs();
    let synthetic = program.synthetic_instance_id_element();
    let instancing = program.vf_instancing();
    if fetch.stride != trueos_helio_artifact::churn_forward::VERTEX_STRIDE
        || fetch.vf_component_packing_dw0 != 0x0000_0A77
        || fetch.packed_vs_input_count != 8
        || fetch.urb_input_read_length != 1
        || fixed.sbe_read_offset != 1
        || fixed.sbe_read_length != 1
        || fixed.num_sf_attributes != 2
        || synthetic.element_index != 2
        || synthetic.vertex_buffer_index != 31
        || synthetic.surface_format != SURFACE_FORMAT_R32G32_UINT as u16
        || synthetic.component_controls != [VFCOMP_STORE_0 as u8; 4]
        || sgvs.vf_sgvs_dw1 != 0xE002_4002
        || sgvs.vf_sgvs_2_dw1 != 0xB002_0002
        || sgvs.vf_sgvs_2_dw2 != 3
        || instancing.iter().enumerate().any(|(index, state)| {
            state.element_index != index as u16 || state.enabled || state.step_rate != 0
        })
    {
        return Err("churn-native-fixed-function-contract");
    }
    let Some(dev) = crate::intel::claimed_device() else {
        return Err("no-device");
    };
    // Reject unvalidated devices before allocating or mutating any Render
    // context state. Capability admission must be side-effect free so a
    // compatibility fallback cannot disturb an unrelated live GuC client.
    if !device_admits_churn_forward_native(hardware_admission, dev.device_id, dev.revision_id) {
        return Err("churn-native-device-mismatch");
    }
    // Validate and cache the immutable native code before acquiring any
    // mapped DMA resources. The aligned code copy is a bounded singleton;
    // service retries cannot leak another VS/PS pair.
    let pipeline = churn_forward_pipeline(artifact, program)?;
    let (vertex_bytes, index_bytes) = build_churn_forward_geometry(meshes)?;
    let expanded_index_blob = build_churn_expanded_indices(max_instances)?;
    let index_offset =
        crate::intel::align_up(vertex_bytes.len(), 64).ok_or("churn-native-geometry-layout")?;
    let geometry_bytes = index_offset
        .checked_add(index_bytes.len())
        .ok_or("churn-native-geometry-layout")?;
    let instance_bytes = max_instances
        .checked_mul(trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN)
        .ok_or("churn-native-instance-capacity")?;
    let compacted_bytes = max_instances
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("churn-native-instance-capacity")?;
    let indirect_bytes = CHURN_FORWARD_DRAW_COUNT
        .checked_mul(trueos_helio_runtime::DrawIndexedIndirectArgs::BYTE_LEN)
        .ok_or("churn-native-indirect-capacity")?;
    let expanded_vertex_count = max_instances
        .checked_mul(CHURN_FORWARD_VERTICES_PER_MESH)
        .ok_or("churn-expanded-vertex-capacity")?;
    let expanded_vertex_bytes = expanded_vertex_count
        .checked_mul(core::mem::size_of::<[f32; 3]>())
        .ok_or("churn-expanded-vertex-capacity")?;
    let expanded_index_count = max_instances
        .checked_mul(CHURN_FORWARD_INDICES_PER_MESH)
        .ok_or("churn-expanded-index-capacity")?;
    let vertex_byte_len =
        u32::try_from(vertex_bytes.len()).map_err(|_| "churn-native-geometry-layout")?;
    let index_byte_len =
        u32::try_from(index_bytes.len()).map_err(|_| "churn-native-geometry-layout")?;
    let instance_byte_len =
        u32::try_from(instance_bytes).map_err(|_| "churn-native-instance-capacity")?;
    let compacted_byte_len =
        u32::try_from(compacted_bytes).map_err(|_| "churn-native-instance-capacity")?;
    let expanded_vertex_count_u32 =
        u32::try_from(expanded_vertex_count).map_err(|_| "churn-expanded-vertex-capacity")?;
    let expanded_vertex_byte_len =
        u32::try_from(expanded_vertex_bytes).map_err(|_| "churn-expanded-vertex-capacity")?;
    let expanded_index_count_u32 =
        u32::try_from(expanded_index_count).map_err(|_| "churn-expanded-index-capacity")?;
    let expanded_index_byte_len =
        u32::try_from(expanded_index_blob.len()).map_err(|_| "churn-expanded-index-capacity")?;

    let geometry = allocate_resident_render_buffer(geometry_bytes)?;
    let expanded_positions = match allocate_resident_render_buffer(expanded_vertex_bytes) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_render_buffer(&geometry);
            return Err(error);
        }
    };
    let expanded_indices = match allocate_resident_render_buffer(expanded_index_blob.len()) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_render_buffer(&expanded_positions);
            let _ = release_resident_render_buffer(&geometry);
            return Err(error);
        }
    };
    let camera = match allocate_resident_render_buffer(
        trueos_helio_runtime::churn::GpuCameraUniforms::BYTE_LEN,
    ) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_render_buffer(&expanded_indices);
            let _ = release_resident_render_buffer(&expanded_positions);
            let _ = release_resident_render_buffer(&geometry);
            return Err(error);
        }
    };
    let instances = match allocate_resident_render_buffer(instance_bytes) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_render_buffer(&camera);
            let _ = release_resident_render_buffer(&expanded_indices);
            let _ = release_resident_render_buffer(&expanded_positions);
            let _ = release_resident_render_buffer(&geometry);
            return Err(error);
        }
    };
    let compacted_indices = match allocate_resident_render_buffer(compacted_bytes) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_render_buffer(&instances);
            let _ = release_resident_render_buffer(&camera);
            let _ = release_resident_render_buffer(&expanded_indices);
            let _ = release_resident_render_buffer(&expanded_positions);
            let _ = release_resident_render_buffer(&geometry);
            return Err(error);
        }
    };
    let indirect_args = match allocate_resident_render_buffer(indirect_bytes) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_render_buffer(&compacted_indices);
            let _ = release_resident_render_buffer(&instances);
            let _ = release_resident_render_buffer(&camera);
            let _ = release_resident_render_buffer(&expanded_indices);
            let _ = release_resident_render_buffer(&expanded_positions);
            let _ = release_resident_render_buffer(&geometry);
            return Err(error);
        }
    };
    if !geometry.write(0, &vertex_bytes)
        || !geometry.write(index_offset, &index_bytes)
        || !expanded_indices.write(0, &expanded_index_blob)
    {
        let _ = release_resident_render_buffer(&indirect_args);
        let _ = release_resident_render_buffer(&compacted_indices);
        let _ = release_resident_render_buffer(&instances);
        let _ = release_resident_render_buffer(&camera);
        let _ = release_resident_render_buffer(&expanded_indices);
        let _ = release_resident_render_buffer(&expanded_positions);
        let _ = release_resident_render_buffer(&geometry);
        return Err("churn-expanded-geometry-upload");
    }
    geometry.flush();
    expanded_indices.flush();
    let native_vf = TriangleNativeDrawContract {
        hardware_admission,
        vs_storage_bindings: [
            TriangleStorageBufferBinding {
                gpu_addr: camera.gpu_base(),
                byte_len: trueos_helio_runtime::churn::GpuCameraUniforms::BYTE_LEN as u32,
            },
            TriangleStorageBufferBinding {
                gpu_addr: instances.gpu_base(),
                byte_len: instance_byte_len,
            },
            TriangleStorageBufferBinding {
                gpu_addr: compacted_indices.gpu_base(),
                byte_len: compacted_byte_len,
            },
        ],
        vf_sgvs_dw1: sgvs.vf_sgvs_dw1,
        vf_sgvs_2_dw1: sgvs.vf_sgvs_2_dw1,
        vf_sgvs_2_dw2: sgvs.vf_sgvs_2_dw2,
        vf_component_packing: [fetch.vf_component_packing_dw0, 0, 0, 0],
        vf_instancing: core::array::from_fn(|index| TriangleVfInstancingState {
            element_index: instancing[index].element_index as u8,
            enabled: instancing[index].enabled,
            step_rate: instancing[index].step_rate,
        }),
    };
    let (transform, transform_unavailable_reason) = match create_resident_churn_transform(
        max_instances,
        hardware_admission,
    ) {
        Ok(transform) => (Some(transform), None),
        Err(reason) => {
            crate::log_warn!(
                target: "render";
                "helio-transform: optional acceleration unavailable reason={} action=native-cpu-expanded-fallback native_vs_ps=preserved window_admission=preserved\n",
                reason,
            );
            (None, Some(reason))
        }
    };
    Ok(ResidentChurnForward {
        vertex_gpu_addr: geometry.gpu_base(),
        vertex_count: (CHURN_FORWARD_MESH_COUNT * CHURN_FORWARD_VERTICES_PER_MESH) as u32,
        vertex_bytes: vertex_byte_len,
        index_gpu_addr: geometry.gpu_base() + index_offset as u64,
        index_count: (CHURN_FORWARD_MESH_COUNT * CHURN_FORWARD_INDICES_PER_MESH) as u32,
        index_bytes: index_byte_len,
        expanded_vertex_count: expanded_vertex_count_u32,
        expanded_vertex_bytes: expanded_vertex_byte_len,
        expanded_index_count: expanded_index_count_u32,
        expanded_index_bytes: expanded_index_byte_len,
        material_rgba: core::array::from_fn(|_| {
            AtomicU32::new(u32::from_le_bytes([255, 255, 255, 255]))
        }),
        pipeline,
        native_vf,
        front_end_contract: TriangleFrontEndContract {
            label: "helio-churn-forward-v1",
            vs_urb_output_length_override: Some(1),
            sbe_read_offset: fixed.sbe_read_offset,
            sbe_read_length: fixed.sbe_read_length,
            force_sbe_read_offset: true,
            force_sbe_read_length: true,
            force_vs_with_vf_synthesized_vue: false,
        },
        geometry,
        expanded_positions,
        expanded_indices,
        camera,
        instances,
        compacted_indices,
        indirect_args,
        transform,
        transform_unavailable_reason,
        max_instances,
    })
}

pub(crate) fn update_resident_churn_forward_frame(
    resident: &ResidentChurnForward,
    frame: &trueos_helio_runtime::churn::InstanceFrame<'_>,
) -> Result<(), &'static str> {
    if frame.instances.len() > resident.max_instances
        || frame.compacted_indices.len() > resident.max_instances
        || frame.instances.len() != frame.compacted_indices.len()
        || frame.draws.len() != CHURN_FORWARD_DRAW_COUNT
    {
        return Err("churn-native-frame-capacity");
    }
    publish_churn_material_rgba(resident, frame.materials)?;
    let instance_first = frame.instance_dirty.first as usize;
    let instance_count = frame.instance_dirty.count as usize;
    let instance_end = instance_first
        .checked_add(instance_count)
        .ok_or("churn-native-dirty-range")?;
    let compacted_first = frame.compacted_indices_dirty.first as usize;
    let compacted_count = frame.compacted_indices_dirty.count as usize;
    let compacted_end = compacted_first
        .checked_add(compacted_count)
        .ok_or("churn-native-dirty-range")?;
    if instance_end > frame.instances.len() || compacted_end > frame.compacted_indices.len() {
        return Err("churn-native-dirty-range");
    }
    if !resident
        .camera
        .write_and_flush(0, &frame.camera.to_le_bytes())
    {
        return Err("churn-native-camera-upload");
    }
    let mut instance_bytes =
        Vec::with_capacity(instance_count * trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN);
    for instance in &frame.instances[instance_first..instance_end] {
        instance_bytes.extend_from_slice(&instance.to_le_bytes());
    }
    let instance_offset = instance_first
        .checked_mul(trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN)
        .ok_or("churn-native-dirty-range")?;
    if !resident
        .instances
        .write_and_flush(instance_offset, &instance_bytes)
    {
        return Err("churn-native-instance-upload");
    }
    let mut compacted_bytes = Vec::with_capacity(compacted_count * 4);
    for index in &frame.compacted_indices[compacted_first..compacted_end] {
        compacted_bytes.extend_from_slice(&index.to_le_bytes());
    }
    let compacted_offset = compacted_first
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("churn-native-dirty-range")?;
    if !resident
        .compacted_indices
        .write_and_flush(compacted_offset, &compacted_bytes)
    {
        return Err("churn-native-compacted-upload");
    }
    let mut indirect_bytes = Vec::with_capacity(
        CHURN_FORWARD_DRAW_COUNT * trueos_helio_runtime::DrawIndexedIndirectArgs::BYTE_LEN,
    );
    for (group, draw) in frame.draws.iter().enumerate() {
        let mesh = frame.meshes[group / trueos_helio_runtime::churn::MATERIAL_COUNT];
        if draw.index_count != mesh.index_count
            || draw.first_index != mesh.first_index
            || draw.base_vertex != mesh.base_vertex
            || draw.first_instance as usize > frame.instances.len()
            || draw
                .first_instance
                .checked_add(draw.instance_count)
                .map_or(true, |end| end as usize > frame.instances.len())
        {
            return Err("churn-native-indirect-contract");
        }
        indirect_bytes.extend_from_slice(&draw.to_le_bytes());
    }
    if !resident.indirect_args.write_and_flush(0, &indirect_bytes) {
        return Err("churn-native-indirect-upload");
    }
    // Preserve the CPU-expanded ABI as an explicit fallback. A successful
    // legacy update disables the compute secondary for the following frame.
    resident.disable_retained_transform_dispatch();
    Ok(())
}

/// Publish one compact retained-transform frame. Instances, compacted indices,
/// and WGPU indexed-indirect records remain GPU-owned outputs; the following
/// Render submission consumes them directly through the native artifact VS or
/// selects the expanded-position compatibility handoff explicitly.
pub(crate) fn update_resident_churn_forward_transform_frame(
    resident: &ResidentChurnForward,
    frame: &trueos_helio_runtime::churn::TransformFrame<'_>,
) -> Result<(), &'static str> {
    let transform = resident.transform.as_ref().ok_or(
        resident
            .retained_transform_unavailable_reason()
            .unwrap_or("churn-transform-unavailable"),
    )?;
    let row_count = u32::try_from(frame.seeds.len()).map_err(|_| "churn-transform-capacity")?;
    if row_count == 0
        || frame.seeds.len() > resident.max_instances
        || frame.draw_templates.len() != CHURN_FORWARD_DRAW_COUNT
    {
        return Err("churn-transform-capacity");
    }
    publish_churn_material_rgba(resident, frame.materials)?;

    let seed_first = frame.seed_dirty.first as usize;
    let seed_count = frame.seed_dirty.count as usize;
    let seed_end = seed_first
        .checked_add(seed_count)
        .ok_or("churn-transform-dirty-range")?;
    let template_first = frame.draw_templates_dirty.first as usize;
    let template_count = frame.draw_templates_dirty.count as usize;
    let template_end = template_first
        .checked_add(template_count)
        .ok_or("churn-transform-dirty-range")?;
    if seed_end > frame.seeds.len() || template_end > frame.draw_templates.len() {
        return Err("churn-transform-dirty-range");
    }
    let previous_rows = transform.row_count.load(Ordering::Acquire);
    if (previous_rows == 0 || previous_rows != row_count)
        && (seed_first != 0 || seed_count != frame.seeds.len())
    {
        return Err("churn-transform-initial-seed-range");
    }
    if previous_rows == 0 && (template_first != 0 || template_count != CHURN_FORWARD_DRAW_COUNT) {
        return Err("churn-transform-initial-template-range");
    }

    let gpu_templates: [crate::intel::gpgpu::GpgpuHelioRetainedDrawTemplate;
        CHURN_FORWARD_DRAW_COUNT] = core::array::from_fn(|group| {
        let template = frame.draw_templates[group];
        crate::intel::gpgpu::GpgpuHelioRetainedDrawTemplate {
            index_count: template.index_count,
            first_index: template.first_index,
            base_vertex: template.base_vertex,
            first_instance: template.first_instance,
            capacity: template.capacity,
            packed_mesh_material: template.packed_mesh_material,
        }
    });
    for (group, template) in frame.draw_templates.iter().enumerate() {
        let mesh = frame.meshes[group / trueos_helio_runtime::churn::MATERIAL_COUNT];
        let draw_group = frame.groups[group];
        if template.index_count != mesh.index_count
            || template.first_index != mesh.first_index
            || template.base_vertex != mesh.base_vertex
            || template.mesh_id() != draw_group.mesh_id
            || template.material_id() != draw_group.material_id
        {
            return Err("churn-transform-template-contract");
        }
    }
    if frame.seeds.iter().any(|seed| {
        seed.draw_group
            != trueos_helio_runtime::churn::GpuRetainedTransformSeed::DISABLED_DRAW_GROUP
            && seed.draw_group as usize >= CHURN_FORWARD_DRAW_COUNT
    }) {
        return Err("churn-transform-seed-group");
    }

    let mut seed_bytes = Vec::with_capacity(
        seed_count * trueos_helio_runtime::churn::GpuRetainedTransformSeed::BYTE_LEN,
    );
    for seed in &frame.seeds[seed_first..seed_end] {
        seed_bytes.extend_from_slice(&seed.to_le_bytes());
    }
    let seed_offset = seed_first
        .checked_mul(trueos_helio_runtime::churn::GpuRetainedTransformSeed::BYTE_LEN)
        .ok_or("churn-transform-dirty-range")?;

    let mut template_bytes = Vec::with_capacity(
        template_count * trueos_helio_runtime::churn::GpuRetainedDrawTemplate::BYTE_LEN,
    );
    for template in &frame.draw_templates[template_first..template_end] {
        template_bytes.extend_from_slice(&template.to_le_bytes());
    }
    let template_offset = template_first
        .checked_mul(trueos_helio_runtime::churn::GpuRetainedDrawTemplate::BYTE_LEN)
        .ok_or("churn-transform-dirty-range")?;

    // Stop the Render encoder from acquiring an old row count while its
    // persistent hierarchy/worklists are being refreshed.  A failed upload
    // deliberately leaves the transformer disabled instead of exposing a
    // mixture of old and new graph inputs.
    transform.row_count.store(0, Ordering::Release);
    let hierarchy_dispatch =
        stage_resident_churn_hierarchy_inputs(&transform.hierarchy, &frame.hierarchy, row_count)?;
    let dispatch = resident
        .transform_dispatch_for_rows_with_hierarchy(row_count, Some(hierarchy_dispatch))
        .ok_or("churn-transform-unavailable")?;
    dispatch
        .validate_templates(&gpu_templates)
        .map_err(|_| "churn-transform-dispatch-contract")?;

    if !resident
        .camera
        .write_and_flush(0, &frame.camera.to_le_bytes())
    {
        return Err("churn-transform-camera-upload");
    }
    if !transform.seeds.write_and_flush(seed_offset, &seed_bytes) {
        return Err("churn-transform-seed-upload");
    }
    if !transform
        .draw_templates
        .write_and_flush(template_offset, &template_bytes)
    {
        return Err("churn-transform-template-upload");
    }

    // Publish graph metadata after every CPU-written range is cache-clean.
    // Node count is the hierarchy-present flag; row count is the final
    // transaction commit observed by the Render encoder.
    publish_resident_churn_hierarchy_counts(&transform.hierarchy, hierarchy_dispatch);
    transform.row_count.store(row_count, Ordering::Release);
    Ok(())
}

/// Native retained graphics handoff. The preceding GPU pass owns all mutable
/// inputs: 208-byte matrix rows, compacted row indices, and the exact indirect
/// record. Graphics only binds those resident ranges and the immutable mesh.
fn prepare_resident_churn_forward_draw(
    _warm: RenderWarmState,
    resident: &ResidentChurnForward,
    group: usize,
    dst_gpu_addr: u64,
    pitch: usize,
    width: usize,
    height: usize,
) -> Option<TriangleDrawPrep> {
    if group >= CHURN_FORWARD_DRAW_COUNT {
        return None;
    }
    Some(TriangleDrawPrep {
        vertex_count: resident.index_count,
        vertex_stride: trueos_helio_artifact::churn_forward::VERTEX_STRIDE,
        vertex_buffer_bytes: resident.vertex_bytes,
        vertex_format: TriangleVertexFormat::PosNormal,
        vertex_gpu_addr: resident.vertex_gpu_addr,
        index_buffer: Some(TriangleIndexBufferPrep {
            index_count: resident.index_count,
            byte_len: resident.index_bytes,
            gpu_addr: resident.index_gpu_addr,
        }),
        indirect_args_gpu_addr: Some(
            resident.indirect_args.gpu_base()
                + (group * trueos_helio_runtime::DrawIndexedIndirectArgs::BYTE_LEN) as u64,
        ),
        native: Some(resident.native_vf),
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch: u32::try_from(pitch).ok()?,
        target_w: u32::try_from(width).ok()?,
        target_h: u32::try_from(height).ok()?,
    })
}

/// Storage-free compatibility graphics half of the retained transformer path.
///
/// Positions and indexed-indirect records are authored by the preceding GPU
/// secondary.  The VS sees only one ordinary Float32x3 VF element; there are
/// deliberately no camera/instance/compaction storage surface bindings and no
/// synthetic instance-ID element on this path.
fn prepare_resident_churn_expanded_draw(
    _warm: RenderWarmState,
    resident: &ResidentChurnForward,
    group: usize,
    dst_gpu_addr: u64,
    pitch: usize,
    width: usize,
    height: usize,
) -> Option<TriangleDrawPrep> {
    if group >= CHURN_FORWARD_DRAW_COUNT
        || resident.expanded_vertex_count == 0
        || resident.expanded_index_count == 0
    {
        return None;
    }
    Some(TriangleDrawPrep {
        vertex_count: resident.expanded_index_count,
        vertex_stride: core::mem::size_of::<[f32; 3]>() as u32,
        vertex_buffer_bytes: resident.expanded_vertex_bytes,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: resident.expanded_positions.gpu_base(),
        index_buffer: Some(TriangleIndexBufferPrep {
            index_count: resident.expanded_index_count,
            byte_len: resident.expanded_index_bytes,
            gpu_addr: resident.expanded_indices.gpu_base(),
        }),
        indirect_args_gpu_addr: Some(
            resident.indirect_args.gpu_base()
                + (group * trueos_helio_runtime::DrawIndexedIndirectArgs::BYTE_LEN) as u64,
        ),
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch: u32::try_from(pitch).ok()?,
        target_w: u32::try_from(width).ok()?,
        target_h: u32::try_from(height).ok()?,
    })
}

pub(crate) fn release_resident_churn_forward(resident: &ResidentChurnForward) -> bool {
    let mut released = true;
    released &= release_resident_render_buffer(&resident.indirect_args);
    released &= release_resident_render_buffer(&resident.compacted_indices);
    released &= release_resident_render_buffer(&resident.instances);
    if let Some(transform) = resident.transform.as_ref() {
        released &= release_resident_render_buffer(&transform.hierarchy.row_leaf_nodes);
        released &= release_resident_render_buffer(&transform.hierarchy.dirty_rows);
        released &= release_resident_render_buffer(&transform.hierarchy.dirty_world_nodes);
        released &= release_resident_render_buffer(&transform.hierarchy.dirty_local_nodes);
        released &= release_resident_render_buffer(&transform.hierarchy.world_affines);
        released &= release_resident_render_buffer(&transform.hierarchy.local_affines);
        released &= release_resident_render_buffer(&transform.hierarchy.dynamic_bindings);
        released &= release_resident_render_buffer(&transform.hierarchy.nodes);
        released &= release_resident_render_buffer(&transform.draw_templates);
        released &= release_resident_render_buffer(&transform.seeds);
    }
    released &= release_resident_render_buffer(&resident.camera);
    released &= release_resident_render_buffer(&resident.expanded_indices);
    released &= release_resident_render_buffer(&resident.expanded_positions);
    released &= release_resident_render_buffer(&resident.geometry);
    // `transform_artifact` is a boot-lifetime shared Render-PPGTT mapping.
    // Releasing one pooled Helio instance must not unmap it for another.
    released
}

#[cfg(test)]
mod churn_forward_geometry_tests {
    use super::{
        CHURN_FORWARD_INDICES_PER_MESH, CHURN_FORWARD_MESH_COUNT, CHURN_FORWARD_VERTICES_PER_MESH,
        build_churn_expanded_indices, build_churn_forward_geometry, churn_hierarchy_node_capacity,
        encode_churn_affines, encode_churn_hierarchy_indices, encode_churn_hierarchy_nodes,
    };

    #[test]
    fn encodes_three_local_indexed_pos_normal_cubes() {
        let meshes = core::array::from_fn(|mesh| trueos_helio_runtime::churn::MeshDescriptor {
            mesh_id: mesh as u32,
            half_extents: [1.0 + mesh as f32, 2.0, 3.0],
            first_vertex: (mesh * CHURN_FORWARD_VERTICES_PER_MESH) as u32,
            vertex_count: CHURN_FORWARD_VERTICES_PER_MESH as u32,
            first_index: (mesh * CHURN_FORWARD_INDICES_PER_MESH) as u32,
            index_count: CHURN_FORWARD_INDICES_PER_MESH as u32,
            base_vertex: (mesh * CHURN_FORWARD_VERTICES_PER_MESH) as i32,
        });
        let (vertices, indices) = build_churn_forward_geometry(&meshes).unwrap();
        assert_eq!(
            vertices.len(),
            CHURN_FORWARD_MESH_COUNT
                * CHURN_FORWARD_VERTICES_PER_MESH
                * trueos_helio_artifact::churn_forward::VERTEX_STRIDE as usize
        );
        assert_eq!(
            indices.len(),
            CHURN_FORWARD_MESH_COUNT * CHURN_FORWARD_INDICES_PER_MESH * core::mem::size_of::<u32>()
        );
        for mesh in 0..CHURN_FORWARD_MESH_COUNT {
            let start = mesh * CHURN_FORWARD_INDICES_PER_MESH * 4;
            let words = indices[start..start + CHURN_FORWARD_INDICES_PER_MESH * 4]
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert!(
                words
                    .iter()
                    .all(|&index| index < CHURN_FORWARD_VERTICES_PER_MESH as u32)
            );
            assert_eq!(words.len(), CHURN_FORWARD_INDICES_PER_MESH);
            for face in 0..6 {
                let base = face as u32 * 4;
                assert_eq!(
                    &words[face * 6..face * 6 + 6],
                    &[base, base + 1, base + 2, base, base + 2, base + 3]
                );
            }
        }
    }

    #[test]
    fn repeats_expanded_indices_over_disjoint_float3_slots() {
        let bytes = build_churn_expanded_indices(2).unwrap();
        let words = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(words.len(), 2 * CHURN_FORWARD_INDICES_PER_MESH);
        assert_eq!(&words[..6], &[0, 1, 2, 0, 2, 3]);
        assert_eq!(
            &words[CHURN_FORWARD_INDICES_PER_MESH..CHURN_FORWARD_INDICES_PER_MESH + 6],
            &[24, 25, 26, 24, 26, 27]
        );
        assert!(
            words[..CHURN_FORWARD_INDICES_PER_MESH]
                .iter()
                .all(|index| *index < CHURN_FORWARD_VERTICES_PER_MESH as u32)
        );
        assert!(words[CHURN_FORWARD_INDICES_PER_MESH..].iter().all(|index| {
            *index >= CHURN_FORWARD_VERTICES_PER_MESH as u32
                && *index < (CHURN_FORWARD_VERTICES_PER_MESH * 2) as u32
        }));
    }

    #[test]
    fn churn_hierarchy_reserves_one_root_and_one_leaf_per_row() {
        assert_eq!(churn_hierarchy_node_capacity(0), None);
        assert_eq!(churn_hierarchy_node_capacity(1), Some(2));
        assert_eq!(
            churn_hierarchy_node_capacity(crate::intel::gpgpu::GPGPU_HELIO_MAX_ROWS as usize),
            Some(crate::intel::gpgpu::GPGPU_HELIO_MAX_ROWS as usize + 1)
        );
        assert_eq!(
            churn_hierarchy_node_capacity(crate::intel::gpgpu::GPGPU_HELIO_MAX_ROWS as usize + 1),
            None
        );
    }

    #[test]
    fn retained_hierarchy_uploads_have_exact_little_endian_gpu_strides() {
        let nodes = [
            trueos_helio_runtime::retained_transform::RetainedTransformNode {
                parent: u32::MAX,
                level: 0,
                local_generation: 1,
                world_generation: 2,
            },
            trueos_helio_runtime::retained_transform::RetainedTransformNode {
                parent: 0,
                level: 1,
                local_generation: 3,
                world_generation: 4,
            },
        ];
        let node_bytes = encode_churn_hierarchy_nodes(&nodes);
        assert_eq!(node_bytes.len(), 2 * 16);
        assert_eq!(&node_bytes[0..4], &u32::MAX.to_le_bytes());
        assert_eq!(&node_bytes[16..20], &0u32.to_le_bytes());
        assert_eq!(&node_bytes[28..32], &4u32.to_le_bytes());

        let affine = trueos_helio_runtime::retained_transform::Affine3x4 {
            rows: core::array::from_fn(|index| index as f32 + 0.25),
        };
        let affine_bytes = encode_churn_affines(&[affine]);
        assert_eq!(affine_bytes.len(), 48);
        assert_eq!(&affine_bytes[0..4], &0.25f32.to_le_bytes());
        assert_eq!(&affine_bytes[44..48], &11.25f32.to_le_bytes());

        let index_bytes = encode_churn_hierarchy_indices(&[0x1122_3344, 0xAABB_CCDD]);
        assert_eq!(index_bytes.len(), 8);
        assert_eq!(&index_bytes[0..4], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&index_bytes[4..8], &0xAABB_CCDDu32.to_le_bytes());
    }
}

/// Allocate zeroed, page-backed storage and map it once into the persistent
/// render PPGTT resource window. The caller owns the returned mapping until it
/// explicitly releases it; ordinary frame submission never remaps these VAs.
pub(crate) fn allocate_resident_render_buffer(
    bytes: usize,
) -> Result<ResidentRenderBuffer, &'static str> {
    if bytes == 0 {
        return Err("resident-resource-empty");
    }
    if crate::intel::claimed_device().is_none() {
        return Err("no-device");
    }
    let warm = warm_state().ok_or("render-boot-not-ready")?;
    if render_ppgtt_pml4_phys() == 0 || warm.vertex_len == 0 {
        return Err("render-ppgtt");
    }
    let storage_bytes = crate::intel::align_up(bytes, 4096).ok_or("resident-resource-align")?;
    let gpu_base = reserve_persistent_render_gpu_va(storage_bytes).ok_or("resident-resource-va")?;
    let Some((storage_phys, storage_virt)) = crate::dma::alloc(storage_bytes, 4096) else {
        recycle_persistent_render_gpu_va(gpu_base, storage_bytes);
        return Err("resident-resource-alloc");
    };
    unsafe {
        core::ptr::write_bytes(storage_virt, 0, storage_bytes);
    }
    crate::intel::dma_flush(storage_virt, storage_bytes);
    if !map_render_ppgtt_range(gpu_base, storage_phys, storage_bytes) {
        crate::dma::dealloc(storage_virt, storage_bytes);
        recycle_persistent_render_gpu_va(gpu_base, storage_bytes);
        return Err("resident-resource-map");
    }
    Ok(ResidentRenderBuffer {
        storage_phys,
        storage_virt,
        storage_bytes,
        gpu_base,
    })
}

/// Tear down a resident render resource. Spirit intentionally never calls
/// this after publishing its runtime catalog, but failed cold-path loads use
/// it to avoid leaving a partial allocation behind.
pub(crate) fn release_resident_render_buffer(buffer: &ResidentRenderBuffer) -> bool {
    if !unmap_render_ppgtt_range(buffer.gpu_base, buffer.storage_bytes) {
        return false;
    }
    crate::dma::dealloc(buffer.storage_virt, buffer.storage_bytes);
    recycle_persistent_render_gpu_va(buffer.gpu_base, buffer.storage_bytes);
    true
}

fn reserve_persistent_render_gpu_va(bytes: usize) -> Option<u64> {
    let bytes = crate::intel::align_up(bytes, 4096)? as u64;
    {
        let mut free = PERSISTENT_RESOURCE_GPU_VA_FREE.lock();
        if let Some(index) = free
            .iter()
            .position(|(start, end)| end.saturating_sub(*start) >= bytes)
        {
            let (start, end) = free[index];
            let next = start.checked_add(bytes)?;
            if next == end {
                free.swap_remove(index);
            } else {
                free[index].0 = next;
            }
            return Some(start);
        }
    }
    loop {
        let current = PERSISTENT_RESOURCE_GPU_VA_CURSOR.load(Ordering::Acquire);
        let aligned = (current.checked_add(4095)?) & !4095;
        let next = aligned.checked_add(bytes)?;
        if next > GPU_VA_PERSISTENT_RESOURCE_LIMIT {
            return None;
        }
        if PERSISTENT_RESOURCE_GPU_VA_CURSOR
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(aligned);
        }
    }
}

fn recycle_persistent_render_gpu_va(gpu_base: u64, bytes: usize) {
    let Some(bytes) = crate::intel::align_up(bytes, 4096).map(|value| value as u64) else {
        return;
    };
    let Some(end) = gpu_base.checked_add(bytes) else {
        return;
    };
    let mut free = PERSISTENT_RESOURCE_GPU_VA_FREE.lock();
    free.push((gpu_base, end));
    free.sort_unstable_by_key(|range| range.0);
    let mut write = 0usize;
    for read in 0..free.len() {
        let range = free[read];
        if write != 0 && range.0 <= free[write - 1].1 {
            free[write - 1].1 = free[write - 1].1.max(range.1);
        } else {
            free[write] = range;
            write += 1;
        }
    }
    free.truncate(write);
}

fn prepare_triangle_draw_resources_for_resident_font_mesh(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    mesh: &ResidentFontMesh,
) -> Option<TriangleDrawPrep> {
    prepare_triangle_draw_resources_for_resident_font_mesh_with_state_clear(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        mesh,
        true,
    )
}

fn prepare_triangle_draw_resources_for_scene_resident_mesh(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    mesh: &ResidentFontMesh,
) -> Option<TriangleDrawPrep> {
    prepare_triangle_draw_resources_for_resident_font_mesh_with_state_clear(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        mesh,
        false,
    )
    .map(|draw| draw.with_indirect_args(mesh.indirect_args_gpu_addr))
}

fn prepare_triangle_draw_resources_for_resident_font_mesh_with_state_clear(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    mesh: &ResidentFontMesh,
    clear_state: bool,
) -> Option<TriangleDrawPrep> {
    if mesh.vertex_count < 3
        || mesh.index_count < 3
        || !mesh.index_count.is_multiple_of(3)
        || mesh.vertex_bytes == 0
        || mesh.index_bytes == 0
    {
        return None;
    }
    if clear_state {
        unsafe {
            core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
        }
        crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);
    }
    Some(TriangleDrawPrep {
        vertex_count: mesh.index_count,
        vertex_stride: core::mem::size_of::<[f32; 3]>() as u32,
        vertex_buffer_bytes: mesh.vertex_bytes,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: mesh.vertex_gpu_addr,
        index_buffer: Some(TriangleIndexBufferPrep {
            index_count: mesh.index_count,
            byte_len: mesh.index_bytes,
            gpu_addr: mesh.index_gpu_addr,
        }),
        indirect_args_gpu_addr: None,
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch: u32::try_from(pitch).ok()?,
        target_w: u32::try_from(rect_w).ok()?,
        target_h: u32::try_from(rect_h).ok()?,
    })
}

fn prepare_triangle_draw_resources_for_vf_vue_vertex_slice(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    label: &'static str,
    vertices: &[[f32; 3]],
    experiment: StreamoutProofExperiment,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if vertices.is_empty() || vertices.len() % 3 != 0 {
        return None;
    }
    if warm.vertex_len < vertices.len().saturating_mul(experiment.vertex_bytes()) {
        return None;
    }
    if warm.draw_state_len == 0 {
        return None;
    }

    let vertex_proof = write_vf_vue_vertex_slice(warm, label, vertices, experiment)?;

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: vertex_proof.vertex_count,
        vertex_stride: vertex_proof.vertex_stride,
        vertex_buffer_bytes: u32::try_from(vertex_proof.byte_len).ok()?,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: vertex_proof.gpu_addr,
        index_buffer: None,
        indirect_args_gpu_addr: None,
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn write_canonical_triangle_vertices(warm: RenderWarmState) -> Option<TriangleVertexUploadProof> {
    write_triangle_vertices_for_geometry(warm, VfPrimitiveGeometry::Canonical)
}

fn write_triangle_vertices_for_geometry(
    warm: RenderWarmState,
    geometry: VfPrimitiveGeometry,
) -> Option<TriangleVertexUploadProof> {
    write_triangle_vertices(warm, geometry.label(), geometry.vertices())
}

fn write_triangle_vertices(
    warm: RenderWarmState,
    label: &'static str,
    triangle: [[f32; 3]; TRIANGLE_DRAW_VERTICES],
) -> Option<TriangleVertexUploadProof> {
    write_triangle_vertex_slice(warm, label, &triangle)
}

fn write_triangle_vertex_slice(
    warm: RenderWarmState,
    label: &'static str,
    triangle: &[[f32; 3]],
) -> Option<TriangleVertexUploadProof> {
    let byte_len = triangle.len().checked_mul(TRIANGLE_DRAW_VERTEX_STRIDE)?;
    if warm.vertex_len < byte_len || warm.vertex_virt.is_null() {
        return None;
    }

    // This is deliberately only a CPU-side upload proof.
    //
    // Facts proven here:
    //   1. the warm vertex allocation is large enough for three vertices,
    //   2. the CPU can write the canonical triangle bytes,
    //   3. the CPU can read back the exact bytes it wrote,
    //   4. the cache maintenance hook has been issued for that byte range.
    //
    // Facts not proven here:
    //   - the GGTT mapping points at this allocation,
    //   - the command streamer consumed 3DSTATE_VERTEX_BUFFERS,
    //   - vertex fetch read these bytes,
    //   - any shader or raster stage produced pixels.
    let vertices = unsafe {
        core::slice::from_raw_parts_mut(
            warm.vertex_virt as *mut f32,
            warm.vertex_len / core::mem::size_of::<f32>(),
        )
    };
    vertices.fill(0.0);

    for (dst, src) in vertices
        .chunks_exact_mut(TRIANGLE_DRAW_VERTEX_DWORDS)
        .take(triangle.len())
        .zip(triangle.iter())
    {
        dst.copy_from_slice(src);
    }

    let readback = unsafe {
        core::slice::from_raw_parts(
            warm.vertex_virt as *const u32,
            triangle.len() * TRIANGLE_DRAW_VERTEX_DWORDS,
        )
    };
    let mut cpu_readback_ok = true;
    for (actual, expected) in readback.iter().zip(triangle.iter().flatten()) {
        if *actual != expected.to_bits() {
            cpu_readback_ok = false;
            break;
        }
    }

    crate::intel::dma_flush(warm.vertex_virt, byte_len);

    let signed_area_2x = (triangle[1][0] - triangle[0][0]) * (triangle[2][1] - triangle[0][1])
        - (triangle[2][0] - triangle[0][0]) * (triangle[1][1] - triangle[0][1]);

    intel_render_focus_log!(
        "vertex-upload-proof accepted={} stage=cpu-write-readback geometry={} bytes={} stride={} count={} gpu=0x{:X} readback_ok={} flush=1 area2={:.3} winding={} v0=[{:.3},{:.3},{:.3}] v1=[{:.3},{:.3},{:.3}] v2=[{:.3},{:.3},{:.3}] does_not_prove=vf_fetch\n",
        cpu_readback_ok as u8,
        label,
        byte_len,
        TRIANGLE_DRAW_VERTEX_STRIDE,
        triangle.len(),
        GPU_VA_VERTEX_BASE,
        cpu_readback_ok as u8,
        signed_area_2x,
        if signed_area_2x >= 0.0 { "ccw" } else { "cw" },
        triangle[0][0],
        triangle[0][1],
        triangle[0][2],
        triangle[1][0],
        triangle[1][1],
        triangle[1][2],
        triangle[2][0],
        triangle[2][1],
        triangle[2][2],
    );

    Some(TriangleVertexUploadProof {
        vertex_count: u32::try_from(triangle.len()).ok()?,
        vertex_stride: TRIANGLE_DRAW_VERTEX_STRIDE as u32,
        byte_len,
        gpu_addr: GPU_VA_VERTEX_BASE,
        signed_area_2x,
        cpu_readback_ok,
    })
}

fn write_vf_vue_vertex_slice(
    warm: RenderWarmState,
    label: &'static str,
    triangle: &[[f32; 3]],
    experiment: StreamoutProofExperiment,
) -> Option<TriangleVertexUploadProof> {
    let vertex_stride = experiment.vertex_bytes();
    let byte_len = triangle.len().checked_mul(vertex_stride)?;
    if warm.vertex_len < byte_len || warm.vertex_virt.is_null() {
        return None;
    }

    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u32, warm.vertex_len / 4)
    };
    words.fill(0);

    for (idx, pos) in triangle.iter().enumerate() {
        match experiment {
            StreamoutProofExperiment::PositionSlot0 | StreamoutProofExperiment::PositionSlot1 => {
                let base = idx * 4;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                words[base + 3] = 1.0f32.to_bits();
            }
            StreamoutProofExperiment::PrmVueHeaderPositionSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0;
                words[base + 1] = 0;
                words[base + 2] = 0;
                words[base + 3] = 1.0f32.to_bits();
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = pos[2].to_bits();
                words[base + 7] = 1.0f32.to_bits();
                intel_render_focus_log!(
                    "vf-prm-vue-header-source v{} geometry={} header=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] position_xyzw=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    label,
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 5]),
                    f32::from_bits(words[base + 6]),
                    f32::from_bits(words[base + 7]),
                );
            }
            StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0;
                words[base + 1] = 0;
                words[base + 2] = 0;
                words[base + 3] = 1.0f32.to_bits();
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = 1.0f32.to_bits();
                words[base + 7] = pos[2].to_bits();
                intel_render_focus_log!(
                    "vf-prm-vue-header-source v{} geometry={} header=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] position_xywz=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    label,
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 5]),
                    f32::from_bits(words[base + 6]),
                    f32::from_bits(words[base + 7]),
                );
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0x5155_0000 | idx as u32;
                words[base + 1] = 0x5155_1000 | idx as u32;
                words[base + 2] = 0x5155_2000 | idx as u32;
                words[base + 3] = 0x5155_3000 | idx as u32;
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = pos[2].to_bits();
                words[base + 7] = 1.0f32.to_bits();
            }
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1 => {
                let base = idx * 8;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                words[base + 3] = 1.0f32.to_bits();
                words[base + 4] = 64.0f32.to_bits();
                words[base + 5] = 0.0f32.to_bits();
                words[base + 6] = 0.0f32.to_bits();
                words[base + 7] = 0.0f32.to_bits();
            }
        }
    }

    crate::intel::dma_flush(warm.vertex_virt, byte_len);

    let signed_area_2x = (triangle[1][0] - triangle[0][0]) * (triangle[2][1] - triangle[0][1])
        - (triangle[2][0] - triangle[0][0]) * (triangle[1][1] - triangle[0][1]);
    let readback =
        unsafe { core::slice::from_raw_parts(warm.vertex_virt as *const u32, byte_len / 4) };
    let cpu_readback_ok = readback.iter().take(byte_len / 4).any(|word| *word != 0);

    intel_render_focus_log!(
        "vf-vue-upload-proof accepted={} stage=cpu-write-readback geometry={} experiment={} bytes={} stride={} count={} gpu=0x{:X} readback_nonzero={} flush=1 area2={:.3} winding={} slot_contract={} does_not_prove=vf_fetch\n",
        cpu_readback_ok as u8,
        label,
        experiment.label(),
        byte_len,
        vertex_stride,
        triangle.len(),
        GPU_VA_VERTEX_BASE,
        cpu_readback_ok as u8,
        signed_area_2x,
        if signed_area_2x >= 0.0 { "ccw" } else { "cw" },
        experiment.vf_slot_contract(),
    );

    Some(TriangleVertexUploadProof {
        vertex_count: u32::try_from(triangle.len()).ok()?,
        vertex_stride: vertex_stride as u32,
        byte_len,
        gpu_addr: GPU_VA_VERTEX_BASE,
        signed_area_2x,
        cpu_readback_ok,
    })
}

fn prepare_vf_streamout_proof_resources(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
    geometry: VfPrimitiveGeometry,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    let vertex_stride = experiment.vertex_bytes();
    if warm.vertex_len < TRIANGLE_DRAW_VERTICES * vertex_stride {
        return None;
    }

    let tri = geometry.vertices();
    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u32, warm.vertex_len / 4)
    };
    words.fill(0);

    for (idx, pos) in tri.iter().enumerate() {
        match experiment {
            StreamoutProofExperiment::PositionSlot0 | StreamoutProofExperiment::PositionSlot1 => {
                let base = idx * 4;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                words[base + 3] = 1.0f32.to_bits();
                intel_render_verbose_log!(
                    "vf-streamout-source v{} experiment={} geometry={} raw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    geometry.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 0]),
                    f32::from_bits(words[base + 1]),
                    f32::from_bits(words[base + 2]),
                    f32::from_bits(words[base + 3]),
                );
            }
            StreamoutProofExperiment::PrmVueHeaderPositionSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0;
                words[base + 1] = 0;
                words[base + 2] = 0;
                words[base + 3] = 1.0f32.to_bits();
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = pos[2].to_bits();
                words[base + 7] = 1.0f32.to_bits();
                intel_render_focus_log!(
                    "vf-prm-vue-header-source v{} experiment={} geometry={} header=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] position_xyzw=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    geometry.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 5]),
                    f32::from_bits(words[base + 6]),
                    f32::from_bits(words[base + 7]),
                );
            }
            StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0;
                words[base + 1] = 0;
                words[base + 2] = 0;
                words[base + 3] = 1.0f32.to_bits();
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = 1.0f32.to_bits();
                words[base + 7] = pos[2].to_bits();
                intel_render_focus_log!(
                    "vf-prm-vue-header-source v{} experiment={} geometry={} header=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] position_xywz=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    geometry.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 5]),
                    f32::from_bits(words[base + 6]),
                    f32::from_bits(words[base + 7]),
                );
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                let base = idx * 8;
                words[base + 0] = 0x5155_0000 | idx as u32;
                words[base + 1] = 0x5155_1000 | idx as u32;
                words[base + 2] = 0x5155_2000 | idx as u32;
                words[base + 3] = 0x5155_3000 | idx as u32;
                words[base + 4] = pos[0].to_bits();
                words[base + 5] = pos[1].to_bits();
                words[base + 6] = pos[2].to_bits();
                words[base + 7] = 1.0f32.to_bits();
                intel_render_verbose_log!(
                    "vf-streamout-source v{} experiment={} geometry={} hdr=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos_f=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    geometry.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    words[base + 4],
                    words[base + 5],
                    words[base + 6],
                    words[base + 7],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 5]),
                    f32::from_bits(words[base + 6]),
                    f32::from_bits(words[base + 7]),
                );
            }
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1 => {
                let base = idx * 8;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                words[base + 3] = 1.0f32.to_bits();
                words[base + 4] = 64.0f32.to_bits();
                words[base + 5] = 0.0f32.to_bits();
                words[base + 6] = 0.0f32.to_bits();
                words[base + 7] = 0.0f32.to_bits();
                intel_render_verbose_log!(
                    "vf-streamout-source v{} experiment={} geometry={} point_size=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] point_size_f={:.3} pos_f=[{:.3},{:.3},{:.3},{:.3}]\n",
                    idx,
                    experiment.label(),
                    geometry.label(),
                    words[base + 4],
                    words[base + 5],
                    words[base + 6],
                    words[base + 7],
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    words[base + 3],
                    f32::from_bits(words[base + 4]),
                    f32::from_bits(words[base + 0]),
                    f32::from_bits(words[base + 1]),
                    f32::from_bits(words[base + 2]),
                    f32::from_bits(words[base + 3]),
                );
            }
        }
    }

    crate::intel::dma_flush(warm.vertex_virt, TRIANGLE_DRAW_VERTICES * vertex_stride);

    Some(TriangleDrawPrep {
        vertex_count: TRIANGLE_DRAW_VERTICES as u32,
        vertex_stride: vertex_stride as u32,
        vertex_buffer_bytes: u32::try_from(TRIANGLE_DRAW_VERTICES * vertex_stride).ok()?,
        vertex_format: TriangleVertexFormat::Float3,
        vertex_gpu_addr: GPU_VA_VERTEX_BASE,
        index_buffer: None,
        indirect_args_gpu_addr: None,
        native: None,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_surface_format: SURFACE_FORMAT_R8G8B8A8_UNORM,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn write_vf_streamout_probe_state(warm: RenderWarmState) -> Result<u32, &'static str> {
    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }

    if !device_is_gfx125(warm.device_id) {
        return Ok(0);
    }

    let slice_hash_table_offset = VF_STREAMOUT_SLICE_HASH_TABLE_OFFSET;
    let end_offset = slice_hash_table_offset
        .checked_add(GFX125_SLICE_HASH_TABLE_BYTES)
        .ok_or("vf-streamout-state-overflow")?;
    if end_offset > warm.draw_state_len {
        return Err("vf-streamout-state-exceeds-state-bo");
    }

    let dwords = unsafe {
        core::slice::from_raw_parts_mut(warm.draw_state_virt as *mut u32, warm.draw_state_len / 4)
    };
    let slice_hash = &mut dwords
        [slice_hash_table_offset / 4..slice_hash_table_offset / 4 + GFX125_SLICE_HASH_TABLE_DWORDS];
    let mut packed = [0u32; GFX125_SLICE_HASH_TABLE_DWORDS];
    gfx125_pack_slice_hash_tables(gfx125_slice_hash_config(warm), &mut packed);
    slice_hash.copy_from_slice(&packed);

    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(slice_hash_table_offset) },
        GFX125_SLICE_HASH_TABLE_BYTES,
    );

    Ok(slice_hash_table_offset as u32)
}

fn submit_triangle_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> bool {
    let Some(render_lease) = reserve_warm_render_storage("mi-triangle") else {
        return false;
    };
    unsafe {
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, core::mem::size_of::<u32>());

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_rgb_triangle_store_batch(
        batch,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DONE,
    ) else {
        crate::log!(
            "primary-triangle batch build failed size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    submit_warm_render_batch(
        &render_lease,
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-triangle",
    )
}
