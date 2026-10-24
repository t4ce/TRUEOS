pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/copy_rect_rgba8.clcpp");
pub(crate) const COPY_RECT_RGBA8_SOURCE_PATH: &str =
    "src/intel/gpgpu/kernels/copy_rect_rgba8.clcpp";
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME: &str = "alpha_blend_worklist_rgba8";
pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/alpha_blend_worklist_rgba8.clcpp");
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME: &str =
    "ui4_nv12_tile64_to_rgba8_frame";
pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_nv12_tile64_to_rgba8_frame.clcpp");
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME: &str = "sprite_quad_worklist_rgba8";
pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/sprite_quad_worklist_rgba8.clcpp");
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME: &str = "ui4_compose_layers_rgba8";
pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE: &str =
    include_str!("kernels/ui4_compose_layers_rgba8.clcpp");
pub(crate) const SKYBOX_SAMPLE_RGB565_KERNEL_NAME: &str = "skybox_sample_rgb565";
pub(crate) const CPP_DEMO_RGBA8_KERNEL_NAME: &str = "cpp_demo_rgba8";
pub(crate) const CPP_DEMO_RGBA8_SOURCE_PATH: &str =
    "../TRUEOS-Blueprints/apps/shadertoy/assets/cpp_gallery/kernel.clcpp";
pub(crate) const SHADERTOY_MANDELBROT_KERNEL_NAME: &str = "shadertoy_mandelbrot";
pub(crate) const SHADERTOY_CUBE_FIELD_KERNEL_NAME: &str = "shadertoy_cube_field";
pub(crate) const SHADERTOY_NGUYEN_KERNEL_NAME: &str = "shadertoy_nguyen";
pub(crate) const SHADERTOY_MANDELBOX_KERNEL_NAME: &str = "shadertoy_mandelbox";
pub(crate) const SHADERTOY_PALETTE_GRID_KERNEL_NAME: &str = "shadertoy_palette_grid";
pub(crate) const SHADERTOY_COSMIC_STRANDS_KERNEL_NAME: &str = "shadertoy_cosmic_strands";
pub(crate) const SHADERTOY_MANDELBROT_SOURCE_PATH: &str =
    "Blueprint:shadertoy/assets/mandelbrot/kernel.clcpp";
pub(crate) const SHADERTOY_CUBE_FIELD_SOURCE_PATH: &str =
    "Blueprint:shadertoy/assets/cube_field/kernel.clcpp";
pub(crate) const SHADERTOY_NGUYEN_SOURCE_PATH: &str =
    "Blueprint:shadertoy/assets/nguyen/kernel.clcpp";
pub(crate) const SHADERTOY_PALETTE_GRID_SOURCE_PATH: &str =
    "Blueprint:shadertoy/assets/palette_grid/kernel.clcpp";
pub(crate) const SHADERTOY_COSMIC_STRANDS_SOURCE_PATH: &str =
    "Blueprint:shadertoy/assets/cosmic_strands/kernel.clcpp";
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME: &str = "cpp_audio_visualizer_rgba8";
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_SOURCE_PATH: &str =
    "../TRUEOS-Blueprints/apps/shadertoy/assets/audio_visualizer/kernel.clcpp";
pub(crate) const PARTICLE_CRAFT_KERNEL_NAME: &str = "particle_craft";
pub(crate) const PARTICLE_CRAFT_STEP_KERNEL_NAME: &str = "particle_craft_step";
pub(crate) const PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME: &str = "particle_craft_bin_tiles";
pub(crate) const PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME: &str = "particle_craft_render_rgba8";
pub(crate) const PARTICLE_CRAFT_SOURCE_PATH: &str =
    "../TRUEOS-Blueprints/apps/shadertoy/assets/particle_craft/kernel.clcpp";
pub(crate) const HELIO_RETAINED_TRANSFORM_KERNEL_NAME: &str = "helio_retained_transform";
pub(crate) const HELIO_RETAINED_TRANSFORM_OPENCL_SOURCE: &str =
    include_str!("kernels/helio_retained_transform.clcpp");
pub(crate) const HELIO_RETAINED_TRANSFORM_SOURCE_PATH: &str =
    "crates/trueos-shader/gpgpu/kernels/helio_retained_transform.clcpp";
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

        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(ALPHA_BLEND_WORKLIST_RGBA8_OPENCL_SOURCE),

        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some(UI4_NV12_TILE64_TO_RGBA8_FRAME_OPENCL_SOURCE)
        }
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => Some(SPRITE_QUAD_WORKLIST_RGBA8_OPENCL_SOURCE),
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => Some(UI4_COMPOSE_LAYERS_RGBA8_OPENCL_SOURCE),
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => None,
        CPP_DEMO_RGBA8_KERNEL_NAME => None,
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME => None,
        PARTICLE_CRAFT_KERNEL_NAME
        | PARTICLE_CRAFT_STEP_KERNEL_NAME
        | PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME
        | PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME => None,




        HELIO_RETAINED_TRANSFORM_KERNEL_NAME => Some(HELIO_RETAINED_TRANSFORM_OPENCL_SOURCE),

        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_BACKGROUND_RGBA8_OPENCL_SOURCE),
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_SPRITE_RGBA8_OPENCL_SOURCE),
        _ => None,
    }
}

