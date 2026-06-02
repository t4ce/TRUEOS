fn upload_triangle_shader_pipeline(
    warm: RenderWarmState,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
) -> Result<TriangleShaderLayout, &'static str> {
    let vs = stage_range("vs", pipeline.vs.meta.kernel, pipeline.vs.code)?;
    let ps = stage_range("ps", pipeline.ps.meta.kernel, pipeline.ps.code)?;

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

    let used_end = core::cmp::max(
        stage_end(vs.code_offset_bytes, vs.code_size_bytes).ok_or("shader-code-overflow")?,
        stage_end(ps.code_offset_bytes, ps.code_size_bytes).ok_or("shader-code-overflow")?,
    );
    if used_end > warm.draw_state_len {
        return Err("shader-code-exceeds-state-bo");
    }

    upload_stage_code(warm.draw_state_virt, vs.code_offset_bytes, pipeline.vs.code)?;
    upload_stage_code(warm.draw_state_virt, ps.code_offset_bytes, pipeline.ps.code)?;

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
            accesses_uav: pipeline.vs.meta.kernel.accesses_uav,
        },
        ps: TriangleShaderStageLayout {
            code_offset_bytes: ps.code_offset_bytes as u32,
            code_gpu_addr: ps_gpu,
            ksp_offset_bytes: pipeline.ps.meta.kernel.ksp_offset_bytes,
            ksp_gpu_addr: ps_gpu + pipeline.ps.meta.kernel.ksp_offset_bytes as u64,
            code_size_bytes: ps.code_size_bytes as u32,
            accesses_uav: pipeline.ps.meta.kernel.accesses_uav,
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
    let vs_first = pipeline.vs.code.first().copied().unwrap_or(0);
    let vs_uploaded_first = uploaded_vs.first().copied().unwrap_or(0);
    let vs_last = pipeline.vs.code.last().copied().unwrap_or(0);
    let vs_uploaded_last = uploaded_vs.last().copied().unwrap_or(0);
    let vs_tail4_start = pipeline.vs.code.len().saturating_sub(4);
    let uploaded_vs_tail4_start = uploaded_vs.len().saturating_sub(4);
    let vs_tail4 = &pipeline.vs.code[vs_tail4_start..];
    let uploaded_vs_tail4 = &uploaded_vs[uploaded_vs_tail4_start..];
    if submit_name == "vs-draw-frontier" || submit_name.starts_with("pdoane") {
        intel_render_focus_log!(
            "intel/render: {} shader-upload-verify note={} vs_match={} vs_baked_sig=0x{:016X} vs_uploaded_sig=0x{:016X} vs_first=0x{:08X}/0x{:08X} vs_last=0x{:08X}/0x{:08X} vs_tail4=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]/[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] ps_match={} ps_baked_sig=0x{:016X} ps_uploaded_sig=0x{:016X}\n",
            submit_name,
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.vs.code == uploaded_vs) as u8,
            vs_baked_sig,
            vs_uploaded_sig,
            vs_first,
            vs_uploaded_first,
            vs_last,
            vs_uploaded_last,
            vs_tail4.first().copied().unwrap_or(0),
            vs_tail4.get(1).copied().unwrap_or(0),
            vs_tail4.get(2).copied().unwrap_or(0),
            vs_tail4.get(3).copied().unwrap_or(0),
            uploaded_vs_tail4.first().copied().unwrap_or(0),
            uploaded_vs_tail4.get(1).copied().unwrap_or(0),
            uploaded_vs_tail4.get(2).copied().unwrap_or(0),
            uploaded_vs_tail4.get(3).copied().unwrap_or(0),
            (pipeline.ps.code == uploaded_ps) as u8,
            ps_baked_sig,
            ps_uploaded_sig,
        );
    } else {
        intel_render_verbose_log!(
            "intel/render: {} shader-upload-verify note={} vs_match={} vs_baked_sig=0x{:016X} vs_uploaded_sig=0x{:016X} vs_first=0x{:08X}/0x{:08X} vs_last=0x{:08X}/0x{:08X} vs_tail4=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]/[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] ps_match={} ps_baked_sig=0x{:016X} ps_uploaded_sig=0x{:016X}\n",
            submit_name,
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.vs.code == uploaded_vs) as u8,
            vs_baked_sig,
            vs_uploaded_sig,
            vs_first,
            vs_uploaded_first,
            vs_last,
            vs_uploaded_last,
            vs_tail4.first().copied().unwrap_or(0),
            vs_tail4.get(1).copied().unwrap_or(0),
            vs_tail4.get(2).copied().unwrap_or(0),
            vs_tail4.get(3).copied().unwrap_or(0),
            uploaded_vs_tail4.first().copied().unwrap_or(0),
            uploaded_vs_tail4.get(1).copied().unwrap_or(0),
            uploaded_vs_tail4.get(2).copied().unwrap_or(0),
            uploaded_vs_tail4.get(3).copied().unwrap_or(0),
            (pipeline.ps.code == uploaded_ps) as u8,
            ps_baked_sig,
            ps_uploaded_sig,
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
    prepare_triangle_draw_resources_with_geometry(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        VfPrimitiveGeometry::Canonical,
    )
}

