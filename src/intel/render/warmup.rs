static RENDER_PPGTT_PML4_PHYS: AtomicU64 = AtomicU64::new(0);
static RENDER_PPGTT: Mutex<Option<crate::intel::ppgtt::SparsePpgtt>> = Mutex::new(None);
// `WARM_STATE` protects readers after publication.  Allocation and PPGTT
// construction also need a gate: without it, two first users can both observe
// `None`, allocate different roots, and let the last publisher silently replace
// the HWLRCA/PPGTT backing already handed to the other user.
static WARM_INIT_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn warm_once(dev: crate::intel::Dev) -> RenderWarmState {
    if let Some(warm) = *WARM_STATE.lock() {
        return warm;
    }
    let _init = WARM_INIT_LOCK.lock();
    if let Some(warm) = *WARM_STATE.lock() {
        return warm;
    }

    let Some((ring_phys, ring_virt)) = crate::dma::alloc(WARM_RING_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys: 0,
            ring_virt: core::ptr::null_mut(),
            ring_len: 0,
            context_phys: 0,
            context_virt: core::ptr::null_mut(),
            context_len: 0,
            batch_phys: 0,
            batch_virt: core::ptr::null_mut(),
            batch_len: 0,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=ring size=0x{:X}\n", WARM_RING_BYTES);
        return warm;
    };
    let Some((context_phys, context_virt)) =
        crate::dma::alloc(WARM_CONTEXT_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys: 0,
            context_virt: core::ptr::null_mut(),
            context_len: 0,
            batch_phys: 0,
            batch_virt: core::ptr::null_mut(),
            batch_len: 0,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=context size=0x{:X}\n", WARM_CONTEXT_BYTES);
        return warm;
    };
    let Some((batch_phys, batch_virt)) =
        crate::dma::alloc(WARM_BATCH_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys: 0,
            batch_virt: core::ptr::null_mut(),
            batch_len: 0,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=batch size=0x{:X}\n", WARM_BATCH_BYTES);
        return warm;
    };
    let Some((draw_state_phys, draw_state_virt)) =
        crate::dma::alloc(WARM_DRAW_STATE_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys: 0,
            draw_state_virt: core::ptr::null_mut(),
            draw_state_len: 0,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=draw-state size=0x{:X}\n", WARM_DRAW_STATE_BYTES);
        return warm;
    };
    let Some((vertex_phys, vertex_virt)) =
        crate::dma::alloc(WARM_VERTEX_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys,
            draw_state_virt,
            draw_state_len: WARM_DRAW_STATE_BYTES,
            vertex_phys: 0,
            vertex_virt: core::ptr::null_mut(),
            vertex_len: 0,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=vertex size=0x{:X}\n", WARM_VERTEX_BYTES);
        return warm;
    };
    let Some((result_phys, result_virt)) =
        crate::dma::alloc(WARM_RESULT_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys,
            draw_state_virt,
            draw_state_len: WARM_DRAW_STATE_BYTES,
            vertex_phys,
            vertex_virt,
            vertex_len: WARM_VERTEX_BYTES,
            result_phys: 0,
            result_virt: core::ptr::null_mut(),
            result_len: 0,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=result size=0x{:X}\n", WARM_RESULT_BYTES);
        return warm;
    };
    let Some((streamout_phys, streamout_virt)) =
        crate::dma::alloc(WARM_STREAMOUT_BYTES, crate::intel::WARM_ALIGN)
    else {
        let warm = RenderWarmState {
            device_id: dev.device_id,
            revision_id: dev.revision_id,
            mmio_base: dev.mmio as usize,
            mmio_len: dev.mmio_len,
            ring_phys,
            ring_virt,
            ring_len: WARM_RING_BYTES,
            context_phys,
            context_virt,
            context_len: WARM_CONTEXT_BYTES,
            batch_phys,
            batch_virt,
            batch_len: WARM_BATCH_BYTES,
            draw_state_phys,
            draw_state_virt,
            draw_state_len: WARM_DRAW_STATE_BYTES,
            vertex_phys,
            vertex_virt,
            vertex_len: WARM_VERTEX_BYTES,
            result_phys,
            result_virt,
            result_len: WARM_RESULT_BYTES,
            streamout_phys: 0,
            streamout_virt: core::ptr::null_mut(),
            streamout_len: 0,
            gpgpu_arena_phys: 0,
            gpgpu_arena_virt: core::ptr::null_mut(),
            gpgpu_arena_len: 0,
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("warm alloc failed part=streamout size=0x{:X}\n", WARM_STREAMOUT_BYTES);
        return warm;
    };
    let (gpgpu_arena_phys, gpgpu_arena_virt, gpgpu_arena_len) = match crate::dma::alloc(
        GPGPU_TILE_ARENA_BYTES,
        crate::intel::WARM_ALIGN,
    ) {
        Some((phys, virt)) => (phys, virt, GPGPU_TILE_ARENA_BYTES),
        None => {
            crate::log!(
                "intel/gpgpu: arena alloc failed arena_bytes=0x{:X} tile_rows={} max_tiles=0 enough_for_shape=0\n",
                GPGPU_TILE_ARENA_BYTES,
                GPGPU_TILE_ROWS,
            );
            (0, core::ptr::null_mut(), 0)
        }
    };

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, WARM_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, WARM_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, WARM_BATCH_BYTES);
        core::ptr::write_bytes(draw_state_virt, 0, WARM_DRAW_STATE_BYTES);
        core::ptr::write_bytes(vertex_virt, 0, WARM_VERTEX_BYTES);
        core::ptr::write_bytes(result_virt, 0, WARM_RESULT_BYTES);
        core::ptr::write_bytes(streamout_virt, 0, WARM_STREAMOUT_BYTES);
        if !gpgpu_arena_virt.is_null() {
            core::ptr::write_bytes(gpgpu_arena_virt, 0, gpgpu_arena_len);
        }
    }

    let warm = RenderWarmState {
        device_id: dev.device_id,
        revision_id: dev.revision_id,
        mmio_base: dev.mmio as usize,
        mmio_len: dev.mmio_len,
        ring_phys,
        ring_virt,
        ring_len: WARM_RING_BYTES,
        context_phys,
        context_virt,
        context_len: WARM_CONTEXT_BYTES,
        batch_phys,
        batch_virt,
        batch_len: WARM_BATCH_BYTES,
        draw_state_phys,
        draw_state_virt,
        draw_state_len: WARM_DRAW_STATE_BYTES,
        vertex_phys,
        vertex_virt,
        vertex_len: WARM_VERTEX_BYTES,
        result_phys,
        result_virt,
        result_len: WARM_RESULT_BYTES,
        streamout_phys,
        streamout_virt,
        streamout_len: WARM_STREAMOUT_BYTES,
        gpgpu_arena_phys,
        gpgpu_arena_virt,
        gpgpu_arena_len,
    };
    if let Some(ppgtt) = crate::intel::ppgtt::build_sparse_ppgtt_for_ranges(&[
        crate::intel::ppgtt::PpgttRange {
            gpu: GPU_VA_BATCH_BASE,
            phys: warm.batch_phys,
            bytes: warm.batch_len,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: GPU_VA_DRAW_STATE_BASE,
            phys: warm.draw_state_phys,
            bytes: warm.draw_state_len,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: GPU_VA_VERTEX_BASE,
            phys: warm.vertex_phys,
            bytes: warm.vertex_len,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: GPU_VA_RESULT_BASE,
            phys: warm.result_phys,
            bytes: warm.result_len,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: GPU_VA_STREAMOUT_BASE,
            phys: warm.streamout_phys,
            bytes: warm.streamout_len,
        },
    ]) {
        RENDER_PPGTT_PML4_PHYS.store(ppgtt.pml4_phys(), Ordering::Release);
        *RENDER_PPGTT.lock() = Some(ppgtt);
    }
    *WARM_STATE.lock() = Some(warm);
    warm
}

