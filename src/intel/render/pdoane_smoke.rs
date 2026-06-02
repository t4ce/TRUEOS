const PDOANE_SMOKE_ENABLED: bool = true;
const PDOANE_URB_PAYLOAD_PROOF_ENABLED: bool = false;
const PDOANE_TARGETED_URB_PAYLOAD_PROOF_ENABLED: bool = true;
const PDOANE_SMOKE_PERIODIC_ENABLED: bool = false;
const PDOANE_ACTIVE_MESA_CONTROL_DRAW: bool = true;
const PDOANE_ACTIVE_VF_SYNTHESIZED_CONTROL_DRAW: bool = true;
const PDOANE_ACTIVE_VF_SYNTHESIZED_EXPERIMENT: StreamoutProofExperiment =
    StreamoutProofExperiment::MesaNoVsRectlist;
const PDOANE_ACTIVE_VF_SYNTHESIZED_GEOMETRY: VfPrimitiveGeometry =
    VfPrimitiveGeometry::ScreenSpaceRectTarget;
const PDOANE_ACTIVE_VF_SYNTHESIZED_BACKEND: BackendProbeMode =
    BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend;
const PDOANE_SMOKE_SUBMIT_NAME: &str = "pdoane-smoke";
const PDOANE_VF_STREAMOUT_SUBMIT_NAME: &str = "pdoane-vf-streamout-proof";
const PDOANE_SMOKE_VERBOSE_LOGS: bool = true;
const PDOANE_STREAMOUT_SENTINEL: u32 = 0xDEAD_BEEF;
const PDOANE_VS_HDC_STORE_MARKER: u32 = 0xC0DE_772A;
const PDOANE_VS_HDC_BTI34_STORE_MARKER: u32 = 0xC0DE_7734;
const PDOANE_VS_HDC_STORE_TS_EOT_MARKER: u32 = 0xC0DE_7733;
const PDOANE_VS_HDC_BTI34_BINDING_TABLE_INDEX: usize = 0x34;
const PDOANE_VS_HDC_BTI34_BINDING_TABLE_ENTRIES: usize =
    PDOANE_VS_HDC_BTI34_BINDING_TABLE_INDEX + 1;
const PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES: usize = 0x3400;
const PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES: usize = 0x3500;
const PDOANE_VS_HDC_BTI34_SURFACE_DWORDS: usize = 16;
const PDOANE_SURFTYPE_BUFFER: u32 = 4;
const PDOANE_SURFACE_FORMAT_RAW: u32 = 0x1FF;
static PDOANE_SHADER_UPLOAD_VERIFY_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static PDOANE_WM_PSD_LIVE_REGS_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
const PDOANE_VERTEX_DWORDS: usize = 4;
const PDOANE_VERTEX_STRIDE: usize = PDOANE_VERTEX_DWORDS * core::mem::size_of::<f32>();
const PDOANE_MESA_VERTEX_DWORDS: usize = 3;
const PDOANE_MESA_VERTEX_STRIDE: usize =
    PDOANE_MESA_VERTEX_DWORDS * core::mem::size_of::<f32>();
const PDOANE_VERTICES: [[f32; PDOANE_VERTEX_DWORDS]; TRIANGLE_DRAW_VERTICES] = [
    [0.0, 0.5, 0.5, 1.0],
    [0.5, -0.5, 0.5, 1.0],
    [-0.5, -0.5, 0.5, 1.0],
];

fn pdoane_smoke_quiet_periodic(reason: &str) -> bool {
    !PDOANE_SMOKE_VERBOSE_LOGS && reason == "periodic-render-60hz"
}

fn prepare_pdoane_vs_hdc_bti34_surface(warm: RenderWarmState) -> bool {
    let binding_table_bytes =
        PDOANE_VS_HDC_BTI34_BINDING_TABLE_ENTRIES * core::mem::size_of::<u32>();
    let surface_bytes = PDOANE_VS_HDC_BTI34_SURFACE_DWORDS * core::mem::size_of::<u32>();
    let binding_end =
        PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES.saturating_add(binding_table_bytes);
    let surface_end = PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES.saturating_add(surface_bytes);
    let ready = PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES & 0x3F == 0
        && PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES & 0x3F == 0
        && binding_end <= warm.draw_state_len
        && surface_end <= warm.draw_state_len;
    if !ready {
        intel_render_focus_log!(
            "intel/render: pdoane-vs-hdc-bti34-surface ready=0 bt_off=0x{:X} bt_bytes=0x{:X} surf_off=0x{:X} surf_bytes=0x{:X} draw_state_len=0x{:X}\n",
            PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES,
            binding_table_bytes,
            PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES,
            surface_bytes,
            warm.draw_state_len,
        );
        return false;
    }

    let target_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_EU_C_STORE_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    let target_bytes = core::mem::size_of::<u32>();
    let extent = target_bytes.saturating_sub(1);
    let surface_width_minus1 = (extent & 0x7F) as u32;
    let surface_height_minus1 = ((extent >> 7) & 0x3FFF) as u32;
    let surface_depth_minus1 = ((extent >> 21) & 0x7FF) as u32;
    let surface_dword0 = (PDOANE_SURFTYPE_BUFFER << 29) | (PDOANE_SURFACE_FORMAT_RAW << 18);
    let surface_dword2 = (surface_height_minus1 << 16) | surface_width_minus1;
    let surface_dword3 = surface_depth_minus1 << 21;

    unsafe {
        let binding_table =
            warm.draw_state_virt.add(PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES) as *mut u32;
        for index in 0..PDOANE_VS_HDC_BTI34_BINDING_TABLE_ENTRIES {
            core::ptr::write_volatile(
                binding_table.add(index),
                PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES as u32,
            );
        }

        let surface =
            warm.draw_state_virt.add(PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES) as *mut u32;
        for index in 0..PDOANE_VS_HDC_BTI34_SURFACE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface.add(0), surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), surface_dword2);
        core::ptr::write_volatile(surface.add(3), surface_dword3);
        core::ptr::write_volatile(surface.add(8), target_gpu as u32);
        core::ptr::write_volatile(surface.add(9), (target_gpu >> 32) as u32);
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES) },
        binding_table_bytes,
    );
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES) },
        surface_bytes,
    );
    intel_render_focus_log!(
        "intel/render: pdoane-vs-hdc-bti34-surface ready=1 bti=0x{:02X} bt_off=0x{:X} bt_entries={} bt_entry=0x{:08X} surf_off=0x{:X} target_gpu=0x{:X} expected=0x{:08X} note=vs-dataport-bound-store-probe\n",
        PDOANE_VS_HDC_BTI34_BINDING_TABLE_INDEX,
        PDOANE_VS_HDC_BTI34_BINDING_TABLE_OFFSET_BYTES,
        PDOANE_VS_HDC_BTI34_BINDING_TABLE_ENTRIES,
        PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES,
        PDOANE_VS_HDC_BTI34_SURFACE_STATE_OFFSET_BYTES,
        target_gpu,
        PDOANE_VS_HDC_BTI34_STORE_MARKER,
    );
    true
}

#[derive(Copy, Clone)]
struct PdoaneReferencePort {
    name: &'static str,
    submit_name: &'static str,
    source: &'static str,
    front_end: TriangleFrontEndContract,
    blend: TriangleBlendProbeMode,
    backend: BackendProbeMode,
    streamout: StreamoutProofExperiment,
    post_draw_sync: PostDrawSyncVariant,
}

const PDOANE_REFERENCE_FRONT_END_GRF2: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf2-urb-start0-urb2-vb02044010-prim0",
    vs_urb_output_length_override: Some(2),
    vs_urb_entries_override: Some(0x0DF8),
    vs_urb_start_override: Some(0),
    vs_dispatch_grf_start_override: Some(2),
    vs_max_threads_field_override: None,
    vs_dw8_override: None,
    vs_simd8_single_instance_dispatch: false,
    vs_urb_read_offset: 0,
    vs_urb_read_length: 1,
    vertex_buffer_dw1_override: Some(0x0204_4010),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32A32_FLOAT),
    primitive_extended_dw1_override: Some(0),
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    sbe_active_component_override: None,
};

const PDOANE_REFERENCE_FRONT_END_GRF1: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf1-urb-start0-urb2-vb02044010-prim0",
    vs_urb_output_length_override: Some(2),
    vs_urb_entries_override: Some(0x0DF8),
    vs_urb_start_override: Some(0),
    vs_dispatch_grf_start_override: Some(1),
    vs_max_threads_field_override: None,
    vs_dw8_override: None,
    vs_simd8_single_instance_dispatch: false,
    vs_urb_read_offset: 0,
    vs_urb_read_length: 1,
    vertex_buffer_dw1_override: Some(0x0204_4010),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32A32_FLOAT),
    primitive_extended_dw1_override: Some(0),
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    sbe_active_component_override: None,
};

const PDOANE_GEN7_PASSTHROUGH_FRONT_END: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-gen7-passthrough-vs-bytes-grf1-urb-start0-urb1-entries704",
    vs_urb_output_length_override: Some(1),
    vs_urb_entries_override: Some(704),
    vs_urb_start_override: Some(0),
    vs_dispatch_grf_start_override: Some(1),
    vs_max_threads_field_override: Some(1),
    vs_dw8_override: Some(0),
    vs_simd8_single_instance_dispatch: false,
    vs_urb_read_offset: 0,
    vs_urb_read_length: 1,
    vertex_buffer_dw1_override: Some(0x0204_4010),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32A32_FLOAT),
    primitive_extended_dw1_override: Some(0),
    sbe_read_offset: 0,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    sbe_active_component_override: None,
};

const PDOANE_REFERENCE_FRONT_END_GRF0: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf0-metadata-urb-start0-urb2-vb02044010-prim0",
    vs_urb_output_length_override: Some(2),
    vs_urb_entries_override: Some(0x0DF8),
    vs_urb_start_override: Some(0),
    vs_dispatch_grf_start_override: None,
    vs_max_threads_field_override: None,
    vs_dw8_override: None,
    vs_simd8_single_instance_dispatch: false,
    vs_urb_read_offset: 0,
    vs_urb_read_length: 1,
    vertex_buffer_dw1_override: Some(0x0204_4010),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32A32_FLOAT),
    primitive_extended_dw1_override: Some(0),
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    sbe_active_component_override: None,
};

