fn upload_artifact(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
) -> Option<UploadedKernelArtifact> {
    upload_artifact_from_sources(dev, artifact, gpu, false)
}

fn upload_artifact_from_sources(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
    strict_runtime_artifact: bool,
) -> Option<UploadedKernelArtifact> {
    // `kfs::read_file` is a synchronous wrapper around a future queued on the
    // current executor. Calling it from an Embassy task cannot make progress:
    // the executor re-entry guard rejects the recursive poll. UI4 reaches
    // first-use uploads from its compositor and producer tasks, so those paths
    // must use the build-embedded artifact instead of freezing the whole UI
    // core. Runtime-artifact overrides remain available to callers outside an
    // executor poll; a strict reload attempted inside one is rejected instead
    // of deadlocking and must eventually be exposed through an async loader.
    // Filesystem visibility alone must not opt an automatic graphics path into
    // disk I/O. Runtime overrides remain on their embedded artifacts until the
    // filesystem capability is fully published.
    let trueosfs_ready = crate::r::readiness::is_set(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
    );
    if trueosfs_ready && !crate::percpu::in_executor_poll() {
        match read_runtime_artifact_bytes(artifact) {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                let path = runtime_artifact_display_path(artifact);
                let spv_bytes = read_runtime_spv_len(artifact).unwrap_or(artifact.spv.len());
                return upload_artifact_bytes(
                    dev,
                    artifact,
                    gpu,
                    bytes.as_slice(),
                    "fs",
                    path.as_str(),
                    spv_bytes,
                );
            }
            Ok(Some(_)) => {
                crate::log_info!(
                    target: "gpgpu";
                    "intel/gpgpu: {} runtime artifact rejected reason=empty path={}\n",
                    artifact.name,
                    runtime_artifact_display_path(artifact)
                );
                if strict_runtime_artifact {
                    return None;
                }
            }
            Ok(None) => {}
            Err(err) => {
                crate::log_info!(
                    target: "gpgpu";
                    "intel/gpgpu: {} runtime artifact read failed path={} err={:?}\n",
                    artifact.name,
                    runtime_artifact_display_path(artifact),
                    err
                );
                if strict_runtime_artifact {
                    return None;
                }
            }
        }
    } else if strict_runtime_artifact {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} runtime artifact reload rejected reason={} path={}\n",
            artifact.name,
            if trueosfs_ready {
                "executor-context-would-deadlock"
            } else {
                "trueosfs-not-ready"
            },
            runtime_artifact_display_path(artifact),
        );
        return None;
    } else {
        static EXECUTOR_EMBEDDED_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);
        if !EXECUTOR_EMBEDDED_FALLBACK_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_info!(
                target: "gpgpu";
                "intel/gpgpu: runtime artifact lookup bypassed kernel={} reason={} fallback=embedded\n",
                artifact.name,
                if trueosfs_ready {
                    "executor-context-would-deadlock"
                } else {
                    "trueosfs-not-ready"
                },
            );
        }
    }

    let source_path = kernel_source_path(artifact.name).unwrap_or("embedded");
    upload_artifact_bytes(
        dev,
        artifact,
        gpu,
        artifact.bin,
        "embedded",
        source_path,
        artifact.spv.len(),
    )
}

