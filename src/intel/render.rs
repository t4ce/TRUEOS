use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_START: usize = RCS_RING_BASE + 0x38;
const RCS_RING_CTL: usize = RCS_RING_BASE + 0x3C;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
const RCS_RING_IMR: usize = RCS_RING_BASE + 0xA8;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_INSTDONE: usize = RCS_RING_BASE + 0x6C;
const RCS_RING_CONTEXT_CONTROL: usize = RCS_RING_BASE + 0x244;
const RCS_RING_CONTEXT_CONTROL_REF: usize = RCS_RING_BASE + 0x5A0;
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
const RCS_RING_EXECLIST_SUBMIT_PORT: usize = RCS_RING_BASE + 0x230;
const RCS_RING_EXECLIST_STATUS_LO: usize = RCS_RING_BASE + 0x234;
const RCS_RING_EXECLIST_STATUS_HI: usize = RCS_RING_BASE + 0x238;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const CURSOR_A_OFFSET: usize = 0x70080;
const CURSOR_B_OFFSET: usize = 0x700C0;
const CURSOR_C_OFFSET: usize = 0x700E0;
const CURSOR_D_OFFSET: usize = 0x73080;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 22 * 4096;
const WARM_BATCH_BYTES: usize = 4096;
const WARM_DRAW_STATE_BYTES: usize = 16 * 4096;
const WARM_VERTEX_BYTES: usize = 4096;
const WARM_RESULT_BYTES: usize = 4096;
const BLT_RING_DWORDS: usize = 4;
const BLT_RING_TAIL_BYTES: usize = BLT_RING_DWORDS * core::mem::size_of::<u32>();
const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
const GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const GPU_VA_BATCH_BASE: u64 = 0x0083_0000;
const GPU_VA_RESULT_BASE: u64 = 0x0084_0000;
const GPU_VA_DRAW_STATE_BASE: u64 = 0x0086_0000;
const GPU_VA_VERTEX_BASE: u64 = 0x0087_0000;
const RING_VALID: u32 = 1;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_GTT: u32 = 2 << 6;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_CTX_VALID: u32 = 1 << 0;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT: u32 = 0xD;
const RCS_EXEC_RESULT_DONE: u32 = 0xC0DE_7701;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const TRIANGLE_MIN_DIM: usize = 8;
// This proof path emits one MI_STORE_DATA_IMM per covered pixel, so keep the
// triangle intentionally small until we switch to an actual draw pipeline.
const TRIANGLE_MAX_W: usize = 20;
const TRIANGLE_MAX_H: usize = 18;
const TRIANGLE_DRAW_VERTICES: usize = 3;
const TRIANGLE_DRAW_VERTEX_DWORDS: usize = crate::intel::shader::TRIANGLE_VERTEX_COMPONENTS;
const TRIANGLE_DRAW_VERTEX_STRIDE: usize = crate::intel::shader::TRIANGLE_VERTEX_STRIDE_BYTES;

#[derive(Copy, Clone, Debug)]
pub struct RenderWarmState {
    pub device_id: u16,
    pub revision_id: u8,
    pub mmio_base: usize,
    pub mmio_len: usize,
    pub ring_phys: u64,
    pub ring_virt: *mut u8,
    pub ring_len: usize,
    pub context_phys: u64,
    pub context_virt: *mut u8,
    pub context_len: usize,
    pub batch_phys: u64,
    pub batch_virt: *mut u8,
    pub batch_len: usize,
    pub draw_state_phys: u64,
    pub draw_state_virt: *mut u8,
    pub draw_state_len: usize,
    pub vertex_phys: u64,
    pub vertex_virt: *mut u8,
    pub vertex_len: usize,
    pub result_phys: u64,
    pub result_virt: *mut u8,
    pub result_len: usize,
}

#[derive(Copy, Clone)]
struct TriangleDrawPrep {
    vertex_count: u32,
    vertex_stride: u32,
    vertex_gpu_addr: u64,
    state_gpu_addr: u64,
    rt_gpu_addr: u64,
    rt_pitch: u32,
    target_w: u32,
    target_h: u32,
}

