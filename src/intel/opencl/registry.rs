//! Registry for known offline-compiled Intel OpenCL artifacts.
//!
//! This keeps the OpenCL facade honest: a kernel is "known" only if the current
//! TRUEOS GPGPU backend can upload and report status for its AOT binary.

use super::artifact::{
    DescriptorField, DescriptorLayout, GpuArtifactProducer, GpuKernelContract, KernelArgAccess,
    KernelArgKind, KernelCallArg, KernelLaunchContract,
};
use crate::intel::gpgpu;

pub(crate) type UploadFn = fn() -> Option<gpgpu::UploadedKernelArtifact>;
pub(crate) type StatusFn = fn() -> Option<gpgpu::UploadedKernelArtifact>;

#[derive(Copy, Clone)]
pub(crate) struct KnownAotKernel {
    pub(crate) name: &'static str,
    pub(crate) artifact: &'static gpgpu::GpgpuKernelArtifact,
    pub(crate) contract: &'static GpuKernelContract<'static>,
    pub(crate) upload: UploadFn,
    pub(crate) status: StatusFn,
    pub(crate) role: KnownKernelRole,
}

impl KnownAotKernel {
    pub(crate) fn upload(self) -> Option<gpgpu::UploadedKernelArtifact> {
        (self.upload)()
    }

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
    Blit,
    Blend,
    WorklistBlend,
    Glyph,
    Present,
    Sprite,
    Mandel,
    Canvas3d,
}

const ADLS: &str = "adls";
const IGC: GpuArtifactProducer = GpuArtifactProducer::IntelIgcOcloc;
const TEXT_OFFSET: u64 = 0x40;
const COPY_CROSS_THREAD_BYTES: u32 = 96;
const COPY_PER_THREAD_BYTES: u32 = 96;
const RECT_WORKLIST_CROSS_THREAD_BYTES: u32 = 96;
const RECT_WORKLIST_PER_THREAD_BYTES: u32 = 96;
const SPRITE64_WORKLIST_CROSS_THREAD_BYTES: u32 = 96;
const SPRITE64_WORKLIST_PER_THREAD_BYTES: u32 = 96;
const GLYPH_MASK_CROSS_THREAD_BYTES: u32 = 128;
const PRESENT_CROSS_THREAD_BYTES: u32 = 128;
const CANVAS3D_PROJECT_CROSS_THREAD_BYTES: u32 = 96;
const CANVAS3D_TRANSFORM_CROSS_THREAD_BYTES: u32 = 128;
const CANVAS3D_CLIP_BOX_CROSS_THREAD_BYTES: u32 = 128;
const CANVAS3D_PLANE_SAMPLE_CROSS_THREAD_BYTES: u32 = 224;
const CANVAS3D_PLANE_FILL_CROSS_THREAD_BYTES: u32 = 256;
const CANVAS3D_PLANE_PATCH_WORKLIST_CROSS_THREAD_BYTES: u32 = 96;
const GENERIC_PER_THREAD_BYTES: u32 = 96;

const BOOT_UPLOAD_CONSUMERS: &[&str] = &["intel::init_once upload"];
const RECT_WORKLIST_CONSUMERS: &[&str] = &[
    "intel::init_once upload",
    "shell2:gpgpu smoke",
    "ui3::ui3_font rect/gradient loops",
    "gpgpu rect worklist probes",
];
const UI3_TEXT_CONSUMERS: &[&str] = &[
    "intel::init_once upload",
    "shell2:gpgpu canvas2d sprites64",
    "ui3::ui3_font text loop",
];
const UI3_CANVAS_CONSUMERS: &[&str] = &[
    "intel::init_once upload",
    "shell2:gpgpu canvas3d cube",
    "shell2:gpgpu canvas3d ico",
    "shell2:gpgpu canvas3d para",
    "ui3::ui3_canvas worker",
    "gpgpu canvas3d probes",
];

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

const SPRITE64_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("atlas_xy", 0, 1),
    DescriptorField::new("dst_xy", 1, 1),
    DescriptorField::new("flags", 2, 1),
    DescriptorField::new("color_rgba", 3, 1),
];
const SPRITE64_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("Sprite64Desc", 4, Some(256), SPRITE64_DESC_FIELDS);

