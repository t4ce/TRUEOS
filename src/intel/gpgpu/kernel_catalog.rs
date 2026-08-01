pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/copy_rect_rgba8.clcpp");
pub(crate) const COPY_RECT_RGBA8_SOURCE_PATH: &str =
    "src/intel/gpgpu/kernels/copy_rect_rgba8.clcpp";
pub(crate) const COPY_RECT_RGBA8_ARTIFACT_FRONTEND: &str = "cpp-for-opencl";
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME: &str = "resolve_tile64_msaa4_rgba8";
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/resolve_tile64_msaa4_rgba8.clcpp");
pub(crate) const FILL_RECT_RGBA8_KERNEL_NAME: &str = "fill_rect_rgba8";
pub(crate) const FILL_RECT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/fill_rect_rgba8.clcpp");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME: &str = "fill_rect_worklist_rgba8";
pub(crate) const FILL_RECT_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/fill_rect_worklist_rgba8.clcpp");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME: &str = "gradient_rect_worklist_rgba8";
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/gradient_rect_worklist_rgba8.clcpp");
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME: &str = "alpha_blend_worklist_rgba8";
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_worklist_rgba8.clcpp");
pub(crate) const GLYPH_MASK_RGBA8_KERNEL_NAME: &str = "glyph_mask_rgba8";
pub(crate) const GLYPH_MASK_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/glyph_mask_rgba8.clcpp");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME: &str =
    "ui4_nv12_tile64_to_rgba8_frame";
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_tile64_to_rgba8_frame.clcpp");
pub(crate) const UI4_RGBA8_TO_NV12_LINEAR_KERNEL_NAME: &str = "ui4_rgba8_to_nv12_linear";
pub(crate) const UI4_RGBA8_TO_NV12_LINEAR_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_rgba8_to_nv12_linear.clcpp");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite_quad_worklist_rgba8";
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite_quad_worklist_rgba8.clcpp");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME: &str = "ui4_compose_layers_rgba8";
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_compose_layers_rgba8.clcpp");
pub(crate) const MANDEL64_WORKLIST_RGBA8_KERNEL_NAME: &str = "mandel64_worklist_rgba8";
pub(crate) const MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/mandel64_worklist_rgba8.clcpp");
pub(crate) const SKYBOX_SAMPLE_RGB565_KERNEL_NAME: &str = "skybox_sample_rgb565";
pub(crate) const SKYBOX_SAMPLE_RGB565_OPENCL_SOURCE: &str =
    include_str!("kernels/skybox_sample_rgb565.clcpp");
pub(crate) const CHART_SINE_RGBA8_KERNEL_NAME: &str = "chart_sine_rgba8";
pub(crate) const CHART_SINE_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/chart_sine_rgba8.clcpp");
pub(crate) const PIXEL_PLASMA_RGBA8_KERNEL_NAME: &str = "pixel_plasma_rgba8";
pub(crate) const PIXEL_PLASMA_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/pixel_plasma_rgba8.clcpp");
pub(crate) const CPP_DEMO_RGBA8_KERNEL_NAME: &str = "cpp_demo_rgba8";
pub(crate) const CPP_DEMO_RGBA8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/cpp_demo_rgba8.clcpp");
pub(crate) const CPP_DEMO_RGBA8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/cpp_demo_rgba8.clcpp";
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME: &str = "cpp_audio_visualizer_rgba8";
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/cpp_audio_visualizer_rgba8.clcpp");
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/cpp_audio_visualizer_rgba8.clcpp";
pub(crate) const PARTICLE_CRAFT_KERNEL_NAME: &str = "particle_craft";
pub(crate) const PARTICLE_CRAFT_STEP_KERNEL_NAME: &str = "particle_craft_step";
pub(crate) const PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME: &str = "particle_craft_bin_tiles";
pub(crate) const PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME: &str = "particle_craft_render_rgba8";
pub(crate) const PARTICLE_CRAFT_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/particle_craft.clcpp");
pub(crate) const PARTICLE_CRAFT_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/particle_craft.clcpp";
pub(crate) const FONT_INSTANCE_RGBA8_KERNEL_NAME: &str = "font_instance_rgba8";
pub(crate) const FONT_INSTANCE_RGBA8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/font_instance_rgba8.clcpp");
pub(crate) const FONT_INSTANCE_RGBA8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/font_instance_rgba8.clcpp";
pub(crate) const LFM25_Q8_PROJECT_PACKED_KERNEL_NAME: &str = "lfm25_q8_project_packed";
pub(crate) const LFM25_Q8_PROJECT_PACKED_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/lfm25_q8_project_packed.clcpp");
pub(crate) const LFM25_Q8_PROJECT_PACKED_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/lfm25_q8_project_packed.clcpp";
pub(crate) const KOKORO_QGEMM_U8_I8_KERNEL_NAME: &str = "kokoro_qgemm_u8_i8";
pub(crate) const KOKORO_QGEMM_U8_I8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/kokoro_qgemm_u8_i8.clcpp");
pub(crate) const KOKORO_QGEMM_U8_I8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/kokoro_qgemm_u8_i8.clcpp";
pub(crate) const KOKORO_CONV1D_U8_U8_KERNEL_NAME: &str = "kokoro_conv1d_u8_u8";
pub(crate) const KOKORO_CONV1D_U8_U8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/kokoro_conv1d_u8_u8.clcpp");
pub(crate) const KOKORO_CONV1D_U8_U8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/kokoro_conv1d_u8_u8.clcpp";
pub(crate) const FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME: &str = "font_outline_coverage_r8";
pub(crate) const FONT_OUTLINE_COVERAGE_R8_OPENCL_SOURCE: &str =
    include_str!("kernels/font_outline_coverage_r8.clcpp");
