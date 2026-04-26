#[derive(Copy, Clone, Debug, Default)]
struct TriangleStageStats {
    ia_vertices: u64,
    ia_primitives: u64,
    vs_invocations: u64,
    hs_invocations: u64,
    ds_invocations: u64,
    gs_invocations: u64,
    gs_primitives: u64,
    cl_invocations: u64,
    cl_primitives: u64,
    ps_invocations: u64,
    cps_invocations: u64,
    ps_depth: u64,
    so_prims_written_0: u64,
    so_write_offset_0: u64,
}

impl TriangleStageStats {
    fn delta_since(self, before: Self) -> Self {
        Self {
            ia_vertices: self.ia_vertices.saturating_sub(before.ia_vertices),
            ia_primitives: self.ia_primitives.saturating_sub(before.ia_primitives),
            vs_invocations: self.vs_invocations.saturating_sub(before.vs_invocations),
            hs_invocations: self.hs_invocations.saturating_sub(before.hs_invocations),
            ds_invocations: self.ds_invocations.saturating_sub(before.ds_invocations),
            gs_invocations: self.gs_invocations.saturating_sub(before.gs_invocations),
            gs_primitives: self.gs_primitives.saturating_sub(before.gs_primitives),
            cl_invocations: self.cl_invocations.saturating_sub(before.cl_invocations),
            cl_primitives: self.cl_primitives.saturating_sub(before.cl_primitives),
            ps_invocations: self.ps_invocations.saturating_sub(before.ps_invocations),
            cps_invocations: self.cps_invocations.saturating_sub(before.cps_invocations),
            ps_depth: self.ps_depth.saturating_sub(before.ps_depth),
            so_prims_written_0: self
                .so_prims_written_0
                .saturating_sub(before.so_prims_written_0),
            so_write_offset_0: self
                .so_write_offset_0
                .saturating_sub(before.so_write_offset_0),
        }
    }
}

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
    pub streamout_phys: u64,
    pub streamout_virt: *mut u8,
    pub streamout_len: usize,
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
struct TriangleVertexUploadProof {
    vertex_count: u32,
    vertex_stride: u32,
    byte_len: usize,
    gpu_addr: u64,
    signed_area_2x: f32,
    cpu_readback_ok: bool,
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
    scissor_rect_offset_bytes: u32,
    slice_hash_table_offset_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct TriangleFrontEndContract {
    label: &'static str,
    vs_urb_output_length_override: Option<u8>,
    sbe_read_offset: u8,
    sbe_read_length: u8,
    force_sbe_read_offset: bool,
    force_sbe_read_length: bool,
}

#[derive(Copy, Clone)]
struct VsStreamoutProofConfig {
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
}

#[derive(Copy, Clone)]
enum TriangleBlendProbeMode {
    ExplicitRt0,
    MesaZeroedState,
    MesaZeroedNoBlendPointer,
}

impl TriangleBlendProbeMode {
    fn for_attempt(attempt: usize) -> Self {
        match attempt {
            1 => Self::MesaZeroedState,
            2 => Self::ExplicitRt0,
            3 => Self::MesaZeroedNoBlendPointer,
            _ => Self::MesaZeroedState,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ExplicitRt0 => "explicit-rt0",
            Self::MesaZeroedState => "mesa-zeroed",
            Self::MesaZeroedNoBlendPointer => "mesa-zeroed-no-blend-ptr",
        }
    }

