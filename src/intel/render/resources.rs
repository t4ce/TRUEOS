fn upload_triangle_shader_pipeline(
    warm: RenderWarmState,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
) -> Result<TriangleShaderLayout, &'static str> {
    let vs = stage_range("vs", pipeline.vs.meta.kernel, pipeline.vs.code)?;
    let ps = stage_range("ps", pipeline.ps.meta.kernel, pipeline.ps.code)?;
    let host_simd16_pipeline = if pipeline.ps.code.as_ptr()
        == crate::intel::shader::triangle_pipeline().ps.code.as_ptr()
    {
        Some(crate::intel::shader::triangle_pipeline_simd16())
    } else if pipeline.ps.code.as_ptr()
        == crate::intel::shader::triangle_pipeline_push_color()
            .ps
            .code
            .as_ptr()
    {
        Some(crate::intel::shader::triangle_pipeline_push_color_simd16())
    } else {
        None
    };
    let host_simd16 = host_simd16_pipeline
        .map(|paired| stage_range("ps-simd16", paired.ps.meta.kernel, paired.ps.code))
        .transpose()?;

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
    if let (Some(host_simd16), Some(host_simd16_pipeline)) =
        (host_simd16, host_simd16_pipeline)
    {
        upload_stage_code(
            warm.draw_state_virt,
            host_simd16.code_offset_bytes,
            host_simd16_pipeline.ps.code,
        )?;
    }

    crate::intel::dma_flush(warm.draw_state_virt, used_end);

    let state_region_offset_bytes =
        crate::intel::align_up(used_end, crate::intel::WARM_ALIGN).ok_or("state-region-align")?;
    if state_region_offset_bytes > warm.draw_state_len {
        return Err("state-region-exceeds-state-bo");
    }

    let bo_gpu_base = GPU_VA_DRAW_STATE_BASE;
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
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    submit_name: &'static str,
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
    let ps_expected_match = pipeline.ps.code == uploaded_ps;
    let color_binding = if pipeline.ps.meta.kernel.push_constant_bytes > 0 {
        "static-ps-push-constant"
    } else {
        "baked-ps-constant"
    };
    let pipeline_note = if pipeline.ps.meta.kernel.push_constant_bytes > 0 {
        crate::intel::shader::triangle_pipeline_push_color_note()
    } else {
        crate::intel::shader::triangle_pipeline_note()
    };
    let vs_first = pipeline.vs.code.first().copied().unwrap_or(0);
    let vs_uploaded_first = uploaded_vs.first().copied().unwrap_or(0);
    let vs_last = pipeline.vs.code.last().copied().unwrap_or(0);
    let vs_uploaded_last = uploaded_vs.last().copied().unwrap_or(0);
    if submit_name == "vs-draw-frontier" {
        intel_render_focus_log!(
            "{} shader-upload-verify note={} vs_match={} vs_baked_sig=0x{:016X} vs_uploaded_sig=0x{:016X} vs_first=0x{:08X}/0x{:08X} vs_last=0x{:08X}/0x{:08X} ps_expected_match={} ps_baked_match={} ps_baked_sig=0x{:016X} ps_uploaded_sig=0x{:016X} color_binding={}\n",
            submit_name,
            pipeline_note,
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
        );
    } else {
        intel_render_verbose_log!(
            "{} shader-upload-verify note={} vs_match={} vs_baked_sig=0x{:016X} vs_uploaded_sig=0x{:016X} vs_first=0x{:08X}/0x{:08X} vs_last=0x{:08X}/0x{:08X} ps_expected_match={} ps_baked_match={} ps_baked_sig=0x{:016X} ps_uploaded_sig=0x{:016X} color_binding={}\n",
            submit_name,
            pipeline_note,
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
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
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
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
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
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
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
    let Some(dev) = crate::intel::claimed_device() else {
        return Err("no-device");
    };
    let warm = warm_once(dev);
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
    let Some(dev) = crate::intel::claimed_device() else {
        return Err("no-device");
    };
    let warm = warm_once(dev);
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
    let used_bytes = index_offset
        .checked_add(index_bytes)
        .ok_or("resident-triangle-bytes")?;
    let storage_bytes =
        crate::intel::align_up(used_bytes, 4096).ok_or("resident-triangle-align")?;
    let gpu_base = reserve_persistent_font_gpu_va(storage_bytes).ok_or("resident-triangle-va")?;
    let vertex_count = u32::try_from(draw_vertices.len()).map_err(|_| "resident-triangle-count")?;
    let vertex_bytes_u32 = u32::try_from(vertex_bytes).map_err(|_| "resident-triangle-count")?;
    let index_count = u32::try_from(draw_indices.len()).map_err(|_| "resident-triangle-count")?;
    let index_bytes_u32 = u32::try_from(index_bytes).map_err(|_| "resident-triangle-count")?;
    let index_gpu_addr = gpu_base
        .checked_add(index_offset as u64)
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
    };
    intel_render_focus_log!(
        "resident-triangle upload authority=gpu-resident phys=0x{:X} gpu=0x{:X} bytes=0x{:X} vertices={} indices={} cpu_uploads=1 retained=1\n",
        resident.storage_phys,
        resident.gpu_base,
        resident.storage_bytes,
        resident.vertex_count,
        resident.index_count,
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
    crate::intel::dma_flush(mesh.storage_virt, mesh.storage_bytes);
    Ok(())
}

pub(crate) fn release_resident_triangle_mesh(mesh: &ResidentTriangleMesh) -> bool {
    if !unmap_render_ppgtt_range(mesh.gpu_base, mesh.storage_bytes) {
        return false;
    }
    crate::dma::dealloc(mesh.storage_virt, mesh.storage_bytes);
    recycle_persistent_triangle_gpu_va(mesh.gpu_base, mesh.storage_bytes);
    true
}

fn reserve_persistent_font_gpu_va(bytes: usize) -> Option<u64> {
    let bytes = crate::intel::align_up(bytes, 4096)? as u64;
    {
        let mut free = PERSISTENT_TRIANGLE_GPU_VA_FREE.lock();
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
        let current = PERSISTENT_FONT_GPU_VA_CURSOR.load(Ordering::Acquire);
        let aligned = (current.checked_add(4095)?) & !4095;
        let next = aligned.checked_add(bytes)?;
        if next > GPU_VA_PERSISTENT_FONT_LIMIT {
            return None;
        }
        if PERSISTENT_FONT_GPU_VA_CURSOR
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(aligned);
        }
    }
}

