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
const RCS_RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
const RCS_RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
const CURSOR_A_OFFSET: usize = 0x70080;
const CURSOR_B_OFFSET: usize = 0x71080;
const CURSOR_C_OFFSET: usize = 0x72080;
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
const RCS_EXEC_RESULT_MI_PROBE_DONE: u32 = 0xC0DE_7711;
const RCS_EXEC_RESULT_3D_NO_DRAW_DONE: u32 = 0xC0DE_7712;
const RCS_EXEC_RESULT_DRAW_PRE3D: u32 = 0xC0DE_7721;
const RCS_EXEC_RESULT_DRAW_POST3D: u32 = 0xC0DE_7722;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const RENDER_MOCS: u32 = 1;
const SURFTYPE_2D: u32 = 1;
const SURFACE_FORMAT_R8G8B8A8_UNORM: u32 = 9;
const SURFACE_FORMAT_R32G32B32_FLOAT: u32 = 64;
const SURFACE_HALIGN_4: u32 = 1;
const SURFACE_VALIGN_4: u32 = 1;
const SHADER_CHANNEL_RED: u32 = 4;
const SHADER_CHANNEL_GREEN: u32 = 5;
const SHADER_CHANNEL_BLUE: u32 = 6;
const SHADER_CHANNEL_ALPHA: u32 = 7;
const SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD: u32 = 0xFFFF_FFFF;
const CLIP_PERSPECTIVE_DIVIDE_DISABLE: u32 = 1 << 9;
const WM_FORCE_KILL_PIXEL_OFF: u32 = 1;
const PS_VECTOR_MASK_ENABLE: u32 = 1 << 30;
const PS_SINGLE_PROGRAM_FLOW: u32 = 1 << 31;
const PS_PUSH_CONSTANT_ENABLE: u32 = 1 << 11;
const PS_MAX_THREADS_SHIFT: u32 = 23;
const PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL: u32 = 1 << 5;
const PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE: u32 = 1 << 6;
const PS_EXTRA_ATTRIBUTE_ENABLE: u32 = 1 << 8;
const PS_EXTRA_PIXEL_SHADER_VALID: u32 = 1 << 31;
const VFCOMP_STORE_SRC: u32 = 1;
const VFCOMP_STORE_0: u32 = 2;
const VFCOMP_STORE_1_FP: u32 = 3;
const PIPELINE_SELECT_3D: u32 = (4 << 16) | (1 << 24) | (1 << 27) | (3 << 29);
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC: u32 =
    2 | (25 << 16) | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VS: u32 = 7 | (16 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_GS: u32 = 8 | (17 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CLIP: u32 = 2 | (18 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SF: u32 = 2 | (19 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM: u32 = 0 | (20 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_HS: u32 = 7 | (27 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_TE: u32 = 3 | (28 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DS: u32 = 9 | (29 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_STREAMOUT: u32 = 3 | (30 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SBE: u32 = 4 | (31 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS: u32 = 10 | (32 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP: u32 = (33 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC: u32 = (35 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BLEND_STATE_POINTERS: u32 = (36 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_VS: u32 = (38 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_BINDING_TABLE_POINTERS_PS: u32 = (42 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_CC_STATE_POINTERS: u32 = (14 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS: u32 = (43 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS: u32 = (47 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_STATISTICS: u32 = (11 << 16) | (1 << 27) | (3 << 29);
const CMD_3DSTATE_VF: u32 = (12 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_MULTISAMPLE: u32 = (13 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_DRAWING_RECTANGLE: u32 = 2 | (1 << 24) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SAMPLE_MASK: u32 = (24 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS_BLEND: u32 = (77 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_WM_DEPTH_STENCIL: u32 = 2 | (78 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_PS_EXTRA: u32 = (79 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_RASTER: u32 = 3 | (80 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_SBE_SWIZ: u32 = 9 | (81 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_HS: u32 = 1 | (89 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_DS: u32 = 1 | (90 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_GS: u32 = 1 | (91 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_URB_ALLOC_VS: u32 = 1 | (88 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_INSTANCING: u32 = 1 | (73 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_SGVS: u32 = (74 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_TOPOLOGY: u32 = (75 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VF_SGVS_2: u32 = 1 | (86 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VERTEX_BUFFERS_1: u32 = 3 | (8 << 16) | (3 << 27) | (3 << 29);
const CMD_3DSTATE_VERTEX_ELEMENTS_1: u32 = 1 | (9 << 16) | (3 << 27) | (3 << 29);
const CMD_3DPRIMITIVE: u32 = 5 | (3 << 24) | (3 << 27) | (3 << 29);
const PIPE_CONTROL_FLUSH_BITS: u32 = (1 << 5) | (1 << 7) | (1 << 12) | (1 << 20) | (1 << 26);
const PIPE_CONTROL_INVALIDATE_BITS: u32 =
    (1 << 2) | (1 << 3) | (1 << 4) | (1 << 10) | (1 << 11) | (1 << 18) | (1 << 20);
const PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
const PIPE_CONTROL_DEST_GGTT: u32 = 1 << 24;
const RESULT_SLOT_PRE3D_DWORD: usize = 0;
const RESULT_SLOT_POST3D_DWORD: usize = 1;
const RESULT_SLOT_FINAL_DWORD: usize = 2;
const TRIANGLE_TOPOLOGY_TRILIST: u32 = 4;
const TRIANGLE_PS_MAX_THREADS: u32 = 63;
const TRIANGLE_VS_URB_START: u32 = 4;
const TRIANGLE_VS_URB_ENTRIES: u32 = 192;
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

#[derive(Copy, Clone)]
struct TriangleShaderStageLayout {
    code_offset_bytes: u32,
    code_gpu_addr: u64,
    ksp_offset_bytes: u32,
    ksp_gpu_addr: u64,
    code_size_bytes: u32,
}

#[derive(Copy, Clone)]
struct TriangleShaderLayout {
    vs: TriangleShaderStageLayout,
    ps: TriangleShaderStageLayout,
    state_region_gpu_addr: u64,
    state_region_offset_bytes: u32,
    used_bytes: u32,
}

#[derive(Copy, Clone)]
struct TriangleProbeStateLayout {
    binding_table_offset_bytes: u32,
    surface_state_offset_bytes: u32,
    sampler_state_offset_bytes: u32,
    blend_state_offset_bytes: u32,
    color_calc_state_offset_bytes: u32,
    cc_viewport_offset_bytes: u32,
    sf_clip_viewport_offset_bytes: u32,
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
    if !submit_result_store_probe(dev, warm) {
        crate::log!("intel/render: primary-triangle skipped reason=mi-store-probe\n");
        return;
    }
    if !submit_3d_no_draw_probe(dev, warm) {
        crate::log!("intel/render: primary-triangle skipped reason=3d-no-draw-probe\n");
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
    dev: crate::intel::Dev,
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

    let pipeline = crate::intel::shader::triangle_pipeline();
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
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
    }

    crate::log!(
        "intel/render: ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} note={}\n",
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        crate::intel::shader::triangle_pipeline_note()
    );

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: draw-path staging skipped reason=shader-layout-error detail={} note={}\n",
                reason,
                crate::intel::shader::triangle_pipeline_note()
            );
            return false;
        }
    };

    crate::log!(
        "intel/render: draw-path staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state = match write_triangle_probe_state(warm, draw, shader_layout) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: draw-path staging skipped reason=probe-state-error detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(1), 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(2), 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        batch,
        warm,
        draw,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: draw-path staging skipped reason=probe-batch-error detail={}\n",
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    crate::log!(
        "intel/render: draw-path batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X}\n",
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes
    );
    log_triangle_probe_state(warm, shader_layout, probe_state);

    submit_warm_render_batch(dev, warm, RCS_EXEC_RESULT_DONE, RESULT_SLOT_FINAL_DWORD, "draw-path")
}

fn submit_result_store_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
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
    let Ok(batch_tail_bytes) =
        encode_result_store_probe_batch(batch, GPU_VA_RESULT_BASE, RCS_EXEC_RESULT_MI_PROBE_DONE)
    else {
        crate::log!("intel/render: mi-store-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-store-probe",
    )
}

fn submit_3d_no_draw_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(1), 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(2), 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE + (RESULT_SLOT_POST3D_DWORD as u64) * 4,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
    ) else {
        crate::log!("intel/render: 3d-no-draw-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        RESULT_SLOT_POST3D_DWORD,
        "3d-no-draw",
    )
}

fn log_render_buffer_layout(warm: RenderWarmState, rt_gpu_addr: Option<u64>) {
    let rt_gpu_addr = rt_gpu_addr.unwrap_or(0);
    crate::log!(
        "intel/render: buffers ring phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} context phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} batch phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} result phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} state phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} vertex phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rt_ggtt=0x{:X}\n",
        warm.ring_phys,
        GPU_VA_RING_BASE,
        warm.ring_len,
        warm.context_phys,
        GPU_VA_CONTEXT_BASE,
        warm.context_len,
        warm.batch_phys,
        GPU_VA_BATCH_BASE,
        warm.batch_len,
        warm.result_phys,
        GPU_VA_RESULT_BASE,
        warm.result_len,
        warm.draw_state_phys,
        GPU_VA_DRAW_STATE_BASE,
        warm.draw_state_len,
        warm.vertex_phys,
        GPU_VA_VERTEX_BASE,
        warm.vertex_len,
        rt_gpu_addr
    );
}

fn log_render_packet_encodings() {
    let (ctx_desc_lo, ctx_desc_hi) = build_execlist_context_descriptor(GPU_VA_CONTEXT_BASE);
    crate::log!(
        "intel/render: encodings mi_store_data_imm=0x{:08X} ctx_desc=0x{:08X}:0x{:08X} state_base_address=0x{:08X} pipe_control=0x{:08X} pc_post_sync_immediate=0x{:08X} pc_dest_ggtt=0x{:08X}\n",
        MI_STORE_DATA_IMM_GGTT_DW1,
        ctx_desc_hi,
        ctx_desc_lo,
        STATE_BASE_ADDRESS_CMD,
        PIPE_CONTROL_CMD,
        PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
        PIPE_CONTROL_DEST_GGTT
    );
}
fn log_triangle_probe_state(
    warm: RenderWarmState,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
) {
    let dwords = unsafe {
        core::slice::from_raw_parts(warm.draw_state_virt as *const u32, warm.draw_state_len / 4)
    };
    let bt_ptr = probe_state
        .binding_table_offset_bytes
        .saturating_sub(shader_layout.state_region_offset_bytes);
    let bt_entry = dwords[probe_state.binding_table_offset_bytes as usize / 4];
    let surface = &dwords[probe_state.surface_state_offset_bytes as usize / 4
        ..probe_state.surface_state_offset_bytes as usize / 4 + 16];
    let blend = &dwords[probe_state.blend_state_offset_bytes as usize / 4
        ..probe_state.blend_state_offset_bytes as usize / 4 + 16];
    let color_calc = &dwords[probe_state.color_calc_state_offset_bytes as usize / 4
        ..probe_state.color_calc_state_offset_bytes as usize / 4 + 16];
    crate::log!(
        "intel/render: probe-state bt_off=0x{:X} bt_entry0=0x{:08X} surf_off=0x{:X} ps_ptr=bt:0x{:X} blend_ptr=0x{:X} cc_ptr=0x{:X}\n",
        probe_state.binding_table_offset_bytes,
        bt_entry,
        probe_state.surface_state_offset_bytes,
        bt_ptr,
        probe_state.blend_state_offset_bytes | 1,
        probe_state.color_calc_state_offset_bytes | 1
    );
    crate::log!(
        "intel/render: probe-surface d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        surface[0],
        surface[1],
        surface[2],
        surface[3],
        surface[4],
        surface[5],
        surface[6],
        surface[7]
    );
    crate::log!(
        "intel/render: probe-surface d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        surface[8],
        surface[9],
        surface[10],
        surface[11],
        surface[12],
        surface[13],
        surface[14],
        surface[15]
    );
    crate::log!(
        "intel/render: probe-blend d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        blend[0],
        blend[1],
        blend[2],
        blend[3],
        blend[4],
        blend[5],
        blend[6],
        blend[7]
    );
    crate::log!(
        "intel/render: probe-blend d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        blend[8],
        blend[9],
        blend[10],
        blend[11],
        blend[12],
        blend[13],
        blend[14],
        blend[15]
    );
    crate::log!(
        "intel/render: probe-cc d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        color_calc[0],
        color_calc[1],
        color_calc[2],
        color_calc[3],
        color_calc[4],
        color_calc[5],
        color_calc[6],
        color_calc[7]
    );
    crate::log!(
        "intel/render: probe-cc d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        color_calc[8],
        color_calc[9],
        color_calc[10],
        color_calc[11],
        color_calc[12],
        color_calc[13],
        color_calc[14],
        color_calc[15]
    );
}

fn write_triangle_probe_state(
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    shader_layout: TriangleShaderLayout,
) -> Result<TriangleProbeStateLayout, &'static str> {
    let mut cursor = shader_layout.state_region_offset_bytes as usize;
    let binding_table_offset = cursor;
    cursor = crate::intel::align_up(binding_table_offset + 4, 64).ok_or("probe-state-align")?;
    let surface_state_offset = cursor;
    cursor = crate::intel::align_up(surface_state_offset + 64, 32).ok_or("probe-state-align")?;
    let sampler_state_offset = cursor;
    cursor = crate::intel::align_up(sampler_state_offset + 16, 64).ok_or("probe-state-align")?;
    let blend_state_offset = cursor;
    cursor = crate::intel::align_up(blend_state_offset + 64, 64).ok_or("probe-state-align")?;
    let color_calc_state_offset = cursor;
    cursor = crate::intel::align_up(color_calc_state_offset + 64, 64).ok_or("probe-state-align")?;
    let cc_viewport_offset = cursor;
    cursor = crate::intel::align_up(cc_viewport_offset + 8, 64).ok_or("probe-state-align")?;
    let sf_clip_viewport_offset = cursor;
    let end_offset = sf_clip_viewport_offset
        .checked_add(64)
        .ok_or("probe-state-overflow")?;
    if end_offset > warm.draw_state_len {
        return Err("probe-state-exceeds-state-bo");
    }

    let dwords = unsafe {
        core::slice::from_raw_parts_mut(warm.draw_state_virt as *mut u32, warm.draw_state_len / 4)
    };
    dwords[binding_table_offset / 4] = surface_state_offset as u32;

    let surface = &mut dwords[surface_state_offset / 4..surface_state_offset / 4 + 16];
    surface.fill(0);
    surface[0] = (SURFTYPE_2D << 29)
        | (SURFACE_FORMAT_R8G8B8A8_UNORM << 18)
        | (SURFACE_HALIGN_4 << 14)
        | (SURFACE_VALIGN_4 << 16);
    surface[1] = RENDER_MOCS << 24;
    surface[2] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);
    surface[3] = draw.rt_pitch.saturating_sub(1);
    surface[7] = (SHADER_CHANNEL_ALPHA << 16)
        | (SHADER_CHANNEL_BLUE << 19)
        | (SHADER_CHANNEL_GREEN << 22)
        | (SHADER_CHANNEL_RED << 25);
    surface[8] = draw.rt_gpu_addr as u32;
    surface[9] = (draw.rt_gpu_addr >> 32) as u32;

    let sampler = &mut dwords[sampler_state_offset / 4..sampler_state_offset / 4 + 4];
    sampler.fill(0);

    let blend = &mut dwords[blend_state_offset / 4..blend_state_offset / 4 + 16];
    blend.fill(0);

    let color_calc = &mut dwords[color_calc_state_offset / 4..color_calc_state_offset / 4 + 16];
    color_calc.fill(0);

    let cc_viewport = &mut dwords[cc_viewport_offset / 4..cc_viewport_offset / 4 + 2];
    cc_viewport[0] = 0.0f32.to_bits();
    cc_viewport[1] = 1.0f32.to_bits();

    let sf_clip_viewport =
        &mut dwords[sf_clip_viewport_offset / 4..sf_clip_viewport_offset / 4 + 16];
    sf_clip_viewport.fill(0);
    sf_clip_viewport[0] = (draw.target_w as f32 * 0.5).to_bits();
    sf_clip_viewport[1] = (-(draw.target_h as f32) * 0.5).to_bits();
    sf_clip_viewport[2] = 1.0f32.to_bits();
    sf_clip_viewport[3] = (draw.target_w as f32 * 0.5).to_bits();
    sf_clip_viewport[4] = (draw.target_h as f32 * 0.5).to_bits();
    sf_clip_viewport[5] = 0.0f32.to_bits();
    sf_clip_viewport[8] = (-32768.0f32).to_bits();
    sf_clip_viewport[9] = 32768.0f32.to_bits();
    sf_clip_viewport[10] = (-32768.0f32).to_bits();
    sf_clip_viewport[11] = 32768.0f32.to_bits();

    let flush_ptr = unsafe {
        warm.draw_state_virt
            .add(shader_layout.state_region_offset_bytes as usize)
    };
    crate::intel::dma_flush(
        flush_ptr,
        end_offset - shader_layout.state_region_offset_bytes as usize,
    );

    Ok(TriangleProbeStateLayout {
        binding_table_offset_bytes: binding_table_offset as u32,
        surface_state_offset_bytes: surface_state_offset as u32,
        sampler_state_offset_bytes: sampler_state_offset as u32,
        blend_state_offset_bytes: blend_state_offset as u32,
        color_calc_state_offset_bytes: color_calc_state_offset as u32,
        cc_viewport_offset_bytes: cc_viewport_offset as u32,
        sf_clip_viewport_offset_bytes: sf_clip_viewport_offset as u32,
    })
}

fn encode_triangle_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;

    fn log_batch_offset(cursor: usize, label: &str) {
        crate::log!(
            "intel/render: batch-off 0x{:03X} {}\n",
            cursor * core::mem::size_of::<u32>(),
            label
        );
    }

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("probe-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn sampler_count_encoding(count: u8) -> u32 {
        match count {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            9..=12 => 3,
            _ => 4,
        }
    }

    fn binding_table_entry_count_encoding(count: u8) -> u32 {
        count.saturating_sub(1) as u32
    }

    fn stage_dispatch_bits(mode: crate::intel::shader::DispatchMode) -> (u32, u32, u32) {
        match mode {
            crate::intel::shader::DispatchMode::Simd8 => (1, 0, 0),
            crate::intel::shader::DispatchMode::Simd16 => (0, 1, 0),
            crate::intel::shader::DispatchMode::Simd32 => (0, 0, 1),
        }
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("probe-pipe-control-header");
        }
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control_post_sync_write(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        immediate_data: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(
            batch_dwords,
            cursor,
            flags_dw1 | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT,
        )?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, immediate_data)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes = crate::intel::align_up(size_bytes, 4096).ok_or("probe-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "probe-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    let binding_table_pool_size = warm
        .draw_state_len
        .saturating_sub(shader_layout.state_region_offset_bytes as usize);
    let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
    let ps_ksp_offset = shader_layout.ps.code_offset_bytes + shader_layout.ps.ksp_offset_bytes;
    let sbe_vertex_read_length = (((pipeline.ps.meta.num_varying_inputs as u32) + 1) / 2).max(1);
    let (ps_dispatch_8, ps_dispatch_16, ps_dispatch_32) =
        stage_dispatch_bits(pipeline.ps.meta.kernel.dispatch_mode);
    let clip_dw1 = 0;
    let clip_dw2 = CLIP_PERSPECTIVE_DIVIDE_DISABLE;
    let clip_dw3 = 0;
    let wm_dw1 = WM_FORCE_KILL_PIXEL_OFF;
    let ps_dw3 =
        (binding_table_entry_count_encoding(pipeline.ps.meta.kernel.binding_table_entry_count)
            << 18)
            | (sampler_count_encoding(pipeline.ps.meta.kernel.sampler_count) << 27)
            | (u32::from(pipeline.ps.meta.uses_vmask) * PS_VECTOR_MASK_ENABLE)
            | PS_SINGLE_PROGRAM_FLOW;
    let ps_dw6 = ps_dispatch_8
        | (ps_dispatch_16 << 1)
        | (ps_dispatch_32 << 2)
        | (u32::from(pipeline.ps.meta.kernel.push_constant_bytes > 0) * PS_PUSH_CONSTANT_ENABLE)
        | ((TRIANGLE_PS_MAX_THREADS as u32) << PS_MAX_THREADS_SHIFT);
    let ps_dw7 = (pipeline.ps.meta.kernel.grf_start_register as u32)
        | ((pipeline.ps.meta.kernel.grf_start_register as u32) << 8)
        | ((pipeline.ps.meta.kernel.grf_start_register as u32) << 16);
    let ps_extra_dw1 = (u32::from(pipeline.ps.meta.computed_stencil)
        * PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL)
        | (u32::from(pipeline.ps.meta.persample_dispatch) * PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE)
        | (u32::from(pipeline.ps.meta.num_varying_inputs > 0) * PS_EXTRA_ATTRIBUTE_ENABLE)
        | ((pipeline.ps.meta.computed_depth_mode as u32) << 26)
        | PS_EXTRA_PIXEL_SHADER_VALID;

    batch_dwords.fill(0);

    log_batch_offset(cursor, "PIPE_CONTROL flush");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    log_batch_offset(cursor, "PIPE_CONTROL invalidate");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "PIPELINE_SELECT");
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POOL_ALLOC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC)?;
    push_addr(batch_dwords, &mut cursor, shader_layout.state_region_gpu_addr)?;
    push(
        batch_dwords,
        &mut cursor,
        (u32::try_from(
            crate::intel::align_up(binding_table_pool_size, 4096)
                .ok_or("probe-binding-pool-align")?,
        )
        .map_err(|_| "probe-binding-pool-convert")?
            & 0xFFFF_F000),
    )?;

    log_batch_offset(cursor, "3DSTATE_SAMPLER_STATE_POINTERS_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_SAMPLER_STATE_POINTERS_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS)?;
    push(batch_dwords, &mut cursor, probe_state.sampler_state_offset_bytes)?;

    let binding_table_pool_base_offset = shader_layout.state_region_offset_bytes;
    let binding_table_pointer_offset = probe_state
        .binding_table_offset_bytes
        .saturating_sub(binding_table_pool_base_offset);

    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
    push(batch_dwords, &mut cursor, binding_table_pointer_offset)?;

    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_CC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC)?;
    push(batch_dwords, &mut cursor, probe_state.cc_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
    push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_BUFFERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_BUFFERS_1)?;
    push(batch_dwords, &mut cursor, draw.vertex_stride | (1 << 14) | (RENDER_MOCS << 16))?;
    push_addr(batch_dwords, &mut cursor, draw.vertex_gpu_addr)?;
    push(batch_dwords, &mut cursor, draw.vertex_count.saturating_mul(draw.vertex_stride))?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_ELEMENTS_1)?;
    push(batch_dwords, &mut cursor, (SURFACE_FORMAT_R32G32B32_FLOAT << 16) | (1 << 25))?;
    push(
        batch_dwords,
        &mut cursor,
        (VFCOMP_STORE_SRC << 28)
            | (VFCOMP_STORE_SRC << 24)
            | (VFCOMP_STORE_SRC << 20)
            | (VFCOMP_STORE_1_FP << 16),
    )?;

    log_batch_offset(cursor, "3DSTATE_VF_STATISTICS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_STATISTICS)?;
    log_batch_offset(cursor, "3DSTATE_VF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS_2");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS_2)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
    push(batch_dwords, &mut cursor, TRIANGLE_TOPOLOGY_TRILIST)?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(
        batch_dwords,
        &mut cursor,
        pipeline.vs.meta.urb_entry_output_length as u32
            | (TRIANGLE_VS_URB_START << 10)
            | (TRIANGLE_VS_URB_START << 21),
    )?;
    push(batch_dwords, &mut cursor, TRIANGLE_VS_URB_ENTRIES | (TRIANGLE_VS_URB_ENTRIES << 16))?;

    log_batch_offset(cursor, "3DSTATE_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
    push(batch_dwords, &mut cursor, vs_ksp_offset & !0x3F)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27),
    )?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        (1 << 11) | ((pipeline.vs.meta.kernel.grf_start_register as u32) << 20),
    )?;
    push(batch_dwords, &mut cursor, 1 | (1 << 2) | ((pipeline.vs.meta.max_threads as u32) << 22))?;
    push(batch_dwords, &mut cursor, ((pipeline.vs.meta.urb_entry_output_length as u32) << 16))?;

    log_batch_offset(cursor, "3DSTATE_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HS)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_TE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
    for _ in 0..4 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DS)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_STREAMOUT");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
    for _ in 0..4 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
    for _ in 0..9 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    log_batch_offset(cursor, "3DSTATE_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLIP)?;
    push(batch_dwords, &mut cursor, clip_dw1)?;
    push(batch_dwords, &mut cursor, clip_dw2)?;
    push(batch_dwords, &mut cursor, clip_dw3)?;

    log_batch_offset(cursor, "3DSTATE_SF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SF)?;
    push(batch_dwords, &mut cursor, (1 << 1) | (128 << 12))?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_RASTER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_RASTER)?;
    push(batch_dwords, &mut cursor, (1 << 16) | (1 << 18) | (1 << 21) | (2 << 22))?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_SBE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
    push(
        batch_dwords,
        &mut cursor,
        (1 << 5)
            | (sbe_vertex_read_length << 11)
            | ((pipeline.ps.meta.num_varying_inputs as u32) << 22)
            | (1 << 28)
            | (1 << 29),
    )?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, pipeline.ps.meta.flat_inputs)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

    // Gen12/Xe-LP keeps attribute swizzle state in a separate packet.
    log_batch_offset(cursor, "3DSTATE_SBE_SWIZ");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    log_batch_offset(cursor, "3DSTATE_WM");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
    push(batch_dwords, &mut cursor, wm_dw1)?;

    log_batch_offset(cursor, "3DSTATE_WM_DEPTH_STENCIL");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_DEPTH_STENCIL)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_CC_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CC_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, probe_state.color_calc_state_offset_bytes | 1)?;

    log_batch_offset(cursor, "3DSTATE_BLEND_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BLEND_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, probe_state.blend_state_offset_bytes | 1)?;

    log_batch_offset(cursor, "3DSTATE_PS_BLEND");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
    push(batch_dwords, &mut cursor, 1 << 30)?;

    log_batch_offset(cursor, "3DSTATE_MULTISAMPLE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_MULTISAMPLE)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
    push(batch_dwords, &mut cursor, 1)?;

    log_batch_offset(cursor, "3DSTATE_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
    push(batch_dwords, &mut cursor, ps_ksp_offset & !0x3F)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, ps_dw3)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, ps_dw6)?;
    push(batch_dwords, &mut cursor, ps_dw7)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_PS_EXTRA");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
    push(batch_dwords, &mut cursor, ps_extra_dw1)?;

    log_batch_offset(cursor, "3DSTATE_DRAWING_RECTANGLE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DRAWING_RECTANGLE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16),
    )?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-3d");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE3D_DWORD as u64) * 4,
        pre3d_value,
    )?;

    log_batch_offset(cursor, "3DPRIMITIVE");
    push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE)?;
    push(batch_dwords, &mut cursor, TRIANGLE_TOPOLOGY_TRILIST)?;
    push(batch_dwords, &mut cursor, draw.vertex_count)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "PIPE_CONTROL post-draw flush");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;

    log_batch_offset(cursor, "PIPE_CONTROL post-sync marker");
    push_pipe_control_post_sync_write(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST3D_DWORD as u64) * 4,
        post3d_value,
        PIPE_CONTROL_FLUSH_BITS,
    )?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_DWORD as u64) * 4,
        done_value,
    )?;
    log_batch_offset(cursor, "MI_BATCH_BUFFER_END");
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    crate::log!(
        "intel/render: probe-ps clip=[0x{:08X},0x{:08X},0x{:08X}] wm=0x{:08X} ps3=0x{:08X} ps6=0x{:08X} ps7=0x{:08X} ps_extra=0x{:08X}\n",
        clip_dw1,
        clip_dw2,
        clip_dw3,
        wm_dw1,
        ps_dw3,
        ps_dw6,
        ps_dw7,
        ps_extra_dw1
    );

    Ok(cursor * core::mem::size_of::<u32>())
}

fn encode_3d_no_draw_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("3d-no-draw-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("3d-no-draw-pipe-control-header");
        }
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes = crate::intel::align_up(size_bytes, 4096).ok_or("3d-no-draw-sba-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "3d-no-draw-sba-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    push_pipe_control_full(batch_dwords, &mut cursor, 0, PIPE_CONTROL_FLUSH_BITS)?;
    push_pipe_control_full(batch_dwords, &mut cursor, 0, PIPE_CONTROL_INVALIDATE_BITS)?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    push(batch_dwords, &mut cursor, PIPE_CONTROL_CMD)?;
    push(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_FLUSH_BITS | PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE | PIPE_CONTROL_DEST_GGTT,
    )?;
    push_addr(batch_dwords, &mut cursor, result_gpu_addr)?;
    push(batch_dwords, &mut cursor, done_value)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

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

    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-triangle",
    )
}

fn submit_warm_render_batch(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    expected_result: u32,
    expected_result_slot_dword: usize,
    submit_name: &'static str,
) -> bool {
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
    write_lrc_ring_tail(warm, ring_tail_bytes as u32);
    let pphwsp_gpu = (GPU_VA_CONTEXT_BASE & !0xFFF) as u32;

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
    crate::intel::mmio_write(dev, RCS_RING_HWS_PGA, pphwsp_gpu);
    let hws_after = crate::intel::mmio_read(dev, RCS_RING_HWS_PGA);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

    crate::log!(
        "intel/render: {} execlist-start desc=0x{:08X}:0x{:08X} hws=0x{:08X} sq=0x{:08X}:0x{:08X} ctx_ctl=0x{:08X}\n",
        submit_name,
        context_desc_hi,
        context_desc_lo,
        hws_after,
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_HI),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_LO),
        crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL)
    );

    let mut completed = false;
    let mut iter = 0usize;
    while iter < 4096 {
        let result0 = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
        let result1 = read_result_dword(warm, RESULT_SLOT_POST3D_DWORD);
        let result2 = read_result_dword(warm, RESULT_SLOT_FINAL_DWORD);
        let observed = match expected_result_slot_dword {
            RESULT_SLOT_PRE3D_DWORD => result0,
            RESULT_SLOT_POST3D_DWORD => result1,
            RESULT_SLOT_FINAL_DWORD => result2,
            _ => result0,
        };
        if observed == expected_result {
            completed = true;
            break;
        }
        if iter == 0 || iter == 256 || iter == 1024 || iter == 4095 {
            crate::log!(
                "intel/render: {} poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} result0=0x{:08X} result1=0x{:08X} result2=0x{:08X}\n",
                submit_name,
                iter,
                crate::intel::mmio_read(dev, RCS_RING_HEAD),
                crate::intel::mmio_read(dev, RCS_RING_TAIL),
                crate::intel::mmio_read(dev, RCS_RING_ACTHD),
                crate::intel::mmio_read(dev, RCS_RING_IPEIR),
                crate::intel::mmio_read(dev, RCS_RING_IPEHR),
                crate::intel::mmio_read(dev, RCS_RING_EIR),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_HI),
                result0,
                result1,
                result2
            );
        }
        core::hint::spin_loop();
        iter += 1;
    }

    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let result0 = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
    let result1 = read_result_dword(warm, RESULT_SLOT_POST3D_DWORD);
    let result2 = read_result_dword(warm, RESULT_SLOT_FINAL_DWORD);
    crate::log!(
        "intel/render: {} complete={} result0=0x{:08X} result1=0x{:08X} result2=0x{:08X} ctl=0x{:08X} instdone=0x{:08X}\n",
        submit_name,
        completed as u8,
        result0,
        result1,
        result2,
        crate::intel::mmio_read(dev, RCS_RING_CTL),
        crate::intel::mmio_read(dev, RCS_RING_INSTDONE)
    );
    crate::intel::display::log_primary_surface_samples("post-render");
    completed
}

fn read_result_dword(warm: RenderWarmState, index: usize) -> u32 {
    unsafe { core::ptr::read_volatile((warm.result_virt as *const u32).add(index)) }
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
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_hi);
}

fn write_lrc_ring_tail(warm: RenderWarmState, ring_tail: u32) {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + 3 {
        return;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + 3] = ring_tail;
    crate::intel::dma_flush(warm.context_virt, warm.context_len);
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

fn encode_result_store_probe_batch(
    batch_dwords: &mut [u32],
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    if batch_dwords.len() < 6 {
        return Err("batch-too-small");
    }

    batch_dwords.fill(0);
    batch_dwords[0] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[1] = result_gpu_addr as u32;
    batch_dwords[2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[3] = done_value;
    batch_dwords[4] = MI_BATCH_BUFFER_END;
    batch_dwords[5] = MI_NOOP;
    Ok(6 * core::mem::size_of::<u32>())
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
