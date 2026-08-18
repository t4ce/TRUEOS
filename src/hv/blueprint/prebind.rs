use alloc::string::String;

pub(crate) fn prebind_base_readiness() -> u32 {
    crate::r::readiness::BACKGROUND_AP_WORKER_READY
}

pub(crate) fn prebind_import_readiness(name: &str) -> u32 {
    let mut mask = 0;

    if is_rayon_import(name) {
        mask |= crate::r::readiness::RAYON_READY;
    }

    if name.starts_with("trueos_cabi_async_fs_") {
        mask |= crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
    }

    if name.starts_with("trueos_cabi_archive_") {
        // Archive jobs run on the background worker pool and complete only
        // after their TRUEOSFS destination writes have committed.
        mask |= crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
            | crate::r::readiness::BACKGROUND_AP_WORKER_READY;
    }

    if name.starts_with("trueos_cabi_vmedia_") {
        // Raster decode is owner-scoped and serviced by the background media
        // worker pool; it never executes a decoder in the VMCALL handler.
        mask |= crate::r::readiness::BACKGROUND_AP_WORKER_READY;
    }

    if name.starts_with("trueos_cabi_net_fetch_") {
        mask |= crate::r::readiness::NET_ANY_CONFIGURED
            | crate::r::readiness::NET_SOCKET_READY
            | crate::r::readiness::TLS_SOCKET_SERVICE_READY;
    } else if name == "trueos_cabi_dns_resolve_ipv4" {
        // The ABI shape is synchronous, but its implementation is a parked
        // AP/VM carrier request serviced by the BSP async secure-DNS stack.
        mask |=
            crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY;
    } else if name.starts_with("trueos_cabi_socket_")
        || name.starts_with("trueos_cabi_tun_")
        || name.starts_with("trueos_mio_")
    {
        mask |= crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::NET_SOCKET_READY;
    }

    if name.starts_with("trueos_cabi_hda_") || name.starts_with("trueos_cabi_audio_") {
        mask |= crate::r::readiness::INTEL_HDA_READY;
    }

    mask
}

pub(crate) fn prebind_import_error(name: &str) -> Option<&'static str> {
    if let Some(reason) = crate::unix_compat::unsupported_unix_import_reason(name) {
        return Some(reason);
    }
    if name.starts_with("trueos_cabi_fs_") || name.starts_with("trueos_cabi_trueosfs_") {
        return Some(
            "synchronous Blueprint filesystem ABI removed; rebuild against trueos::async_fs",
        );
    }
    match name {
        "trueos_tokio_tls_current_slot" => Some(
            "legacy TLS ABI import trueos_tokio_tls_current_slot; rebuild/refetch the blueprint so TLS uses WLS trueos_cabi_wls_current_slot",
        ),
        _ => None,
    }
}

fn is_rayon_import(name: &str) -> bool {
    name.starts_with("trueos_cabi_rayon_") || name.starts_with("trueos_rayon_")
}

#[cfg(test)]
mod tests {
    #[test]
    fn directory_stream_imports_fail_prebind_before_guest_execution() {
        for name in [
            "opendir",
            "fdopendir",
            "readdir",
            "readdir_r",
            "closedir",
            "dirfd",
        ] {
            let error = super::prebind_import_error(name)
                .unwrap_or_else(|| panic!("{name} was accepted as a functional import"));
            assert!(error.contains("directory-stream"), "unexpected error: {error}");
        }
    }
}

pub(crate) fn prebind_required_readiness(module_bytes: &[u8]) -> Result<u32, String> {
    let module = super::parse_blueprint(module_bytes).map_err(String::from)?;
    let unpacked = super::unpack_blueprint(&module).map_err(String::from)?;

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(super::elf_type_name(unpacked.as_slice()), Some("REL"))
    {
        return Err(String::from("only ELF REL blueprints are supported for app-vm launch"));
    }

    let mut required = prebind_base_readiness();
    let imports = super::elf_imports(unpacked.as_slice()).map_err(String::from)?;
    for import in imports.iter() {
        if let Some(err) = prebind_import_error(import.name) {
            return Err(alloc::format!("unsupported Blueprint import {}: {}", import.name, err));
        }
        required |= prebind_import_readiness(import.name);
    }
    Ok(required)
}