fn recycle_persistent_triangle_gpu_va(gpu_base: u64, bytes: usize) {
    let Some(bytes) = crate::intel::align_up(bytes, 4096).map(|value| value as u64) else {
        return;
    };
    let Some(end) = gpu_base.checked_add(bytes) else {
        return;
    };
    let mut free = PERSISTENT_TRIANGLE_GPU_VA_FREE.lock();
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
    if mesh.vertex_count < 3
        || mesh.index_count < 3
        || !mesh.index_count.is_multiple_of(3)
        || mesh.vertex_bytes == 0
        || mesh.index_bytes == 0
    {
        return None;
    }
    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);
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
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch: u32::try_from(pitch).ok()?,
        target_w: u32::try_from(rect_w).ok()?,
        target_h: u32::try_from(rect_h).ok()?,
    })
}

fn prepare_triangle_draw_resources_for_gpu_font_mesh(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    mesh: crate::intel::gpgpu::GpgpuFontOutlineMesh,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if mesh.storage_phys == 0
        || mesh.storage_bytes == 0
        || mesh.vertex_count < 3
        || mesh.index_count < 3
        || !mesh.index_count.is_multiple_of(3)
        || mesh.vertex_stride != 2 * core::mem::size_of::<f32>() as u32
    {
        return None;
    }
    let vertex_bytes = mesh.vertex_count.checked_mul(mesh.vertex_stride)?;
    let index_bytes = mesh
        .index_count
        .checked_mul(core::mem::size_of::<u32>() as u32)?;
    let vertex_end = mesh.vertex_offset_bytes.checked_add(vertex_bytes)? as usize;
    let index_end = mesh.index_offset_bytes.checked_add(index_bytes)? as usize;
    if vertex_end > mesh.storage_bytes || index_end > mesh.storage_bytes {
        return None;
    }
    if !map_render_ppgtt_range(GPU_VA_COMPUTE_FONT_MESH_BASE, mesh.storage_phys, mesh.storage_bytes)
    {
        return None;
    }

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    let vertex_gpu_addr =
        GPU_VA_COMPUTE_FONT_MESH_BASE.checked_add(mesh.vertex_offset_bytes as u64)?;
    let index_gpu_addr =
        GPU_VA_COMPUTE_FONT_MESH_BASE.checked_add(mesh.index_offset_bytes as u64)?;
    intel_render_focus_log!(
        "gpu-font-mesh-import accepted=1 producer=gpgpu consumer=3d storage_phys=0x{:X} storage_gpu=0x{:X} storage_bytes=0x{:X} vertices={} vertex_stride={} vertex_bytes={} vb_gpu=0x{:X} indices={} index_bytes={} ib_gpu=0x{:X} bounds=[{:.2},{:.2}..{:.2},{:.2}] cpu_geometry_copy=0 shared_physical_storage=1 ppgtt_mapped=1\n",
        mesh.storage_phys,
        GPU_VA_COMPUTE_FONT_MESH_BASE,
        mesh.storage_bytes,
        mesh.vertex_count,
        mesh.vertex_stride,
        vertex_bytes,
        vertex_gpu_addr,
        mesh.index_count,
        index_bytes,
        index_gpu_addr,
        mesh.min_x,
        mesh.min_y,
        mesh.max_x,
        mesh.max_y,
    );

    Some(TriangleDrawPrep {
        vertex_count: mesh.index_count,
        vertex_stride: mesh.vertex_stride,
        vertex_buffer_bytes: vertex_bytes,
        vertex_format: TriangleVertexFormat::Float2,
        vertex_gpu_addr,
        index_buffer: Some(TriangleIndexBufferPrep {
            index_count: mesh.index_count,
            byte_len: index_bytes,
            gpu_addr: index_gpu_addr,
        }),
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch,
        target_w,
        target_h,
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
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
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
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
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
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-triangle",
    )
}

fn submit_vertical_stripes_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> bool {
    let stripe_x_phase = PRIMARY_STRIPE_X_PHASE.fetch_add(MI_STRIPE_X_STEP_PX, Ordering::AcqRel);

    unsafe {
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, core::mem::size_of::<u32>());

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_vertical_stripe_store_batch(
        batch,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        stripe_x_phase,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DONE,
    ) else {
        crate::log!(
            "primary-mi-stripes batch build failed size={}x{} pitch=0x{:X} batch=0x{:X} phase={}\n",
            rect_w,
            rect_h,
            pitch,
            warm.batch_len,
            stripe_x_phase
        );
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    if should_log_primary_probe("periodic", PRIMARY_PROBE_SEQ.load(Ordering::Acquire)) {
        crate::log!(
            "primary-mi-stripes phase={} step={} stripes={} width={}\n",
            stripe_x_phase,
            MI_STRIPE_X_STEP_PX,
            MI_STRIPE_COUNT,
            MI_STRIPE_WIDTH_PX
        );
    }

    submit_warm_render_batch(dev, warm, RCS_EXEC_RESULT_DONE, RESULT_SLOT_PRE3D_DWORD, "mi-stripes")
}

fn submit_mi_scanout_store_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> bool {
    if rect_w == 0 || rect_h == 0 {
        crate::log!("mi-scanout-store-proof accepted=0 reason=empty-target\n");
        return false;
    }

    let x = (rect_w / 2).min(rect_w.saturating_sub(1));
    let y = (rect_h / 2).min(rect_h.saturating_sub(1));
    let Some(before) = crate::intel::display::sample_primary_surface_pixel(x as u32, y as u32)
    else {
        crate::log!("mi-scanout-store-proof accepted=0 reason=no-before-sample\n");
        return false;
    };
    let color = before ^ 0x00FF_FFFF;
    let Some(pixel_offset) = y
        .checked_mul(pitch)
        .and_then(|v| v.checked_add(x.saturating_mul(4)))
    else {
        crate::log!("mi-scanout-store-proof accepted=0 reason=offset-overflow\n");
        return false;
    };
    let pixel_gpu = dst_gpu_addr.saturating_add(pixel_offset as u64);

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, RESULT_DEBUG_SENTINEL);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_single_store_probe_batch(
        batch,
        pixel_gpu,
        color,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_MI_SCANOUT_DONE,
    ) else {
        crate::log!("mi-scanout-store-proof accepted=0 reason=batch-build\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_SCANOUT_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-scanout-store",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let marker = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
    let after = crate::intel::display::sample_primary_surface_pixel(x as u32, y as u32)
        .unwrap_or(0xFFFF_FFFF);
    let accepted =
        completed && marker == RCS_EXEC_RESULT_MI_SCANOUT_DONE && after == color && before != after;

    intel_render_focus_log!(
        "mi-scanout-store-proof accepted={} completed={} marker=0x{:08X} xy={}x{} gpu=0x{:X} pitch=0x{:X} before=0x{:08X} after=0x{:08X} color=0x{:08X} does_not_prove=3d_pipeline_or_ps\n",
        accepted as u8,
        completed as u8,
        marker,
        x,
        y,
        pixel_gpu,
        pitch,
        before,
        after,
        color,
    );

    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "mi-scanout-store");
    }
    accepted
}