const PDOANE_REFERENCE_FRONT_END_GRF2_VS_READ1: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf2-vsread1-urb-start0-urb2-vb02044010-prim0",
    vs_urb_output_length_override: Some(2),
    vs_urb_entries_override: Some(0x0DF8),
    vs_urb_start_override: Some(0),
    vs_dispatch_grf_start_override: Some(2),
    vs_max_threads_field_override: None,
    vs_dw8_override: None,
    vs_simd8_single_instance_dispatch: false,
    vs_urb_read_offset: 1,
    vs_urb_read_length: 1,
    vertex_buffer_dw1_override: Some(0x0204_4010),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32A32_FLOAT),
    primitive_extended_dw1_override: Some(0),
    sbe_read_offset: 1,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    sbe_active_component_override: None,
};

const PDOANE_REFERENCE_FRONT_END_GRF2_MAX1: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf2-max1-urb-start0-urb2-vb02044010-prim0",
    vs_max_threads_field_override: Some(1),
    ..PDOANE_REFERENCE_FRONT_END_GRF2
};

const PDOANE_REFERENCE_FRONT_END_GRF2_SINGLE: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf2-single-urb-start0-urb2-vb02044010-prim0",
    vs_simd8_single_instance_dispatch: true,
    ..PDOANE_REFERENCE_FRONT_END_GRF2
};

const PDOANE_REFERENCE_FRONT_END_GRF2_URB1: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf2-urb-start0-urb1-vb02044010-prim0",
    vs_urb_output_length_override: Some(1),
    ..PDOANE_REFERENCE_FRONT_END_GRF2
};

const PDOANE_REFERENCE_FRONT_END_GRF2_DW8_ZERO: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-reference-vec4-grf2-dw8zero-urb-start0-urb2-vb02044010-prim0",
    vs_dw8_override: Some(0),
    ..PDOANE_REFERENCE_FRONT_END_GRF2
};

const PDOANE_REFERENCE_FRONT_END_GRF2_VS_OUTPUT_READ1: TriangleFrontEndContract =
    TriangleFrontEndContract {
        label: "pdoane-reference-vec4-grf2-vs-output-read1-urb-start0-urb2-vb02044010-prim0",
        vs_dw8_override: Some((1 << 16) | (1 << 21)),
        ..PDOANE_REFERENCE_FRONT_END_GRF2
    };

const PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1: TriangleFrontEndContract =
    TriangleFrontEndContract {
        label: "pdoane-reference-vec3-mesa-ve-baked-grf2-urb-start4-urb1-vb0204400c-sbe0-prim0",
        vs_urb_output_length_override: Some(1),
        vs_urb_start_override: Some(4),
        vs_dispatch_grf_start_override: Some(2),
        vertex_buffer_dw1_override: Some(0x0204_400C),
        vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32_FLOAT),
        sbe_read_offset: 0,
        sbe_read_length: 0,
        force_sbe_read_offset: false,
        force_sbe_read_length: false,
        ..PDOANE_REFERENCE_FRONT_END_GRF2
    };

const PDOANE_MESA_CLEAN_FRONT_END: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-mesa-clean-vec3-baked-grf-urb-start0-vb0204400c-prim-topology",
    vs_urb_output_length_override: Some(1),
    vs_urb_start_override: Some(0),
    vertex_buffer_dw1_override: Some(0x0204_400C),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32_FLOAT),
    primitive_extended_dw1_override: None,
    ..TRIANGLE_DEFAULT_FRONT_END_CONTRACT
};

const PDOANE_MESA_CLEAN_FRONT_END_DW8_LEN1: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-mesa-clean-vec3-baked-grf-urb-default-vb0204400c-dw8-len1",
    vs_dw8_override: Some(1 << 16),
    ..PDOANE_MESA_CLEAN_FRONT_END
};

const PDOANE_XELP_ACTIVE_FRONT_END: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-xelp-active-vec4-baked-grf0-urb-start4-urb1-slot0-entries704-vb02044010-prim-topology",
    vs_urb_output_length_override: Some(1),
    vs_urb_entries_override: Some(704),
    vs_urb_start_override: Some(4),
    vs_dispatch_grf_start_override: None,
    vs_max_threads_field_override: None,
    vs_dw8_override: None,
    vs_simd8_single_instance_dispatch: false,
    vs_urb_read_offset: 0,
    vs_urb_read_length: 1,
    vertex_buffer_dw1_override: Some(0x0204_4010),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32A32_FLOAT),
    primitive_extended_dw1_override: None,
    sbe_read_offset: 0,
    sbe_read_length: 1,
    force_sbe_read_offset: true,
    force_sbe_read_length: true,
    sbe_active_component_override: None,
};

const PDOANE_VF_CONTROL_FRONT_END: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "pdoane-vf-control-slot0-xyzw-vb02044010-prim0",
    vertex_buffer_dw1_override: Some(0x0204_4010),
    primitive_extended_dw1_override: None,
    ..TRIANGLE_DEFAULT_FRONT_END_CONTRACT
};

const PDOANE_REFERENCE_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-reference-port-grf2",
    submit_name: "pdoane-vs-streamout-proof-grf2",
    source: "pdoane-osdev-gfx-gfx_c",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_REFERENCE_PORT_SLOT0: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-reference-port-grf2-slot0",
    submit_name: "pdoane-vs-streamout-proof-grf2-slot0",
    source: "pdoane-osdev-gfx-gfx_c-vue-slot0-diagnostic",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot0Xyzw,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_REFERENCE_PORT_HEADER_AND_POSITION: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-reference-port-grf2-header-pos",
    submit_name: "pdoane-vs-streamout-proof-grf2-header-pos",
    source: "pdoane-osdev-gfx-gfx_c-vue-header-and-position-diagnostic",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::HeaderAndPositionSlots01,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_MESA_VE_REFERENCE_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-reference-port-mesa-clean-ve",
    submit_name: "pdoane-vs-streamout-proof-grf2-mesa-ve-urb1",
    source: "pdoane-osdev-gfx-gfx_c-screen-space-vertices-with-mesa-gfx125-vertex-element",
    front_end: PDOANE_MESA_CLEAN_FRONT_END,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSfMesaSbe,
    streamout: StreamoutProofExperiment::MesaNoVsRectlist,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_XELP_ACTIVE_REFERENCE_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-xelp-active-port",
    submit_name: "pdoane-smoke",
    source: "pdoane-osdev-gfx-gfx_c-vec4-front-end-with-current-gfx125-vs",
    front_end: PDOANE_XELP_ACTIVE_FRONT_END,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcTriListMesaActiveBlockSwiz,
    streamout: StreamoutProofExperiment::PositionSlot0Xyzw,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_VS_HDC_STORE_REFERENCE: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-vs-hdc-store-marker",
    submit_name: "pdoane-vs-hdc-store-proof",
    source: "trueos-gpgpu-hdc1-stateless-store-before-original-vs-urb-eot",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::FlushBit26Hdc,
};

const PDOANE_VS_HDC_BTI34_STORE_REFERENCE: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-vs-hdc-bti34-store-marker",
    submit_name: "pdoane-vs-hdc-bti34-store-proof",
    source: "trueos-gpgpu-hdc1-bti34-store-before-original-vs-urb-eot",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::FlushBit26Hdc,
};

const PDOANE_VS_HDC_STORE_TS_EOT_REFERENCE: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-vs-hdc-store-ts-eot-marker",
    submit_name: "pdoane-vs-hdc-store-ts-eot-proof",
    source: "trueos-gpgpu-hdc1-stateless-store-before-thread-spawner-eot-as-vs",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::FlushBit26Hdc,
};

const PDOANE_GEN7_PASSTHROUGH_REFERENCE: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-gen7-passthrough-vs-bytes",
    submit_name: "pdoane-vs-streamout-proof-gen7-passthrough-bytes",
    source: "pdoane-osdev-gfx-shaders-passthrough_p_vs_c-ivybridge-eu-bytes",
    front_end: PDOANE_GEN7_PASSTHROUGH_FRONT_END,
    ..PDOANE_REFERENCE_PORT
};

const MESA_REFERENCE_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "mesa-host-reference-port",
    submit_name: "pdoane-vs-streamout-proof-mesa12",
    source: "mesa-gfx125-host-simple-triangle-front-end",
    front_end: TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcTriListMesaActiveBlockSwiz,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const MESA_ACTIVE_GRF2_FRONT_END: TriangleFrontEndContract = TriangleFrontEndContract {
    label: "mesa-like-active-grf2-urb0df8-vb0204400c-clean",
    vs_urb_entries_override: Some(0x0DF8),
    vs_dispatch_grf_start_override: Some(2),
    vertex_buffer_dw1_override: Some(0x0204_400C),
    vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32_FLOAT),
    primitive_extended_dw1_override: Some(0),
    ..TRIANGLE_DEFAULT_FRONT_END_CONTRACT
};

const MESA_ACTIVE_GRF2_REFERENCE_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "mesa-host-reference-port-grf2",
    submit_name: "pdoane-vs-streamout-proof-mesa12-grf2",
    front_end: MESA_ACTIVE_GRF2_FRONT_END,
    ..MESA_REFERENCE_PORT
};

const MESA_ACTIVE_GRF2_REFERENCE_PORT_SLOT0: PdoaneReferencePort = PdoaneReferencePort {
    name: "mesa-host-reference-port-grf2-slot0",
    submit_name: "pdoane-vs-streamout-proof-mesa12-grf2-slot0",
    streamout: StreamoutProofExperiment::PositionSlot0,
    front_end: MESA_ACTIVE_GRF2_FRONT_END,
    ..MESA_REFERENCE_PORT_SLOT0
};