fn render_ppgtt_pml4_phys() -> u64 {
    RENDER_PPGTT_PML4_PHYS.load(Ordering::Acquire)
}

pub(crate) fn map_render_ppgtt_range(gpu: u64, phys: u64, bytes: usize) -> bool {
    let mut guard = RENDER_PPGTT.lock();
    let Some(ppgtt) = guard.as_mut() else {
        return false;
    };
    ppgtt
        .map_range(crate::intel::ppgtt::PpgttRange { gpu, phys, bytes })
        .is_some()
}

pub(crate) fn map_render_ppgtt_scanout_range(gpu: u64, phys: u64, bytes: usize) -> bool {
    let mut guard = RENDER_PPGTT.lock();
    let Some(ppgtt) = guard.as_mut() else {
        return false;
    };
    ppgtt
        .map_scanout_range(crate::intel::ppgtt::PpgttRange { gpu, phys, bytes })
        .is_some()
}

pub(crate) fn unmap_render_ppgtt_range(gpu: u64, bytes: usize) -> bool {
    let mut guard = RENDER_PPGTT.lock();
    let Some(ppgtt) = guard.as_mut() else {
        return false;
    };
    ppgtt.unmap_range(gpu, bytes).is_some()
}

pub fn warm_state() -> Option<RenderWarmState> {
    *WARM_STATE.lock()
}

