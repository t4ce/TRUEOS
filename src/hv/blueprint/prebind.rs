use alloc::string::String;

pub(crate) fn prebind_base_readiness() -> u32 {
    crate::r::readiness::BACKGROUND_AP_WORKER_READY
}

pub(crate) fn prebind_import_readiness(name: &str) -> u32 {
    let mut mask = 0;

    if name.starts_with("trueos_cabi_gfx_upload_texture_") {
        mask |= crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY;
    } else if name.starts_with("trueos_cabi_gfx_texture_")
        || name.starts_with("trueos_cabi_gfx_capture_")
    {
        mask |= crate::r::readiness::GFX_BACKEND_READY;
    }

    if name.starts_with("trueos_cabi_fs_") || name.starts_with("trueos_cabi_trueosfs_") {
        mask |= crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
    }

    if name.starts_with("trueos_cabi_net_fetch_") {
        mask |= crate::r::readiness::NET_ANY_CONFIGURED
            | crate::r::readiness::NET_SOCKET_READY
            | crate::r::readiness::TLS_SOCKET_SERVICE_READY;
    } else if name.starts_with("trueos_cabi_socket_") || name.starts_with("trueos_mio_") {
        mask |= crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::NET_SOCKET_READY;
    }

    if name.starts_with("trueos_cabi_hda_") || name.starts_with("trueos_cabi_audio_") {
        mask |= crate::r::readiness::INTEL_HDA_READY;
    }

    mask
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
        required |= prebind_import_readiness(import.name);
    }
    Ok(required)
}