const MESA_ACTIVE_CONST_URB_REFERENCE_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "mesa-active-const-urb-port",
    submit_name: "pdoane-vs-streamout-proof-mesa12-const-urb",
    source: "trueos-constant-position-vs-urb-export-active-backend",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const MESA_REFERENCE_PORT_SLOT0: PdoaneReferencePort = PdoaneReferencePort {
    name: "mesa-host-reference-port-slot0",
    submit_name: "pdoane-vs-streamout-proof-mesa12-slot0",
    source: "mesa-gfx125-host-simple-triangle-front-end",
    front_end: TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot0,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_REFERENCE_PORT_GRF2: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-export-grf2",
    submit_name: "pdoane-vs-streamout-proof-const-urb-grf2",
    source: "trueos-constant-position-vs-urb-export-no-vf-attribute-dependency",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_KSP0_REFERENCE_PORT_GRF2: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-export-ksp0-grf2",
    submit_name: "pdoane-vs-streamout-proof-const-urb-ksp0-grf2",
    source: "trueos-constant-position-vs-urb-export-ksp0-no-uav-dw3",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_REFERENCE_PORT_GRF1: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-export-grf1",
    submit_name: "pdoane-vs-streamout-proof-const-urb-grf1",
    source: "trueos-constant-position-vs-urb-export-no-vf-attribute-dependency",
    front_end: TriangleFrontEndContract {
        label: "pdoane-reference-const-urb-grf1-urb-start0-urb1",
        vs_urb_output_length_override: Some(1),
        vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32_FLOAT),
        ..PDOANE_REFERENCE_FRONT_END_GRF1
    },
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_REFERENCE_PORT_GRF0: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-export-grf0",
    submit_name: "pdoane-vs-streamout-proof-const-urb-grf0",
    source: "trueos-constant-position-vs-urb-export-no-vf-attribute-dependency",
    front_end: TriangleFrontEndContract {
        label: "pdoane-reference-const-urb-grf0-urb-start0-urb1",
        vs_urb_output_length_override: Some(1),
        vertex_element_format_override: Some(SURFACE_FORMAT_R32G32B32_FLOAT),
        ..PDOANE_REFERENCE_FRONT_END_GRF0
    },
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_REFERENCE_PORTS: [PdoaneReferencePort; 3] = [
    PDOANE_CONST_URB_REFERENCE_PORT_GRF2,
    PDOANE_CONST_URB_REFERENCE_PORT_GRF1,
    PDOANE_CONST_URB_REFERENCE_PORT_GRF0,
];

const PDOANE_CONST_URB_HANDLE_G0_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g0",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g0",
    source: "trueos-constant-position-vs-urb-export-handle-g0-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G1_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g1",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g1",
    source: "trueos-constant-position-vs-urb-export-handle-g1-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G2_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g2",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g2",
    source: "trueos-constant-position-vs-urb-export-handle-g2-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G3_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g3",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g3",
    source: "trueos-constant-position-vs-urb-export-handle-g3-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G4_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g4",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g4",
    source: "trueos-constant-position-vs-urb-export-handle-g4-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G5_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g5",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g5",
    source: "trueos-constant-position-vs-urb-export-handle-g5-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G6_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g6",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g6",
    source: "trueos-constant-position-vs-urb-export-handle-g6-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_CONST_URB_HANDLE_G7_PORT: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-const-urb-handle-g7",
    submit_name: "pdoane-vs-streamout-proof-const-urb-handle-g7",
    source: "trueos-constant-position-vs-urb-export-handle-g7-brw-asm-exact-send",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MESA_VE_URB1,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot1,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_URB_PAYLOAD_PROOF_PORTS: [PdoaneReferencePort; 12] = [
    PDOANE_REFERENCE_PORT,
    PDOANE_REFERENCE_PORT_SLOT0,
    PDOANE_REFERENCE_PORT_HEADER_AND_POSITION,
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf2-dw8zero",
        submit_name: "pdoane-vs-streamout-proof-grf2-dw8zero",
        source: "pdoane-osdev-gfx-gfx_c-vs-dw8-zero-gen7-shape",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF2_DW8_ZERO,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf2-vs-output-read1",
        submit_name: "pdoane-vs-streamout-proof-grf2-vs-output-read1",
        source: "gen125-3dstate-vs-dw8-output-read-offset-slot1-diagnostic",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF2_VS_OUTPUT_READ1,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PDOANE_MESA_VE_REFERENCE_PORT,
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf2-max1",
        submit_name: "pdoane-vs-streamout-proof-grf2-max1",
        source: "pdoane-osdev-gfx-gfx_c-minimal-vs-thread-field",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF2_MAX1,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf2-single",
        submit_name: "pdoane-vs-streamout-proof-grf2-single",
        source: "gen125-3dstate-vs-simd8-single-instance-diagnostic",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF2_SINGLE,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf2-urb1",
        submit_name: "pdoane-vs-streamout-proof-grf2-urb1",
        source: "pdoane-osdev-gfx-gfx_c-with-mesa-vs-urb-output-length",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF2_URB1,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf2-vsread1",
        submit_name: "pdoane-vs-streamout-proof-grf2-vsread1",
        source: "pdoane-osdev-gfx-gfx_c",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF2_VS_READ1,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf1",
        submit_name: "pdoane-vs-streamout-proof-grf1",
        source: "pdoane-osdev-gfx-gfx_c",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF1,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
    PdoaneReferencePort {
        name: "pdoane-reference-port-grf0",
        submit_name: "pdoane-vs-streamout-proof-grf0",
        source: "pdoane-osdev-gfx-gfx_c",
        front_end: PDOANE_REFERENCE_FRONT_END_GRF0,
        blend: TriangleBlendProbeMode::MesaZeroedState,
        backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
        streamout: StreamoutProofExperiment::PositionSlot1,
        post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
    },
];

const PDOANE_VF_STREAMOUT_REFERENCE: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-vf-synthesized-vue-pos-slot0-xyzw",
    submit_name: PDOANE_VF_STREAMOUT_SUBMIT_NAME,
    source: "rendercopy_gen7_no_vs_shape_with_pdoane_vec4_vb",
    front_end: PDOANE_REFERENCE_FRONT_END_GRF0,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl,
    streamout: StreamoutProofExperiment::PositionSlot0Xyzw,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

const PDOANE_VF_CONTROL_REFERENCE: PdoaneReferencePort = PdoaneReferencePort {
    name: "pdoane-vf-control-mesa-no-vs-rectlist",
    submit_name: "pdoane-vf-control-mesa-no-vs-rectlist",
    source: "igt-rendercopy-gen7-no-vs-rectlist-control",
    front_end: PDOANE_VF_CONTROL_FRONT_END,
    blend: TriangleBlendProbeMode::MesaZeroedState,
    backend: PDOANE_ACTIVE_VF_SYNTHESIZED_BACKEND,
    streamout: PDOANE_ACTIVE_VF_SYNTHESIZED_EXPERIMENT,
    post_draw_sync: PostDrawSyncVariant::NoPostDrawPipeControl,
};

fn seed_pdoane_streamout_sentinel(warm: RenderWarmState) {
    if warm.streamout_virt.is_null() || warm.streamout_len == 0 {
        return;
    }
    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.streamout_virt as *mut u32, warm.streamout_len / 4)
    };
    words.fill(PDOANE_STREAMOUT_SENTINEL);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);
}

fn log_pdoane_urb_payload_probe(
    submit_name: &'static str,
    reference: PdoaneReferencePort,
    warm: RenderWarmState,
    completed: bool,
    soft_accepted: bool,
    vertex_count: usize,
    experiment: StreamoutProofExperiment,
    before: TriangleStageStats,
    after: TriangleStageStats,
) -> bool {
    let sample_bytes = experiment
        .vertex_bytes()
        .saturating_mul(vertex_count)
        .min(warm.streamout_len);
    if sample_bytes != 0 {
        crate::intel::dma_flush(warm.streamout_virt, sample_bytes);
    }
    let word_count = sample_bytes / core::mem::size_of::<u32>();
    let inspect_words = core::cmp::min(word_count, 8);
    let base = warm.streamout_virt as *const u32;
    const STREAMOUT_DEBUG_MARKER_OFFSET_BYTES: usize = 0x80;
    const STREAMOUT_DEBUG_MARKER_VALUE: u32 = 0xC0DE_F00D;
    let streamout_marker = if warm.streamout_len
        >= STREAMOUT_DEBUG_MARKER_OFFSET_BYTES + core::mem::size_of::<u32>()
    {
        unsafe {
            let marker_ptr = warm
                .streamout_virt
                .add(STREAMOUT_DEBUG_MARKER_OFFSET_BYTES);
            crate::intel::dma_flush(marker_ptr, core::mem::size_of::<u32>());
            core::ptr::read_volatile(marker_ptr as *const u32)
        }
    } else {
        0
    };
    let streamout_marker_ok = streamout_marker == STREAMOUT_DEBUG_MARKER_VALUE;
    let mut changed_dwords = 0usize;
    let mut first = [PDOANE_STREAMOUT_SENTINEL; 8];
    for index in 0..word_count {
        let value = unsafe { core::ptr::read_volatile(base.add(index)) };
        if value != PDOANE_STREAMOUT_SENTINEL {
            changed_dwords = changed_dwords.saturating_add(1);
        }
        if index < inspect_words {
            first[index] = value;
        }
    }
    let delta = after.delta_since(before);
    let payload_changed = changed_dwords != 0;
    let accepted = (completed || soft_accepted || payload_changed) && delta.vs_invocations > 0;
    let verdict = if accepted && payload_changed {
        "vs-wrote-observable-streamout-payload"
    } else if delta.vs_invocations > 0 {
        "vs-ran-no-observable-streamout-payload"
    } else if delta.ia_vertices > 0 || delta.ia_primitives > 0 {
        "vf-advanced-no-vs-payload"
    } else {
        "no-front-end-payload"
    };
    intel_render_focus_log!(
        "intel/render: pdoane-urb-payload-proof submit={} variant={} front_end={} grf_override={} payload_proof={} thread_accepted={} completed={} soft_accepted={} payload_changed={} changed_dwords={} sentinel=0x{:08X} sample_bytes={} experiment={} vertex_count={} delta_ia_vtx={} delta_ia_prim={} delta_vs={} delta_so0={} delta_so_write0={} streamout_marker=0x{:08X} marker_ok={} marker_expected=0x{:08X} first8=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] first4_f=[{:.3},{:.3},{:.3},{:.3}] verdict={} next={}\n",
        submit_name,
        reference.name,
        reference.front_end.label,
        reference
            .front_end
            .vs_dispatch_grf_start_override
            .map(|value| value as u32)
            .unwrap_or(0),
        payload_changed as u8,
        accepted as u8,
        completed as u8,
        soft_accepted as u8,
        payload_changed as u8,
        changed_dwords,
        PDOANE_STREAMOUT_SENTINEL,
        sample_bytes,
        experiment.label(),
        vertex_count,
        delta.ia_vertices,
        delta.ia_primitives,
        delta.vs_invocations,
        delta.so_prims_written_0,
        delta.so_write_offset_0,
        streamout_marker,
        streamout_marker_ok as u8,
        STREAMOUT_DEBUG_MARKER_VALUE,
        first[0],
        first[1],
        first[2],
        first[3],
        first[4],
        first[5],
        first[6],
        first[7],
        f32::from_bits(first[0]),
        f32::from_bits(first[1]),
        f32::from_bits(first[2]),
        f32::from_bits(first[3]),
        verdict,
        if accepted {
            "compare-vue-slot-contract-against-clip-input"
        } else {
            "fix-vs-urb-or-streamout-payload-contract-before-wm"
        },
    );
    payload_changed
}