pub fn log_cursor_plane_info(warm: RenderWarmState) {
    let caps = cursor_plane_caps(warm.device_id);
    intel_render_verbose_log!(
        "intel/display: cursor-plane platform={} rev=0x{:02X} max={}x{} pipes={} layout={} regs=A:0x{:X},B:0x{:X},C:0x{:X},D:0x{:X}\n",
        caps.platform,
        warm.revision_id,
        caps.max_width,
        caps.max_height,
        caps.pipe_count,
        caps.layout,
        CURSOR_A_OFFSET,
        CURSOR_B_OFFSET,
        CURSOR_C_OFFSET,
        CURSOR_D_OFFSET
    );
}

pub fn log_sprite_plane_info(warm: RenderWarmState) {
    let caps = sprite_plane_caps(warm.device_id);
    intel_render_verbose_log!(
        "intel/display: sprite-planes platform={} display_ver={} pipes={} overlays/pipe={} type=universal props=rotation:{} reflect_x:{} alpha:1 blend:pixel-none|premulti|coverage zpos:immutable csc:{} range:limited|full scaler:{} damage_clips:{}\n",
        caps.platform,
        caps.display_ver,
        caps.pipe_count,
        caps.overlays_per_pipe,
        caps.rotation,
        caps.reflect_x as u8,
        caps.csc,
        caps.scaling_filter,
        caps.damage_clips as u8
    );
}

pub(crate) fn init_global_rcs_workarounds_for_boot(dev: crate::intel::Dev) -> bool {
    crate::intel::mmio_write(
        dev,
        RCS_CS_DEBUG_MODE1,
        crate::intel::mask_en(FF_DOP_CLOCK_GATE_DISABLE),
    );

    if device_is_gfx125(dev.device_id) {
        // Mesa's gfx125 init path enables these TBIMR-related raster controls
        // before any client context is admitted.  This is a physical-RCS
        // property, not state owned by an individual Render/Spirit context.
        crate::intel::mmio_write(dev, CHICKEN_RASTER_2, gfx125_chicken_raster_2_value());
    }

    let accepted = global_rcs_workarounds_ready(dev);
    let cs_debug_mode1 = crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE1);
    let chicken_raster_2 = device_is_gfx125(dev.device_id)
        .then(|| crate::intel::mmio_read(dev, CHICKEN_RASTER_2))
        .unwrap_or(0);
    crate::log_info!(
        target: "render";
        "intel/gt-global-init: rcs_workarounds accepted={} ownership=boot-only device=0x{:04X} cs_debug_mode1=0x{:08X} ff_dop_cg_disable={} chicken_raster_2=0x{:08X} gfx125_tbimr={}\n",
        accepted as u8,
        dev.device_id,
        cs_debug_mode1,
        ((cs_debug_mode1 & FF_DOP_CLOCK_GATE_DISABLE) != 0) as u8,
        chicken_raster_2,
        (!device_is_gfx125(dev.device_id)
            || chicken_raster_2
                & (TBIMR_BATCH_SIZE_OVERRIDE | TBIMR_OPEN_BATCH_ENABLE | TBIMR_FAST_CLIP)
                == (TBIMR_BATCH_SIZE_OVERRIDE | TBIMR_OPEN_BATCH_ENABLE | TBIMR_FAST_CLIP))
            as u8,
    );
    accepted
}