fn prepare_triangle_draw_resources_with_geometry(
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

    let vertex_proof = write_triangle_vertices(warm, geometry)?;

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: vertex_proof.vertex_count,
        vertex_stride: vertex_proof.vertex_stride,
        vertex_gpu_addr: vertex_proof.gpu_addr,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn write_triangle_vertices(
    warm: RenderWarmState,
    geometry: VfPrimitiveGeometry,
) -> Option<TriangleVertexUploadProof> {
    let tri = geometry.vertices();
    let byte_len = TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_STRIDE;
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
        .take(TRIANGLE_DRAW_VERTICES)
        .zip(tri.iter())
    {
        dst.copy_from_slice(src);
    }

    let mut expected = [0u32; TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_DWORDS];
    for (dst, src) in expected.iter_mut().zip(tri.iter().flatten()) {
        *dst = src.to_bits();
    }

    let readback = unsafe {
        core::slice::from_raw_parts(
            warm.vertex_virt as *const u32,
            TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_DWORDS,
        )
    };
    let cpu_readback_ok = readback == expected.as_slice();

    crate::intel::dma_flush(warm.vertex_virt, byte_len);

    let signed_area_2x = (tri[1][0] - tri[0][0]) * (tri[2][1] - tri[0][1])
        - (tri[2][0] - tri[0][0]) * (tri[1][1] - tri[0][1]);

    intel_render_focus_log!(
        "intel/render: vertex-upload-proof accepted={} stage=cpu-write-readback geometry={} bytes={} stride={} count={} gpu=0x{:X} readback_ok={} flush=1 area2={:.3} winding={} v0=[{:.3},{:.3},{:.3}] v1=[{:.3},{:.3},{:.3}] v2=[{:.3},{:.3},{:.3}] does_not_prove=vf_fetch\n",
        cpu_readback_ok as u8,
        geometry.label(),
        byte_len,
        TRIANGLE_DRAW_VERTEX_STRIDE,
        TRIANGLE_DRAW_VERTICES,
        GPU_VA_VERTEX_BASE,
        cpu_readback_ok as u8,
        signed_area_2x,
        if signed_area_2x >= 0.0 { "ccw" } else { "cw" },
        tri[0][0],
        tri[0][1],
        tri[0][2],
        tri[1][0],
        tri[1][1],
        tri[1][2],
        tri[2][0],
        tri[2][1],
        tri[2][2],
    );

    Some(TriangleVertexUploadProof {
        vertex_count: TRIANGLE_DRAW_VERTICES as u32,
        vertex_stride: TRIANGLE_DRAW_VERTEX_STRIDE as u32,
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

    let tri = geometry.vertices_for_target(target_w, target_h);
    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u32, warm.vertex_len / 4)
    };
    words.fill(0);

    for (idx, pos) in tri.iter().enumerate() {
        match experiment {
            StreamoutProofExperiment::PositionSlot0
            | StreamoutProofExperiment::PositionSlot0Xyzw
            | StreamoutProofExperiment::PositionSlot1 => {
                let base = idx * 4;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                words[base + 3] = 1.0f32.to_bits();
                intel_render_verbose_log!(
                    "intel/render: vf-streamout-source v{} experiment={} geometry={} raw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[{:.3},{:.3},{:.3},{:.3}]\n",
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
            StreamoutProofExperiment::MesaNoVsRectlist => {
                let base = idx * 3;
                words[base + 0] = pos[0].to_bits();
                words[base + 1] = pos[1].to_bits();
                words[base + 2] = pos[2].to_bits();
                intel_render_verbose_log!(
                    "intel/render: vf-streamout-source v{} experiment={} geometry={} mesa_pos=[0x{:08X},0x{:08X},0x{:08X}] pos_f=[{:.3},{:.3},{:.3}] forced_w=1.0\n",
                    idx,
                    experiment.label(),
                    geometry.label(),
                    words[base + 0],
                    words[base + 1],
                    words[base + 2],
                    f32::from_bits(words[base + 0]),
                    f32::from_bits(words[base + 1]),
                    f32::from_bits(words[base + 2]),
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
                    "intel/render: vf-streamout-source v{} experiment={} geometry={} hdr=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos_f=[{:.3},{:.3},{:.3},{:.3}]\n",
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
        }
    }

    crate::intel::dma_flush(warm.vertex_virt, TRIANGLE_DRAW_VERTICES * vertex_stride);

    Some(TriangleDrawPrep {
        vertex_count: geometry.draw_vertex_count(),
        vertex_stride: vertex_stride as u32,
        vertex_gpu_addr: GPU_VA_VERTEX_BASE,
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
            "intel/render: primary-triangle batch build failed size={}x{} pitch=0x{:X}\n",
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
            "intel/render: primary-mi-stripes batch build failed size={}x{} pitch=0x{:X} batch=0x{:X} phase={}\n",
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
            "intel/render: primary-mi-stripes phase={} step={} stripes={} width={}\n",
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
        crate::log!("intel/render: mi-scanout-store-proof accepted=0 reason=empty-target\n");
        return false;
    }

    let x = (rect_w / 2).min(rect_w.saturating_sub(1));
    let y = (rect_h / 2).min(rect_h.saturating_sub(1));
    let Some(before) = crate::intel::display::sample_primary_surface_pixel(x as u32, y as u32)
    else {
        crate::log!("intel/render: mi-scanout-store-proof accepted=0 reason=no-before-sample\n");
        return false;
    };
    let color = before ^ 0x00FF_FFFF;
    let Some(pixel_offset) = y
        .checked_mul(pitch)
        .and_then(|v| v.checked_add(x.saturating_mul(4)))
    else {
        crate::log!("intel/render: mi-scanout-store-proof accepted=0 reason=offset-overflow\n");
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
        crate::log!("intel/render: mi-scanout-store-proof accepted=0 reason=batch-build\n");
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
        "intel/render: mi-scanout-store-proof accepted={} completed={} marker=0x{:08X} xy={}x{} gpu=0x{:X} pitch=0x{:X} before=0x{:08X} after=0x{:08X} color=0x{:08X} does_not_prove=3d_pipeline_or_ps\n",
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