const MANDEL64_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("src_xy", 0, 1),
    DescriptorField::new("dst_xy", 1, 1),
    DescriptorField::new("flags", 2, 1),
    DescriptorField::new("color_rgba", 3, 1),
];
const MANDEL64_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("Mandel64Desc", 4, Some(512), MANDEL64_DESC_FIELDS);

const PATCH_DESC_FIELDS: &[DescriptorField<'_>] = &[
    DescriptorField::new("dst_pitch_bytes", 0, 1),
    DescriptorField::new("dst_width", 1, 1),
    DescriptorField::new("dst_height", 2, 1),
    DescriptorField::new("rect_x", 3, 1),
    DescriptorField::new("rect_y", 4, 1),
    DescriptorField::new("rect_width", 5, 1),
    DescriptorField::new("rect_height", 6, 1),
    DescriptorField::new("canvas_width", 7, 1),
    DescriptorField::new("canvas_height", 8, 1),
    DescriptorField::new("flags", 9, 1),
    DescriptorField::new("origin_q16", 10, 4),
    DescriptorField::new("axis_u_q16", 14, 4),
    DescriptorField::new("axis_v_q16", 18, 4),
    DescriptorField::new("constraint0_q16", 22, 4),
    DescriptorField::new("constraint1_q16", 26, 4),
    DescriptorField::new("constraint2_q16", 30, 4),
    DescriptorField::new("constraint3_q16", 34, 4),
    DescriptorField::new("constraint4_q16", 38, 4),
    DescriptorField::new("constraint5_q16", 42, 4),
    DescriptorField::new("constraint6_q16", 46, 4),
    DescriptorField::new("constraint7_q16", 50, 4),
    DescriptorField::new("constraint_count", 54, 1),
    DescriptorField::new("color_rgba", 55, 1),
];
const PATCH_DESC: DescriptorLayout<'_> =
    DescriptorLayout::new("Canvas3dPlanePatchDesc", 56, Some(32), PATCH_DESC_FIELDS);

macro_rules! ro_buf {
    ($index:expr, $name:expr, $ty:expr, $binding:expr, $payload:expr) => {
        KernelCallArg::buffer($index, $name, $ty, KernelArgAccess::ReadOnly, $binding, $payload)
    };
}