pub(crate) fn global_rcs_workarounds_ready(dev: crate::intel::Dev) -> bool {
    let cs_debug_ready =
        crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE1) & FF_DOP_CLOCK_GATE_DISABLE != 0;
    let raster_ready = !device_is_gfx125(dev.device_id)
        || crate::intel::mmio_read(dev, CHICKEN_RASTER_2)
            & (TBIMR_BATCH_SIZE_OVERRIDE | TBIMR_OPEN_BATCH_ENABLE | TBIMR_FAST_CLIP)
            == (TBIMR_BATCH_SIZE_OVERRIDE | TBIMR_OPEN_BATCH_ENABLE | TBIMR_FAST_CLIP);
    cs_debug_ready && raster_ready
}

pub fn forcewake_render_acquire(warm: RenderWarmState) -> bool {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };

    // Render clients consume the physical-GT contract.  They never acquire,
    // release, or repair device-global forcewake/workaround registers while a
    // different GuC context may be resident on RCS0.
    let ok = crate::intel::physical_gt_ready(dev);
    let cs_debug_mode1 = crate::intel::mmio_read(dev, RCS_CS_DEBUG_MODE1);

    if should_log_primary_probe_detail() {
        crate::log!(
            "forcewake ownership=boot-only runtime_writes=0 render_ack=0x{:08X} gt_ack=0x{:08X} cs_debug_mode1=0x{:08X} ff_dop_cg_disable={} ok={}\n",
            crate::intel::mmio_read(dev, FORCEWAKE_ACK_RENDER),
            crate::intel::mmio_read(dev, FORCEWAKE_ACK_GT),
            cs_debug_mode1,
            ((cs_debug_mode1 & FF_DOP_CLOCK_GATE_DISABLE) != 0) as u8,
            ok as u8
        );
    }

    ok
}

fn gfx125_chicken_raster_2_value() -> u32 {
    let bits = TBIMR_BATCH_SIZE_OVERRIDE | TBIMR_OPEN_BATCH_ENABLE | TBIMR_FAST_CLIP;
    crate::intel::mask_en(bits)
}

#[derive(Copy, Clone)]
struct Gfx125SliceHashConfig {
    geometry_dss_enable: u32,
    ppipe_subslices: [u8; GFX125_PIXEL_PIPES],
    ppipe_mask1: u32,
    ppipe_mask2: u32,
    cross_slice_hashing_mode: u32,
}

fn gfx125_slice_hash_config(warm: RenderWarmState) -> Gfx125SliceHashConfig {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };
    let geometry_dss_enable = crate::intel::mmio_read(dev, GFX125_GEOMETRY_DSS_ENABLE);
    let mut ppipe_subslices = [0u8; GFX125_PIXEL_PIPES];
    let ppipe_mask = (1u32 << GFX125_DUAL_SUBSLICES_PER_PIXEL_PIPE) - 1;

    for (ppipe, count) in ppipe_subslices.iter_mut().enumerate() {
        let shift = ppipe * GFX125_DUAL_SUBSLICES_PER_PIXEL_PIPE;
        *count = ((geometry_dss_enable >> shift) & ppipe_mask).count_ones() as u8;
    }

    let mut ppipe_mask1 = 0u32;
    let mut ppipe_mask2 = 0u32;
    for (ppipe, count) in ppipe_subslices.iter().copied().enumerate() {
        if count > 0 {
            ppipe_mask1 |= 1u32 << ppipe;
        }
        if count > 1 {
            ppipe_mask2 |= 1u32 << ppipe;
        }
    }

    if ppipe_mask1 == 0 {
        ppipe_subslices[0] = 1;
        ppipe_mask1 = 1;
    }

    let cross_slice_hashing_mode = if ppipe_mask1.count_ones() > 1 {
        GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32
    } else {
        0
    };

    Gfx125SliceHashConfig {
        geometry_dss_enable,
        ppipe_subslices,
        ppipe_mask1,
        ppipe_mask2,
        cross_slice_hashing_mode,
    }
}

fn gfx125_logbase2_ceil(value: usize) -> usize {
    if value <= 1 {
        0
    } else {
        (usize::BITS - (value - 1).leading_zeros()) as usize
    }
}

