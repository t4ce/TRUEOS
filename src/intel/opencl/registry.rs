//! Registry for known offline-compiled Intel OpenCL artifacts.
//!
//! This keeps the OpenCL facade honest: a kernel is "known" only if the current
//! TRUEOS GPGPU backend can upload and report status for its AOT binary.

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use spin::Mutex;

use super::artifact::{
    BuiltProgram, DescriptorField, DescriptorLayout, GpuArtifactProducer, GpuKernelContract,
    KernelArgAccess, KernelArgDesc, KernelCallArg, KernelLaunchContract, KernelMetadata,
    ProgramArtifact, ProgramBinaryKind,
};
use crate::intel::gpgpu;

pub(crate) type UploadFn = fn() -> Option<gpgpu::UploadedKernelArtifact>;
pub(crate) type StatusFn = fn() -> Option<gpgpu::UploadedKernelArtifact>;

#[derive(Copy, Clone)]
pub(crate) struct KnownAotKernel {
    pub(crate) name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) artifact: &'static gpgpu::GpgpuKernelArtifact,
    pub(crate) contract: &'static GpuKernelContract<'static>,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) upload: UploadFn,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) status: StatusFn,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) role: KnownKernelRole,
}

impl KnownAotKernel {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn upload(self) -> Option<gpgpu::UploadedKernelArtifact> {
        (self.upload)()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn status(self) -> Option<gpgpu::UploadedKernelArtifact> {
        (self.status)()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KnownKernelRole {
    Copy,
    Fill,
    WorklistFill,
    WorklistGradient,
    WorklistBlend,
    Glyph,
    Present,
    Sprite,
    Mandel,
    FluidX3d,
    Chart,
    Pixel,
    CppDemo,
    CppAudio,
    Lfm25Q8,
    KokoroQgemm,
    KokoroConv1d,
    Font,
}

const ADLS: &str = "adls";
const IGC: GpuArtifactProducer = GpuArtifactProducer::IntelIgcOcloc;
const TEXT_OFFSET: u64 = 0x40;
const COPY_CROSS_THREAD_BYTES: u32 = 96;
const COPY_PER_THREAD_BYTES: u32 = 96;
const RECT_WORKLIST_CROSS_THREAD_BYTES: u32 = 96;
const FILL_RECT_WORKLIST_CROSS_THREAD_BYTES: u32 = 64;
const RECT_WORKLIST_PER_THREAD_BYTES: u32 = 96;
const SPRITE_QUAD_WORKLIST_CROSS_THREAD_BYTES: u32 = 128;
const SPRITE_QUAD_WORKLIST_PER_THREAD_BYTES: u32 = 96;
const UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES: u32 = 128;
const GLYPH_MASK_CROSS_THREAD_BYTES: u32 = 128;
const SKYBOX_CROSS_THREAD_BYTES: u32 = 160;
const CHART_CROSS_THREAD_BYTES: u32 = 128;
const PIXEL_PLASMA_CROSS_THREAD_BYTES: u32 = 128;
const CPP_DEMO_CROSS_THREAD_BYTES: u32 =
    gpgpu::CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes;
const CPP_AUDIO_VISUALIZER_CROSS_THREAD_BYTES: u32 =
    gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes;
const LFM25_Q8_PROJECT_PACKED_CROSS_THREAD_BYTES: u32 =
    gpgpu::LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes;
const KOKORO_QGEMM_U8_I8_CROSS_THREAD_BYTES: u32 =
    gpgpu::KOKORO_QGEMM_U8_I8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes;
const KOKORO_CONV1D_U8_U8_CROSS_THREAD_BYTES: u32 =
    gpgpu::KOKORO_CONV1D_U8_U8_ADLS_CPP_ABI_CONTRACT.cross_thread_data_bytes;
const FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES: u32 = 128;
const GENERIC_PER_THREAD_BYTES: u32 = 96;

const BOOT_UPLOAD_CONSUMERS: &[&str] = &["intel::init_once upload"];
const RECT_WORKLIST_CONSUMERS: &[&str] = &[
    "intel::init_once upload",
    "font service rect/gradient loops",
    "gpgpu rect worklist probes",
];
const TEXT_RENDER_CONSUMERS: &[&str] = &["intel::init_once upload", "font service text loop"];

const FILL_RECT_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("dst_xy", 0, 1),
    DescriptorField::new("size", 1, 1),
    DescriptorField::new("color_rgba", 2, 1),
];
const FILL_RECT_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("FillRectDesc", 3, Some(256), FILL_RECT_DESC_FIELDS);

const GRADIENT_RECT_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("dst_xy", 0, 1),
    DescriptorField::new("size", 1, 1),
    DescriptorField::new("color0_rgba", 2, 1),
    DescriptorField::new("color1_rgba", 3, 1),
    DescriptorField::new("flags", 4, 1),
];
const GRADIENT_RECT_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("GradientRectDesc", 5, Some(256), GRADIENT_RECT_DESC_FIELDS);

const ALPHA_BLEND_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("src_xy", 0, 1),
    DescriptorField::new("dst_xy", 1, 1),
    DescriptorField::new("size", 2, 1),
    DescriptorField::new("flags", 3, 1),
    DescriptorField::new("color_rgba", 4, 1),
];
const ALPHA_BLEND_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("AlphaBlendDesc", 5, Some(256), ALPHA_BLEND_DESC_FIELDS);