macro_rules! wo_buf {
    ($index:expr, $name:expr, $ty:expr, $binding:expr, $payload:expr) => {
        KernelCallArg::buffer($index, $name, $ty, KernelArgAccess::WriteOnly, $binding, $payload)
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

macro_rules! int4_arg {
    ($index:expr, $name:expr, $payload:expr) => {
        KernelCallArg::value_kind($index, $name, "int4", KernelArgKind::Pod, 16, 16, $payload)
    };
}

const NO_DESCS: &[DescriptorLayout<'_>] = &[];
const FILL_RECT_DESCS: &[DescriptorLayout<'_>] = &[FILL_RECT_DESC];
const GRADIENT_RECT_DESCS: &[DescriptorLayout<'_>] = &[GRADIENT_RECT_DESC];
const ALPHA_BLEND_DESCS: &[DescriptorLayout<'_>] = &[ALPHA_BLEND_DESC];
const SPRITE64_DESCS: &[DescriptorLayout<'_>] = &[SPRITE64_DESC];
const MANDEL64_DESCS: &[DescriptorLayout<'_>] = &[MANDEL64_DESC];
const PATCH_DESCS: &[DescriptorLayout<'_>] = &[PATCH_DESC];

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
    source_path: "src/intel/kernels/copy_rect_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
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
    source_path: "src/intel/kernels/fill_rect_rgba8.cl",
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
    rw_buf!(0, "dst_rgba", "__global uint*", 0, 12),
    ro_buf!(1, "descs", "__global const uint*", 1, 14),
    u32_arg!(2, "dst_pitch_bytes", 16),
    u32_arg!(3, "desc_base", 17),
    u32_arg!(4, "desc_count", 18),
];
const FILL_RECT_WORKLIST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/fill_rect_worklist_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: RECT_WORKLIST_CROSS_THREAD_BYTES,
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
    source_path: "src/intel/kernels/gradient_rect_worklist_rgba8.cl",
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

const BLIT_RGBA8_NEAREST_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_rgba", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    u32_arg!(2, "src_pitch_bytes", 16),
    u32_arg!(3, "dst_pitch_bytes", 17),
    u32_arg!(4, "src_x", 18),
    u32_arg!(5, "src_y", 19),
    u32_arg!(6, "src_width", 20),
    u32_arg!(7, "src_height", 21),
    u32_arg!(8, "dst_x", 22),
    u32_arg!(9, "dst_y", 23),
    u32_arg!(10, "dst_width", 24),
    u32_arg!(11, "dst_height", 25),
];
const BLIT_RGBA8_NEAREST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::BLIT_RGBA8_NEAREST_KERNEL_NAME,
    source_path: "src/intel/kernels/blit_rgba8_nearest.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: COPY_CROSS_THREAD_BYTES,
    per_thread_bytes: COPY_PER_THREAD_BYTES,
    binding_count: 2,
    args: BLIT_RGBA8_NEAREST_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: BOOT_UPLOAD_CONSUMERS,
};

const ALPHA_BLEND_WORKLIST_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_rgba", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    ro_buf!(2, "descs", "__global const uint*", 2, 16),
    u32_arg!(3, "src_pitch_bytes", 18),
    u32_arg!(4, "dst_pitch_bytes", 19),
    u32_arg!(5, "desc_base", 20),
    u32_arg!(6, "desc_count", 21),
];
const ALPHA_BLEND_WORKLIST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/alpha_blend_worklist_rgba8.cl",
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
    source_path: "src/intel/kernels/glyph_mask_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: GLYPH_MASK_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: GLYPH_MASK_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: UI3_TEXT_CONSUMERS,
};

const PRESENT_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_rgba", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_xrgb", "__global uint*", 1, 14),
    u32_arg!(2, "src_pitch_bytes", 16),
    u32_arg!(3, "dst_pitch_bytes", 17),
    u32_arg!(4, "src_x", 18),
    u32_arg!(5, "src_y", 19),
    u32_arg!(6, "dst_x", 20),
    u32_arg!(7, "dst_y", 21),
    u32_arg!(8, "width", 22),
    u32_arg!(9, "height", 23),
    u32_arg!(10, "flip_y", 24),
];
const PRESENT_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME,
    source_path: "src/intel/kernels/present_rgba8_to_primary_xrgb_rect.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: PRESENT_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: PRESENT_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_2d(None),
    consumers: &[
        "r::ui_surface present",
        "intel::present_rgba_frame_to_primary",
    ],
};

const SPRITE64_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "atlas_rgba", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    ro_buf!(2, "descs", "__global const Sprite64Desc*", 2, 16),
    u32_arg!(3, "atlas_pitch_bytes", 18),
    u32_arg!(4, "dst_pitch_bytes", 19),
    u32_arg!(5, "desc_base", 20),
    u32_arg!(6, "desc_count", 21),
];
const SPRITE64_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/sprite64_worklist_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: SPRITE64_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: SPRITE64_WORKLIST_PER_THREAD_BYTES,
    binding_count: 3,
    args: SPRITE64_ARGS,
    descriptor_layouts: SPRITE64_DESCS,
    launch: KernelLaunchContract::descriptor_worklist(16),
    consumers: UI3_TEXT_CONSUMERS,
};

const MANDEL64_ARGS: &[KernelCallArg<'_>] = FILL_RECT_WORKLIST_ARGS;
const MANDEL64_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/mandel64_worklist_rgba8.cl",
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
        "shell2:gpgpu canvas2d mandel64",
        "gpgpu mandel64 probe",
    ],
};

const CANVAS3D_PROJECT_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "vertices_q16", "__global const int4*", 0, 12),
    wo_buf!(1, "out_points", "__global Canvas3dProjectedPoint*", 1, 14),
    u32_arg!(2, "src_first_vertex", 16),
    u32_arg!(3, "out_first_point", 17),
    u32_arg!(4, "vertex_count", 18),
    u32_arg!(5, "canvas_width", 19),
    u32_arg!(6, "canvas_height", 20),
];
const CANVAS3D_PROJECT_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_PROJECT_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_project_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_PROJECT_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_PROJECT_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: UI3_CANVAS_CONSUMERS,
};

const CANVAS3D_TRANSFORM_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_vertices_q16", "__global const int4*", 0, 12),
    wo_buf!(1, "dst_vertices_q16", "__global int4*", 1, 14),
    u32_arg!(2, "src_first_vertex", 16),
    u32_arg!(3, "dst_first_vertex", 17),
    u32_arg!(4, "vertex_count", 18),
    int4_arg!(5, "scale_q16", 20),
    int4_arg!(6, "quat_q16", 24),
    int4_arg!(7, "delta_q16", 28),
];
const CANVAS3D_TRANSFORM_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_TRANSFORM_Q16_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_transform_q16.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_TRANSFORM_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_TRANSFORM_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: UI3_CANVAS_CONSUMERS,
};

