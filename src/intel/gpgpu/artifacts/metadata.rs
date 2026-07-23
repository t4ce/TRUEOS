#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) bin: &'static [u8],
    pub(crate) spv: &'static [u8],
    pub(crate) bin_sha256: [u8; 32],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct UploadedKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) source: &'static str,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
    pub(crate) bin_sha256: [u8; 32],
}

unsafe impl Send for UploadedKernelArtifact {}
unsafe impl Sync for UploadedKernelArtifact {}

pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: COPY_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: COPY_RECT_RGBA8_ADLS_BIN,
    spv: COPY_RECT_RGBA8_ADLS_SPV,
    bin_sha256: COPY_RECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN,
        spv: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_SPV,
        bin_sha256: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const FILL_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: FILL_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: FILL_RECT_RGBA8_ADLS_BIN,
    spv: FILL_RECT_RGBA8_ADLS_SPV,
    bin_sha256: FILL_RECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: FILL_RECT_WORKLIST_RGBA8_ADLS_BIN,
        spv: FILL_RECT_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: FILL_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN,
        spv: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN,
        spv: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const GLYPH_MASK_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: GLYPH_MASK_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: GLYPH_MASK_RGBA8_ADLS_BIN,
    spv: GLYPH_MASK_RGBA8_ADLS_SPV,
    bin_sha256: GLYPH_MASK_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: UI4_NV12_YTILE_TO_PRIMARY_XRGB_KERNEL_NAME,
        target: "adls",
        bin: UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN,
        spv: UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_SPV,
        bin_sha256: UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_BIN_SHA256,
    };

pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME,
        target: "adls",
        bin: UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN,
        spv: UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV,
        bin_sha256: UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN_SHA256,
    };

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN,
        spv: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV,
        bin_sha256: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN,
        spv: UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV,
        bin_sha256: UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: MANDEL64_WORKLIST_RGBA8_ADLS_BIN,
    spv: MANDEL64_WORKLIST_RGBA8_ADLS_SPV,
    bin_sha256: MANDEL64_WORKLIST_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
    target: "adls",
    bin: SKYBOX_SAMPLE_RGB565_ADLS_BIN,
    spv: SKYBOX_SAMPLE_RGB565_ADLS_SPV,
    bin_sha256: SKYBOX_SAMPLE_RGB565_ADLS_BIN_SHA256,
};

pub(crate) const CHART_SINE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CHART_SINE_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: CHART_SINE_RGBA8_ADLS_BIN,
    spv: CHART_SINE_RGBA8_ADLS_SPV,
    bin_sha256: CHART_SINE_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: PIXEL_PLASMA_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: PIXEL_PLASMA_RGBA8_ADLS_BIN,
    spv: PIXEL_PLASMA_RGBA8_ADLS_SPV,
    bin_sha256: PIXEL_PLASMA_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const FONT_OUTLINE_MESH_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: FONT_OUTLINE_MESH_KERNEL_NAME,
    target: "adls",
    bin: FONT_OUTLINE_MESH_ADLS_BIN,
    spv: FONT_OUTLINE_MESH_ADLS_SPV,
    bin_sha256: FONT_OUTLINE_MESH_ADLS_BIN_SHA256,
};

pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
        target: "adls",
        bin: FONT_OUTLINE_COVERAGE_R8_ADLS_BIN,
        spv: FONT_OUTLINE_COVERAGE_R8_ADLS_SPV,
        bin_sha256: FONT_OUTLINE_COVERAGE_R8_ADLS_BIN_SHA256,
    };

pub(crate) const SCENE_AABB_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: SCENE_AABB_KERNEL_NAME,
    target: "adls",
    bin: SCENE_AABB_ADLS_BIN,
    spv: SCENE_AABB_ADLS_SPV,
    bin_sha256: SCENE_AABB_ADLS_BIN_SHA256,
};

pub(crate) const LAB256_MULTIPHASE_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: LAB256_MULTIPHASE_KERNEL_NAME,
    target: "adls",
    bin: LAB256_MULTIPHASE_ADLS_BIN,
    spv: LAB256_MULTIPHASE_ADLS_SPV,
    bin_sha256: LAB256_MULTIPHASE_ADLS_BIN_SHA256,
};

pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN,
        spv: SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_SPV,
        bin_sha256: SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_SHA256,
    };

pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact {
        name: SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME,
        target: "adls",
        bin: SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN,
        spv: SPIRIT_VFX_SPRITE_RGBA8_ADLS_SPV,
        bin_sha256: SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_SHA256,
    };
