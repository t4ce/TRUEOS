pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
#[cfg(not(feature = "intel_gpu_cpp_aot"))]
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/copy_rect_rgba8.cl");
#[cfg(feature = "intel_gpu_cpp_aot")]
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/copy_rect_rgba8.clcpp");
#[cfg(not(feature = "intel_gpu_cpp_aot"))]
pub(crate) const COPY_RECT_RGBA8_SOURCE_PATH: &str = "src/intel/gpgpu/kernels/copy_rect_rgba8.cl";
#[cfg(feature = "intel_gpu_cpp_aot")]
pub(crate) const COPY_RECT_RGBA8_SOURCE_PATH: &str =
    "src/intel/gpgpu/kernels/copy_rect_rgba8.clcpp";
#[cfg(not(feature = "intel_gpu_cpp_aot"))]
pub(crate) const COPY_RECT_RGBA8_ARTIFACT_FRONTEND: &str = "opencl-c";
#[cfg(feature = "intel_gpu_cpp_aot")]
pub(crate) const COPY_RECT_RGBA8_ARTIFACT_FRONTEND: &str = "cpp-for-opencl";
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME: &str = "resolve_tile64_msaa4_rgba8";
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/resolve_tile64_msaa4_rgba8.cl");
pub(crate) const FILL_RECT_RGBA8_KERNEL_NAME: &str = "fill_rect_rgba8";
pub(crate) const FILL_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/fill_rect_rgba8.cl");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME: &str = "fill_rect_worklist_rgba8";
pub(crate) const FILL_RECT_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/fill_rect_worklist_rgba8.cl");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME: &str = "gradient_rect_worklist_rgba8";
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/gradient_rect_worklist_rgba8.cl");
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME: &str = "alpha_blend_worklist_rgba8";
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_worklist_rgba8.cl");
pub(crate) const GLYPH_MASK_RGBA8_KERNEL_NAME: &str = "glyph_mask_rgba8";
pub(crate) const GLYPH_MASK_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/glyph_mask_rgba8.cl");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME: &str =
    "ui4_nv12_ytile_to_primary_xrgb";
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_ytile_to_primary_xrgb.cl");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME: &str =
    "ui4_nv12_tile64_to_rgba8_frame";
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_tile64_to_rgba8_frame.cl");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite_quad_worklist_rgba8";
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite_quad_worklist_rgba8.cl");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME: &str = "ui4_compose_layers_rgba8";
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_compose_layers_rgba8.cl");
pub(crate) const MANDEL64_WORKLIST_RGBA8_KERNEL_NAME: &str = "mandel64_worklist_rgba8";
pub(crate) const MANDEL64_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/mandel64_worklist_rgba8.cl");
pub(crate) const SKYBOX_SAMPLE_RGB565_KERNEL_NAME: &str = "skybox_sample_rgb565";
pub(crate) const SKYBOX_SAMPLE_RGB565_OPENCL_SOURCE: &str =
    include_str!("kernels/skybox_sample_rgb565.cl");
pub(crate) const CHART_SINE_RGBA8_KERNEL_NAME: &str = "chart_sine_rgba8";
pub(crate) const CHART_SINE_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/chart_sine_rgba8.cl");
pub(crate) const PIXEL_PLASMA_RGBA8_KERNEL_NAME: &str = "pixel_plasma_rgba8";
pub(crate) const PIXEL_PLASMA_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/pixel_plasma_rgba8.cl");
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
pub(crate) const LFM25_Q8_PROJECT_KERNEL_NAME: &str = "lfm25_q8_project";
pub(crate) const LFM25_Q8_PROJECT_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/lfm25_q8_project.clcpp");
pub(crate) const LFM25_Q8_PROJECT_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/lfm25_q8_project.clcpp";
pub(crate) const LFM25_Q8_PROJECT_PACKED_KERNEL_NAME: &str = "lfm25_q8_project_packed";
pub(crate) const LFM25_Q8_PROJECT_PACKED_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/lfm25_q8_project_packed.clcpp");
pub(crate) const LFM25_Q8_PROJECT_PACKED_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/lfm25_q8_project_packed.clcpp";
pub(crate) const FONT_OUTLINE_MESH_KERNEL_NAME: &str = "font_outline_mesh";
pub(crate) const FONT_OUTLINE_MESH_OPENCL_SOURCE: &str =
    include_str!("kernels/font_outline_mesh.cl");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME: &str = "font_outline_coverage_r8";