pub(crate) fn kernel_source_path(name: &str) -> Option<&'static str> {
    match name {

        COPY_RECT_RGBA8_KERNEL_NAME => Some(COPY_RECT_RGBA8_SOURCE_PATH),

        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/alpha_blend_worklist_rgba8.clcpp")
        }

        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.clcpp")
        }
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/sprite_quad_worklist_rgba8.clcpp")
        }
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => {
            Some("src/intel/gpgpu/kernels/ui4_compose_layers_rgba8.clcpp")
        }
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => {
            Some("Blueprint:skybox/assets/skybox_sample_rgb565/kernel.clcpp")
        }
        CPP_DEMO_RGBA8_KERNEL_NAME => Some(CPP_DEMO_RGBA8_SOURCE_PATH),
        SHADERTOY_MANDELBROT_KERNEL_NAME => Some(SHADERTOY_MANDELBROT_SOURCE_PATH),
        SHADERTOY_CUBE_FIELD_KERNEL_NAME => Some(SHADERTOY_CUBE_FIELD_SOURCE_PATH),
        SHADERTOY_NGUYEN_KERNEL_NAME => Some(SHADERTOY_NGUYEN_SOURCE_PATH),
        SHADERTOY_MANDELBOX_KERNEL_NAME => Some("Blueprint:Cubes/Cube/mandelbox/kernel.clcpp"),
        SHADERTOY_PALETTE_GRID_KERNEL_NAME => Some(SHADERTOY_PALETTE_GRID_SOURCE_PATH),
        SHADERTOY_COSMIC_STRANDS_KERNEL_NAME => Some(SHADERTOY_COSMIC_STRANDS_SOURCE_PATH),
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME => Some(CPP_AUDIO_VISUALIZER_RGBA8_SOURCE_PATH),
        PARTICLE_CRAFT_KERNEL_NAME
        | PARTICLE_CRAFT_STEP_KERNEL_NAME
        | PARTICLE_CRAFT_BIN_TILES_KERNEL_NAME
        | PARTICLE_CRAFT_RENDER_RGBA8_KERNEL_NAME => Some(PARTICLE_CRAFT_SOURCE_PATH),




        HELIO_RETAINED_TRANSFORM_KERNEL_NAME => Some(HELIO_RETAINED_TRANSFORM_SOURCE_PATH),

        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_BACKGROUND_RGBA8_SOURCE_PATH),
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => Some(SPIRIT_VFX_SPRITE_RGBA8_SOURCE_PATH),
        _ => None,
    }
}

include!("kernels/artifacts/adls/cpp/copy_rect_rgba8.contract.rs");

include!("kernels/artifacts/adls/cpp/alpha_blend_worklist_rgba8.contract.rs");


include!("kernels/artifacts/adls/cpp/helio_retained_transform.contract.rs");

include!("kernels/artifacts/adls/cpp/skybox_sample_rgb565.contract.rs");
include!("kernels/artifacts/adls/cpp/sprite_quad_worklist_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/ui4_compose_layers_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/ui4_nv12_tile64_to_rgba8_frame.contract.rs");
embedded_compute_artifact!(pub(crate) COPY_RECT_RGBA8_ADLS_BIN, COPY_RECT_RGBA8_ADLS_SPV,
    COPY_RECT_RGBA8_ADLS_BIN_STORAGE,
    "kernels/artifacts/adls/cpp/copy_rect_rgba8.bin"
);

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


embedded_compute_artifact!(pub(crate) ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN, ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV,
    ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_STORAGE,
    "kernels/artifacts/adls/cpp/alpha_blend_worklist_rgba8.bin"
);