const SPRITE_QUAD_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("c0", 0, 4),
    DescriptorField::new("c1", 4, 4),
    DescriptorField::new("c2", 8, 4),
    DescriptorField::new("c3", 12, 4),
    DescriptorField::new("color_rgba", 16, 1),
    DescriptorField::new("flags", 17, 1),
];
const SPRITE_QUAD_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("SpriteQuadDesc", 18, Some(256), SPRITE_QUAD_DESC_FIELDS);

const UI4_COMPOSE_LAYER_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("src_gpu", 0, 2),
    DescriptorField::new("src_pitch_bytes", 2, 1),
    DescriptorField::new("src_width", 3, 1),
    DescriptorField::new("src_height", 4, 1),
    DescriptorField::new("dst_xy", 5, 2),
    DescriptorField::new("dst_extent", 7, 2),
    DescriptorField::new("opacity", 9, 1),
    DescriptorField::new("flags", 10, 1),
];
const UI4_COMPOSE_LAYER_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("Ui4ComposeLayerDesc", 12, Some(32), UI4_COMPOSE_LAYER_DESC_FIELDS);

const MANDEL64_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("src_xy", 0, 1),
    DescriptorField::new("dst_xy", 1, 1),
    DescriptorField::new("flags", 2, 1),
    DescriptorField::new("color_rgba", 3, 1),
];
const MANDEL64_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("Mandel64Desc", 4, Some(512), MANDEL64_DESC_FIELDS);

macro_rules! ro_buf {
    ($index:expr, $name:expr, $ty:expr, $binding:expr, $payload:expr) => {
        KernelCallArg::buffer($index, $name, $ty, KernelArgAccess::ReadOnly, $binding, $payload)
    };
}

macro_rules! rw_buf {
    ($index:expr, $name:expr, $ty:expr, $binding:expr, $payload:expr) => {
        KernelCallArg::buffer($index, $name, $ty, KernelArgAccess::ReadWrite, $binding, $payload)
    };
}

macro_rules! u32_arg {
    ($index:expr, $name:expr, $payload:expr) => {
        KernelCallArg::value($index, $name, "uint", 4, 4, $payload)
    };
}

macro_rules! f32_arg {
    ($index:expr, $name:expr, $payload:expr) => {
        KernelCallArg::value($index, $name, "float", 4, 4, $payload)
    };
}