pub(crate) const SCENE_AABB_KERNEL_NAME: &str = "scene_aabb";
pub(crate) const SCENE_AABB_OPENCL_SOURCE: &str = include_str!("kernels/scene_aabb.clcpp");
pub(crate) const HELIO_RETAINED_TRANSFORM_KERNEL_NAME: &str = "helio_retained_transform";
pub(crate) const HELIO_RETAINED_TRANSFORM_OPENCL_SOURCE: &str =
    include_str!("kernels/helio_retained_transform.clcpp");
pub(crate) const HELIO_RETAINED_TRANSFORM_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/helio_retained_transform.clcpp";
pub(crate) const LAB256_MULTIPHASE_KERNEL_NAME: &str = "lab256_multiphase";
pub(crate) const LAB256_MULTIPHASE_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/lab256_multiphase.clcpp");
pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME: &str = "spirit_vfx_background_rgba8";
pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/spirit_vfx_background_rgba8.clcpp");
pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/spirit_vfx_background_rgba8.clcpp";
pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME: &str = "spirit_vfx_sprite_rgba8";
pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/spirit_vfx_sprite_rgba8.clcpp");
pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/spirit_vfx_sprite_rgba8.clcpp";

pub(crate) fn kernel_opencl_source(name: &str) -> Option<&'static str> {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME => Some(COPY_RECT_RGBA8_OPENCL_SOURCE),
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME => Some(RESOLVE_TILE64_MSAA4_RGBA8_OPENCL_SOURCE),
        FILL_RECT_RGBA8_KERNEL_NAME => Some(FILL_RECT_RGBA8_OPENCL_SOURCE),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => Some(FILL_RECT_WORKLIST_RGBA8_OPENCL_SOURCE),
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some(GRADIENT_RECT_WORKLIST_RGBA8_OPENCL_SOURCE)
        }
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE),
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some(GLYPH_MASK_RGBA8_OPENCL_SOURCE),
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some(UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE)
        }
        UI4_RGBA8_TO_NV12_LINEAR_KERNEL_NAME => Some(UI4_RGBA8_TO_NV12_LINEAR_OPENCL_SOURCE),
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => Some(SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE),
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => Some(UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE),
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => Some(MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE),
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some(SKYBOX_SAMPLE_RGB565_OPENCL_SOURCE),
        CHART_SINE_RGBA8_KERNEL_NAME => Some(CHART_SINE_RGBA8_OPENCL_SOURCE),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some(PIXEL_PLASMA_RGBA8_OPENCL_SOURCE),
        CPP_DEMO_RGBA8_KERNEL_NAME => Some(CPP_DEMO_RGBA8_OPENCL_SOURCE),
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME => Some(CPP_AUDIO_VISUALIZER_RGBA8_OPENCL_SOURCE),
        PARTICLE_CRAFT_KERNEL_NAME
        | PARTICLE_CRAFT_STEP_KERNEL_NAME
        | PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME
        | PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME => Some(PARTICLE_CRAFT_OPENCL_SOURCE),
        FONT_INSTANCE_RGBA8_KERNEL_NAME => Some(FONT_INSTANCE_RGBA8_OPENCL_SOURCE),
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => Some(LFM25_Q8_PROJECT_PACKED_OPENCL_SOURCE),
        KOKORO_QGEMM_U8_I8_KERNEL_NAME => Some(KOKORO_QGEMM_U8_I8_OPENCL_SOURCE),
        KOKORO_CONV1D_U8_U8_KERNEL_NAME => Some(KOKORO_CONV1D_U8_U8_OPENCL_SOURCE),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => Some(FONT_OUTLINE_COVERAGE_R8_OPENCL_SOURCE),
        SCENE_AABB_KERNEL_NAME => Some(SCENE_AABB_OPENCL_SOURCE),
        HELIO_RETAINED_TRANSFORM_KERNEL_NAME => Some(HELIO_RETAINED_TRANSFORM_OPENCL_SOURCE),
        LAB256_MULTIPHASE_KERNEL_NAME => Some(LAB256_MULTIPHASE_OPENCL_SOURCE),
        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_BACKGROUND_RGBA8_OPENCL_SOURCE),
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_SPRITE_RGBA8_OPENCL_SOURCE),
        _ => None,
    }
}