pub(crate) const FONT_OUTLINE_COVERAGE_R8_OPENCL_SOURCE: &str =
    include_str!("kernels/font_outline_coverage_r8.cl");
pub(crate) const SCENE_AABB_KERNEL_NAME: &str = "scene_aabb";
pub(crate) const SCENE_AABB_OPENCL_SOURCE: &str = include_str!("kernels/scene_aabb.cl");
pub(crate) const LAB256_MULTIPHASE_KERNEL_NAME: &str = "lab256_multiphase";
pub(crate) const LAB256_MULTIPHASE_OPENCL_SOURCE: &str =
    include_str!("../../../crates/trueos-shader/gpgpu/kernels/lab256_multiphase.cl");
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
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME => {
            Some(UI4_NV12_YTILE_TO_PRIMARY_XRGB_OPENCL_SOURCE)
        }
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some(UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE)
        }
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
        LFM25_Q8_PROJECT_KERNEL_NAME => Some(LFM25_Q8_PROJECT_OPENCL_SOURCE),
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => Some(LFM25_Q8_PROJECT_PACKED_OPENCL_SOURCE),
        FONT_OUTLINE_MESH_KERNEL_NAME => Some(FONT_OUTLINE_MESH_OPENCL_SOURCE),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => Some(FONT_OUTLINE_COVERAGE_R8_OPENCL_SOURCE),
        SCENE_AABB_KERNEL_NAME => Some(SCENE_AABB_OPENCL_SOURCE),
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
            Some("src/intel/gpgpu/kernels/resolve_tile64_msaa4_rgba8.cl")
        }
        FILL_RECT_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/fill_rect_rgba8.cl"),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/fill_rect_worklist_rgba8.cl")
        }
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/gradient_rect_worklist_rgba8.cl")
        }
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/alpha_blend_worklist_rgba8.cl")
        }
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/glyph_mask_rgba8.cl"),
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_ytile_to_primary_xrgb.cl")
        }
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.cl")
        }
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/sprite_quad_worklist_rgba8.cl")
        }
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_compose_layers_rgba8.cl")
        }
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/mandel64_worklist_rgba8.cl")
        }
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some("src/intel/gpgpu/kernels/skybox_sample_rgb565.cl"),
        CHART_SINE_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/chart_sine_rgba8.cl"),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some("src/intel/gpgpu/kernels/pixel_plasma_rgba8.cl"),
        CPP_DEMO_RGBA8_KERNEL_NAME => Some(CPP_DEMO_RGBA8_SOURCE_PATH),
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME => Some(CPP_AUDIO_VISUALIZER_RGBA8_SOURCE_PATH),
        PARTICLE_CRAFT_KERNEL_NAME
        | PARTICLE_CRAFT_STEP_KERNEL_NAME
        | PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME
        | PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME => Some(PARTICLE_CRAFT_SOURCE_PATH),
        FONT_INSTANCE_RGBA8_KERNEL_NAME => Some(FONT_INSTANCE_RGBA8_SOURCE_PATH),
        LFM25_Q8_PROJECT_KERNEL_NAME => Some(LFM25_Q8_PROJECT_SOURCE_PATH),
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => Some(LFM25_Q8_PROJECT_PACKED_SOURCE_PATH),
        FONT_OUTLINE_MESH_KERNEL_NAME => Some("src/intel/gpgpu/kernels/font_outline_mesh.cl"),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/font_outline_coverage_r8.cl")
        }
        SCENE_AABB_KERNEL_NAME => Some("src/intel/gpgpu/kernels/scene_aabb.cl"),
        LAB256_MULTIPHASE_KERNEL_NAME => {
            Some("crates/trueos-shader/gpgpu/kernels/lab256_multiphase.cl")
        }
        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_BACKGROUND_RGBA8_SOURCE_PATH),
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_SPRITE_RGBA8_SOURCE_PATH),
        _ => None,
    }
}

#[cfg(not(feature = "intel_gpu_cpp_aot"))]
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.bin");
#[cfg(not(feature = "intel_gpu_cpp_aot"))]
pub(crate) const COPY_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.spv");
#[cfg(feature = "intel_gpu_cpp_aot")]
include!("kernels/artifacts/adls/cpp/copy_rect_rgba8.contract.rs");
#[cfg(feature = "intel_gpu_cpp_aot")]
pub(crate) const COPY_RECT_RGBA8_CPP_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/copy_rect_rgba8.bin");
#[cfg(feature = "intel_gpu_cpp_aot")]
pub(crate) const COPY_RECT_RGBA8_CPP_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/cpp/copy_rect_rgba8.spv");
#[cfg(feature = "intel_gpu_cpp_aot")]
pub(crate) const COPY_RECT_RGBA8_CPP_ADLS_BIN_SHA256: [u8; 32] =
    COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