const NO_DESCS: &[DescriptorLayout<'_>] = &[];
const FILL_RECT_DESCS: &[DescriptorLayout<'_>] = &[FILL_RECT_DESC];
const GRADIENT_RECT_DESCS: &[DescriptorLayout<'_>] = &[GRADIENT_RECT_DESC];
const ALPHA_BLEND_DESCS: &[DescriptorLayout<'_>] = &[ALPHA_BLEND_DESC];
const SPRITE_QUAD_DESCS: &[DescriptorLayout<'_>] = &[SPRITE_QUAD_DESC];
const UI4_COMPOSE_LAYER_DESCS: &[DescriptorLayout<'_>] = &[UI4_COMPOSE_LAYER_DESC];
const MANDEL64_DESCS: &[DescriptorLayout<'_>] = &[MANDEL64_DESC];

const COPY_RECT_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_rgba", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    u32_arg!(2, "src_pitch_bytes", 16),
    u32_arg!(3, "dst_pitch_bytes", 17),
    u32_arg!(4, "src_x", 18),
    u32_arg!(5, "src_y", 19),
    u32_arg!(6, "dst_x", 20),
    u32_arg!(7, "dst_y", 21),
    u32_arg!(8, "width", 22),
    u32_arg!(9, "height", 23),
];
const COPY_RECT_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::COPY_RECT_RGBA8_KERNEL_NAME,
    source_path: gpgpu::COPY_RECT_RGBA8_SOURCE_PATH,
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: gpgpu::COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
    cross_thread_bytes: COPY_CROSS_THREAD_BYTES,
    per_thread_bytes: COPY_PER_THREAD_BYTES,
    binding_count: 2,
    args: COPY_RECT_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(Some(2)),
    consumers: BOOT_UPLOAD_CONSUMERS,
};

const FILL_RECT_ARGS: &[KernelCallArg<'_>] = &[
    rw_buf!(0, "dst_rgba", "__global uint*", 0, 12),
    u32_arg!(1, "dst_pitch_bytes", 14),
    u32_arg!(2, "dst_x", 15),
    u32_arg!(3, "dst_y", 16),
    u32_arg!(4, "width", 17),
    u32_arg!(5, "height", 18),
    u32_arg!(6, "color_rgba", 19),
];
const FILL_RECT_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::FILL_RECT_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/fill_rect_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: COPY_CROSS_THREAD_BYTES,
    per_thread_bytes: COPY_PER_THREAD_BYTES,
    binding_count: 1,
    args: FILL_RECT_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: BOOT_UPLOAD_CONSUMERS,
};

const FILL_RECT_WORKLIST_ARGS: &[KernelCallArg<'_>] = &[
    rw_buf!(0, "dst_rgba", "__global uint*", 0, 8),
    ro_buf!(1, "descs", "__global const uint*", 1, 10),
    u32_arg!(2, "dst_pitch_bytes", 12),
    u32_arg!(3, "desc_base", 13),
    u32_arg!(4, "desc_count", 14),
];
const FILL_RECT_WORKLIST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/fill_rect_worklist_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: FILL_RECT_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: RECT_WORKLIST_PER_THREAD_BYTES,
    binding_count: 2,
    args: FILL_RECT_WORKLIST_ARGS,
    descriptor_layouts: FILL_RECT_DESCS,
    launch: KernelLaunchContract::descriptor_worklist(16),
    consumers: RECT_WORKLIST_CONSUMERS,
};

const GRADIENT_RECT_WORKLIST_ARGS: &[KernelCallArg<'_>] = FILL_RECT_WORKLIST_ARGS;
const GRADIENT_RECT_WORKLIST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/gradient_rect_worklist_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: RECT_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: RECT_WORKLIST_PER_THREAD_BYTES,
    binding_count: 2,
    args: GRADIENT_RECT_WORKLIST_ARGS,
    descriptor_layouts: GRADIENT_RECT_DESCS,
    launch: KernelLaunchContract::descriptor_worklist(16),
    consumers: RECT_WORKLIST_CONSUMERS,
};

const ALPHA_BLEND_WORKLIST_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_rgba", "__global const uint*", 0, 8),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 10),
    ro_buf!(2, "descs", "__global const uint*", 2, 12),
    u32_arg!(3, "src_pitch_bytes", 14),
    u32_arg!(4, "dst_pitch_bytes", 15),
    u32_arg!(5, "desc_base", 16),
    u32_arg!(6, "desc_count", 17),
];
const ALPHA_BLEND_WORKLIST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/alpha_blend_worklist_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: RECT_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: RECT_WORKLIST_PER_THREAD_BYTES,
    binding_count: 3,
    args: ALPHA_BLEND_WORKLIST_ARGS,
    descriptor_layouts: ALPHA_BLEND_DESCS,
    launch: KernelLaunchContract::descriptor_worklist(16),
    consumers: RECT_WORKLIST_CONSUMERS,
};

const GLYPH_MASK_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "mask_u8", "__global const uchar*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    u32_arg!(2, "mask_pitch_bytes", 16),
    u32_arg!(3, "dst_pitch_bytes", 17),
    u32_arg!(4, "mask_x", 18),
    u32_arg!(5, "mask_y", 19),
    u32_arg!(6, "dst_x", 20),
    u32_arg!(7, "dst_y", 21),
    u32_arg!(8, "width", 22),
    u32_arg!(9, "height", 23),
    u32_arg!(10, "color_rgba", 24),
];
const GLYPH_MASK_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::GLYPH_MASK_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/glyph_mask_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: GLYPH_MASK_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: GLYPH_MASK_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: TEXT_RENDER_CONSUMERS,
};

