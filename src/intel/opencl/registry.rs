//! Registry for known offline-compiled Intel OpenCL artifacts.
//!
//! This keeps the OpenCL facade honest: a kernel is "known" only if the current
//! TRUEOS GPGPU backend can upload and report status for its AOT binary.

use crate::intel::gpgpu;

pub(crate) type UploadFn = fn() -> Option<gpgpu::UploadedKernelArtifact>;
pub(crate) type StatusFn = fn() -> Option<gpgpu::UploadedKernelArtifact>;

#[derive(Copy, Clone)]
pub(crate) struct KnownAotKernel {
    pub(crate) name: &'static str,
    pub(crate) artifact: &'static gpgpu::GpgpuKernelArtifact,
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

pub(crate) const KNOWN_AOT_KERNELS: &[KnownAotKernel] = &[
    KnownAotKernel {
        name: gpgpu::COPY_RECT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::COPY_RECT_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_copy_rect_rgba8_kernel,
        status: gpgpu::copy_rect_rgba8_upload_status,
        role: KnownKernelRole::Copy,
    },
    KnownAotKernel {
        name: gpgpu::FILL_RECT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::FILL_RECT_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_fill_rect_rgba8_kernel,
        status: gpgpu::fill_rect_rgba8_upload_status,
        role: KnownKernelRole::Fill,
    },
    KnownAotKernel {
        name: gpgpu::FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_fill_rect_worklist_rgba8_kernel,
        status: gpgpu::fill_rect_worklist_rgba8_upload_status,
        role: KnownKernelRole::WorklistFill,
    },
    KnownAotKernel {
        name: gpgpu::GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_gradient_rect_worklist_rgba8_kernel,
        status: gpgpu::gradient_rect_worklist_rgba8_upload_status,
        role: KnownKernelRole::WorklistGradient,
    },
    KnownAotKernel {
        name: gpgpu::BLIT_RGBA8_NEAREST_KERNEL_NAME,
        artifact: &gpgpu::BLIT_RGBA8_NEAREST_ADLS_ARTIFACT,
        upload: gpgpu::upload_blit_rgba8_nearest_kernel,
        status: gpgpu::blit_rgba8_nearest_upload_status,
        role: KnownKernelRole::Blit,
    },
    KnownAotKernel {
        name: gpgpu::ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_alpha_blend_worklist_rgba8_kernel,
        status: gpgpu::alpha_blend_worklist_rgba8_upload_status,
        role: KnownKernelRole::WorklistBlend,
    },
    KnownAotKernel {
        name: gpgpu::GLYPH_MASK_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::GLYPH_MASK_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_glyph_mask_rgba8_kernel,
        status: gpgpu::glyph_mask_rgba8_upload_status,
        role: KnownKernelRole::Glyph,
    },
    KnownAotKernel {
        name: gpgpu::PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_KERNEL_NAME,
        artifact: &gpgpu::PRESENT_RGBA8_TO_PRIMARY_XRGB_RECT_ADLS_ARTIFACT,
        upload: gpgpu::upload_present_rgba8_to_primary_xrgb_rect_kernel,
        status: gpgpu::present_rgba8_to_primary_xrgb_rect_upload_status,
        role: KnownKernelRole::Present,
    },
    KnownAotKernel {
        name: gpgpu::SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::SPRITE64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_sprite64_worklist_rgba8_kernel,
        status: gpgpu::sprite64_worklist_rgba8_upload_status,
        role: KnownKernelRole::Sprite,
    },
    KnownAotKernel {
        name: gpgpu::MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_mandel64_worklist_rgba8_kernel,
        status: gpgpu::mandel64_worklist_rgba8_upload_status,
        role: KnownKernelRole::Mandel,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PROJECT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PROJECT_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_canvas3d_project_rgba8_kernel,
        status: gpgpu::canvas3d_project_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_TRANSFORM_Q16_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_TRANSFORM_Q16_ADLS_ARTIFACT,
        upload: gpgpu::upload_canvas3d_transform_q16_kernel,
        status: gpgpu::canvas3d_transform_q16_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_CLIP_BOX_Q16_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_CLIP_BOX_Q16_ADLS_ARTIFACT,
        upload: gpgpu::upload_canvas3d_clip_box_q16_kernel,
        status: gpgpu::canvas3d_clip_box_q16_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_SAMPLE_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_SAMPLE_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_canvas3d_plane_sample_rgba8_kernel,
        status: gpgpu::canvas3d_plane_sample_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_FILL_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_FILL_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_canvas3d_plane_fill_rgba8_kernel,
        status: gpgpu::canvas3d_plane_fill_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_ADLS_ARTIFACT,
        upload: gpgpu::upload_canvas3d_plane_patch_fill_cut_rgba8_kernel,
        status: gpgpu::canvas3d_plane_patch_fill_cut_rgba8_upload_status,
        role: KnownKernelRole::Canvas3d,
    },
    KnownAotKernel {
        name: gpgpu::CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_KERNEL_NAME,
        artifact: &gpgpu::CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_ADLS_ARTIFACT,
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
