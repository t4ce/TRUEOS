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
    // disk I/O. Diagnostic TRUEOSFS publication exposes a root to explicit
    // Shell2 consumers before global readiness; runtime overrides remain on
    // their embedded artifacts until the filesystem capability is published.
    let trueosfs_ready = crate::r::readiness::is_set(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
    );
    if trueosfs_ready && !crate::percpu::in_executor_poll() {
        match read_runtime_artifact_bytes(artifact.name) {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                let path = runtime_artifact_display_path(artifact.name);
                let spv_bytes = read_runtime_spv_len(artifact.name).unwrap_or(artifact.spv.len());
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
                    runtime_artifact_display_path(artifact.name)
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
                    runtime_artifact_display_path(artifact.name),
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
            runtime_artifact_display_path(artifact.name),
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
    if bin.is_empty() {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=empty source={} path={}\n",
            artifact.name,
            source,
            source_path
        );
        return None;
    }
    if let Err(reason) = validate_kernel_artifact_bytes(bin) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason={} source={} path={} bytes=0x{:X}\n",
            artifact.name,
            reason,
            source,
            source_path,
            bin.len()
        );
        return None;
    }
    let actual_sha256 = sha256_digest(bin);
    let requires_allowlisted_sha = matches!(
        artifact.name,
        CHART_SINE_RGBA8_KERNEL_NAME
            | PIXEL_PLASMA_RGBA8_KERNEL_NAME
            | FONT_OUTLINE_MESH_KERNEL_NAME
            | FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME
    );
    if requires_allowlisted_sha && actual_sha256 != artifact.bin_sha256 {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: {} upload rejected reason=sha256-not-allowlisted source={} path={} expected={} actual={}\n",
            artifact.name,
            source,
            source_path,
            digest_hex(&artifact.bin_sha256).as_str(),
            digest_hex(&actual_sha256).as_str()
        );
        return None;
    }

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
    };
    let source_bytes = kernel_opencl_source(artifact.name)
        .map(|source| source.len())
        .unwrap_or(0);
    let sha256 = digest_hex(&upload.bin_sha256);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} upload ok=1 target={} source={} path={} source_bytes=0x{:X} spv_bytes=0x{:X} phys=0x{:X} gpu=0x{:X} bytes=0x{:X} mapped=0x{:X} sha256={}\n",
        artifact.name,
        upload.target,
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

fn runtime_artifact_rel_path(name: &str, ext: &str) -> String {
    alloc::format!("gpgpu/adls/{name}.{ext}")
}

fn runtime_artifact_display_path(name: &str) -> String {
    alloc::format!("/{}", runtime_artifact_rel_path(name, "bin"))
}

fn read_runtime_artifact_bytes(name: &str) -> Result<Option<Vec<u8>>, crate::io::kfs::FsError> {
    match crate::io::kfs::read_file(runtime_artifact_rel_path(name, "bin").as_str()) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(crate::io::kfs::FsError::NoRoot | crate::io::kfs::FsError::NotFound) => Ok(None),
        Err(err) => Err(err),
    }
}

fn read_runtime_spv_len(name: &str) -> Option<usize> {
    match crate::io::kfs::read_file_len(runtime_artifact_rel_path(name, "spv").as_str()) {
        Ok(len) => Some(len),
        Err(_) => None,
    }
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
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    if machine != ELF_MACHINE_INTEL_GT {
        return Err("wrong-machine");
    }
    Ok(())
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