const SPRITE_QUAD_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_rgba", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    ro_buf!(2, "descs", "__global const uint*", 2, 16),
    u32_arg!(3, "src_pitch_bytes", 18),
    u32_arg!(4, "dst_pitch_bytes", 19),
    u32_arg!(5, "src_width", 20),
    u32_arg!(6, "src_height", 21),
    u32_arg!(7, "dst_width", 22),
    u32_arg!(8, "dst_height", 23),
    u32_arg!(9, "desc_base", 24),
    u32_arg!(10, "desc_count", 25),
];
const SPRITE_QUAD_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/sprite_quad_worklist_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: SPRITE_QUAD_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: SPRITE_QUAD_WORKLIST_PER_THREAD_BYTES,
    binding_count: 3,
    args: SPRITE_QUAD_ARGS,
    descriptor_layouts: SPRITE_QUAD_DESCS,
    launch: KernelLaunchContract::descriptor_worklist(16),
    consumers: &["intel::init_once upload", "explicit sprite batches"],
};

const UI4_COMPOSE_LAYERS_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "base_xrgb", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    ro_buf!(2, "layers", "__global const uint*", 2, 16),
    u32_arg!(3, "base_pitch_bytes", 18),
    u32_arg!(4, "dst_pitch_bytes", 19),
    u32_arg!(5, "dst_width", 20),
    u32_arg!(6, "dst_height", 21),
    u32_arg!(7, "damage_x", 22),
    u32_arg!(8, "damage_y", 23),
    u32_arg!(9, "damage_width", 24),
    u32_arg!(10, "damage_height", 25),
    u32_arg!(11, "layer_count", 26),
    u32_arg!(12, "flags", 27),
];
const UI4_COMPOSE_LAYERS_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/ui4_compose_layers_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: UI4_COMPOSE_LAYERS_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 3,
    args: UI4_COMPOSE_LAYERS_ARGS,
    descriptor_layouts: UI4_COMPOSE_LAYER_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["ui4 persistent GuC compositor"],
};

const MANDEL64_ARGS: &[KernelCallArg<'_>] = FILL_RECT_WORKLIST_ARGS;
const MANDEL64_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/mandel64_worklist_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: RECT_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: RECT_WORKLIST_PER_THREAD_BYTES,
    binding_count: 2,
    args: MANDEL64_ARGS,
    descriptor_layouts: MANDEL64_DESCS,
    launch: KernelLaunchContract::descriptor_worklist(16),
    consumers: &[
        "intel::init_once upload",
        "ui4::gpgpu_preview_consumer_service_task",
        "gpgpu mandel64 probe",
    ],
};

const SKYBOX_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "skybox_rgb565", "__global const ushort*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    u32_arg!(2, "sky_pitch_bytes", 16),
    u32_arg!(3, "sky_width", 17),
    u32_arg!(4, "sky_height", 18),
    u32_arg!(5, "dst_pitch_bytes", 19),
    u32_arg!(6, "dst_width", 20),
    u32_arg!(7, "dst_height", 21),
    u32_arg!(8, "rect_x", 22),
    u32_arg!(9, "rect_y", 23),
    u32_arg!(10, "rect_width", 24),
    u32_arg!(11, "rect_height", 25),
    f32_arg!(12, "right_x", 26),
    f32_arg!(13, "right_y", 27),
    f32_arg!(14, "right_z", 28),
    f32_arg!(15, "up_x", 29),
    f32_arg!(16, "up_y", 30),
    f32_arg!(17, "up_z", 31),
    f32_arg!(18, "forward_x", 32),
    f32_arg!(19, "forward_y", 33),
    f32_arg!(20, "forward_z", 34),
    f32_arg!(21, "aspect_tan_half_fov_y", 35),
    f32_arg!(22, "tan_half_fov_y", 36),
];
const SKYBOX_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/skybox_sample_rgb565.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: SKYBOX_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: SKYBOX_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["explicit skybox renderer", "blueprint:skybox"],
};

const CHART_ARGS: &[KernelCallArg<'_>] = &[
    rw_buf!(0, "dst_rgba", "__global uint*", 0, 12),
    u32_arg!(1, "dst_pitch_bytes", 14),
    u32_arg!(2, "dst_width", 15),
    u32_arg!(3, "dst_height", 16),
    u32_arg!(4, "rect_x", 17),
    u32_arg!(5, "rect_y", 18),
    u32_arg!(6, "rect_width", 19),
    u32_arg!(7, "rect_height", 20),
    f32_arg!(8, "phase", 21),
    f32_arg!(9, "cycles", 22),
    f32_arg!(10, "amplitude", 23),
    f32_arg!(11, "line_width_px", 24),
    u32_arg!(12, "background_rgba", 25),
    u32_arg!(13, "minor_grid_rgba", 26),
    u32_arg!(14, "major_grid_rgba", 27),
    u32_arg!(15, "axis_rgba", 28),
    u32_arg!(16, "line_rgba", 29),
    u32_arg!(17, "glow_rgba", 30),
    u32_arg!(18, "flags", 31),
];
const CHART_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CHART_SINE_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/chart_sine_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CHART_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 1,
    args: CHART_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["ui4::gpgpu_preview_consumer_service_task"],
};