unsafe impl Send for RenderWarmState {}
unsafe impl Sync for RenderWarmState {}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);
static PRIMARY_TRIANGLE_SUBMITTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn warm_once(dev: crate::intel::Dev) -> RenderWarmState {
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
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=ring size=0x{:X}\n", WARM_RING_BYTES);
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
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!(
            "intel/render: warm alloc failed part=context size=0x{:X}\n",
            WARM_CONTEXT_BYTES
        );
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
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=batch size=0x{:X}\n", WARM_BATCH_BYTES);
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
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!(
            "intel/render: warm alloc failed part=draw-state size=0x{:X}\n",
            WARM_DRAW_STATE_BYTES
        );
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
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=vertex size=0x{:X}\n", WARM_VERTEX_BYTES);
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
        };
        *WARM_STATE.lock() = Some(warm);
        crate::log!("intel/render: warm alloc failed part=result size=0x{:X}\n", WARM_RESULT_BYTES);
        return warm;
    };

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, WARM_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, WARM_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, WARM_BATCH_BYTES);
        core::ptr::write_bytes(draw_state_virt, 0, WARM_DRAW_STATE_BYTES);
        core::ptr::write_bytes(vertex_virt, 0, WARM_VERTEX_BYTES);
        core::ptr::write_bytes(result_virt, 0, WARM_RESULT_BYTES);
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
    };
    *WARM_STATE.lock() = Some(warm);
    warm
}

pub fn warm_state() -> Option<RenderWarmState> {
    *WARM_STATE.lock()
}