const CANVAS3D_CLIP_BOX_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "src_vertices_q16", "__global const int4*", 0, 12),
    wo_buf!(1, "dst_vertices_q16", "__global int4*", 1, 14),
    u32_arg!(2, "src_first_vertex", 16),
    u32_arg!(3, "dst_first_vertex", 17),
    u32_arg!(4, "vertex_count", 18),
    int4_arg!(5, "min_q16", 20),
    int4_arg!(6, "max_q16", 24),
];
const CANVAS3D_CLIP_BOX_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_clip_box_q16.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_CLIP_BOX_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_CLIP_BOX_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: UI3_CANVAS_CONSUMERS,
};

const CANVAS3D_PLANE_SAMPLE_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "unused_q16", "__global const int4*", 0, 12),
    wo_buf!(1, "out_points", "__global Canvas3dProjectedPoint*", 1, 14),
    u32_arg!(2, "out_first_point", 16),
    u32_arg!(3, "sample_count", 17),
    u32_arg!(4, "canvas_width", 18),
    u32_arg!(5, "canvas_height", 19),
    int4_arg!(6, "origin_q16", 20),
    int4_arg!(7, "axis_u_q16", 24),
    int4_arg!(8, "axis_v_q16", 28),
    int4_arg!(9, "constraint0_q16", 32),
    int4_arg!(10, "constraint1_q16", 36),
    int4_arg!(11, "constraint2_q16", 40),
    int4_arg!(12, "constraint3_q16", 44),
    u32_arg!(13, "constraint_count", 48),
    u32_arg!(14, "u_steps", 49),
    u32_arg!(15, "v_steps", 50),
    u32_arg!(16, "color_rgba", 51),
];
const CANVAS3D_PLANE_SAMPLE_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_plane_sample_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_PLANE_SAMPLE_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_PLANE_SAMPLE_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: UI3_CANVAS_CONSUMERS,
};

const CANVAS3D_PLANE_FILL_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "unused_src", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    u32_arg!(2, "dst_pitch_bytes", 16),
    u32_arg!(3, "dst_width", 17),
    u32_arg!(4, "dst_height", 18),
    u32_arg!(5, "rect_x", 19),
    u32_arg!(6, "rect_y", 20),
    u32_arg!(7, "rect_width", 21),
    u32_arg!(8, "rect_height", 22),
    u32_arg!(9, "canvas_width", 23),
    u32_arg!(10, "canvas_height", 24),
    int4_arg!(11, "origin_q16", 28),
    int4_arg!(12, "axis_u_q16", 32),
    int4_arg!(13, "axis_v_q16", 36),
    int4_arg!(14, "constraint0_q16", 40),
    int4_arg!(15, "constraint1_q16", 44),
    int4_arg!(16, "constraint2_q16", 48),
    int4_arg!(17, "constraint3_q16", 52),
    u32_arg!(18, "constraint_count", 56),
    u32_arg!(19, "color_rgba", 57),
];
const CANVAS3D_PLANE_FILL_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_plane_fill_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_PLANE_FILL_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_PLANE_FILL_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: UI3_CANVAS_CONSUMERS,
};

const CANVAS3D_PLANE_PATCH_FILL_CUT_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_plane_patch_fill_cut_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_PLANE_FILL_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_PLANE_FILL_ARGS,
    descriptor_layouts: NO_DESCS,
    launch: KernelLaunchContract::nd_range_1d(),
    consumers: UI3_CANVAS_CONSUMERS,
};