fn prepare_pdoane_reference_draw_resources(
    warm: RenderWarmState,
    reference: PdoaneReferencePort,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
) -> Option<TriangleDrawPrep> {
    let target_w = u32::try_from(rect_w).ok()?;
    let target_h = u32::try_from(rect_h).ok()?;
    let rt_pitch = u32::try_from(pitch).ok()?;
    if warm.draw_state_len == 0 {
        return None;
    }
    let vertex_stride = write_pdoane_reference_vertices_for(warm, reference)?;

    unsafe {
        core::ptr::write_bytes(warm.draw_state_virt, 0, warm.draw_state_len);
    }
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);

    Some(TriangleDrawPrep {
        vertex_count: TRIANGLE_DRAW_VERTICES as u32,
        vertex_stride: vertex_stride as u32,
        vertex_gpu_addr: GPU_VA_VERTEX_BASE,
        state_gpu_addr: GPU_VA_DRAW_STATE_BASE,
        rt_gpu_addr: dst_gpu_addr,
        rt_pitch,
        target_w,
        target_h,
    })
}

fn write_pdoane_reference_vertices_for(
    warm: RenderWarmState,
    reference: PdoaneReferencePort,
) -> Option<usize> {
    if matches!(reference.streamout, StreamoutProofExperiment::MesaNoVsRectlist) {
        write_pdoane_mesa_vec3_vertices(warm)
    } else {
        write_pdoane_reference_vertices(warm).map(|_| PDOANE_VERTEX_STRIDE)
    }
}

fn write_pdoane_reference_vertices(warm: RenderWarmState) -> Option<()> {
    let byte_len = TRIANGLE_DRAW_VERTICES * PDOANE_VERTEX_STRIDE;
    if warm.vertex_len < byte_len || warm.vertex_virt.is_null() {
        return None;
    }

    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u32, warm.vertex_len / 4)
    };
    words.fill(0);
    for (idx, vertex) in PDOANE_VERTICES.iter().enumerate() {
        let base = idx * PDOANE_VERTEX_DWORDS;
        for (component, value) in vertex.iter().enumerate() {
            words[base + component] = value.to_bits();
        }
    }
    crate::intel::dma_flush(warm.vertex_virt, byte_len);

    if PDOANE_SMOKE_VERBOSE_LOGS {
        intel_render_focus_log!(
            "intel/render: pdoane-reference-vertex-upload accepted=1 source=pdoane-osdev-gfx-gfx_c format=vec4 bytes={} stride={} count={} gpu=0x{:X} v0=[{:.3},{:.3},{:.3},{:.3}] v1=[{:.3},{:.3},{:.3},{:.3}] v2=[{:.3},{:.3},{:.3},{:.3}]\n",
            byte_len,
            PDOANE_VERTEX_STRIDE,
            TRIANGLE_DRAW_VERTICES,
            GPU_VA_VERTEX_BASE,
            PDOANE_VERTICES[0][0],
            PDOANE_VERTICES[0][1],
            PDOANE_VERTICES[0][2],
            PDOANE_VERTICES[0][3],
            PDOANE_VERTICES[1][0],
            PDOANE_VERTICES[1][1],
            PDOANE_VERTICES[1][2],
            PDOANE_VERTICES[1][3],
            PDOANE_VERTICES[2][0],
            PDOANE_VERTICES[2][1],
            PDOANE_VERTICES[2][2],
            PDOANE_VERTICES[2][3],
        );
    }

    Some(())
}

fn write_pdoane_mesa_vec3_vertices(warm: RenderWarmState) -> Option<usize> {
    let byte_len = TRIANGLE_DRAW_VERTICES * PDOANE_MESA_VERTEX_STRIDE;
    if warm.vertex_len < byte_len || warm.vertex_virt.is_null() {
        return None;
    }

    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u32, warm.vertex_len / 4)
    };
    words.fill(0);
    for (idx, vertex) in PDOANE_VERTICES.iter().enumerate() {
        let base = idx * PDOANE_MESA_VERTEX_DWORDS;
        words[base + 0] = vertex[0].to_bits();
        words[base + 1] = vertex[1].to_bits();
        words[base + 2] = vertex[2].to_bits();
    }
    crate::intel::dma_flush(warm.vertex_virt, byte_len);

    if PDOANE_SMOKE_VERBOSE_LOGS {
        intel_render_focus_log!(
            "intel/render: pdoane-reference-vertex-upload accepted=1 source=pdoane-osdev-gfx-gfx_c+mesa-ve format=vec3 bytes={} stride={} count={} gpu=0x{:X} v0=[{:.3},{:.3},{:.3}] v1=[{:.3},{:.3},{:.3}] v2=[{:.3},{:.3},{:.3}]\n",
            byte_len,
            PDOANE_MESA_VERTEX_STRIDE,
            TRIANGLE_DRAW_VERTICES,
            GPU_VA_VERTEX_BASE,
            PDOANE_VERTICES[0][0],
            PDOANE_VERTICES[0][1],
            PDOANE_VERTICES[0][2],
            PDOANE_VERTICES[1][0],
            PDOANE_VERTICES[1][1],
            PDOANE_VERTICES[1][2],
            PDOANE_VERTICES[2][0],
            PDOANE_VERTICES[2][1],
            PDOANE_VERTICES[2][2],
        );
    }

    Some(PDOANE_MESA_VERTEX_STRIDE)
}

fn write_pdoane_active_rectlist_i16_vertices(
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    geometry: VfPrimitiveGeometry,
) -> Option<TriangleDrawPrep> {
    const RECTLIST_I16_STRIDE: usize = 8;
    const RECTLIST_I16_WORDS_PER_VERTEX: usize = RECTLIST_I16_STRIDE / core::mem::size_of::<u16>();

    let vertex_count = draw.vertex_count as usize;
    let byte_len = vertex_count.checked_mul(RECTLIST_I16_STRIDE)?;
    if warm.vertex_len < byte_len || warm.vertex_virt.is_null() {
        return None;
    }

    fn coord_bits(value: f32) -> u16 {
        let rounded = value as i32;
        let clamped = if rounded < i16::MIN as i32 {
            i16::MIN
        } else if rounded > i16::MAX as i32 {
            i16::MAX
        } else {
            rounded as i16
        };
        clamped as u16
    }

    let vertices = geometry.vertices_for_target(draw.target_w, draw.target_h);
    let words = unsafe {
        core::slice::from_raw_parts_mut(warm.vertex_virt as *mut u16, warm.vertex_len / 2)
    };
    words.fill(0);
    for (index, vertex) in vertices.iter().take(vertex_count).enumerate() {
        let base = index * RECTLIST_I16_WORDS_PER_VERTEX;
        words[base] = coord_bits(vertex[0]);
        words[base + 1] = coord_bits(vertex[1]);
    }
    crate::intel::dma_flush(warm.vertex_virt, byte_len);

    let readback = unsafe {
        core::slice::from_raw_parts(warm.vertex_virt as *const u16, vertex_count * RECTLIST_I16_WORDS_PER_VERTEX)
    };
    intel_render_focus_log!(
        "intel/render: pdoane-active-rectlist-i16-upload accepted=1 source=igt-rendercopy-gen7 format=R16G16_SSCALED bytes={} stride={} count={} gpu=0x{:X} v0=[{},{}] v1=[{},{}] v2=[{},{}] raw16=[0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X}] note=active-vf-payload-proof\n",
        byte_len,
        RECTLIST_I16_STRIDE,
        vertex_count,
        GPU_VA_VERTEX_BASE,
        vertices[0][0] as i32,
        vertices[0][1] as i32,
        vertices[1][0] as i32,
        vertices[1][1] as i32,
        vertices[2][0] as i32,
        vertices[2][1] as i32,
        readback.get(0).copied().unwrap_or(0),
        readback.get(1).copied().unwrap_or(0),
        readback.get(2).copied().unwrap_or(0),
        readback.get(3).copied().unwrap_or(0),
        readback.get(4).copied().unwrap_or(0),
        readback.get(5).copied().unwrap_or(0),
        readback.get(6).copied().unwrap_or(0),
        readback.get(7).copied().unwrap_or(0),
        readback.get(8).copied().unwrap_or(0),
        readback.get(9).copied().unwrap_or(0),
        readback.get(10).copied().unwrap_or(0),
        readback.get(11).copied().unwrap_or(0),
    );

    Some(TriangleDrawPrep {
        vertex_stride: RECTLIST_I16_STRIDE as u32,
        ..draw
    })
}