fn upload_artifact_bytes(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
    bin: &[u8],
    source: &'static str,
    source_path: &str,
    spv_bytes: usize,
) -> Option<UploadedKernelArtifact> {
    let actual_sha256 = match admit_kernel_artifact_bytes(
        artifact,
        dev.device_id,
        dev.revision_id,
        bin,
    ) {
        Ok(digest) => digest,
        Err(error) => {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: {} upload rejected reason={} target={} device=0x{:04X} revision=0x{:02X} source={} path={} bytes=0x{:X}\n",
                artifact.name,
                error.label(),
                artifact.target,
                dev.device_id,
                dev.revision_id,
                source,
                source_path,
                bin.len()
            );
            if matches!(error, GpgpuArtifactAdmissionError::ZebinHashMismatch) {
                let actual = sha256_digest(bin);
                crate::log_error!(
                    target: "gpgpu";
                    "intel/gpgpu: {} hash expected={} actual={}\n",
                    artifact.name,
                    digest_hex(&artifact.bin_sha256).as_str(),
                    digest_hex(&actual).as_str()
                );
            }
            return None;
        }
    };
    let mapped_bytes = align_up(bin.len(), super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(bin.as_ptr(), virt, bin.len());
    }
    super::dma_flush(virt, mapped_bytes);

    let uploaded = unsafe { core::slice::from_raw_parts(virt, bin.len()) };
    let verified = uploaded == bin;
    if !verified {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=verify source={} path={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            source,
            source_path,
            phys,
            gpu,
            bin.len()
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }

    if !super::map_ggtt(dev, phys, mapped_bytes, gpu) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=ggtt-map source={} path={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            source,
            source_path,
            phys,
            gpu,
            mapped_bytes
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }
    super::ggtt_invalidate(dev);

    let upload = UploadedKernelArtifact {
        name: artifact.name,
        target: artifact.target,
        source,
        gpu,
        phys,
        bytes: bin.len(),
        mapped_bytes,
        verified,
        bin_sha256: actual_sha256,
        device_id: dev.device_id,
        revision_id: dev.revision_id,
        abi_schema_version: artifact
            .abi_contract
            .map(|contract| contract.schema_version),
    };
    let source_bytes = kernel_opencl_source(artifact.name)
        .map(|source| source.len())
        .unwrap_or(0);
    let sha256 = digest_hex(&upload.bin_sha256);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} upload ok=1 target={} device=0x{:04X} revision=0x{:02X} abi_schema={} source={} path={} source_bytes=0x{:X} spv_bytes=0x{:X} phys=0x{:X} gpu=0x{:X} bytes=0x{:X} mapped=0x{:X} sha256={}\n",
        artifact.name,
        upload.target,
        upload.device_id,
        upload.revision_id,
        upload.abi_schema_version.unwrap_or(0),
        upload.source,
        source_path,
        source_bytes,
        spv_bytes,
        upload.phys,
        upload.gpu,
        upload.bytes,
        upload.mapped_bytes,
        sha256.as_str(),
    );
    Some(upload)
}

fn runtime_artifact_rel_path(artifact: GpgpuKernelArtifact, ext: &str) -> String {
    alloc::format!("gpgpu/{}/{}.{ext}", artifact.target, artifact.name)
}

fn runtime_artifact_display_path(artifact: GpgpuKernelArtifact) -> String {
    alloc::format!("/{}", runtime_artifact_rel_path(artifact, "bin"))
}

fn read_runtime_artifact_bytes(
    artifact: GpgpuKernelArtifact,
) -> Result<Option<Vec<u8>>, crate::io::kfs::FsError> {
    match crate::io::kfs::read_file(runtime_artifact_rel_path(artifact, "bin").as_str()) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(crate::io::kfs::FsError::NoRoot | crate::io::kfs::FsError::NotFound) => Ok(None),
        Err(err) => Err(err),
    }
}

