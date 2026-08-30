/// Relocatable, workload-neutral contract for persistent RGBA16F 3D resources.
///
/// The artifact carries logical resources, views, sampler semantics, and the
/// compiler-authenticated binding-table layout. Runtime resolution contributes
/// only the page-backed allocation addresses. Intel SURFACE_STATE and
/// SAMPLER_STATE bit encoding deliberately remains a later, hardware-proven
/// lowering step.
#[allow(dead_code)]
pub(crate) mod volume_resources {
    use super::{GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL, GpgpuRgba16FloatVolume3d};
    #[cfg(test)]
    use super::GpgpuVolumePhysicalBacking;
    use sha2::{Digest, Sha256};
    use trueos_helio_artifact::{Artifact, SectionKind};

    include!("volume_resources/contract.rs");
    include!("volume_resources/program.rs");
    include!("volume_resources/validation.rs");
    include!("volume_resources/tests.rs");

    #[cfg(test)]
    pub(crate) fn test_ping_pong_volume_resource() -> (alloc::vec::Vec<u8>, &'static [u8]) {
        const METADATA: &[u8] = br#"{\"source\":\"igc\",\"profile\":\"ping-pong-volume\"}
"#;
        let mut resource = tests::ping_pong_fixture();
        let digest = Sha256::digest(METADATA);
        resource[48..80].copy_from_slice(&digest);
        (resource, METADATA)
    }
}