pub(crate) fn kernel_source_path(name: &str) -> Option<&'static str> {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME => Some(COPY_RECT_RGBA8_SOURCE_PATH),
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/resolve_tile64_msaa4_rgba8.clcpp")
        }
        FILL_RECT_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/fill_rect_rgba8.clcpp"),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/fill_rect_worklist_rgba8.clcpp")
        }
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/gradient_rect_worklist_rgba8.clcpp")
        }
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/alpha_blend_worklist_rgba8.clcpp")
        }
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/glyph_mask_rgba8.clcpp"),
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.clcpp")
        }
        UI4_RGBA8_TO_NV12_LINEAR_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_rgba8_to_nv12_linear.clcpp")
        }
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/sprite_quad_worklist_rgba8.clcpp")
        }
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_compose_layers_rgba8.clcpp")
        }
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/mandel64_worklist_rgba8.clcpp")
        }
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/skybox_sample_rgb565.clcpp")
        }
        CHART_SINE_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/chart_sine_rgba8.clcpp"),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/pixel_plasma_rgba8.clcpp"),
        CPP_DEMO_RGBA8_KERNEL_NAME => Some(CPP_DEMO_RGBA8_SOURCE_PATH),
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME => Some(CPP_AUDIO_VISUALIZER_RGBA8_SOURCE_PATH),
        PARTICLE_CRAFT_KERNEL_NAME
        | PARTICLE_CRAFT_STEP_KERNEL_NAME
        | PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME
        | PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME => Some(PARTICLE_CRAFT_SOURCE_PATH),
        FONT_INSTANCE_RGBA8_KERNEL_NAME => Some(FONT_INSTANCE_RGBA8_SOURCE_PATH),
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => Some(LFM25_Q8_PROJECT_PACKED_SOURCE_PATH),
        KOKORO_QGEMM_U8_I8_KERNEL_NAME => Some(KOKORO_QGEMM_U8_I8_SOURCE_PATH),
        KOKORO_CONV1D_U8_U8_KERNEL_NAME => Some(KOKORO_CONV1D_U8_U8_SOURCE_PATH),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/font_outline_coverage_r8.clcpp")
        }
        SCENE_AABB_KERNEL_NAME => Some("src/intel/gpgpu/kernels/scene_aabb.clcpp"),
        HELIO_RETAINED_TRANSFORM_KERNEL_NAME => Some(HELIO_RETAINED_TRANSFORM_SOURCE_PATH),
        LAB256_MULTIPHASE_KERNEL_NAME => {
            Some("crates/trueos-shader/gpgpu/kernels/lab256_multiphase.clcpp")
        }
        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_BACKGROUND_RGBA8_SOURCE_PATH),
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_SPRITE_RGBA8_SOURCE_PATH),
        _ => None,
    }
}