const PIXEL_PLASMA_ARGS: &[KernelCallArg<'_>] = &[
    rw_buf!(0, "dst_rgba", "__global uint*", 0, 12),
    u32_arg!(1, "dst_pitch_bytes", 14),
    u32_arg!(2, "dst_width", 15),
    u32_arg!(3, "dst_height", 16),
    u32_arg!(4, "rect_x", 17),
    u32_arg!(5, "rect_y", 18),
    u32_arg!(6, "rect_width", 19),
    u32_arg!(7, "rect_height", 20),
    f32_arg!(8, "time", 21),
    f32_arg!(9, "spatial_scale", 22),
    f32_arg!(10, "intensity", 23),
    u32_arg!(11, "low_rgba", 24),
    u32_arg!(12, "mid_rgba", 25),
    u32_arg!(13, "high_rgba", 26),
    u32_arg!(14, "flags", 27),
];
const PIXEL_PLASMA_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::PIXEL_PLASMA_RGBA8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/pixel_plasma_rgba8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: PIXEL_PLASMA_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 1,
    args: PIXEL_PLASMA_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["ui4::gpgpu_preview_consumer_service_task"],
};

const CPP_DEMO_ARGS: &[KernelCallArg<'_>] = &[
    rw_buf!(0, "dst_rgba", "__global uint*", 0, 12),
    u32_arg!(1, "dst_pitch_bytes", 14),
    u32_arg!(2, "dst_width", 15),
    u32_arg!(3, "dst_height", 16),
    u32_arg!(4, "rect_x", 17),
    u32_arg!(5, "rect_y", 18),
    u32_arg!(6, "rect_width", 19),
    u32_arg!(7, "rect_height", 20),
    f32_arg!(8, "time_seconds", 21),
    u32_arg!(9, "demo_mode", 22),
    u32_arg!(10, "seed", 23),
    u32_arg!(11, "flags", 24),
];
const CPP_DEMO_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CPP_DEMO_RGBA8_KERNEL_NAME,
    source_path: gpgpu::CPP_DEMO_RGBA8_SOURCE_PATH,
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: gpgpu::CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT.entry_offset,
    cross_thread_bytes: CPP_DEMO_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 1,
    args: CPP_DEMO_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["shell2:cpp", "ui4::gpgpu_preview_consumer_service_task"],
};

const CPP_AUDIO_VISUALIZER_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "audio_snapshot", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    u32_arg!(2, "dst_pitch_bytes", 16),
    u32_arg!(3, "dst_width", 17),
    u32_arg!(4, "dst_height", 18),
    f32_arg!(5, "time_seconds", 19),
    u32_arg!(6, "frame", 20),
    u32_arg!(7, "flags", 21),
];
const CPP_AUDIO_VISUALIZER_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME,
    source_path: gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_SOURCE_PATH,
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT.entry_offset,
    cross_thread_bytes: CPP_AUDIO_VISUALIZER_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CPP_AUDIO_VISUALIZER_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(Some(2)),
    consumers: &[
        "shell2:cpp interactive-gallery/audio",
        "ui4::gpgpu_preview_consumer_service_task",
    ],
};

const LFM25_Q8_PROJECT_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "weights", "__global const uint*", 0, 12),
    ro_buf!(1, "activation", "__global const uint*", 1, 14),
    rw_buf!(2, "output", "__global float*", 2, 16),
    u32_arg!(3, "weight_offset", 18),
    u32_arg!(4, "columns", 19),
    u32_arg!(5, "rows", 20),
];
const LFM25_Q8_PROJECT_PACKED_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::LFM25_Q8_PROJECT_PACKED_KERNEL_NAME,
    source_path: gpgpu::LFM25_Q8_PROJECT_PACKED_SOURCE_PATH,
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: gpgpu::LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT.entry_offset,
    cross_thread_bytes: LFM25_Q8_PROJECT_PACKED_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 3,
    args: LFM25_Q8_PROJECT_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: &["lfm2.5 fixed packed Q8x16 DP4A reasoning projection"],
};