fn read_runtime_spv_len(artifact: GpgpuKernelArtifact) -> Option<usize> {
    match crate::io::kfs::read_file_len(runtime_artifact_rel_path(artifact, "spv").as_str()) {
        Ok(len) => Some(len),
        Err(_) => None,
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactAdmissionError {
    InvalidTargetPolicy(GpgpuKernelTargetError),
    TargetLabelMismatch,
    UnsupportedPciDevice,
    UnsupportedRevision,
    InvalidElf(&'static str),
    EmptyExpectedZebinHash,
    ZebinHashMismatch,
    InvalidAbiContract(GpgpuKernelAbiContractError),
    ContractKernelNameMismatch,
    ContractTargetMismatch,
    ContractZebinHashMismatch,
    ContractSpirvHashMismatch,
    ContractElfMismatch(&'static str),
}

impl GpgpuArtifactAdmissionError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::InvalidTargetPolicy(error) => error.label(),
            Self::TargetLabelMismatch => "artifact-target-label-mismatch",
            Self::UnsupportedPciDevice => "artifact-pci-device-incompatible",
            Self::UnsupportedRevision => "artifact-revision-incompatible",
            Self::InvalidElf(reason) => reason,
            Self::EmptyExpectedZebinHash => "artifact-empty-zebin-hash",
            Self::ZebinHashMismatch => "artifact-zebin-sha256-mismatch",
            Self::InvalidAbiContract(error) => error.label(),
            Self::ContractKernelNameMismatch => "artifact-contract-kernel-name-mismatch",
            Self::ContractTargetMismatch => "artifact-contract-target-mismatch",
            Self::ContractZebinHashMismatch => "artifact-contract-zebin-sha256-mismatch",
            Self::ContractSpirvHashMismatch => "artifact-contract-spirv-sha256-mismatch",
            Self::ContractElfMismatch(reason) => reason,
        }
    }
}

/// Pure admission check used immediately before DMA allocation.
///
/// A filesystem override deliberately has no weaker policy than an embedded
/// artifact: until signed external manifests exist, its bytes must match the
/// build-generated allowlist exactly.
pub(crate) fn admit_kernel_artifact_bytes(
    artifact: GpgpuKernelArtifact,
    device_id: u16,
    revision_id: u8,
    bin: &[u8],
) -> Result<[u8; 32], GpgpuArtifactAdmissionError> {
    artifact
        .target_policy
        .validate()
        .map_err(GpgpuArtifactAdmissionError::InvalidTargetPolicy)?;
    if artifact.target != artifact.target_policy.label {
        return Err(GpgpuArtifactAdmissionError::TargetLabelMismatch);
    }
    if !artifact.target_policy.supports(device_id, revision_id) {
        if !artifact.target_policy.supports_device_id(device_id) {
            return Err(GpgpuArtifactAdmissionError::UnsupportedPciDevice);
        }
        return Err(GpgpuArtifactAdmissionError::UnsupportedRevision);
    }

    validate_kernel_artifact_bytes(bin).map_err(GpgpuArtifactAdmissionError::InvalidElf)?;
    if artifact.bin_sha256 == [0; 32] {
        return Err(GpgpuArtifactAdmissionError::EmptyExpectedZebinHash);
    }
    let actual_sha256 = sha256_digest(bin);
    if actual_sha256 != artifact.bin_sha256 {
        return Err(GpgpuArtifactAdmissionError::ZebinHashMismatch);
    }

    if let Some(contract) = artifact.abi_contract {
        contract
            .validate()
            .map_err(GpgpuArtifactAdmissionError::InvalidAbiContract)?;
        if contract.kernel_name != artifact.name {
            return Err(GpgpuArtifactAdmissionError::ContractKernelNameMismatch);
        }
        if contract.target != artifact.target_policy {
            return Err(GpgpuArtifactAdmissionError::ContractTargetMismatch);
        }
        if contract.zebin_sha256 != artifact.bin_sha256 {
            return Err(GpgpuArtifactAdmissionError::ContractZebinHashMismatch);
        }
        if sha256_digest(artifact.spv) != contract.spv_sha256 {
            return Err(GpgpuArtifactAdmissionError::ContractSpirvHashMismatch);
        }
        validate_contract_elf(bin, contract)
            .map_err(GpgpuArtifactAdmissionError::ContractElfMismatch)?;
    }

    Ok(actual_sha256)
}

fn validate_kernel_artifact_bytes(bytes: &[u8]) -> Result<(), &'static str> {
    const ELF64_HEADER_BYTES: usize = 64;
    const ELF_MACHINE_INTEL_GT: u16 = 0x00CD;
    if bytes.len() < ELF64_HEADER_BYTES {
        return Err("truncated-elf");
    }
    if &bytes[0..4] != b"\x7FELF" {
        return Err("not-elf");
    }
    if bytes[4] != 2 {
        return Err("not-elf64");
    }
    if bytes[5] != 1 {
        return Err("not-little-endian");
    }
    if bytes[6] != 1 {
        return Err("invalid-elf-version");
    }
    let elf_type = read_u16(bytes, 16).ok_or("truncated-elf")?;
    if elf_type != 1 {
        return Err("not-relocatable-elf");
    }
    let machine = read_u16(bytes, 18).ok_or("truncated-elf")?;
    if machine != ELF_MACHINE_INTEL_GT {
        return Err("wrong-machine");
    }
    if read_u32(bytes, 20) != Some(1) {
        return Err("invalid-elf-version");
    }
    if read_u16(bytes, 52) != Some(ELF64_HEADER_BYTES as u16) {
        return Err("invalid-elf-header-size");
    }
    Ok(())
}