include!("kernels/artifacts/adls/cpp/copy_rect_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/alpha_blend_worklist_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/chart_sine_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/fill_rect_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/fill_rect_worklist_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/font_outline_coverage_r8.contract.rs");
include!("kernels/artifacts/adls/cpp/glyph_mask_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/gradient_rect_worklist_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/helio_retained_transform.contract.rs");
include!("kernels/artifacts/adls/cpp/lab256_multiphase.contract.rs");
include!("kernels/artifacts/adls/cpp/mandel64_worklist_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/pixel_plasma_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/resolve_tile64_msaa4_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/scene_aabb.contract.rs");
include!("kernels/artifacts/adls/cpp/skybox_sample_rgb565.contract.rs");
include!("kernels/artifacts/adls/cpp/sprite_quad_worklist_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/ui4_compose_layers_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/ui4_nv12_tile64_to_rgba8_frame.contract.rs");
include!("kernels/artifacts/adls/cpp/ui4_rgba8_to_nv12_linear.contract.rs");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/copy_rect_rgba8.bin");
pub(crate) const COPY_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/copy_rect_rgba8.spv");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(COPY_RECT_RGBA8_ADLS_BIN.len() == 11_328);
const _: () = assert!(COPY_RECT_RGBA8_ADLS_SPV.len() == 4_788);
const _: () = {
    let contract = COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.simd_width == 16);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 96);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.implicit_payload_args.len() == 3);
    assert!(matches!(
        contract.implicit_payload_args[0].kind,
        GpgpuArtifactImplicitArgKind::GlobalIdOffset
    ));
    assert!(
        contract.implicit_payload_args[0].offset_bytes == 0
            && contract.implicit_payload_args[0].size_bytes == 12
    );
    assert!(matches!(
        contract.implicit_payload_args[1].kind,
        GpgpuArtifactImplicitArgKind::LocalSize
    ));
    assert!(
        contract.implicit_payload_args[1].offset_bytes == 12
            && contract.implicit_payload_args[1].size_bytes == 12
    );
    assert!(matches!(
        contract.implicit_payload_args[2].kind,
        GpgpuArtifactImplicitArgKind::EnqueuedLocalSize
    ));
    assert!(
        contract.implicit_payload_args[2].offset_bytes == 32
            && contract.implicit_payload_args[2].size_bytes == 12
    );
    assert!(contract.per_thread_payload_args.len() == 1);
    assert!(matches!(
        contract.per_thread_payload_args[0].kind,
        GpgpuArtifactPerThreadArgKind::LocalId
    ));
    assert!(
        contract.per_thread_payload_args[0].offset_bytes == 0
            && contract.per_thread_payload_args[0].size_bytes == 96
    );
    assert!(contract.bindings.len() == 2);
    assert!(contract.bindings[0].arg_index == 0 && contract.bindings[0].bti == 0);
    assert!(contract.bindings[1].arg_index == 1 && contract.bindings[1].bti == 1);
    assert!(contract.payload_args.len() == 10);
    assert!(contract.payload_args[0].arg_index == 0);
    assert!(contract.payload_args[0].offset_bytes == 48);
    assert!(contract.payload_args[0].size_bytes == 8);
    assert!(matches!(contract.payload_args[0].kind, GpgpuArtifactArgKind::ByPointer));
    assert!(matches!(contract.payload_args[0].access, GpgpuArtifactArgAccess::ReadOnly));
    assert!(matches!(contract.payload_args[0].address_mode, GpgpuArtifactAddressMode::Stateful));
    assert!(contract.payload_args[1].arg_index == 1);
    assert!(contract.payload_args[1].offset_bytes == 56);
    assert!(contract.payload_args[1].size_bytes == 8);
    assert!(matches!(contract.payload_args[1].kind, GpgpuArtifactArgKind::ByPointer));
    assert!(matches!(contract.payload_args[1].access, GpgpuArtifactArgAccess::ReadWrite));
    assert!(matches!(contract.payload_args[1].address_mode, GpgpuArtifactAddressMode::Stateful));
    let mut scalar = 2;
    while scalar < contract.payload_args.len() {
        assert!(contract.payload_args[scalar].arg_index as usize == scalar);
        assert!(contract.payload_args[scalar].offset_bytes == 56 + scalar as u32 * 4);
        assert!(contract.payload_args[scalar].size_bytes == 4);
        assert!(matches!(contract.payload_args[scalar].kind, GpgpuArtifactArgKind::ByValue));
        scalar += 1;
    }
};
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/resolve_tile64_msaa4_rgba8.bin");
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/resolve_tile64_msaa4_rgba8.spv");
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/fill_rect_rgba8.bin");
pub(crate) const FILL_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/fill_rect_rgba8.spv");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/fill_rect_worklist_rgba8.bin");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/fill_rect_worklist_rgba8.spv");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/gradient_rect_worklist_rgba8.bin");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/gradient_rect_worklist_rgba8.spv");

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/alpha_blend_worklist_rgba8.bin");
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/alpha_blend_worklist_rgba8.spv");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/glyph_mask_rgba8.bin");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/glyph_mask_rgba8.spv");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/ui4_nv12_tile64_to_rgba8_frame.bin");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/ui4_nv12_tile64_to_rgba8_frame.spv");
const UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN_BYTES: usize =
    include_bytes!("kernels/artifacts/adls/cpp/ui4_rgba8_to_nv12_linear.bin").len();