const KOKORO_QGEMM_U8_I8_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "packed_weights", "__global const uint*", 0, 12),
    ro_buf!(1, "weight_sums", "__global const int*", 1, 14),
    ro_buf!(2, "weight_scales", "__global const float*", 2, 16),
    ro_buf!(3, "activations", "__global const uint*", 3, 18),
    ro_buf!(4, "bias", "__global const float*", 4, 20),
    rw_buf!(5, "output", "__global float*", 5, 22),
    u32_arg!(6, "matrix_rows", 24),
    u32_arg!(7, "output_columns", 25),
    u32_arg!(8, "reduction_words", 26),
    u32_arg!(9, "activation_stride_words", 27),
    u32_arg!(10, "output_stride", 28),
    u32_arg!(11, "activation_zero_point", 29),
    f32_arg!(12, "activation_scale", 30),
    u32_arg!(13, "has_bias", 31),
];
const KOKORO_QGEMM_U8_I8_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::KOKORO_QGEMM_U8_I8_KERNEL_NAME,
    source_path: gpgpu::KOKORO_QGEMM_U8_I8_SOURCE_PATH,
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: gpgpu::KOKORO_QGEMM_U8_I8_ADLS_CPP_ABI_CONTRACT.entry_offset,
    cross_thread_bytes: KOKORO_QGEMM_U8_I8_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 6,
    args: KOKORO_QGEMM_U8_I8_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["ttstt Kokoro quantized MatMulInteger projection"],
};

const KOKORO_CONV1D_U8_U8_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "packed_weights", "__global const uint*", 0, 12),
    ro_buf!(1, "weight_tap_sums", "__global const uint*", 1, 14),
    ro_buf!(2, "packed_activations", "__global const uint*", 2, 16),
    rw_buf!(3, "output", "__global int*", 3, 18),
    u32_arg!(4, "input_length", 20),
    u32_arg!(5, "output_base", 21),
    u32_arg!(6, "tile_length", 22),
    u32_arg!(7, "activation_origin", 23),
    u32_arg!(8, "activation_rows", 24),
    u32_arg!(9, "input_channels", 25),
    u32_arg!(10, "output_channels", 26),
    u32_arg!(11, "kernel_size", 27),
    u32_arg!(12, "dilation", 28),
    u32_arg!(13, "pad_left", 29),
    u32_arg!(14, "activation_zero_point", 30),
    u32_arg!(15, "weight_zero_point", 31),
];
const KOKORO_CONV1D_U8_U8_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::KOKORO_CONV1D_U8_U8_KERNEL_NAME,
    source_path: gpgpu::KOKORO_CONV1D_U8_U8_SOURCE_PATH,
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: gpgpu::KOKORO_CONV1D_U8_U8_ADLS_CPP_ABI_CONTRACT.entry_offset,
    cross_thread_bytes: KOKORO_CONV1D_U8_U8_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 4,
    args: KOKORO_CONV1D_U8_U8_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &["ttstt Kokoro dominant stride-one ConvInteger family"],
};

const FONT_OUTLINE_COVERAGE_R8_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "outline_ops", "__global const uint*", 0, 12),
    rw_buf!(1, "mask_u8", "__global uchar*", 1, 14),
    u32_arg!(2, "op_count", 16),
    u32_arg!(3, "subdivisions", 17),
    u32_arg!(4, "mask_pitch_bytes", 18),
    u32_arg!(5, "mask_width", 19),
    u32_arg!(6, "mask_height", 20),
    u32_arg!(7, "rect_x", 21),
    u32_arg!(8, "rect_y", 22),
    u32_arg!(9, "rect_width", 23),
    u32_arg!(10, "rect_height", 24),
    f32_arg!(11, "optical_bias_px", 25),
];
const FONT_OUTLINE_COVERAGE_R8_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
    source_path: "src/intel/gpgpu/kernels/font_outline_coverage_r8.clcpp",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: FONT_OUTLINE_COVERAGE_R8_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &[
        "intel::gpu_font default analytical coverage",
        "gridpaper resident scene at every supported scale",
    ],
};