fn gfx125_compute_pixel_hash_table_nway(
    mask1: u32,
    mask2: u32,
    table: &mut [u8; GFX125_SLICE_HASH_TABLE_ENTRIES],
) {
    let mut mask2 = mask2;
    if mask1 == mask2 {
        mask2 = 0;
    }

    let mut phys_ids = [0usize; 64];
    let mut num_ids = 0usize;
    for bit in 0..u32::BITS as usize {
        let bit_mask = 1u32 << bit;
        if (mask1 & bit_mask) != 0 {
            phys_ids[num_ids] = bit;
            num_ids += 1;
        }
        if (mask2 & bit_mask) != 0 {
            phys_ids[num_ids] = bit;
            num_ids += 1;
        }
    }

    if num_ids == 0 {
        table.fill(0);
        return;
    }

    let bits = gfx125_logbase2_ceil(num_ids);
    let mut swzy = [0usize; 64];
    for (k, slot) in swzy.iter_mut().enumerate().take(num_ids) {
        let mut t = num_ids;
        let mut s = 0usize;

        for l in 0..bits {
            if (k & (1usize << l)) != 0 {
                s += (t + 1) >> 1;
                t >>= 1;
            } else {
                t = (t + 1) >> 1;
            }
        }

        *slot = s;
    }

    let mut swzx = [0usize; 64];
    if mask1 != 0 && mask2 != 0 {
        for (k, slot) in swzx.iter_mut().enumerate().take(num_ids) {
            let mut l = k;
            let mut t = num_ids;
            let mut s = 0usize;
            let mut in_range = false;

            while t > 1 {
                let first_in_range = t <= GFX125_SLICE_HASH_DIM && !in_range;
                in_range |= first_in_range;

                if l >= ((t + 1) >> 1) {
                    if !in_range {
                        s += (t + 1) >> 1;
                    } else if first_in_range {
                        s += 1;
                    } else {
                        s += ((t + 1) >> 1) << 1;
                    }

                    l -= (t + 1) >> 1;
                    t >>= 1;
                } else {
                    t = (t + 1) >> 1;
                }
            }

            *slot = s;
        }
    } else {
        for (k, slot) in swzx.iter_mut().enumerate().take(num_ids) {
            *slot = k;
        }
    }

    for y in 0..GFX125_SLICE_HASH_DIM {
        let row = y * GFX125_SLICE_HASH_DIM;
        let k = y % num_ids;
        for x in 0..GFX125_SLICE_HASH_DIM {
            let l = x % num_ids;
            table[row + x] = phys_ids[(swzx[l] + swzy[k]) % num_ids] as u8;
        }
    }
}

fn gfx125_pack_slice_hash_tables(
    config: Gfx125SliceHashConfig,
    dwords: &mut [u32; GFX125_SLICE_HASH_TABLE_DWORDS],
) {
    let mut entries = [0u8; GFX125_SLICE_HASH_TABLE_ENTRIES];
    gfx125_compute_pixel_hash_table_nway(config.ppipe_mask1, config.ppipe_mask2, &mut entries);
    dwords.fill(0);

    for table_idx in 0..GFX125_SLICE_HASH_TABLES {
        let table_base = table_idx * GFX125_SLICE_HASH_TABLE_DWORDS_PER_TABLE;
        for (entry_idx, entry) in entries.iter().copied().enumerate() {
            let dword_idx = table_base + (entry_idx / 8);
            let shift = (entry_idx % 8) * 4;
            dwords[dword_idx] |= (entry as u32) << shift;
        }
    }
}

fn gfx125_3d_mode_dw1(config: Gfx125SliceHashConfig) -> u32 {
    config.cross_slice_hashing_mode | (0b11 << 16) | (1 << 6) | (1 << 22)
}

fn gfx125_3d_mode_dw3() -> u32 {
    // Keep RHWO disabled for bring-up so the first render proof does not depend
    // on an optimization state that Mesa conditionally toggles later.
    (1 << 15) | (1 << 31)
}

pub fn forcewake_render_sanity(warm: RenderWarmState) {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };
    let before = crate::intel::mmio_read(dev, RCS_RING_IMR);
    let toggled = before ^ 0x0000_0001;
    crate::intel::mmio_write(dev, RCS_RING_IMR, toggled);
    let after = crate::intel::mmio_read(dev, RCS_RING_IMR);
    crate::intel::mmio_write(dev, RCS_RING_IMR, before);
    let restored = crate::intel::mmio_read(dev, RCS_RING_IMR);
    intel_render_verbose_log!(
        "sanity reg=RCS_IMR before=0x{:08X} wrote=0x{:08X} after=0x{:08X} restored=0x{:08X}\n",
        before,
        toggled,
        after,
        restored
    );
}