fn validate_contract_elf(
    bytes: &[u8],
    contract: &GpgpuKernelAbiContract,
) -> Result<(), &'static str> {
    const ELF64_SECTION_HEADER_BYTES: usize = 64;
    const SHN_XINDEX: usize = 0xFFFF;
    const SHT_PROGBITS: u32 = 1;
    const SHF_EXECINSTR: u64 = 1 << 2;

    let section_table_offset =
        usize::try_from(read_u64(bytes, 40).ok_or("contract-elf-missing-section-table")?)
            .map_err(|_| "contract-elf-section-table-overflow")?;
    let section_header_bytes =
        read_u16(bytes, 58).ok_or("contract-elf-missing-section-table")? as usize;
    let section_count = read_u16(bytes, 60).ok_or("contract-elf-missing-section-table")? as usize;
    let section_names_index =
        read_u16(bytes, 62).ok_or("contract-elf-missing-section-table")? as usize;
    if section_header_bytes != ELF64_SECTION_HEADER_BYTES
        || section_count == 0
        || section_names_index == 0
        || section_names_index == SHN_XINDEX
        || section_names_index >= section_count
    {
        return Err("contract-elf-invalid-section-table");
    }
    let section_table_bytes = section_header_bytes
        .checked_mul(section_count)
        .ok_or("contract-elf-section-table-overflow")?;
    let section_table_end = section_table_offset
        .checked_add(section_table_bytes)
        .ok_or("contract-elf-section-table-overflow")?;
    if section_table_end > bytes.len() {
        return Err("contract-elf-section-table-out-of-bounds");
    }

    let names_header =
        section_header(bytes, section_table_offset, section_header_bytes, section_names_index)
            .ok_or("contract-elf-string-table-out-of-bounds")?;
    let names_offset =
        usize::try_from(read_u64(names_header, 24).ok_or("contract-elf-invalid-string-table")?)
            .map_err(|_| "contract-elf-string-table-overflow")?;
    let names_bytes =
        usize::try_from(read_u64(names_header, 32).ok_or("contract-elf-invalid-string-table")?)
            .map_err(|_| "contract-elf-string-table-overflow")?;
    let names_end = names_offset
        .checked_add(names_bytes)
        .ok_or("contract-elf-string-table-overflow")?;
    let names = bytes
        .get(names_offset..names_end)
        .ok_or("contract-elf-string-table-out-of-bounds")?;

    let mut matching_section = None;
    let mut index = 1;
    while index < section_count {
        let header = section_header(bytes, section_table_offset, section_header_bytes, index)
            .ok_or("contract-elf-section-header-out-of-bounds")?;
        let name_offset =
            read_u32(header, 0).ok_or("contract-elf-section-header-out-of-bounds")? as usize;
        let Some(name) = elf_string(names, name_offset) else {
            return Err("contract-elf-section-name-out-of-bounds");
        };
        if name == contract.text_section_name.as_bytes() {
            if matching_section.is_some() {
                return Err("contract-elf-duplicate-text-section");
            }
            matching_section = Some(header);
        }
        index += 1;
    }

    let header = matching_section.ok_or("contract-elf-text-section-missing")?;
    if read_u32(header, 4) != Some(SHT_PROGBITS) {
        return Err("contract-elf-text-section-wrong-type");
    }
    let flags = read_u64(header, 8).ok_or("contract-elf-invalid-text-section")?;
    if flags & SHF_EXECINSTR == 0 {
        return Err("contract-elf-text-section-not-executable");
    }
    let section_offset = read_u64(header, 24).ok_or("contract-elf-invalid-text-section")?;
    let section_size = read_u64(header, 32).ok_or("contract-elf-invalid-text-section")?;
    let section_end = section_offset
        .checked_add(section_size)
        .ok_or("contract-elf-text-section-overflow")?;
    let entry_end = contract
        .text_offset
        .checked_add(contract.text_size)
        .ok_or("contract-elf-text-range-overflow")?;
    if contract.text_offset < section_offset || entry_end > section_end {
        return Err("contract-elf-text-range-mismatch");
    }
    if usize::try_from(section_end).map_or(true, |end| end > bytes.len()) {
        return Err("contract-elf-text-range-out-of-bounds");
    }
    Ok(())
}

fn section_header(
    bytes: &[u8],
    table_offset: usize,
    entry_bytes: usize,
    index: usize,
) -> Option<&[u8]> {
    let offset = table_offset.checked_add(entry_bytes.checked_mul(index)?)?;
    bytes.get(offset..offset.checked_add(entry_bytes)?)
}

fn elf_string(bytes: &[u8], offset: usize) -> Option<&[u8]> {
    let tail = bytes.get(offset..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    tail.get(..end)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?))
}

fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