pub(crate) const KNOWN_AOT_KERNELS: &[KnownAotKernel] = &[
    KnownAotKernel {
        name: gpgpu::COPY_RECT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::COPY_RECT_RGBA8_ADLS_ARTIFACT,
        contract: &COPY_RECT_CONTRACT,
        upload: gpgpu::upload_copy_rect_rgba8_kernel,
        status: gpgpu::copy_rect_rgba8_upload_status,
        role: KnownKernelRole::Copy,
    },
    KnownAotKernel {
        name: gpgpu::FILL_RECT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::FILL_RECT_RGBA8_ADLS_ARTIFACT,
        contract: &FILL_RECT_CONTRACT,
        upload: gpgpu::upload_fill_rect_rgba8_kernel,
        status: gpgpu::fill_rect_rgba8_upload_status,
        role: KnownKernelRole::Fill,
    },
    KnownAotKernel {
        name: gpgpu::FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &FILL_RECT_WORKLIST_CONTRACT,
        upload: gpgpu::upload_fill_rect_worklist_rgba8_kernel,
        status: gpgpu::fill_rect_worklist_rgba8_upload_status,
        role: KnownKernelRole::WorklistFill,
    },
    KnownAotKernel {
        name: gpgpu::GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &GRADIENT_RECT_WORKLIST_CONTRACT,
        upload: gpgpu::upload_gradient_rect_worklist_rgba8_kernel,
        status: gpgpu::gradient_rect_worklist_rgba8_upload_status,
        role: KnownKernelRole::WorklistGradient,
    },
    KnownAotKernel {
        name: gpgpu::ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &ALPHA_BLEND_WORKLIST_CONTRACT,
        upload: gpgpu::upload_alpha_blend_worklist_rgba8_kernel,
        status: gpgpu::alpha_blend_worklist_rgba8_upload_status,
        role: KnownKernelRole::WorklistBlend,
    },
    KnownAotKernel {
        name: gpgpu::GLYPH_MASK_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::GLYPH_MASK_RGBA8_ADLS_ARTIFACT,
        contract: &GLYPH_MASK_CONTRACT,
        upload: gpgpu::upload_glyph_mask_rgba8_kernel,
        status: gpgpu::glyph_mask_rgba8_upload_status,
        role: KnownKernelRole::Glyph,
    },
    KnownAotKernel {
        name: gpgpu::SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &SPRITE_QUAD_CONTRACT,
        upload: gpgpu::upload_sprite_quad_worklist_rgba8_kernel,
        status: gpgpu::sprite_quad_worklist_rgba8_upload_status,
        role: KnownKernelRole::Sprite,
    },
    KnownAotKernel {
        name: gpgpu::UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT,
        contract: &UI4_COMPOSE_LAYERS_CONTRACT,
        upload: gpgpu::upload_ui4_compose_layers_rgba8_kernel,
        status: gpgpu::ui4_compose_layers_rgba8_upload_status,
        role: KnownKernelRole::Present,
    },
    KnownAotKernel {
        name: gpgpu::MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &MANDEL64_CONTRACT,
        upload: gpgpu::upload_mandel64_worklist_rgba8_kernel,
        status: gpgpu::mandel64_worklist_rgba8_upload_status,
        role: KnownKernelRole::Mandel,
    },
    KnownAotKernel {
        name: gpgpu::SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
        artifact: &gpgpu::SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT,
        contract: &SKYBOX_CONTRACT,
        upload: gpgpu::upload_skybox_sample_rgb565_kernel,
        status: gpgpu::skybox_sample_rgb565_upload_status,
        role: KnownKernelRole::FluidX3d,
    },
    KnownAotKernel {
        name: gpgpu::CHART_SINE_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CHART_SINE_RGBA8_ADLS_ARTIFACT,
        contract: &CHART_CONTRACT,
        upload: gpgpu::upload_chart_sine_rgba8_kernel,
        status: gpgpu::chart_sine_rgba8_upload_status,
        role: KnownKernelRole::Chart,
    },
    KnownAotKernel {
        name: gpgpu::PIXEL_PLASMA_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT,
        contract: &PIXEL_PLASMA_CONTRACT,
        upload: gpgpu::upload_pixel_plasma_rgba8_kernel,
        status: gpgpu::pixel_plasma_rgba8_upload_status,
        role: KnownKernelRole::Pixel,
    },
    KnownAotKernel {
        name: gpgpu::CPP_DEMO_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT,
        contract: &CPP_DEMO_CONTRACT,
        upload: gpgpu::upload_cpp_demo_rgba8_kernel,
        status: gpgpu::cpp_demo_rgba8_upload_status,
        role: KnownKernelRole::CppDemo,
    },
    KnownAotKernel {
        name: gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT,
        contract: &CPP_AUDIO_VISUALIZER_CONTRACT,
        upload: gpgpu::upload_cpp_audio_visualizer_rgba8_kernel,
        status: gpgpu::cpp_audio_visualizer_rgba8_upload_status,
        role: KnownKernelRole::CppAudio,
    },
    KnownAotKernel {
        name: gpgpu::LFM25_Q8_PROJECT_PACKED_KERNEL_NAME,
        artifact: &gpgpu::LFM25_Q8_PROJECT_PACKED_ADLS_ARTIFACT,
        contract: &LFM25_Q8_PROJECT_PACKED_CONTRACT,
        upload: gpgpu::upload_lfm25_q8_project_packed_kernel,
        status: gpgpu::lfm25_q8_project_packed_upload_status,
        role: KnownKernelRole::Lfm25Q8,
    },
    KnownAotKernel {
        name: gpgpu::KOKORO_QGEMM_U8_I8_KERNEL_NAME,
        artifact: &gpgpu::KOKORO_QGEMM_U8_I8_ADLS_ARTIFACT,
        contract: &KOKORO_QGEMM_U8_I8_CONTRACT,
        upload: gpgpu::upload_kokoro_qgemm_u8_i8_kernel,
        status: gpgpu::kokoro_qgemm_u8_i8_upload_status,
        role: KnownKernelRole::KokoroQgemm,
    },
    KnownAotKernel {
        name: gpgpu::KOKORO_CONV1D_U8_U8_KERNEL_NAME,
        artifact: &gpgpu::KOKORO_CONV1D_U8_U8_ADLS_ARTIFACT,
        contract: &KOKORO_CONV1D_U8_U8_CONTRACT,
        upload: gpgpu::upload_kokoro_conv1d_u8_u8_kernel,
        status: gpgpu::kokoro_conv1d_u8_u8_upload_status,
        role: KnownKernelRole::KokoroConv1d,
    },
    KnownAotKernel {
        name: gpgpu::FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
        artifact: &gpgpu::FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT,
        contract: &FONT_OUTLINE_COVERAGE_R8_CONTRACT,
        upload: gpgpu::upload_font_outline_coverage_r8_kernel,
        status: gpgpu::font_outline_coverage_r8_upload_status,
        role: KnownKernelRole::Font,
    },
];

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
static SOURCE_PROGRAM_CACHE: Mutex<BTreeMap<&'static str, &'static ProgramArtifact<'static>>> =
    Mutex::new(BTreeMap::new());