#[cfg(feature = "intel_gpu_cpp_aot")]
const _: () = assert!(matches!(COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
#[cfg(feature = "intel_gpu_cpp_aot")]
const _: () = assert!(COPY_RECT_RGBA8_CPP_ADLS_BIN.len() == 11_328);
#[cfg(feature = "intel_gpu_cpp_aot")]
const _: () = assert!(COPY_RECT_RGBA8_CPP_ADLS_SPV.len() == 4_788);
#[cfg(feature = "intel_gpu_cpp_aot")]
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
    include_bytes!("kernels/artifacts/adls/resolve_tile64_msaa4_rgba8.bin");
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/resolve_tile64_msaa4_rgba8.spv");
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_rgba8.bin");
pub(crate) const FILL_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_rgba8.spv");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_worklist_rgba8.bin");
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/fill_rect_worklist_rgba8.spv");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/gradient_rect_worklist_rgba8.bin");
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/gradient_rect_worklist_rgba8.spv");

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/alpha_blend_worklist_rgba8.bin");
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/alpha_blend_worklist_rgba8.spv");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/glyph_mask_rgba8.bin");
pub(crate) const GLYPH_MASK_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/glyph_mask_rgba8.spv");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_ytile_to_primary_xrgb.bin");
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_ytile_to_primary_xrgb.spv");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_tile64_to_rgba8_frame.bin");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_nv12_tile64_to_rgba8_frame.spv");

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite_quad_worklist_rgba8.bin");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/sprite_quad_worklist_rgba8.spv");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_compose_layers_rgba8.bin");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/ui4_compose_layers_rgba8.spv");
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/mandel64_worklist_rgba8.bin");
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/mandel64_worklist_rgba8.spv");
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/skybox_sample_rgb565.bin");
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/skybox_sample_rgb565.spv");
pub(crate) const CHART_SINE_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/chart_sine_rgba8.bin");
pub(crate) const CHART_SINE_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/chart_sine_rgba8.spv");
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/pixel_plasma_rgba8.bin");
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/pixel_plasma_rgba8.spv");
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
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lfm25_q8_project.contract.rs"
);
pub(crate) const LFM25_Q8_PROJECT_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lfm25_q8_project.bin"
);
pub(crate) const LFM25_Q8_PROJECT_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lfm25_q8_project.spv"
);
pub(crate) const LFM25_Q8_PROJECT_ADLS_BIN_SHA256: [u8; 32] =
    LFM25_Q8_PROJECT_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(LFM25_Q8_PROJECT_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = {
    let contract = LFM25_Q8_PROJECT_ADLS_CPP_ABI_CONTRACT;
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
    assert!(contract.bindings.len() == 3);
    assert!(contract.payload_args.len() == 6);
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
pub(crate) const FONT_OUTLINE_MESH_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_mesh.bin");
pub(crate) const FONT_OUTLINE_MESH_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_mesh.spv");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_coverage_r8.bin");
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/font_outline_coverage_r8.spv");
pub(crate) const SCENE_AABB_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/scene_aabb.bin");
pub(crate) const SCENE_AABB_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/scene_aabb.spv");
pub(crate) const LAB256_MULTIPHASE_ADLS_BIN: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/lab256_multiphase.bin"
);
pub(crate) const LAB256_MULTIPHASE_ADLS_SPV: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/lab256_multiphase.spv"
);
const _: () = assert!(LAB256_MULTIPHASE_ADLS_BIN.len() == 52_632);
const _: () = assert!(LAB256_MULTIPHASE_ADLS_SPV.len() == 28_884);
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
#[cfg(not(feature = "intel_gpu_cpp_aot"))]
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x10, 0x86, 0x60, 0x24, 0xAA, 0xFF, 0xAE, 0x96, 0xF9, 0x2C, 0xFC, 0x25, 0xA5, 0xFB, 0x18, 0x8C,
    0xA4, 0x21, 0x99, 0x47, 0x89, 0xAF, 0xBC, 0x4D, 0xBA, 0x3D, 0xDC, 0x29, 0x0B, 0xD5, 0x83, 0xAB,
];
pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x89, 0xFA, 0xF1, 0x14, 0xBA, 0x35, 0x1D, 0xFE, 0x8C, 0x5A, 0xF8, 0x99, 0x66, 0x00, 0x26, 0xD4,
    0x66, 0xF1, 0x1D, 0xDF, 0x86, 0xF1, 0xE2, 0x8C, 0x0F, 0x18, 0x98, 0xCA, 0x9B, 0x5B, 0xD7, 0xDF,
];
pub(crate) const FILL_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xAB, 0x51, 0x9A, 0x0E, 0x4E, 0x47, 0x31, 0xE5, 0x8F, 0xF6, 0x5D, 0x75, 0xBF, 0x92, 0x93, 0x4C,
    0xD7, 0x31, 0xA0, 0x88, 0x23, 0xB0, 0x40, 0x28, 0x62, 0x0E, 0x86, 0x54, 0x9F, 0x45, 0x06, 0xF4,
];
pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xCF, 0x5B, 0x9B, 0x47, 0xCF, 0xBC, 0x5E, 0xD2, 0x2A, 0x90, 0x4A, 0x37, 0xF2, 0x49, 0xDA, 0x42,
    0xB7, 0xC9, 0x9B, 0x1D, 0x4F, 0x45, 0x28, 0xBC, 0xA2, 0xB8, 0xC6, 0x0E, 0x54, 0x03, 0x62, 0x79,
];
pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xC0, 0x3A, 0xEE, 0xFC, 0x4D, 0x20, 0x23, 0xD5, 0xEE, 0x70, 0x3C, 0x5D, 0xBB, 0xB3, 0x1E, 0xBC,
    0x20, 0x93, 0xB1, 0x04, 0xBE, 0x00, 0xDB, 0x2B, 0xC7, 0x8D, 0x29, 0xC5, 0x30, 0xF4, 0x27, 0x37,
];

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xF9, 0x0C, 0x66, 0xC5, 0xFB, 0xA3, 0xED, 0x22, 0xEB, 0x42, 0xD0, 0x08, 0xAC, 0x94, 0x38, 0x2F,
    0xD7, 0x4D, 0x35, 0xF5, 0x5C, 0x41, 0x04, 0x4C, 0x10, 0x14, 0x49, 0x70, 0x95, 0xAB, 0x3C, 0xD3,
];
pub(crate) const GLYPH_MASK_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x90, 0x8D, 0xF0, 0x7D, 0x62, 0xB0, 0x69, 0xF3, 0x1A, 0x04, 0x6D, 0x29, 0x02, 0xDF, 0xF9, 0xA0,
    0xFA, 0x33, 0xE4, 0x9A, 0x1C, 0x25, 0x3B, 0x74, 0xA4, 0xE7, 0xCC, 0x18, 0xDF, 0x66, 0xD3, 0x78,
];
pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN_SHA256: [u8; 32] = [
    0x65, 0xEA, 0x37, 0xF3, 0xCE, 0x33, 0xAC, 0x92, 0x67, 0x92, 0x80, 0x34, 0xC3, 0xE5, 0x59, 0xF8,
    0x85, 0x47, 0xE9, 0x02, 0x9D, 0x29, 0x22, 0x31, 0xC9, 0x11, 0x6F, 0x25, 0x85, 0x13, 0x2E, 0x4D,
];
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_SHA256: [u8; 32] = [
    0xF3, 0x3F, 0x0F, 0x2F, 0x53, 0x1A, 0xA4, 0xDF, 0x74, 0xB9, 0x32, 0xFD, 0x51, 0x9D, 0x5C, 0x09,
    0x6F, 0x95, 0x76, 0xB9, 0x4C, 0x09, 0xCF, 0x1E, 0x20, 0xB7, 0x42, 0x15, 0x10, 0x92, 0xE0, 0xB5,
];

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x8D, 0xFC, 0x62, 0x17, 0xFF, 0x63, 0x46, 0xFE, 0x26, 0x60, 0x07, 0x9F, 0xC9, 0x05, 0xED, 0x5E,
    0x48, 0x18, 0x7A, 0xF4, 0x8B, 0x0C, 0x90, 0xC5, 0xE0, 0xD5, 0xE5, 0x6A, 0x80, 0xEF, 0x34, 0x37,
];
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xFE, 0x3D, 0xC7, 0xDF, 0x1B, 0x4B, 0x8B, 0x50, 0x31, 0x3C, 0xCE, 0x32, 0xF4, 0xAA, 0xB0, 0xA5,
    0xDE, 0xDA, 0x1A, 0x22, 0x95, 0x68, 0x83, 0x10, 0x9D, 0x9F, 0xF0, 0xD6, 0xF7, 0x82, 0x8B, 0x14,
];
pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x8B, 0x17, 0x46, 0x98, 0x4F, 0x74, 0x15, 0x6C, 0xCD, 0xBE, 0xB9, 0x43, 0x1D, 0xF9, 0xD2, 0x50,
    0x61, 0x28, 0x56, 0x55, 0x06, 0x7D, 0xE8, 0xEB, 0xD5, 0x28, 0x3B, 0x08, 0xDE, 0x00, 0xD9, 0x1F,
];
pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_BIN_SHA256: [u8; 32] = [
    0xC0, 0xBA, 0x26, 0xB3, 0x1C, 0x54, 0xED, 0x16, 0xC8, 0x32, 0x3E, 0x3D, 0xE6, 0x54, 0xAA, 0x90,
    0x8A, 0xB4, 0xBC, 0xDC, 0xCF, 0xC8, 0xBB, 0xD5, 0xF1, 0x05, 0x2F, 0x1B, 0xF7, 0x75, 0x76, 0x38,
];
pub(crate) const CHART_SINE_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x79, 0xeb, 0x20, 0xbc, 0x33, 0x7e, 0x17, 0x2a, 0x8c, 0xcd, 0xdc, 0xdc, 0x66, 0x54, 0xee, 0xa9,
    0x92, 0xe8, 0x9f, 0xb5, 0xfb, 0x67, 0xb2, 0xf3, 0x2c, 0xaa, 0xd1, 0xc1, 0xaf, 0xa1, 0xc0, 0xe4,
];
pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0x42, 0xfb, 0x1d, 0xd0, 0x56, 0x8b, 0xb2, 0x44, 0xc4, 0x4f, 0x87, 0xd1, 0x46, 0xe0, 0x36, 0xa7,
    0x2d, 0xf6, 0x0c, 0xb8, 0x11, 0x71, 0x5c, 0x37, 0x0e, 0xc9, 0x59, 0xde, 0x6d, 0x3a, 0xf8, 0x93,
];
pub(crate) const FONT_OUTLINE_MESH_ADLS_BIN_SHA256: [u8; 32] = [
    0xbf, 0x78, 0xe5, 0xd6, 0x87, 0x0f, 0x23, 0x03, 0xb7, 0x07, 0xd3, 0x03, 0x20, 0xd8, 0xda, 0xa1,
    0x55, 0x54, 0x08, 0x5a, 0x75, 0xd4, 0x7a, 0x48, 0xb5, 0x1f, 0xb9, 0x32, 0xf4, 0xfa, 0x3d, 0x25,
];
pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_BIN_SHA256: [u8; 32] = [
    0xA4, 0xF0, 0xDD, 0xDC, 0x7F, 0x2A, 0x9D, 0x9D, 0x67, 0xE5, 0xE7, 0x14, 0x59, 0xD5, 0x4D, 0xA2,
    0xE4, 0xA7, 0xAD, 0xE8, 0xCD, 0x1A, 0xF8, 0xC2, 0x72, 0x83, 0xA8, 0x84, 0xF2, 0x21, 0xB8, 0x36,
];
pub(crate) const SCENE_AABB_ADLS_BIN_SHA256: [u8; 32] = [
    0xB4, 0x1B, 0xA8, 0x00, 0x0A, 0x68, 0x2A, 0xAC, 0x20, 0x1B, 0xB0, 0x49, 0x88, 0x51, 0xD2, 0x16,
    0x0D, 0x9F, 0xAF, 0xFE, 0x4A, 0x12, 0x09, 0x1D, 0xB2, 0x8E, 0x11, 0x55, 0xB6, 0x0F, 0x3F, 0x2D,
];
pub(crate) const LAB256_MULTIPHASE_ADLS_BIN_SHA256: [u8; 32] = [
    0x6F, 0x51, 0xFF, 0x13, 0x4F, 0x9F, 0x1F, 0xA2, 0x2C, 0xC2, 0x13, 0xD2, 0x48, 0x18, 0xA6, 0x7D,
    0x75, 0x93, 0xBC, 0x98, 0x24, 0x1C, 0xE9, 0x2E, 0xCD, 0xA9, 0x8E, 0x50, 0xC2, 0x92, 0x96, 0xD2,
];
