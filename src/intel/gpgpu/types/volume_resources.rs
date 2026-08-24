/// Relocatable Helio-to-TRUEOS contract for persistent RGBA16F 3D resources.
///
/// The artifact carries logical resources, views, sampler semantics, and the
/// compiler-authenticated binding-table layout. Runtime resolution contributes
/// only the page-backed allocation addresses. Intel SURFACE_STATE and
/// SAMPLER_STATE bit encoding deliberately remains a later, hardware-proven
/// lowering step.
#[allow(dead_code)]
pub(crate) mod helio_volume_resources {
    use super::{GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL, GpgpuRgba16FloatVolume3d};
    use sha2::{Digest, Sha256};
    use trueos_helio_artifact::{Artifact, SectionKind};

    include!("volume_resources/contract.rs");
    include!("volume_resources/program.rs");
    include!("volume_resources/validation.rs");
    include!("volume_resources/tests.rs");
}