pub(crate) fn known_aot_kernel(name: &str) -> Option<&'static KnownAotKernel> {
    KNOWN_AOT_KERNELS.iter().find(|kernel| kernel.name == name)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn is_known_aot_kernel(name: &str) -> bool {
    known_aot_kernel(name).is_some()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn known_aot_kernel_by_source(source: &str) -> Option<&'static KnownAotKernel> {
    KNOWN_AOT_KERNELS
        .iter()
        .find(|kernel| gpgpu::kernel_opencl_source(kernel.name) == Some(source))
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn build_program_from_known_source(
    source: &str,
    build_options: &str,
) -> Option<BuiltProgram<'static>> {
    if !build_options.trim().is_empty() {
        return None;
    }
    let kernel = known_aot_kernel_by_source(source)?;

    {
        let cache = SOURCE_PROGRAM_CACHE.lock();
        if let Some(program) = cache.get(kernel.name) {
            return Some(BuiltProgram::from_artifact(program));
        }
    }

    let mut args = Vec::with_capacity(kernel.contract.args.len());
    for arg in kernel.contract.args.iter().copied() {
        args.push(KernelArgDesc::from_call_arg(arg));
    }
    let args: &'static [KernelArgDesc<'static>] = Box::leak(args.into_boxed_slice());
    let kernels: &'static [KernelMetadata<'static>] = Box::leak(
        alloc::vec![KernelMetadata::with_gpgpu_artifact(
            kernel.name,
            args,
            kernel.artifact,
        )]
        .into_boxed_slice(),
    );
    let program: &'static ProgramArtifact<'static> = Box::leak(Box::new(ProgramArtifact {
        name: kernel.name,
        target: kernel.artifact.target,
        binary_kind: ProgramBinaryKind::IntelGenBinary,
        binary: kernel.artifact.bin,
        binary_sha256: Some(kernel.artifact.bin_sha256),
        spirv: Some(kernel.artifact.spv),
        source: gpgpu::kernel_opencl_source(kernel.name),
        build_options: "",
        kernels,
        gpgpu_artifact: Some(kernel.artifact),
    }));

    SOURCE_PROGRAM_CACHE.lock().insert(kernel.name, program);
    Some(BuiltProgram::from_artifact(program))
}