fn prepare_mesa_reference_draw_resources_from_pdoane_target(
    warm: RenderWarmState,
    target: TriangleDrawPrep,
) -> Option<TriangleDrawPrep> {
    let vertex_proof = write_triangle_vertices(warm, VfPrimitiveGeometry::Canonical)?;
    Some(TriangleDrawPrep {
        vertex_count: vertex_proof.vertex_count,
        vertex_stride: vertex_proof.vertex_stride,
        vertex_gpu_addr: vertex_proof.gpu_addr,
        state_gpu_addr: target.state_gpu_addr,
        rt_gpu_addr: target.rt_gpu_addr,
        rt_pitch: target.rt_pitch,
        target_w: target.target_w,
        target_h: target.target_h,
    })
}

fn submit_pdoane_vs_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    reference: PdoaneReferencePort,
) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_pdoane_streamout_sentinel(warm);
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vs_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        reference.streamout,
        0,
        VsStreamoutProofConfig {
            pipeline,
            shader_layout,
            front_end_contract: reference.front_end,
            post_draw_sync: reference.post_draw_sync,
        },
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: {} skipped reason=batch-encode detail={}\n",
                reference.submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_focus_log!(
        "intel/render: {} batch-ready bytes=0x{:X} experiment={} front_end={} source={} sentinel=0x{:08X} note=urb-payload-before-clip-sf-diagnostic\n",
        reference.submit_name,
        batch_tail_bytes,
        reference.streamout.label(),
        reference.front_end.label,
        reference.source,
        PDOANE_STREAMOUT_SENTINEL,
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        reference.submit_name,
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let soft_accepted = if completed {
        false
    } else {
        maybe_soft_accept_streamout_submit(
            reference.submit_name,
            warm,
            stats_before,
            stats_after,
            true,
            reference
                .streamout
                .vertex_bytes()
                .saturating_mul(draw.vertex_count as usize),
        )
    };
    log_triangle_stage_diagnosis(
        reference.submit_name,
        completed,
        stats_before,
        stats_after,
    );
    let accepted = log_pdoane_urb_payload_probe(
        reference.submit_name,
        reference,
        warm,
        completed,
        soft_accepted,
        draw.vertex_count as usize,
        reference.streamout,
        stats_before,
        stats_after,
    );
    log_streamout_proof_result(
        reference.submit_name,
        warm,
        completed,
        draw.vertex_count as usize,
        reference.streamout,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, reference.submit_name);
    }
    accepted
}

fn submit_pdoane_vs_hdc_store_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
) -> bool {
    let pipeline = crate::intel::shader::triangle_hdc_vs_store_probe_pipeline();
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} skipped reason=shader-layout detail={}\n",
                PDOANE_VS_HDC_STORE_REFERENCE.submit_name,
                reason
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(
        warm,
        pipeline,
        shader_layout,
        PDOANE_VS_HDC_STORE_REFERENCE.submit_name,
    );
    let streamout_payload = submit_pdoane_vs_streamout_proof(
        dev,
        warm,
        draw,
        pipeline,
        shader_layout,
        PDOANE_VS_HDC_STORE_REFERENCE,
    );
    let observed = read_result_dword(warm, RESULT_SLOT_GPGPU_EU_C_STORE_DWORD);
    let accepted = observed == PDOANE_VS_HDC_STORE_MARKER;
    intel_render_focus_log!(
        "intel/render: pdoane-vs-hdc-store-proof eu-store accepted={} observed=0x{:08X} expected=0x{:08X} result_slot={} gpu_offset=0x{:X} streamout_payload={} note=shader-side-store-before-original-vs-urb-eot\n",
        accepted as u8,
        observed,
        PDOANE_VS_HDC_STORE_MARKER,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD * core::mem::size_of::<u32>(),
        streamout_payload as u8,
    );
    accepted
}

fn submit_pdoane_vs_hdc_bti34_store_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
) -> bool {
    let surface_ready = prepare_pdoane_vs_hdc_bti34_surface(warm);
    let pipeline = crate::intel::shader::triangle_hdc_vs_bti34_store_probe_pipeline();
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} skipped reason=shader-layout detail={}\n",
                PDOANE_VS_HDC_BTI34_STORE_REFERENCE.submit_name,
                reason
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(
        warm,
        pipeline,
        shader_layout,
        PDOANE_VS_HDC_BTI34_STORE_REFERENCE.submit_name,
    );
    let streamout_payload = submit_pdoane_vs_streamout_proof(
        dev,
        warm,
        draw,
        pipeline,
        shader_layout,
        PDOANE_VS_HDC_BTI34_STORE_REFERENCE,
    );
    let observed = read_result_dword(warm, RESULT_SLOT_GPGPU_EU_C_STORE_DWORD);
    let accepted = surface_ready && observed == PDOANE_VS_HDC_BTI34_STORE_MARKER;
    intel_render_focus_log!(
        "intel/render: pdoane-vs-hdc-bti34-store-proof eu-store accepted={} surface_ready={} observed=0x{:08X} expected=0x{:08X} result_slot={} gpu_offset=0x{:X} streamout_payload={} note=shader-side-bti34-store-before-original-vs-urb-eot\n",
        accepted as u8,
        surface_ready as u8,
        observed,
        PDOANE_VS_HDC_BTI34_STORE_MARKER,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD * core::mem::size_of::<u32>(),
        streamout_payload as u8,
    );
    accepted
}

fn submit_pdoane_vs_hdc_store_ts_eot_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
) -> bool {
    let pipeline = crate::intel::shader::triangle_hdc_vs_store_ts_eot_probe_pipeline();
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} skipped reason=shader-layout detail={}\n",
                PDOANE_VS_HDC_STORE_TS_EOT_REFERENCE.submit_name,
                reason
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(
        warm,
        pipeline,
        shader_layout,
        PDOANE_VS_HDC_STORE_TS_EOT_REFERENCE.submit_name,
    );
    let streamout_payload = submit_pdoane_vs_streamout_proof(
        dev,
        warm,
        draw,
        pipeline,
        shader_layout,
        PDOANE_VS_HDC_STORE_TS_EOT_REFERENCE,
    );
    let observed = read_result_dword(warm, RESULT_SLOT_GPGPU_EU_C_STORE_DWORD);
    let accepted = observed == PDOANE_VS_HDC_STORE_TS_EOT_MARKER;
    intel_render_focus_log!(
        "intel/render: pdoane-vs-hdc-store-ts-eot-proof eu-store accepted={} observed=0x{:08X} expected=0x{:08X} result_slot={} gpu_offset=0x{:X} streamout_payload={} note=shader-side-store-before-thread-spawner-eot-as-vs-stage diagnostic=separates-hdc-side-effect-from-urb-eot-export\n",
        accepted as u8,
        observed,
        PDOANE_VS_HDC_STORE_TS_EOT_MARKER,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD,
        RESULT_SLOT_GPGPU_EU_C_STORE_DWORD * core::mem::size_of::<u32>(),
        streamout_payload as u8,
    );
    accepted
}

fn submit_pdoane_vf_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
) -> bool {
    let reference = PDOANE_VF_STREAMOUT_REFERENCE;
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_pdoane_streamout_sentinel(warm);
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vf_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        reference.streamout,
        0,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: {} skipped reason=batch-encode detail={}\n",
                reference.submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_focus_log!(
        "intel/render: {} batch-ready bytes=0x{:X} experiment={} source={} sentinel=0x{:08X} note=vf-synthesized-vue-streamout-isolates-sol-from-vs-urb-write\n",
        reference.submit_name,
        batch_tail_bytes,
        reference.streamout.label(),
        reference.source,
        PDOANE_STREAMOUT_SENTINEL,
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        reference.submit_name,
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let soft_accepted = if completed {
        false
    } else {
        maybe_soft_accept_streamout_submit(
            reference.submit_name,
            warm,
            stats_before,
            stats_after,
            true,
            reference
                .streamout
                .vertex_bytes()
                .saturating_mul(draw.vertex_count as usize),
        )
    };
    log_triangle_stage_diagnosis(reference.submit_name, completed, stats_before, stats_after);
    let accepted = log_pdoane_urb_payload_probe(
        reference.submit_name,
        reference,
        warm,
        completed,
        soft_accepted,
        draw.vertex_count as usize,
        reference.streamout,
        stats_before,
        stats_after,
    );
    log_streamout_proof_result(
        reference.submit_name,
        warm,
        completed,
        draw.vertex_count as usize,
        reference.streamout,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, reference.submit_name);
    }
    accepted
}

