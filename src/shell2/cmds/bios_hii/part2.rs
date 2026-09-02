fn build_catalogue() -> Result<HiiCatalogue, String> {
    let captured = locate_captured_sections()?;
    let capture = CaptureMetadata {
        source: captured.source,
        hii_bytes: captured.hii.len(),
        config_captured: captured.config_captured,
    };
    parse_hii_export(captured.hii, capture)
}

fn locate_captured_sections() -> Result<CapturedSections, String> {
    if let Some(payload) = limine_hii_payload()? {
        return parse_payload_sections(payload, "limine-experimental-hii-capture");
    }

    let tables = crate::efi::configuration_tables()
        .map_err(|error| alloc::format!("configuration tables: {error:?}"))?;
    let entry = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
        .ok_or_else(|| String::from("captured HII payload is absent"))?;
    if entry.vendor_table == 0 {
        return Err(String::from("TRBIOS1 table pointer is zero"));
    }

    let catalog_phys = crate::limine::try_as_phys_addr(entry.vendor_table as u64)
        .ok_or_else(|| String::from("TRBIOS1 table pointer is not mappable"))?;
    require_range(catalog_phys, size_of::<CatalogHeader>(), "catalog header")?;
    let mapping = crate::pci::mmio::map_limine_struct::<CatalogHeader>(catalog_phys)
        .map_err(|error| alloc::format!("catalog map: {error:?}"))?;
    let catalog = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };
    if catalog.magic != CATALOG_MAGIC || catalog.version != VERSION {
        return Err(String::from("unsupported TRBIOS1 magic/version"));
    }
    if usize::from(catalog.header_bytes) < size_of::<CatalogHeader>() {
        return Err(String::from("TRBIOS1 header_bytes is too small"));
    }
    let payload_len = catalog.payload_bytes as usize;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!(
            "TRBIOS1 payload bytes={} outside bound",
            payload_len
        ));
    }
    let payload_phys = crate::limine::try_as_phys_addr(catalog.payload_phys)
        .ok_or_else(|| String::from("TRBIOS1 payload pointer is not mappable"))?;
    require_range(payload_phys, payload_len, "catalog payload")?;
    let payload_mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| alloc::format!("payload map: {error:?}"))?;
    let payload = unsafe { core::slice::from_raw_parts(payload_mapping.as_ptr(), payload_len) };
    if crc32fast::hash(payload) != catalog.payload_crc32 {
        return Err(String::from("TRBIOS1 aggregate CRC mismatch"));
    }
    parse_payload_sections(payload, "firmware-scout-trbios1")
}

fn limine_hii_payload() -> Result<Option<&'static [u8]>, String> {
    let Some(response) = crate::limine::trueos_hii_capture_response() else {
        return Ok(None);
    };
    let len = usize::try_from(response.size)
        .map_err(|_| String::from("Limine HII payload size does not fit usize"))?;
    if len == 0 || len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!(
            "Limine HII payload bytes={} outside bound",
            len
        ));
    }
    let phys = crate::limine::try_as_phys_addr(response.address)
        .ok_or_else(|| String::from("Limine HII payload pointer is not mappable"))?;
    require_range(phys, len, "limine HII payload")?;
    let mapping = crate::pci::mmio::map_mmio_region_exact(phys, len)
        .map_err(|error| alloc::format!("Limine HII payload map: {error:?}"))?;
    Ok(Some(unsafe {
        core::slice::from_raw_parts(mapping.as_ptr(), len)
    }))
}

fn parse_payload_sections(
    payload: &'static [u8],
    source: &'static str,
) -> Result<CapturedSections, String> {
    let header = read_struct::<PayloadHeader>(payload, 0)?;
    if header.magic != PAYLOAD_MAGIC || header.version != VERSION {
        return Err(String::from("unsupported TRPAY1 magic/version"));
    }
    if usize::from(header.header_bytes) < size_of::<PayloadHeader>()
        || usize::from(header.section_entry_bytes) < size_of::<SectionEntry>()
    {
        return Err(String::from("TRPAY1 header or entry size is too small"));
    }
    let count = header.section_count as usize;
    if count == 0 || count > MAX_SECTIONS || header.total_bytes as usize != payload.len() {
        return Err(alloc::format!(
            "TRPAY1 shape invalid sections={} total_bytes={}",
            count,
            header.total_bytes
        ));
    }
    let entry_bytes = header.section_entry_bytes as usize;
    let directory_end = usize::from(header.header_bytes)
        .checked_add(
            count
                .checked_mul(entry_bytes)
                .ok_or_else(|| String::from("section directory overflow"))?,
        )
        .ok_or_else(|| String::from("section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("TRPAY1 section directory is truncated"));
    }

    let mut ranges = Vec::<(usize, usize)>::with_capacity(count);
    let mut hii: Option<&'static [u8]> = None;
    let mut config_captured = false;
    for index in 0..count {
        let entry_offset = usize::from(header.header_bytes)
            .checked_add(
                index
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| String::from("section entry overflow"))?,
            )
            .ok_or_else(|| String::from("section entry overflow"))?;
        let entry = read_struct::<SectionEntry>(payload, entry_offset)?;
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| String::from("section range overflow"))?;
        if entry.length == 0 || start < directory_end || end > payload.len() {
            return Err(alloc::format!("TRPAY1 section {} range invalid", index));
        }
        if ranges
            .iter()
            .any(|&(left, right)| start < right && end > left)
        {
            return Err(alloc::format!("TRPAY1 section {} overlaps another", index));
        }
        ranges.push((start, end));
        let bytes = &payload[start..end];
        if crc32fast::hash(bytes) != entry.crc32 {
            return Err(alloc::format!("TRPAY1 section {} CRC mismatch", index));
        }
        match entry.kind {
            SEC_HII => {
                if hii.is_some() {
                    return Err(String::from("TRPAY1 contains duplicate HII sections"));
                }
                if bytes.len() > MAX_HII_BYTES {
                    return Err(alloc::format!(
                        "HII section bytes={} outside bound",
                        bytes.len()
                    ));
                }
                hii = Some(bytes);
            }
            SEC_CONFIG => {
                if bytes.len() >= 2
                    && bytes.len() % 2 == 0
                    && read_u16(bytes, bytes.len() - 2)? == 0
                {
                    config_captured = true;
                }
            }
            _ => {}
        }
    }
    let hii = hii.ok_or_else(|| String::from("TRPAY1 HII section is absent"))?;
    if hii.is_empty() {
        return Err(String::from("TRPAY1 HII section is empty"));
    }
    Ok(CapturedSections {
        source,
        hii,
        config_captured,
    })
}