embedded_compute_artifact!(pub(crate) UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN, UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV,
    UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_STORAGE,
    "kernels/artifacts/adls/cpp/ui4_nv12_tile64_to_rgba8_frame.bin"
);
embedded_compute_artifact!(pub(crate) SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN, SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV,
    SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN_STORAGE,
    "kernels/artifacts/adls/cpp/sprite_quad_worklist_rgba8.bin"
);
embedded_compute_artifact!(pub(crate) UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN, UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV,
    UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN_STORAGE,
    "kernels/artifacts/adls/cpp/ui4_compose_layers_rgba8.bin"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_demo_rgba8.contract.rs"
);
pub(crate) const CPP_DEMO_RGBA8_ADLS_BIN_BYTES: usize = shadertoy_package::SHADERTOY_CPP_GALLERY_PACKAGE.bin_bytes;
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
    assert!(contract.cross_thread_data_bytes == 288);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 1);
    assert!(contract.bindings[0].arg_index == 0);
    assert!(contract.bindings[0].bti == 0);
    assert!(contract.payload_args.len() == 15);
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_mandelbrot.contract.rs"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_cube_field.contract.rs"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_nguyen.contract.rs"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_mandelbox.contract.rs"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_palette_grid.contract.rs"
);
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_cosmic_strands.contract.rs"
);
const _: () = {
    let contracts = [
        (SHADERTOY_MANDELBROT_ADLS_CPP_ABI_CONTRACT, 5),
        (SHADERTOY_CUBE_FIELD_ADLS_CPP_ABI_CONTRACT, 5),
        (SHADERTOY_NGUYEN_ADLS_CPP_ABI_CONTRACT, 5),
        (SHADERTOY_MANDELBOX_ADLS_CPP_ABI_CONTRACT, 6),
        (SHADERTOY_PALETTE_GRID_ADLS_CPP_ABI_CONTRACT, 5),
        (SHADERTOY_COSMIC_STRANDS_ADLS_CPP_ABI_CONTRACT, 5),
    ];
    let mut index = 0;
    while index < contracts.len() {
        let (contract, payload_args) = contracts[index];
        assert!(matches!(contract.validate(), Ok(())));
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
        assert!(contract.payload_args.len() == payload_args);
        index += 1;
    }
    assert!(SHADERTOY_MANDELBROT_ADLS_CPP_ABI_CONTRACT.bindings.len() == 1);
    assert!(SHADERTOY_CUBE_FIELD_ADLS_CPP_ABI_CONTRACT.bindings.len() == 2);
    assert!(SHADERTOY_NGUYEN_ADLS_CPP_ABI_CONTRACT.bindings.len() == 1);
    assert!(SHADERTOY_MANDELBOX_ADLS_CPP_ABI_CONTRACT.bindings.len() == 3);
    assert!(SHADERTOY_PALETTE_GRID_ADLS_CPP_ABI_CONTRACT.bindings.len() == 1);
    assert!(
        SHADERTOY_COSMIC_STRANDS_ADLS_CPP_ABI_CONTRACT
            .bindings
            .len()
            == 1
    );
};
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/cpp_audio_visualizer_rgba8.contract.rs"
);
pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN_BYTES: usize = shadertoy_package::SHADERTOY_AUDIO_VISUALIZER_PACKAGE.bin_bytes;
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
pub(crate) const PARTICLE_CRAFT_ADLS_BIN_BYTES: usize = shadertoy_package::SHADERTOY_PARTICLE_CRAFT_PACKAGE.bin_bytes;
pub(crate) const PARTICLE_CRAFT_ADLS_SPV_BYTES: usize = shadertoy_package::SHADERTOY_PARTICLE_CRAFT_PACKAGE.spv_bytes;
pub(crate) const PARTICLE_CRAFT_ADLS_BIN_SHA256: [u8; 32] =
    PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = assert!(matches!(PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(matches!(PARTICLE_CRAFT_BIN_TILES_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () =
    assert!(matches!(PARTICLE_CRAFT_RENDER_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
const _: () = assert!(PARTICLE_CRAFT_ADLS_BIN_BYTES == 157_536);
const _: () = assert!(PARTICLE_CRAFT_ADLS_SPV_BYTES == 91_764);
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






// Retained-transform code is carried by Picasso-based apps, never the kernel.
pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_BIN: &[u8] = &[];
pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_SPV: &[u8] = &[];
include!("kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.contract.rs");
include!("kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.contract.rs");
embedded_compute_artifact!(pub(crate) SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN, SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_SPV,
    SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_STORAGE,
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.bin"
);
embedded_compute_artifact!(pub(crate) SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN, SPIRIT_VFX_SPRITE_RGBA8_ADLS_SPV,
    SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_STORAGE,
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.bin"
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

const _: () =
    assert!(matches!(ALPHA_BLEND_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT.validate(), Ok(())));
pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_BIN_SHA256: [u8; 32] =
    HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.zebin_sha256;
const _: () = {
    let contract = HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT;
    assert!(matches!(contract.validate(), Ok(())));


    assert!(contract.target.pci_device_ids.len() == 1);
    assert!(contract.target.pci_device_ids[0] == 0x4680);
    assert!(contract.target.revision_min == 0x0C);
    assert!(contract.target.revision_max == 0x0C);
    assert!(contract.entry_offset == 64);
    assert!(contract.entry_size == 41_056);
    assert!(contract.simd_width == 16);
    assert!(contract.grf_count == 128);
    assert!(contract.scratch_bytes == 0);
    assert!(contract.slm_bytes == 0);
    assert!(contract.cross_thread_data_bytes == 224);
    assert!(contract.per_thread_data_bytes == 96);
    assert!(contract.bindings.len() == 16);
    assert!(contract.payload_args.len() == 24);
};

pub(crate) const SHADERTOY_PROTEAN_CLOUDS_KERNEL_NAME: &str = "shadertoy_protean_clouds";
include!(
    "../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/shadertoy_protean_clouds.contract.rs"
);