#[used]
#[unsafe(link_section = ".gpgpu_artifacts")]
static UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN_STORAGE: [u8; UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN_BYTES] =
    *include_bytes!("kernels/artifacts/adls/cpp/ui4_rgba8_to_nv12_linear.bin");
pub(crate) const UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN: &[u8] =
    &UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN_STORAGE;
pub(crate) const UI4_RGBA8_TO_NV12_LINEAR_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/ui4_rgba8_to_nv12_linear.spv");

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/sprite_quad_worklist_rgba8.bin");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/sprite_quad_worklist_rgba8.spv");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/ui4_compose_layers_rgba8.bin");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/ui4_compose_layers_rgba8.spv");
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/mandel64_worklist_rgba8.bin");
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/mandel64_worklist_rgba8.spv");
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/skybox_sample_rgb565.bin");
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/skybox_sample_rgb565.spv");
pub(crate) const CHART_SINE_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/chart_sine_rgba8.bin");
pub(crate) const CHART_SINE_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/chart_sine_rgba8.spv");
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/pixel_plasma_rgba8.bin");
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/pixel_plasma_rgba8.spv");
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_demo_rgba8.contract.rs"
);
pub(crate) const CPP_DEMO_RGBA8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_demo_rgba8.bin"
);
pub(crate) const CPP_DEMO_RGBA8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_demo_rgba8.spv"
);
pub(crate) const CPP_DEMO_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = {
    let contract = CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 128);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 1);
    assert!(contract.bindings[0].arg_index == 0);
    assert!(contract.bindings[0].bti == 0);
    assert!(contract.payload_args.len() == 12);
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_audio_visualizer_rgba8.contract.rs"
);
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_audio_visualizer_rgba8.bin"
);
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_audio_visualizer_rgba8.spv"
);
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () =
    assert!(matches!(CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = {
    let contract = CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 96);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 2);
    assert!(contract.bindings[0].arg_index == 0);
    assert!(contract.bindings[0].bti == 0);
    assert!(contract.bindings[1].arg_index == 1);
    assert!(contract.bindings[1].bti == 1);
    assert!(contract.payload_args.len() == 8);
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/particle_craft.contract.rs"
);
pub(crate) const PARTICLE_CRAFT_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/particle_craft.bin"
);
pub(crate) const PARTICLE_CRAFT_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/particle_craft.spv"
);
pub(crate) const PARTICLE_CRAFT_ADLS_BIN_SHA256: [u8; 32] =
    PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(matches!(PARTICLE_CRAFT_BIN_TILES_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () =
    assert!(matches!(PARTICLE_CRAFT_RENDER_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(PARTICLE_CRAFT_ADLS_BIN.len() == 157_536);
const _: () = assert!(PARTICLE_CRAFT_ADLS_SPV.len() == 91_764);
const _: () = {
    let step = PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT;
    let bin = PARTICLE_CRAFT_BIN_TILES_ADLS_CPP_ABI_CONTRACT;
    let render = PARTICLE_CRAFT_RENDER_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(step.target.pci_device_ids.len() == 1);
    assert!(step.target.pci_device_ids[0] == 0x4680);
    assert!(step.target.revision_min == 0x0C && step.target.revision_max == 0x0C);
    assert!(bin.target.pci_device_ids.len() == 1);
    assert!(bin.target.pci_device_ids[0] == 0x4680);
    assert!(bin.target.revision_min == 0x0C && bin.target.revision_max == 0x0C);
    assert!(render.target.pci_device_ids.len() == 1);
    assert!(render.target.pci_device_ids[0] == 0x4680);
    assert!(render.target.revision_min == 0x0C && render.target.revision_max == 0x0C);
    let mut digest_byte = 0;
    while digest_byte < 32 {
        assert!(step.zebin_sha256[digest_byte] == bin.zebin_sha256[digest_byte]);
        assert!(step.zebin_sha256[digest_byte] == render.zebin_sha256[digest_byte]);
        assert!(step.spv_sha256[digest_byte] == bin.spv_sha256[digest_byte]);
        assert!(step.spv_sha256[digest_byte] == render.spv_sha256[digest_byte]);
        digest_byte += 1;
    }
    assert!(step.simd_width == 16 && bin.simd_width == 16 && render.simd_width == 16);
    assert!(step.grf_count == 128 && bin.grf_count == 128 && render.grf_count == 128);
    assert!(step.scratch_bytes == 0 && bin.scratch_bytes == 0 && render.scratch_bytes == 0);
    assert!(step.slm_bytes == 0 && bin.slm_bytes == 0 && render.slm_bytes == 0);
    assert!(step.cross_thread_data_bytes == 64);
    assert!(bin.cross_thread_data_bytes == 96);
    assert!(render.cross_thread_data_bytes == 96);
    assert!(
        step.per_thread_data_bytes == 96
            && bin.per_thread_data_bytes == 96
            && render.per_thread_data_bytes == 96
    );
    assert!(step.bindings.len() == 2 && bin.bindings.len() == 3 && render.bindings.len() == 4);
    assert!(
        step.payload_args.len() == 2
            && bin.payload_args.len() == 3
            && render.payload_args.len() == 4
    );
    assert!(step.payload_args[0].offset_bytes == 48);
    assert!(step.payload_args[1].offset_bytes == 56);
    assert!(bin.payload_args[0].offset_bytes == 48);
    assert!(bin.payload_args[1].offset_bytes == 56);
    assert!(bin.payload_args[2].offset_bytes == 64);
    assert!(render.payload_args[0].offset_bytes == 48);
    assert!(render.payload_args[1].offset_bytes == 56);
    assert!(render.payload_args[2].offset_bytes == 64);
    assert!(render.payload_args[3].offset_bytes == 72);
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/font_instance_rgba8.contract.rs"
);
pub(crate) const FONT_INSTANCE_RGBA8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/font_instance_rgba8.bin"
);
pub(crate) const FONT_INSTANCE_RGBA8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/font_instance_rgba8.spv"
);
pub(crate) const FONT_INSTANCE_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    FONT_INSTANCE_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(FONT_INSTANCE_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(FONT_INSTANCE_RGBA8_ADLS_BIN.len() == 72_896);
const _: () = {
    let contract = FONT_INSTANCE_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.entry_offset == 64);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 128);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 3);
    assert!(contract.bindings[0].arg_index == 0 && contract.bindings[0].bti == 0);
    assert!(contract.bindings[1].arg_index == 1 && contract.bindings[1].bti == 1);
    assert!(contract.bindings[2].arg_index == 2 && contract.bindings[2].bti == 2);
    assert!(contract.payload_args.len() == 11);
    assert!(contract.payload_args[0].offset_bytes == 48);
    assert!(contract.payload_args[1].offset_bytes == 56);
    assert!(contract.payload_args[2].offset_bytes == 64);
    let mut scalar = 3;
    while scalar < contract.payload_args.len() {
        assert!(contract.payload_args[scalar].arg_index as usize == scalar);
        assert!(contract.payload_args[scalar].offset_bytes == 60 + scalar as u32 * 4);
        assert!(contract.payload_args[scalar].size_bytes == 4);
        assert!(matches!(contract.payload_args[scalar].kind, GpgpuArtifactArgKind::ByValue));
        scalar += 1;
    }
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lfm25_q8_project_packed.contract.rs"
);
pub(crate) const LFM25_Q8_PROJECT_PACKED_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lfm25_q8_project_packed.bin"
);
pub(crate) const LFM25_Q8_PROJECT_PACKED_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lfm25_q8_project_packed.spv"
);
pub(crate) const LFM25_Q8_PROJECT_PACKED_ADLS_BIN_SHA256: [u8; 32] =
    LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(LFM25_Q8_PROJECT_PACKED_ADLS_BIN.len() == 23_432);