const CANVAS3D_PLANE_PATCH_WORKLIST_ARGS: &[KernelCallArg<'_>] = &[
    ro_buf!(0, "unused_src", "__global const uint*", 0, 12),
    rw_buf!(1, "dst_rgba", "__global uint*", 1, 14),
    ro_buf!(2, "descs", "__global const uint*", 2, 16),
    u32_arg!(3, "desc_base", 18),
    u32_arg!(4, "desc_count", 19),
    u32_arg!(5, "work_base", 20),
];
const CANVAS3D_PLANE_PATCH_WORKLIST_CONTRACT: GpuKernelContract<'_> = GpuKernelContract {
    name: gpgpu::CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME,
    source_path: "src/intel/kernels/canvas3d_plane_patch_worklist_rgba8.cl",
    producer: IGC,
    target: ADLS,
    entry_text_offset_bytes: TEXT_OFFSET,
    cross_thread_bytes: CANVAS3D_PLANE_PATCH_WORKLIST_CROSS_THREAD_BYTES,
    per_thread_bytes: GENERIC_PER_THREAD_BYTES,
    binding_count: 2,
    args: CANVAS3D_PLANE_PATCH_WORKLIST_ARGS,
    descriptor_layouts: PATCH_DESCS,
    launch: KernelLaunchContract::tiled_descriptor_worklist(16, 8, 16),
    consumers: UI3_CANVAS_CONSUMERS,
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
        name: gpgpu::BLIT_RGBA8_NEAREST_KERNEL_NAME,
        artifact: &gpgpu::BLIT_RGBA8_NEAREST_ADLS_ARTIFACT,
        contract: &BLIT_RGBA8_NEAREST_CONTRACT,
        upload: gpgpu::upload_blit_rgba8_nearest_kernel,
        status: gpgpu::blit_rgba8_nearest_upload_status,
        role: KnownKernelRole::Blit,
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
        name: gpgpu::PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME,
        artifact: &gpgpu::PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_ARTIFACT,
        contract: &PRESENT_CONTRACT,
        upload: gpgpu::upload_present_rgba8_to_primary_xrgb_rect_kernel,
        status: gpgpu::present_rgba8_to_primary_xrgb_rect_upload_status,
        role: KnownKernelRole::Present,
    },
    KnownAotKernel {
        name: gpgpu::SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::SPRITE64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &SPRITE64_CONTRACT,
        upload: gpgpu::upload_sprite64_worklist_rgba8_kernel,
        status: gpgpu::sprite64_worklist_rgba8_upload_status,
        role: KnownKernelRole::Sprite,
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
        name: gpgpu::CANVAS3D_PROJECT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PROJECT_RGBA8_ADLS_ARTIFACT,
        contract: &CANVAS3D_PROJECT_CONTRACT,
        upload: gpgpu::upload_canvas3d_project_rgba8_kernel,
        status: gpgpu::canvas3d_project_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_TRANSFORM_Q16_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_TRANSFORM_Q16_ADLS_ARTIFACT,
        contract: &CANVAS3D_TRANSFORM_CONTRACT,
        upload: gpgpu::upload_canvas3d_transform_q16_kernel,
        status: gpgpu::canvas3d_transform_q16_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_CLIP_BOX_Q16_ADLS_ARTIFACT,
        contract: &CANVAS3D_CLIP_BOX_CONTRACT,
        upload: gpgpu::upload_canvas3d_clip_box_q16_kernel,
        status: gpgpu::canvas3d_clip_box_q16_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_ARTIFACT,
        contract: &CANVAS3D_PLANE_SAMPLE_CONTRACT,
        upload: gpgpu::upload_canvas3d_plane_sample_rgba8_kernel,
        status: gpgpu::canvas3d_plane_sample_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_FILL_RGBA8_ADLS_ARTIFACT,
        contract: &CANVAS3D_PLANE_FILL_CONTRACT,
        upload: gpgpu::upload_canvas3d_plane_fill_rgba8_kernel,
        status: gpgpu::canvas3d_plane_fill_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_ARTIFACT,
        contract: &CANVAS3D_PLANE_PATCH_FILL_CUT_CONTRACT,
        upload: gpgpu::upload_canvas3d_plane_patch_fill_cut_rgba8_kernel,
        status: gpgpu::canvas3d_plane_patch_fill_cut_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_ARTIFACT,
        contract: &CANVAS3D_PLANE_PATCH_WORKLIST_CONTRACT,
        upload: gpgpu::upload_canvas3d_plane_patch_worklist_rgba8_kernel,
        status: gpgpu::canvas3d_plane_patch_worklist_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
];

pub(crate) fn known_aot_kernel(name: &str) -> Option<&'static KnownAotKernel> {
    KNOWN_AOT_KERNELS.iter().find(|kernel| kernel.name == name)
}

pub(crate) fn is_known_aot_kernel(name: &str) -> bool {
    known_aot_kernel(name).is_some()
}