fn submit_pdoane_smoke_once(reason: &'static str) -> bool {
    if !PDOANE_SMOKE_ENABLED {
        return false;
    }
    if !PDOANE_SMOKE_PERIODIC_ENABLED && reason == "periodic-render-60hz" {
        return false;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: pdoane-smoke skipped reason=no-device trigger={}\n", reason);
        return false;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: pdoane-smoke skipped reason=no-surface trigger={}\n", reason);
        return false;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: pdoane-smoke skipped reason=no-dimensions trigger={}\n", reason);
        return false;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!(
            "intel/render: pdoane-smoke skipped reason=bad-pitch trigger={} width={}\n",
            reason,
            width
        );
        return false;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("intel/render: pdoane-smoke skipped reason=warm-buffers trigger={}\n", reason);
        return false;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: pdoane-smoke skipped reason=forcewake trigger={}\n", reason);
        return false;
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: pdoane-smoke skipped reason=ggtt-map trigger={}\n", reason);
        return false;
    }

    let reference = PDOANE_XELP_ACTIVE_REFERENCE_PORT;
    let Some(draw) = prepare_pdoane_reference_draw_resources(
        warm,
        reference,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) else {
        crate::log!(
            "intel/render: pdoane-smoke skipped reason=draw-resources trigger={} size={}x{} pitch=0x{:X}\n",
            reason,
            width,
            height,
            pitch_bytes
        );
        return false;
    };

    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: pdoane-smoke skipped reason=placeholder-pipeline vs_src={} ps_src={} note={}\n",
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: pdoane-smoke skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    let blend_mode = reference.blend;
    let backend_mode = reference.backend;
    let quiet_periodic = pdoane_smoke_quiet_periodic(reason);
    if !quiet_periodic
        && !PDOANE_SHADER_UPLOAD_VERIFY_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed)
    {
        log_uploaded_triangle_shader_verification(
            warm,
            pipeline,
            shader_layout,
            PDOANE_SMOKE_SUBMIT_NAME,
        );
    }
    if quiet_periodic {
        set_render_focus_log_suppressed(true);
    }
    let probe_state_result =
        write_triangle_probe_state(warm, draw, shader_layout, blend_mode, backend_mode);
    if quiet_periodic {
        set_render_focus_log_suppressed(false);
    }
    let probe_state = match probe_state_result {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: pdoane-smoke skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    let ran_payload_proofs = PDOANE_URB_PAYLOAD_PROOF_ENABLED && !quiet_periodic;
    if ran_payload_proofs {
        let vf_streamout_accepted = submit_pdoane_vf_streamout_proof(dev, warm, draw);
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            PDOANE_VF_STREAMOUT_SUBMIT_NAME,
        );
        let mut urb_payload_accepted = false;
        for proof_reference in PDOANE_URB_PAYLOAD_PROOF_PORTS {
            urb_payload_accepted |= submit_pdoane_vs_streamout_proof(
                dev,
                warm,
                draw,
                pipeline,
                shader_layout,
                proof_reference,
            );
            recover_render_engine_after_nonretired_submit(
                dev,
                warm,
                proof_reference.submit_name,
            );
        }
        if let Some(mesa_draw) = prepare_mesa_reference_draw_resources_from_pdoane_target(warm, draw)
        {
            for mesa_reference in [MESA_REFERENCE_PORT, MESA_REFERENCE_PORT_SLOT0] {
                urb_payload_accepted |= submit_pdoane_vs_streamout_proof(
                    dev,
                    warm,
                    mesa_draw,
                    pipeline,
                    shader_layout,
                    mesa_reference,
                );
                recover_render_engine_after_nonretired_submit(
                    dev,
                    warm,
                    mesa_reference.submit_name,
                );
            }
            let restored = write_pdoane_reference_vertices_for(warm, reference).is_some();
            intel_render_focus_log!(
                "intel/render: pdoane-smoke mesa-input-proof restored_pdoane_vertices={} active_stride={} mesa_stride={} pdoane_stride={} variants=slot1,slot0 note=extra-proof-aligns-baked-vs-with-host-12-byte-vf-contract\n",
                restored as u8,
                draw.vertex_stride,
                crate::intel::shader::TRIANGLE_VERTEX_STRIDE_BYTES,
                PDOANE_VERTEX_STRIDE,
            );
        } else {
            intel_render_focus_log!(
                "intel/render: pdoane-smoke mesa-input-proof skipped reason=mesa-vertex-upload-failed note=extra-proof-aligns-baked-vs-with-host-12-byte-vf-contract\n",
            );
        }
        let mut const_urb_accepted = false;
        let const_urb_ksp0_pipeline =
            crate::intel::shader::triangle_const_urb_ksp0_vs_probe_pipeline();
        match upload_triangle_shader_pipeline(warm, const_urb_ksp0_pipeline) {
            Ok(const_urb_ksp0_shader_layout) => {
                log_uploaded_triangle_shader_verification(
                    warm,
                    const_urb_ksp0_pipeline,
                    const_urb_ksp0_shader_layout,
                    PDOANE_CONST_URB_KSP0_REFERENCE_PORT_GRF2.submit_name,
                );
                const_urb_accepted |= submit_pdoane_vs_streamout_proof(
                    dev,
                    warm,
                    draw,
                    const_urb_ksp0_pipeline,
                    const_urb_ksp0_shader_layout,
                    PDOANE_CONST_URB_KSP0_REFERENCE_PORT_GRF2,
                );
                recover_render_engine_after_nonretired_submit(
                    dev,
                    warm,
                    PDOANE_CONST_URB_KSP0_REFERENCE_PORT_GRF2.submit_name,
                );
            }
            Err(reason) => {
                intel_render_focus_log!(
                    "intel/render: pdoane-smoke const-urb-ksp0-proof skipped reason=shader-layout detail={}\n",
                    reason,
                );
            }
        }
        let const_urb_pipeline = crate::intel::shader::triangle_const_urb_vs_probe_pipeline();
        match upload_triangle_shader_pipeline(warm, const_urb_pipeline) {
            Ok(const_urb_shader_layout) => {
                log_uploaded_triangle_shader_verification(
                    warm,
                    const_urb_pipeline,
                    const_urb_shader_layout,
                    PDOANE_CONST_URB_REFERENCE_PORT_GRF2.submit_name,
                );
                for const_reference in PDOANE_CONST_URB_REFERENCE_PORTS {
                    const_urb_accepted |= submit_pdoane_vs_streamout_proof(
                        dev,
                        warm,
                        draw,
                        const_urb_pipeline,
                        const_urb_shader_layout,
                        const_reference,
                    );
                    recover_render_engine_after_nonretired_submit(
                        dev,
                        warm,
                        const_reference.submit_name,
                    );
                }
            }
            Err(reason) => {
                intel_render_focus_log!(
                    "intel/render: pdoane-smoke const-urb-proof skipped reason=shader-layout detail={}\n",
                    reason,
                );
            }
        }
        urb_payload_accepted |= const_urb_accepted;
        intel_render_focus_log!(
            "intel/render: pdoane-smoke const-urb-proof accepted={} variants=ksp0-grf2,grf2,grf1,grf0 note=constant-position-vs-removes-vf-attribute-dependency-before-urb-eot\n",
            const_urb_accepted as u8,
        );
        let const_handle_probes = [
            (
                PDOANE_CONST_URB_HANDLE_G0_PORT,
                crate::intel::shader::triangle_const_urb_handle_g0_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G1_PORT,
                crate::intel::shader::triangle_const_urb_handle_g1_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G2_PORT,
                crate::intel::shader::triangle_const_urb_handle_g2_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G3_PORT,
                crate::intel::shader::triangle_const_urb_handle_g3_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G4_PORT,
                crate::intel::shader::triangle_const_urb_handle_g4_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G5_PORT,
                crate::intel::shader::triangle_const_urb_handle_g5_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G6_PORT,
                crate::intel::shader::triangle_const_urb_handle_g6_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G7_PORT,
                crate::intel::shader::triangle_const_urb_handle_g7_probe_pipeline(),
            ),
        ];
        let mut const_handle_accepted = false;
        for (handle_port, handle_pipeline) in const_handle_probes {
            match upload_triangle_shader_pipeline(warm, handle_pipeline) {
                Ok(handle_shader_layout) => {
                    log_uploaded_triangle_shader_verification(
                        warm,
                        handle_pipeline,
                        handle_shader_layout,
                        handle_port.submit_name,
                    );
                    const_handle_accepted |= submit_pdoane_vs_streamout_proof(
                        dev,
                        warm,
                        draw,
                        handle_pipeline,
                        handle_shader_layout,
                        handle_port,
                    );
                    recover_render_engine_after_nonretired_submit(
                        dev,
                        warm,
                        handle_port.submit_name,
                    );
                }
                Err(reason) => {
                    intel_render_focus_log!(
                        "intel/render: pdoane-smoke const-urb-handle-proof skipped submit={} reason=shader-layout detail={}\n",
                        handle_port.submit_name,
                        reason,
                    );
                }
            }
        }
        urb_payload_accepted |= const_handle_accepted;
        intel_render_focus_log!(
            "intel/render: pdoane-smoke const-urb-handle-proof accepted={} variants=g0,g1,g2,g3,g4,g5,g6,g7 note=sweep-urb-send-header-source-and-brw-asm-exact-send-control\n",
            const_handle_accepted as u8,
        );
        let gen7_passthrough_pipeline =
            crate::intel::shader::triangle_pdoane_gen7_passthrough_probe_pipeline();
        let gen7_passthrough_accepted = match upload_triangle_shader_pipeline(
            warm,
            gen7_passthrough_pipeline,
        ) {
            Ok(gen7_shader_layout) => {
                log_uploaded_triangle_shader_verification(
                    warm,
                    gen7_passthrough_pipeline,
                    gen7_shader_layout,
                    PDOANE_GEN7_PASSTHROUGH_REFERENCE.submit_name,
                );
                let accepted = submit_pdoane_vs_streamout_proof(
                    dev,
                    warm,
                    draw,
                    gen7_passthrough_pipeline,
                    gen7_shader_layout,
                    PDOANE_GEN7_PASSTHROUGH_REFERENCE,
                );
                recover_render_engine_after_nonretired_submit(
                    dev,
                    warm,
                    PDOANE_GEN7_PASSTHROUGH_REFERENCE.submit_name,
                );
                accepted
            }
            Err(reason) => {
                intel_render_focus_log!(
                    "intel/render: pdoane-smoke gen7-passthrough-proof skipped reason=shader-layout detail={}\n",
                    reason,
                );
                false
            }
        };
        urb_payload_accepted |= gen7_passthrough_accepted;
        intel_render_focus_log!(
            "intel/render: pdoane-smoke gen7-passthrough-proof accepted={} note=direct-vendored-pdoane-ivybridge-eu-bytes-on-current-gfx125-engine\n",
            gen7_passthrough_accepted as u8,
        );
        let hdc_store_accepted = submit_pdoane_vs_hdc_store_proof(dev, warm, draw);
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            PDOANE_VS_HDC_STORE_REFERENCE.submit_name,
        );
        let hdc_bti34_store_accepted =
            submit_pdoane_vs_hdc_bti34_store_proof(dev, warm, draw);
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            PDOANE_VS_HDC_BTI34_STORE_REFERENCE.submit_name,
        );
        let hdc_store_ts_eot_accepted =
            submit_pdoane_vs_hdc_store_ts_eot_proof(dev, warm, draw);
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            PDOANE_VS_HDC_STORE_TS_EOT_REFERENCE.submit_name,
        );
        urb_payload_accepted |=
            hdc_store_accepted || hdc_bti34_store_accepted || hdc_store_ts_eot_accepted;
        intel_render_focus_log!(
            "intel/render: pdoane-smoke urb-payload-proof accepted={} vf_streamout_proof={} const_urb={} const_urb_handle={} gen7_passthrough={} vs_hdc_store={} vs_hdc_bti34_store={} vs_hdc_store_ts_eot={} variants=grf2,grf2-slot0,grf2-header-pos,grf2-dw8zero,grf2-vs-output-read1,grf2-mesa-ve-urb1,grf2-max1,grf2-single,grf2-urb1,grf2-vsread1,grf1,grf0,mesa12-slot1,mesa12-slot0,const-urb-ksp0-grf2,const-urb-grf2,const-urb-grf1,const-urb-grf0,const-urb-handle-g0,const-urb-handle-g1,const-urb-handle-g2,const-urb-handle-g3,const-urb-handle-g4,const-urb-handle-g5,const-urb-handle-g6,const-urb-handle-g7,gen7-pdoane-passthrough,vs-hdc-store,vs-hdc-bti34-store,vs-hdc-store-ts-eot experiment={} note=draw-follows-with-fresh-batch\n",
            urb_payload_accepted as u8,
            vf_streamout_accepted as u8,
            const_urb_accepted as u8,
            const_handle_accepted as u8,
            gen7_passthrough_accepted as u8,
            hdc_store_accepted as u8,
            hdc_bti34_store_accepted as u8,
            hdc_store_ts_eot_accepted as u8,
            reference.streamout.label(),
        );
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "pdoane-urb-payload-proof-isolation",
        );
    }

    let (shader_layout, probe_state) = if ran_payload_proofs {
        if write_pdoane_reference_vertices_for(warm, reference).is_none() {
            crate::log!(
                "intel/render: pdoane-smoke skipped reason=vertex-restore active={} stride={}\n",
                reference.name,
                draw.vertex_stride
            );
            return false;
        }
        let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
            Ok(layout) => layout,
            Err(reason) => {
                crate::log!(
                    "intel/render: pdoane-smoke skipped reason=shader-restore detail={}\n",
                    reason
                );
                return false;
            }
        };
        let probe_state =
            match write_triangle_probe_state(warm, draw, shader_layout, blend_mode, backend_mode) {
                Ok(layout) => layout,
                Err(reason) => {
                    crate::log!(
                        "intel/render: pdoane-smoke skipped reason=probe-state-restore detail={}\n",
                        reason
                    );
                    return false;
                }
            };
        intel_render_focus_log!(
            "intel/render: pdoane-smoke state-restore accepted=1 note=normal-pipeline-restored-after-vs-hdc-store-proof\n",
        );
        (shader_layout, probe_state)
    } else {
        (shader_layout, probe_state)
    };

    let mut active_reference = reference;
    let mut active_draw = draw;
    let mut active_shader_layout = shader_layout;
    let mut active_probe_state = probe_state;
    let mut active_blend_mode = blend_mode;
    let mut active_backend_mode = backend_mode;
    let mut active_batch_mode = TriangleBatchMode::Draw;
    let mut active_streamout = active_reference.streamout;
    let mut active_pipeline = pipeline;
    if PDOANE_ACTIVE_MESA_CONTROL_DRAW {
        if quiet_periodic {
            set_render_focus_log_suppressed(true);
        }
        if let Some(mesa_draw) = prepare_mesa_reference_draw_resources_from_pdoane_target(warm, draw)
        {
            let mesa_pipeline = crate::intel::shader::triangle_pipeline();
            match upload_triangle_shader_pipeline(warm, mesa_pipeline) {
                Ok(mesa_shader_layout) => {
                    match write_triangle_probe_state(
                        warm,
                        mesa_draw,
                        mesa_shader_layout,
                        MESA_ACTIVE_GRF2_REFERENCE_PORT.blend,
                        MESA_ACTIVE_GRF2_REFERENCE_PORT.backend,
                    ) {
                        Ok(mesa_probe_state) => {
                            active_reference = MESA_ACTIVE_GRF2_REFERENCE_PORT;
                            active_draw = mesa_draw;
                            active_shader_layout = mesa_shader_layout;
                            active_probe_state = mesa_probe_state;
                            active_blend_mode = MESA_ACTIVE_GRF2_REFERENCE_PORT.blend;
                            active_backend_mode = MESA_ACTIVE_GRF2_REFERENCE_PORT.backend;
                            active_streamout = MESA_ACTIVE_GRF2_REFERENCE_PORT.streamout;
                            active_pipeline = mesa_pipeline;
                            intel_render_focus_log!(
                                "intel/render: pdoane-smoke active-control mode=mesa-reference enabled=1 shader=oracle-triangle-vs front_end={} backend={} draw_stride={} vertex_count={} note=control-draw-uses-host-oracle-vs-bytes-and-active-backend\n",
                                active_reference.front_end.label,
                                active_backend_mode.label(),
                                active_draw.vertex_stride,
                                active_draw.vertex_count,
                            );
                        }
                        Err(reason) => {
                            intel_render_focus_log!(
                                "intel/render: pdoane-smoke active-control mode=mesa-reference enabled=0 reason=probe-state detail={} fallback={}\n",
                                reason,
                                reference.name,
                            );
                        }
                    }
                }
                Err(reason) => {
                    intel_render_focus_log!(
                        "intel/render: pdoane-smoke active-control mode=mesa-reference enabled=0 reason=shader-layout detail={} fallback={}\n",
                        reason,
                        reference.name,
                    );
                }
            }
        } else {
            intel_render_focus_log!(
                "intel/render: pdoane-smoke active-control mode=mesa-reference enabled=0 reason=mesa-draw-resources fallback={}\n",
                reference.name,
            );
        }
        if quiet_periodic {
            set_render_focus_log_suppressed(false);
        }
    }
    if PDOANE_ACTIVE_VF_SYNTHESIZED_CONTROL_DRAW {
        if quiet_periodic {
            set_render_focus_log_suppressed(true);
        }
        if let Some(mut vf_draw) = prepare_vf_streamout_proof_resources(
            warm,
            draw.rt_gpu_addr,
            draw.rt_pitch as usize,
            draw.target_w as usize,
            draw.target_h as usize,
            PDOANE_ACTIVE_VF_SYNTHESIZED_EXPERIMENT,
            PDOANE_ACTIVE_VF_SYNTHESIZED_GEOMETRY,
        ) {
            match upload_triangle_shader_pipeline(warm, pipeline) {
                Ok(vf_shader_layout) => {
                    let vf_backend_mode = PDOANE_ACTIVE_VF_SYNTHESIZED_BACKEND;
                    if matches!(
                        vf_backend_mode,
                        BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
                    ) && matches!(
                        PDOANE_ACTIVE_VF_SYNTHESIZED_EXPERIMENT,
                        StreamoutProofExperiment::PositionSlot1
                    ) {
                        if let Some(rect_draw) = write_pdoane_active_rectlist_i16_vertices(
                            warm,
                            vf_draw,
                            PDOANE_ACTIVE_VF_SYNTHESIZED_GEOMETRY,
                        ) {
                            vf_draw = rect_draw;
                        }
                    }
                    match write_triangle_probe_state(
                        warm,
                        vf_draw,
                        vf_shader_layout,
                        active_blend_mode,
                        vf_backend_mode,
                    ) {
                        Ok(vf_probe_state) => {
                            active_reference = PDOANE_VF_CONTROL_REFERENCE;
                            active_draw = vf_draw;
                            active_shader_layout = vf_shader_layout;
                            active_probe_state = vf_probe_state;
                            active_blend_mode = PDOANE_VF_CONTROL_REFERENCE.blend;
                            active_backend_mode = vf_backend_mode;
                            active_batch_mode = TriangleBatchMode::VfDraw;
                            active_streamout = PDOANE_ACTIVE_VF_SYNTHESIZED_EXPERIMENT;
                            intel_render_focus_log!(
                                "intel/render: pdoane-smoke active-control mode=vf-synthesized-vue enabled=1 experiment={} geometry={} backend={} draw_stride={} vertex_count={} note=control-draw-bypasses-vs-ksp-urb-export-to-test-fixed-function-clip-raster-ps-frontier\n",
                                active_streamout.label(),
                                PDOANE_ACTIVE_VF_SYNTHESIZED_GEOMETRY.label(),
                                active_backend_mode.label(),
                                active_draw.vertex_stride,
                                active_draw.vertex_count,
                            );
                        }
                        Err(reason) => {
                            intel_render_focus_log!(
                                "intel/render: pdoane-smoke active-control mode=vf-synthesized-vue enabled=0 reason=probe-state detail={} fallback_mode={:?}\n",
                                reason,
                                active_batch_mode,
                            );
                        }
                    }
                }
                Err(reason) => {
                    intel_render_focus_log!(
                        "intel/render: pdoane-smoke active-control mode=vf-synthesized-vue enabled=0 reason=shader-layout detail={} fallback_mode={:?}\n",
                        reason,
                        active_batch_mode,
                    );
                }
            }
        } else {
            intel_render_focus_log!(
                "intel/render: pdoane-smoke active-control mode=vf-synthesized-vue enabled=0 reason=vf-draw-resources experiment={} fallback_mode={:?}\n",
                PDOANE_ACTIVE_VF_SYNTHESIZED_EXPERIMENT.label(),
                active_batch_mode,
            );
        }
        if quiet_periodic {
            set_render_focus_log_suppressed(false);
        }
    }

    if PDOANE_TARGETED_URB_PAYLOAD_PROOF_ENABLED
        && !quiet_periodic
        && matches!(active_batch_mode, TriangleBatchMode::Draw)
    {
        let active_payload_accepted = submit_pdoane_vs_streamout_proof(
            dev,
            warm,
            active_draw,
            active_pipeline,
            active_shader_layout,
            active_reference,
        );
        recover_render_engine_after_nonretired_submit(dev, warm, active_reference.submit_name);

        let slot0_payload_accepted = if active_reference.source == MESA_REFERENCE_PORT.source {
            let accepted = submit_pdoane_vs_streamout_proof(
                dev,
                warm,
                active_draw,
                active_pipeline,
                active_shader_layout,
                MESA_ACTIVE_GRF2_REFERENCE_PORT_SLOT0,
            );
            recover_render_engine_after_nonretired_submit(
                dev,
                warm,
                MESA_ACTIVE_GRF2_REFERENCE_PORT_SLOT0.submit_name,
            );
            accepted
        } else {
            false
        };

        let const_handle_probes = [
            (
                PDOANE_CONST_URB_HANDLE_G0_PORT,
                crate::intel::shader::triangle_const_urb_handle_g0_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G1_PORT,
                crate::intel::shader::triangle_const_urb_handle_g1_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G2_PORT,
                crate::intel::shader::triangle_const_urb_handle_g2_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G3_PORT,
                crate::intel::shader::triangle_const_urb_handle_g3_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G4_PORT,
                crate::intel::shader::triangle_const_urb_handle_g4_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G5_PORT,
                crate::intel::shader::triangle_const_urb_handle_g5_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G6_PORT,
                crate::intel::shader::triangle_const_urb_handle_g6_probe_pipeline(),
            ),
            (
                PDOANE_CONST_URB_HANDLE_G7_PORT,
                crate::intel::shader::triangle_const_urb_handle_g7_probe_pipeline(),
            ),
        ];
        let mut const_handle_accepted = false;
        for (handle_port, handle_pipeline) in const_handle_probes {
            match upload_triangle_shader_pipeline(warm, handle_pipeline) {
                Ok(handle_shader_layout) => {
                    log_uploaded_triangle_shader_verification(
                        warm,
                        handle_pipeline,
                        handle_shader_layout,
                        handle_port.submit_name,
                    );
                    const_handle_accepted |= submit_pdoane_vs_streamout_proof(
                        dev,
                        warm,
                        active_draw,
                        handle_pipeline,
                        handle_shader_layout,
                        handle_port,
                    );
                    recover_render_engine_after_nonretired_submit(
                        dev,
                        warm,
                        handle_port.submit_name,
                    );
                }
                Err(reason) => {
                    intel_render_focus_log!(
                        "intel/render: pdoane-smoke targeted-const-urb-handle-proof skipped submit={} reason=shader-layout detail={}\n",
                        handle_port.submit_name,
                        reason,
                    );
                }
            }
        }

        let hdc_store_accepted = submit_pdoane_vs_hdc_store_proof(dev, warm, active_draw);
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            PDOANE_VS_HDC_STORE_REFERENCE.submit_name,
        );

        active_shader_layout = match upload_triangle_shader_pipeline(warm, active_pipeline) {
            Ok(layout) => layout,
            Err(reason) => {
                crate::log!(
                    "intel/render: pdoane-smoke skipped reason=targeted-urb-shader-restore detail={}\n",
                    reason
                );
                return false;
            }
        };
        active_probe_state = match write_triangle_probe_state(
            warm,
            active_draw,
            active_shader_layout,
            active_blend_mode,
            active_backend_mode,
        ) {
            Ok(layout) => layout,
            Err(reason) => {
                crate::log!(
                    "intel/render: pdoane-smoke skipped reason=targeted-urb-state-restore detail={}\n",
                    reason
                );
                return false;
            }
        };
        intel_render_focus_log!(
            "intel/render: pdoane-smoke targeted-urb-payload-proof accepted={} active={} slot0={} const_handle={} hdc_store={} active_ref={} active_streamout={} restored=1 note=visible-draw-follows-after-fresh-state\n",
            (active_payload_accepted
                || slot0_payload_accepted
                || const_handle_accepted
                || hdc_store_accepted) as u8,
            active_payload_accepted as u8,
            slot0_payload_accepted as u8,
            const_handle_accepted as u8,
            hdc_store_accepted as u8,
            active_reference.name,
            active_reference.streamout.label(),
        );
    }

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    if quiet_periodic {
        set_render_focus_log_suppressed(true);
    }
    let encoded_batch = encode_triangle_probe_batch(
        batch,
        warm,
        active_draw,
        active_blend_mode,
        active_pipeline,
        active_shader_layout,
        active_probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        active_batch_mode,
        active_streamout,
        active_reference.front_end,
        active_backend_mode,
        active_reference.post_draw_sync,
    );
    if quiet_periodic {
        set_render_focus_log_suppressed(false);
    }
    let batch_tail_bytes = match encoded_batch {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: pdoane-smoke skipped reason=batch-encode detail={}\n",
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    if PDOANE_SMOKE_VERBOSE_LOGS {
        intel_render_focus_log!(
            "intel/render: pdoane-smoke batch-ready trigger={} name={} bytes=0x{:X} rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} front_end={} blend={} backend={} streamout={} postdraw={} shape=reference-port prechecks=0 state_sequence=pipeline_select-sba-pointers-urb-vb-ve-vs-disable_hs_te_ds_gs-clip-streamout_off-sf-sbe-wm-ps-null_depth-real_3dprimitive source={} port_status=cohesive-xelp-port-not-gen7-copy\n",
            reason,
            active_reference.name,
            batch_tail_bytes,
            active_draw.rt_gpu_addr,
            active_draw.vertex_gpu_addr,
            active_draw.state_gpu_addr,
            active_draw.target_w,
            active_draw.target_h,
            active_draw.rt_pitch,
            active_reference.front_end.label,
            active_blend_mode.label(),
            active_backend_mode.label(),
            active_reference.streamout.label(),
            active_reference.post_draw_sync.label(),
            active_reference.source,
        );
    }

    if quiet_periodic {
        set_render_focus_log_suppressed(true);
    }
    let row_before = crate::intel::mmio_read(dev, ROW_INSTDONE);
    let sampler_before = crate::intel::mmio_read(dev, SAMPLER_INSTDONE);
    let tdl0_before = crate::intel::mmio_read(dev, TDL_THR_STATUS0);
    let tdl1_before = crate::intel::mmio_read(dev, TDL_THR_STATUS1);
    let tdl_disp_before = crate::intel::mmio_read(dev, TDL_THR_DISP_COUNT);
    let tdl_pf_before = crate::intel::mmio_read(dev, TDL_THR_PF_COUNT);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        PDOANE_SMOKE_SUBMIT_NAME,
    );
    if quiet_periodic {
        set_render_focus_log_suppressed(false);
    }
    let sc = crate::intel::mmio_read(dev, SC_INSTDONE);
    let sc_extra = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA);
    let sc_extra2 = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA2);
    let row_after = crate::intel::mmio_read(dev, ROW_INSTDONE);
    let sampler_after = crate::intel::mmio_read(dev, SAMPLER_INSTDONE);
    let tdl0_after = crate::intel::mmio_read(dev, TDL_THR_STATUS0);
    let tdl1_after = crate::intel::mmio_read(dev, TDL_THR_STATUS1);
    let tdl_disp_after = crate::intel::mmio_read(dev, TDL_THR_DISP_COUNT);
    let tdl_pf_after = crate::intel::mmio_read(dev, TDL_THR_PF_COUNT);
    let tdl_pf0 = crate::intel::mmio_read(dev, TDL_THR_PF_STATUS0);
    let tdl_pf1 = crate::intel::mmio_read(dev, TDL_THR_PF_STATUS1);
    let instps = crate::intel::mmio_read(dev, RCS_RING_INSTPS);
    let psmi_ctl = crate::intel::mmio_read(dev, RCS_RING_PSMI_CTL);
    let ring_instdone = crate::intel::mmio_read(dev, RCS_RING_INSTDONE);
    let rcu_mode = crate::intel::mmio_read(dev, GEN12_RCU_MODE);
    let mcr_selector = crate::intel::mmio_read(dev, MCR_SELECTOR);
    let chicken_raster_2 = crate::intel::mmio_read(dev, CHICKEN_RASTER_2);
    let gfx_mode = crate::intel::mmio_read(dev, GFX_MODE);
    if !quiet_periodic
        && !PDOANE_WM_PSD_LIVE_REGS_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed)
    {
        intel_render_focus_log!(
            "intel/render: pdoane-smoke wm-psd-live-regs completed={} backend={} row=0x{:08X}->0x{:08X}/d=0x{:08X} sampler=0x{:08X}->0x{:08X}/d=0x{:08X} tdl0=0x{:08X}->0x{:08X}/d=0x{:08X} tdl1=0x{:08X}->0x{:08X}/d=0x{:08X} tdl_disp=0x{:08X}->0x{:08X}/d=0x{:08X} tdl_pf=0x{:08X}->0x{:08X}/d=0x{:08X} tdl_pf0=0x{:08X} tdl_pf1=0x{:08X} sc=0x{:08X} sc_extra=0x{:08X} sc_extra2=0x{:08X} ring_instdone=0x{:08X} instps=0x{:08X} psmi_ctl=0x{:08X} rcu_mode=0x{:08X} mcr=0x{:08X} chicken_raster_2=0x{:08X} gfx_mode=0x{:08X} meaning=live-smoke-boundary-snapshot\n",
            completed as u8,
            active_backend_mode.label(),
            row_before,
            row_after,
            row_after.wrapping_sub(row_before),
            sampler_before,
            sampler_after,
            sampler_after.wrapping_sub(sampler_before),
            tdl0_before,
            tdl0_after,
            tdl0_after.wrapping_sub(tdl0_before),
            tdl1_before,
            tdl1_after,
            tdl1_after.wrapping_sub(tdl1_before),
            tdl_disp_before,
            tdl_disp_after,
            tdl_disp_after.wrapping_sub(tdl_disp_before),
            tdl_pf_before,
            tdl_pf_after,
            tdl_pf_after.wrapping_sub(tdl_pf_before),
            tdl_pf0,
            tdl_pf1,
            sc,
            sc_extra,
            sc_extra2,
            ring_instdone,
            instps,
            psmi_ctl,
            rcu_mode,
            mcr_selector,
            chicken_raster_2,
            gfx_mode,
        );
    }
    if PDOANE_SMOKE_VERBOSE_LOGS {
        intel_render_focus_log!(
            "intel/render: pdoane-smoke result completed={} trigger={} name={} note=single-end-to-end-reference-port-no-precheck-ladder\n",
            completed as u8,
            reason,
            reference.name,
        );
    }
    completed
}