const _: () = assert!(LFM25_Q8_PROJECT_PACKED_ADLS_SPV.len() == 14_740);
const _: () = {
    let contract = LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.entry_offset == 64);
    assert!(contract.entry_size == 3_856);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 96);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 3);
    assert!(contract.bindings[0].arg_index == 0 && contract.bindings[0].bti == 0);
    assert!(contract.bindings[1].arg_index == 1 && contract.bindings[1].bti == 1);
    assert!(contract.bindings[2].arg_index == 2 && contract.bindings[2].bti == 2);
    assert!(contract.payload_args.len() == 6);
    assert!(contract.payload_args[0].offset_bytes == 48);
    assert!(contract.payload_args[1].offset_bytes == 56);
    assert!(contract.payload_args[2].offset_bytes == 64);
    assert!(contract.payload_args[3].offset_bytes == 72);
    assert!(contract.payload_args[4].offset_bytes == 76);
    assert!(contract.payload_args[5].offset_bytes == 80);
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/kokoro_qgemm_u8_i8.contract.rs"
);
pub(crate) const KOKORO_QGEMM_U8_I8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/kokoro_qgemm_u8_i8.bin"
);
pub(crate) const KOKORO_QGEMM_U8_I8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/kokoro_qgemm_u8_i8.spv"
);
pub(crate) const KOKORO_QGEMM_U8_I8_ADLS_BIN_SHA256: [u8; 32] =
    KOKORO_QGEMM_U8_I8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(KOKORO_QGEMM_U8_I8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(KOKORO_QGEMM_U8_I8_ADLS_BIN.len() == 24_032);
const _: () = assert!(KOKORO_QGEMM_U8_I8_ADLS_SPV.len() == 10_212);
const _: () = {
    let contract = KOKORO_QGEMM_U8_I8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.entry_offset == 64);
    assert!(contract.entry_size == 5_592);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 128);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 6);
    assert!(contract.payload_args.len() == 14);
    let mut pointer = 0;
    while pointer < 6 {
        assert!(contract.bindings[pointer].arg_index as usize == pointer);
        assert!(contract.bindings[pointer].bti as usize == pointer);
        assert!(contract.payload_args[pointer].offset_bytes == 48 + pointer as u32 * 8);
        pointer += 1;
    }
    let mut scalar = 6;
    while scalar < contract.payload_args.len() {
        assert!(contract.payload_args[scalar].arg_index as usize == scalar);
        assert!(contract.payload_args[scalar].offset_bytes == 72 + scalar as u32 * 4);
        assert!(contract.payload_args[scalar].size_bytes == 4);
        assert!(matches!(contract.payload_args[scalar].kind, GpgpuArtifactArgKind::ByValue));
        scalar += 1;
    }
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/kokoro_conv1d_u8_u8.contract.rs"
);
pub(crate) const KOKORO_CONV1D_U8_U8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/kokoro_conv1d_u8_u8.bin"
);
pub(crate) const KOKORO_CONV1D_U8_U8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/kokoro_conv1d_u8_u8.spv"
);
pub(crate) const KOKORO_CONV1D_U8_U8_ADLS_BIN_SHA256: [u8; 32] =
    KOKORO_CONV1D_U8_U8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(KOKORO_CONV1D_U8_U8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(KOKORO_CONV1D_U8_U8_ADLS_BIN.len() == 29_480);
const _: () = assert!(KOKORO_CONV1D_U8_U8_ADLS_SPV.len() == 15_124);
const _: () = {
    let contract = KOKORO_CONV1D_U8_U8_ADLS_CPP_ABI_CONTRACT;
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.entry_offset == 64);
    assert!(contract.entry_size == 6_056);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 128);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 4);
    assert!(contract.payload_args.len() == 16);
    let mut pointer = 0;
    while pointer < 4 {
        assert!(contract.bindings[pointer].arg_index as usize == pointer);
        assert!(contract.bindings[pointer].bti as usize == pointer);
        assert!(contract.payload_args[pointer].offset_bytes == 48 + pointer as u32 * 8);
        pointer += 1;
    }
    let mut scalar = 4;
    while scalar < contract.payload_args.len() {
        assert!(contract.payload_args[scalar].arg_index as usize == scalar);
        assert!(contract.payload_args[scalar].offset_bytes == 64 + scalar as u32 * 4);
        assert!(contract.payload_args[scalar].size_bytes == 4);
        assert!(matches!(contract.payload_args[scalar].kind, GpgpuArtifactArgKind::ByValue));
        scalar += 1;
    }
};
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/font_outline_coverage_r8.bin");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/font_outline_coverage_r8.spv");
pub(crate) const SCENE_AABB_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/scene_aabb.bin");
pub(crate) const SCENE_AABB_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/scene_aabb.spv");
// Keep the native retained-transform Zebin in release images even while the
// ADL-S compatibility policy parks its submission path.  `.gpgpu_artifacts`
// is retained by the linker script specifically for dormant, explicitly
// gated GPU experiments; parking the path must not silently erase its baked
// provenance from the image.
const HELIO_RETAINED_TRANSFORM_ADLS_BIN_BYTES: usize =
    include_bytes!("kernels/artifacts/adls/cpp/helio_retained_transform.bin").len();