pub fn log_cursor_plane_info(warm: RenderWarmState) {
    let caps = cursor_plane_caps(warm.device_id);
    crate::log!(
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
    crate::log!(
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

    crate::intel::mmio_write(
        dev,
        FORCEWAKE_RENDER,
        crate::intel::mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
    );
    let render_cleared = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_RENDER, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let render_ok = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_GT, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let gt_ok =
        wait_eq(dev, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL, FORCEWAKE_POLL_ITERS);

    crate::log!(
        "intel/render: forcewake render_cleared={} render_ack=0x{:08X} gt_ack=0x{:08X} ok={}\n",
        render_cleared as u8,
        crate::intel::mmio_read(dev, FORCEWAKE_ACK_RENDER),
        crate::intel::mmio_read(dev, FORCEWAKE_ACK_GT),
        (render_ok && gt_ok) as u8
    );

    render_ok && gt_ok
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
    crate::log!(
        "intel/render: sanity reg=RCS_IMR before=0x{:08X} wrote=0x{:08X} after=0x{:08X} restored=0x{:08X}\n",
        before,
        toggled,
        after,
        restored
    );
}

pub(crate) fn submit_primary_triangle_once() {
    if PRIMARY_TRIANGLE_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-device\n");
        return;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-surface\n");
        return;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-dimensions\n");
        return;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("intel/render: primary-triangle skipped reason=bad-pitch width={}\n", width);
        return;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
    {
        crate::log!("intel/render: primary-triangle skipped reason=warm-buffers\n");
        return;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: primary-triangle skipped reason=forcewake\n");
        return;
    }
    if !map_smoke_buffers(dev, warm) {
        crate::log!("intel/render: primary-triangle skipped reason=ggtt-map\n");
        return;
    }
    if submit_triangle_draw_to_surface(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) {
        return;
    }
    if !submit_triangle_to_surface(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) {
        crate::log!("intel/render: primary-triangle submit failed\n");
    }
}

fn wait_eq(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn map_smoke_buffers(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let ok_ring = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let ok_context = super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let ok_batch = super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let ok_draw_state =
        super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE);
    let ok_vertex = super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE);
    let ok_result = super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    if ok_ring && ok_context && ok_batch && ok_draw_state && ok_vertex && ok_result {
        super::ggtt_invalidate(dev);
        true
    } else {
        false
    }
}

fn submit_triangle_draw_to_surface(
    _dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: draw-path staging skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };

    let Some(pipeline) = crate::intel::shader::triangle_pipeline() else {
        crate::log!(
            "intel/render: draw-path staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=awaiting-baked-shaders vs_src={} ps_src={} note={}\n",
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    };

    crate::log!(
        "intel/render: draw-path staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=pipeline-ready vs_dwords={} ps_dwords={} varyings={} ps_dispatch={:?}\n",
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        pipeline.vs.code.len(),
        pipeline.ps.code.len(),
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );
    false
}

fn prepare_triangle_draw_resources(
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
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

    let vertices = unsafe {
        core::slice::from_raw_parts_mut(
            warm.vertex_virt as *mut f32,
            (warm.vertex_len / core::mem::size_of::<f32>())
                .max(TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_DWORDS),
        )
    };
    vertices.fill(0.0);

    // Keep this orthographic and centered in clip space so the first baked VS
    // only needs to pass through position.
    let tri = [[0.0f32, 0.72, 0.0], [-0.72, -0.58, 0.0], [0.72, -0.58, 0.0]];
    for (dst, src) in vertices
        .chunks_exact_mut(TRIANGLE_DRAW_VERTEX_DWORDS)
        .take(TRIANGLE_DRAW_VERTICES)
        .zip(tri.iter())
    {
        dst.copy_from_slice(src);
    }
    crate::intel::dma_flush(warm.vertex_virt, TRIANGLE_DRAW_VERTICES * TRIANGLE_DRAW_VERTEX_STRIDE);

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: TRIANGLE_DRAW_VERTICES as u32,
        vertex_stride: TRIANGLE_DRAW_VERTEX_STRIDE as u32,
        vertex_gpu_addr: GPU_VA_VERTEX_BASE,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch,
        target_w,
        target_h,
    })
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
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

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

    let ring_tail_bytes = build_ring_batch_start(warm, GPU_VA_BATCH_BASE);
    let Some(ring_ctl) = ring_ctl_value(warm.ring_len) else {
        return false;
    };
    if !init_gen12_lrc_context_image(
        warm,
        GPU_VA_RING_BASE as u32,
        ring_tail_bytes as u32,
        ring_ctl,
    ) {
        return false;
    }
    let (context_desc_lo, context_desc_hi) = build_execlist_context_descriptor(GPU_VA_CONTEXT_BASE);

    crate::intel::mmio_write(
        dev,
        RCS_RING_MODE_GEN7,
        masked_bit_enable(GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_after);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

    let mut completed = false;
    let mut iter = 0usize;
    while iter < 4096 {
        let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
        if result0 == RCS_EXEC_RESULT_DONE {
            completed = true;
            break;
        }
        if iter == 0 || iter == 256 || iter == 1024 || iter == 4095 {
            crate::log!(
                "intel/render: primary-triangle poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} result0=0x{:08X}\n",
                iter,
                crate::intel::mmio_read(dev, RCS_RING_HEAD),
                crate::intel::mmio_read(dev, RCS_RING_TAIL),
                crate::intel::mmio_read(dev, RCS_RING_ACTHD),
                crate::intel::mmio_read(dev, RCS_RING_IPEIR),
                crate::intel::mmio_read(dev, RCS_RING_IPEHR),
                crate::intel::mmio_read(dev, RCS_RING_EIR),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_HI),
                result0
            );
        }
        core::hint::spin_loop();
        iter += 1;
    }

    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let result0 = unsafe { core::ptr::read_volatile(warm.result_virt as *const u32) };
    crate::log!(
        "intel/render: primary-triangle complete={} result0=0x{:08X} ctl=0x{:08X} instdone=0x{:08X}\n",
        completed as u8,
        result0,
        crate::intel::mmio_read(dev, RCS_RING_CTL),
        crate::intel::mmio_read(dev, RCS_RING_INSTDONE)
    );
    completed
}

fn build_ring_batch_start(warm: RenderWarmState, batch_gpu_addr: u64) -> usize {
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.ring_virt as *mut u32, BLT_RING_DWORDS) };
    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT;
    dwords[1] = batch_gpu_addr as u32;
    dwords[2] = (batch_gpu_addr >> 32) as u32;
    dwords[3] = MI_NOOP;
    crate::intel::dma_flush(warm.ring_virt, BLT_RING_TAIL_BYTES);
    BLT_RING_TAIL_BYTES
}

fn ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn build_execlist_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | CTX_DESC_FORCE_RESTORE
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    (desc, (context_gpu_addr >> 32) as u32)
}