    fn blend_state_pointer_dword(self, offset_bytes: u32) -> u32 {
        match self {
            Self::MesaZeroedNoBlendPointer => 0,
            _ => offset_bytes | 1,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TriangleBatchMode {
    Draw,
    VfDraw,
    StreamoutProof,
    VfStreamoutProof,
    VsStreamoutProof,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum StreamoutProofExperiment {
    PositionSlot0,
    PositionSlot1,
    HeaderAndPositionSlots01,
}

const CMD_3DSTATE_VERTEX_ELEMENTS_2: u32 = 3 | (9 << 16) | (3 << 27) | (3 << 29);

impl StreamoutProofExperiment {
    fn label(self) -> &'static str {
        match self {
            Self::PositionSlot0 => "pos-slot0",
            Self::PositionSlot1 => "pos-slot1",
            Self::HeaderAndPositionSlots01 => "header+pos-slots01",
        }
    }

    fn alternate(self) -> Self {
        match self {
            Self::PositionSlot0 => Self::PositionSlot1,
            Self::PositionSlot1 => Self::HeaderAndPositionSlots01,
            Self::HeaderAndPositionSlots01 => Self::PositionSlot0,
        }
    }

    fn vertex_bytes(self) -> usize {
        match self {
            Self::PositionSlot0 | Self::PositionSlot1 => 16,
            Self::HeaderAndPositionSlots01 => 32,
        }
    }

    fn vertex_read_length(self) -> u32 {
        1
    }

    fn so_decl_header(self) -> u32 {
        match self {
            Self::PositionSlot0 | Self::PositionSlot1 => {
                3 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29)
            }
            Self::HeaderAndPositionSlots01 => 5 | (23 << 16) | (1 << 24) | (3 << 27) | (3 << 29),
        }
    }

    fn so_decl_buffer_selects(self) -> u32 {
        1
    }

    fn so_decl_num_entries(self) -> u32 {
        match self {
            Self::PositionSlot0 | Self::PositionSlot1 => 1,
            Self::HeaderAndPositionSlots01 => 2,
        }
    }

    fn so_decl_entry_dwords(self) -> [u32; 4] {
        match self {
            Self::PositionSlot0 => [0x0000_000F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::PositionSlot1 => [0x0000_001F, 0x0000_0000, 0x0000_0000, 0x0000_0000],
            Self::HeaderAndPositionSlots01 => [0x0000_000F, 0x0000_0000, 0x0000_001F, 0x0000_0000],
        }
    }

    fn compatible(self) -> bool {
        true
    }

    fn vf_slot_contract(self) -> &'static str {
        match self {
            Self::PositionSlot0 => "slot0=position",
            Self::PositionSlot1 => "slot0=zero slot1=position",
            Self::HeaderAndPositionSlots01 => "slot0=header slot1=position",
        }
    }

    fn vf_vertex_element_count(self) -> usize {
        match self {
            Self::PositionSlot0 => 1,
            Self::PositionSlot1 | Self::HeaderAndPositionSlots01 => 2,
        }
    }
}

fn select_streamout_proof_experiment(probe_seq: u32) -> StreamoutProofExperiment {
    match probe_seq % 3 {
        0 => StreamoutProofExperiment::PositionSlot1,
        1 => StreamoutProofExperiment::HeaderAndPositionSlots01,
        _ => StreamoutProofExperiment::PositionSlot0,
    }
}

impl TriangleBatchMode {
    fn topology(self) -> u32 {
        match self {
            Self::Draw | Self::VfDraw => TRIANGLE_TOPOLOGY_TRILIST,
            Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof => {
                TRIANGLE_TOPOLOGY_POINTLIST
            }
        }
    }

    fn streamout_enabled(self) -> bool {
        matches!(self, Self::StreamoutProof | Self::VfStreamoutProof | Self::VsStreamoutProof)
    }
}

fn is_streamout_submit_name(submit_name: &str) -> bool {
    matches!(submit_name, "streamout-proof" | "vf-streamout-proof" | "vs-streamout-proof")
}

fn is_vf_streamout_submit_name(submit_name: &str) -> bool {
    submit_name == "vf-streamout-proof"
}

fn is_triangle_debug_submit_name(submit_name: &str) -> bool {
    is_surface_draw_submit_name(submit_name) || is_streamout_submit_name(submit_name)
}

fn is_surface_draw_submit_name(submit_name: &str) -> bool {
    matches!(submit_name, "draw-path" | "vf-draw-path" | "vs-draw-frontier")
}

unsafe impl Send for RenderWarmState {}
unsafe impl Sync for RenderWarmState {}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);
static PRIMARY_TRIANGLE_SUBMITTED: AtomicBool = AtomicBool::new(false);
static PRIMARY_PROBE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static WARM_BUFFERS_MAPPED: AtomicBool = AtomicBool::new(false);
static PRIMARY_STRIPE_X_PHASE: AtomicU32 = AtomicU32::new(0);
static PRIMARY_PROBE_SEQ: AtomicU32 = AtomicU32::new(0);