#[used]
#[unsafe(link_section = ".gpgpu_artifacts")]
static HELIO_RETAINED_TRANSFORM_ADLS_BIN_STORAGE: [u8; HELIO_RETAINED_TRANSFORM_ADLS_BIN_BYTES] =
    *include_bytes!("kernels/artifacts/adls/cpp/helio_retained_transform.bin");
pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_BIN: &[u8] =
    &HELIO_RETAINED_TRANSFORM_ADLS_BIN_STORAGE;
pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/helio_retained_transform.spv");
pub(crate) const LAB256_MULTIPHASE_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lab256_multiphase.bin"
);
pub(crate) const LAB256_MULTIPHASE_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lab256_multiphase.spv"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.contract.rs"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.contract.rs"
);
pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.bin"
);
pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.spv"
);
pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.bin"
);
pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv"
);
pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    SPIRIT_VFX_SPRITE_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN.len() == 109_608);
const _: () = assert!(SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN.len() == 656_728);
const _: () = {
    let background = SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_CPP_ABI_CONTRACT;
    let sprite = SPIRIT_VFX_SPRITE_RGBA8_ADLS_CPP_ABI_CONTRACT;
    assert!(matches!(background.validate(), Ok(())));
    assert!(matches!(sprite.validate(), Ok(())));
    assert!(background.target.pci_device_ids.len() == 1);
    assert!(background.target.pci_device_ids[0] == 0x4680);
    assert!(background.target.revision_min == 0x0C);
    assert!(background.target.revision_max == 0x0C);
    assert!(background.simd_width == 16);
    assert!(background.scratch_bytes == 0);
    assert!(background.slm_bytes == 0);
    assert!(background.cross_thread_data_bytes == 64);
    assert!(background.per_thread_data_bytes == 96);
    assert!(background.bindings.len() == 2);
    assert!(sprite.target.pci_device_ids.len() == 1);
    assert!(sprite.target.pci_device_ids[0] == 0x4680);
    assert!(sprite.target.revision_min == 0x0C);
    assert!(sprite.target.revision_max == 0x0C);
    assert!(sprite.simd_width == 16);
    assert!(sprite.scratch_bytes == 0);
    assert!(sprite.slm_bytes == 0);
    assert!(sprite.cross_thread_data_bytes == 96);
    assert!(sprite.per_thread_data_bytes == 96);
    assert!(sprite.bindings.len() == 3);
};
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    RESOLVE_TILE64_MSAA4_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    FILL_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    FILL_RECT_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(FILL_RECT_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    GRADIENT_RECT_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    ALPHA_BLEND_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () =
    assert!(matches!(ALPHA_BLEND_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    GLYPH_MASK_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_SHA256: [u8; 32] =
    UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN_SHA256: [u8; 32] =
    UI4_RGBA8_TO_NV12_LINEAR_ADLS_CPP_ABI_CONTRACT.zebin_sha256;

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    SPRITE_QUAD_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    UI4_COMPOSE_LAYERS_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    MANDEL64_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_BIN_SHA256: [u8; 32] =
    SKYBOX_SAMPLE_RGB565_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const CHART_SINE_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    CHART_SINE_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_BIN_SHA256: [u8; 32] =
    PIXEL_PLASMA_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_BIN_SHA256: [u8; 32] =
    FONT_OUTLINE_COVERAGE_R8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const SCENE_AABB_ADLS_BIN_SHA256: [u8; 32] =
    SCENE_AABB_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_BIN_SHA256: [u8; 32] =
    HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = {
    let contract = HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT;
    assert!(matches!(contract.validate(), Ok(())));
    assert!(HELIO_RETAINED_TRANSFORM_ADLS_BIN.len() == 208_096);
    assert!(HELIO_RETAINED_TRANSFORM_ADLS_SPV.len() == 150_604);
    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.entry_offset == 64);
    assert!(contract.entry_size == 43_096);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 224);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 16);
    assert!(contract.payload_args.len() == 24);
};
pub(crate) const LAB256_MULTIPHASE_ADLS_BIN_SHA256: [u8; 32] =
    LAB256_STEP_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