fn execlist_submit_port_push(
    dev: crate::intel::Dev,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_hi);
}

fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

fn mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | mi_lri_num_regs(num_regs)
}

fn push_mi_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn init_gen12_lrc_context_image(
    warm: RenderWarmState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 192 {
        return false;
    }

    state[0] = MI_NOOP;
    let mut idx = 1usize;

    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x2244;
    state[idx + 1] = 0x0009_0009;
    state[idx + 2] = 0x2034;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2030;
    state[idx + 5] = ring_tail;
    state[idx + 6] = 0x2038;
    state[idx + 7] = ring_start;
    state[idx + 8] = 0x203C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = 0x2168;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2140;
    state[idx + 13] = 0;
    state[idx + 14] = 0x2110;
    state[idx + 15] = 0;
    state[idx + 16] = 0x211C;
    state[idx + 17] = 0;
    state[idx + 18] = 0x2114;
    state[idx + 19] = 0;
    state[idx + 20] = 0x2118;
    state[idx + 21] = 0;
    state[idx + 22] = 0x21C0;
    state[idx + 23] = 0;
    state[idx + 24] = 0x21C4;
    state[idx + 25] = 0;
    state[idx + 26] = 0x21C8;
    state[idx + 27] = GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT;
    state[idx + 28] = 0x2180;
    state[idx + 29] = 0;
    idx += 30;

    push_mi_nops(state, &mut idx, 5);

    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x23A8;
    state[idx + 1] = 0;
    state[idx + 2] = 0x228C;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2288;
    state[idx + 5] = 0;
    state[idx + 6] = 0x2284;
    state[idx + 7] = 0;
    state[idx + 8] = 0x2280;
    state[idx + 9] = 0;
    state[idx + 10] = 0x227C;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2278;
    state[idx + 13] = 0;
    state[idx + 14] = 0x2274;
    state[idx + 15] = 0;
    state[idx + 16] = 0x2270;
    state[idx + 17] = 0;
    idx += 18;

    state[idx] = mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x21B0;
    state[idx + 1] = 0;
    state[idx + 2] = 0x25A8;
    state[idx + 3] = 0;
    state[idx + 4] = 0x25AC;
    state[idx + 5] = 0;
    idx += 6;

    push_mi_nops(state, &mut idx, 6);

    state[idx] = mi_lri_cmd(1, 0);
    idx += 1;
    state[idx] = 0x20C8;
    state[idx + 1] = 0x7FFF_FFFF;
    idx += 2;

    push_mi_nops(state, &mut idx, 13);

    state[idx] = mi_lri_cmd(51, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x2588;
    state[idx + 1] = 0;
    state[idx + 2] = 0x2588;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2588;
    state[idx + 5] = 0;
    state[idx + 6] = 0x2588;
    state[idx + 7] = 0;
    state[idx + 8] = 0x2588;
    state[idx + 9] = 0;
    state[idx + 10] = 0x2588;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2028;
    state[idx + 13] = 0;
    state[idx + 14] = 0x209C;
    state[idx + 15] = masked_bit_disable(RING_MI_MODE_STOP_RING);
    state[idx + 16] = 0x20C0;
    state[idx + 17] = 0;
    state[idx + 18] = 0x2178;
    state[idx + 19] = 0;
    state[idx + 20] = 0x217C;
    state[idx + 21] = 0;
    state[idx + 22] = 0x2358;
    state[idx + 23] = 0;
    state[idx + 24] = 0x2170;
    state[idx + 25] = 0;
    state[idx + 26] = 0x2150;
    state[idx + 27] = 0;
    state[idx + 28] = 0x2154;
    state[idx + 29] = 0;
    state[idx + 30] = 0x2158;
    state[idx + 31] = 0;
    state[idx + 32] = 0x241C;
    state[idx + 33] = 0;
    state[idx + 34] = 0x2600;
    state[idx + 35] = 0;
    state[idx + 36] = 0x2604;
    state[idx + 37] = 0;
    state[idx + 38] = 0x2608;
    state[idx + 39] = 0;
    state[idx + 40] = 0x260C;
    state[idx + 41] = 0;
    state[idx + 42] = 0x2610;
    state[idx + 43] = 0;
    state[idx + 44] = 0x2614;
    state[idx + 45] = 0;
    state[idx + 46] = 0x2618;
    state[idx + 47] = 0;
    state[idx + 48] = 0x261C;
    state[idx + 49] = 0;
    state[idx + 50] = 0x2620;
    state[idx + 51] = 0;
    state[idx + 52] = 0x2624;
    state[idx + 53] = 0;
    state[idx + 54] = 0x2628;
    state[idx + 55] = 0;
    state[idx + 56] = 0x262C;
    state[idx + 57] = 0;
    state[idx + 58] = 0x2630;
    state[idx + 59] = 0;
    state[idx + 60] = 0x2634;
    state[idx + 61] = 0;
    state[idx + 62] = 0x2638;
    state[idx + 63] = 0;
    state[idx + 64] = 0x263C;
    state[idx + 65] = 0;
    state[idx + 66] = 0x2640;
    state[idx + 67] = 0;
    state[idx + 68] = 0x2644;
    state[idx + 69] = 0;
    state[idx + 70] = 0x2648;
    state[idx + 71] = 0;
    state[idx + 72] = 0x264C;
    state[idx + 73] = 0;
    state[idx + 74] = 0x2650;
    state[idx + 75] = 0;
    state[idx + 76] = 0x2654;
    state[idx + 77] = 0;
    state[idx + 78] = 0x2658;
    state[idx + 79] = 0;
    state[idx + 80] = 0x265C;
    state[idx + 81] = 0;
    state[idx + 82] = 0x2660;
    state[idx + 83] = 0;
    state[idx + 84] = 0x2664;
    state[idx + 85] = 0;
    state[idx + 86] = 0x2668;
    state[idx + 87] = 0;
    state[idx + 88] = 0x266C;
    state[idx + 89] = 0;
    state[idx + 90] = 0x2670;
    state[idx + 91] = 0;
    state[idx + 92] = 0x2674;
    state[idx + 93] = 0;
    state[idx + 94] = 0x2678;
    state[idx + 95] = 0;
    state[idx + 96] = 0x267C;
    state[idx + 97] = 0;
    state[idx + 98] = 0x2068;
    state[idx + 99] = 0;
    state[idx + 100] = 0x2084;
    state[idx + 101] = 0;
    idx += 102;

    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = MI_BATCH_BUFFER_END | 1;

    crate::intel::dma_flush(warm.context_virt, warm.context_len);
    true
}

fn encode_rgb_triangle_store_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;

    if batch_dwords.len() <= RESERVED_END_DWORDS + STORE_DWORDS {
        return Err("batch-too-small");
    }
    if rect_w < TRIANGLE_MIN_DIM || rect_h < TRIANGLE_MIN_DIM {
        return Err("triangle-too-small");
    }

    let tri_w = rect_w.min(TRIANGLE_MAX_W).max(TRIANGLE_MIN_DIM);
    let tri_h = rect_h.min(TRIANGLE_MAX_H).max(TRIANGLE_MIN_DIM);
    let origin_x = rect_w.saturating_sub(tri_w) / 2;
    let origin_y = rect_h.saturating_sub(tri_h) / 2;
    let v0x = origin_x as i32 + (tri_w as i32 / 2);
    let v0y = origin_y as i32;
    let v1x = origin_x as i32;
    let v1y = origin_y as i32 + tri_h.saturating_sub(1) as i32;
    let v2x = origin_x as i32 + tri_w.saturating_sub(1) as i32;
    let v2y = v1y;
    let area = edge_fn(v0x, v0y, v1x, v1y, v2x, v2y);
    if area == 0 {
        return Err("triangle-degenerate");
    }

    let min_x = v0x.min(v1x).min(v2x).max(0) as usize;
    let max_x = (v0x.max(v1x).max(v2x) + 1).min(rect_w as i32) as usize;
    let min_y = v0y.min(v1y).min(v2y).max(0) as usize;
    let max_y = (v0y.max(v1y).max(v2y) + 1).min(rect_h as i32) as usize;

    batch_dwords.fill(0);
    let writable_limit = batch_dwords
        .len()
        .saturating_sub(RESERVED_END_DWORDS + STORE_DWORDS);
    let mut idx = 0usize;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = (x as i32) * 2 + 1;
            let py = (y as i32) * 2 + 1;
            let w0 = edge_fn2(v1x, v1y, v2x, v2y, px, py);
            let w1 = edge_fn2(v2x, v2y, v0x, v0y, px, py);
            let w2 = edge_fn2(v0x, v0y, v1x, v1y, px, py);
            if !same_sign_or_zero(area, w0)
                || !same_sign_or_zero(area, w1)
                || !same_sign_or_zero(area, w2)
            {
                continue;
            }
            if idx + STORE_DWORDS > writable_limit {
                return Err("batch-exhausted");
            }

            let r = bary_to_u8(w0, area);
            let g = bary_to_u8(w1, area);
            let b = bary_to_u8(w2, area);
            let color = pack_xrgb8888(r, g, b);
            let dst = dst_gpu_addr
                .saturating_add((y as u64).saturating_mul(pitch as u64))
                .saturating_add((x as u64).saturating_mul(4));

            batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
            batch_dwords[idx + 1] = dst as u32;
            batch_dwords[idx + 2] = (dst >> 32) as u32;
            batch_dwords[idx + 3] = color;
            idx += STORE_DWORDS;
        }
    }

    if idx == 0 {
        return Err("triangle-empty");
    }
    if idx + STORE_DWORDS > batch_dwords.len().saturating_sub(RESERVED_END_DWORDS) {
        return Err("batch-no-result-slot");
    }

    batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[idx + 1] = result_gpu_addr as u32;
    batch_dwords[idx + 2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[idx + 3] = done_value;
    idx += STORE_DWORDS;

    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += RESERVED_END_DWORDS;
    Ok(idx * core::mem::size_of::<u32>())
}

fn edge_fn(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i64 {
    let ax = ax as i64;
    let ay = ay as i64;
    let bx = bx as i64;
    let by = by as i64;
    let px = px as i64;
    let py = py as i64;
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn edge_fn2(ax: i32, ay: i32, bx: i32, by: i32, px2: i32, py2: i32) -> i64 {
    let ax2 = (ax * 2) as i64;
    let ay2 = (ay * 2) as i64;
    let bx2 = (bx * 2) as i64;
    let by2 = (by * 2) as i64;
    let px2 = px2 as i64;
    let py2 = py2 as i64;
    (px2 - ax2) * (by2 - ay2) - (py2 - ay2) * (bx2 - ax2)
}

fn same_sign_or_zero(area: i64, value: i64) -> bool {
    if area >= 0 { value >= 0 } else { value <= 0 }
}

fn bary_to_u8(weight: i64, area: i64) -> u32 {
    let num = weight.unsigned_abs().saturating_mul(255);
    let den = area.unsigned_abs().max(1);
    ((num + (den / 2)) / den).min(255) as u32
}

fn pack_xrgb8888(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

struct CursorPlaneCaps {
    platform: &'static str,
    layout: &'static str,
    max_width: u16,
    max_height: u16,
    pipe_count: u8,
}

struct SpritePlaneCaps {
    platform: &'static str,
    display_ver: u8,
    pipe_count: u8,
    overlays_per_pipe: u8,
    rotation: &'static str,
    reflect_x: bool,
    csc: &'static str,
    scaling_filter: &'static str,
    damage_clips: bool,
}

fn cursor_plane_caps(device_id: u16) -> CursorPlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => CursorPlaneCaps {
            platform: "ADL-S",
            layout: "TGL/XE_D",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A | 0x4626 | 0x4628
        | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => CursorPlaneCaps {
            platform: "ADL-P/N",
            layout: "TGL/XE_LPD",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        _ => CursorPlaneCaps {
            platform: "unknown",
            layout: "generic",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
    }
}

fn sprite_plane_caps(device_id: u16) -> SpritePlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => SpritePlaneCaps {
            platform: "ADL-S",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A | 0x4626 | 0x4628
        | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => SpritePlaneCaps {
            platform: "ADL-P/N",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        _ => SpritePlaneCaps {
            platform: "unknown",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
    }
}
