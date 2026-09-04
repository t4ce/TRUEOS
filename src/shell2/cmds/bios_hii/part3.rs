fn parse_hii_export(
    bytes: &[u8],
    capture: CaptureMetadata,
) -> Result<HiiCatalogue, String> {
    const LIST_HEADER_BYTES: usize = 20;
    const PACKAGE_HEADER_BYTES: usize = 4;

    if bytes.len() < LIST_HEADER_BYTES {
        return Err(String::from("captured HII export is shorter than one package list"));
    }

    let mut catalogue = HiiCatalogue {
        capture,
        lists: Vec::new(),
        packages: Vec::new(),
        string_packages: Vec::new(),
        form_packages: Vec::new(),
        device_path_packages: Vec::new(),
        stats: CatalogueStats::default(),
    };
    let mut list_offset = 0usize;
    while list_offset < bytes.len() {
        if catalogue.lists.len() >= MAX_PACKAGE_LISTS {
            return Err(String::from("HII package-list count exceeds bound"));
        }
        let header_end = checked_end(list_offset, LIST_HEADER_BYTES, bytes.len(), "list header")?;
        let guid_bytes: [u8; 16] = bytes[list_offset..list_offset + 16]
            .try_into()
            .map_err(|_| String::from("package-list GUID is truncated"))?;
        let list_len = read_u32(bytes, list_offset + 16)? as usize;
        if list_len < LIST_HEADER_BYTES {
            return Err(alloc::format!(
                "package list {} length={} is too small",
                catalogue.lists.len(),
                list_len
            ));
        }
        let list_end = checked_end(list_offset, list_len, bytes.len(), "package list")?;
        let list_index = catalogue.lists.len();
        let first_package = catalogue.packages.len();
        let mut package_offset = header_end;
        let mut package_index = 0usize;
        while package_offset < list_end {
            if catalogue.packages.len() >= MAX_PACKAGES {
                return Err(String::from("HII package count exceeds bound"));
            }
            checked_end(
                package_offset,
                PACKAGE_HEADER_BYTES,
                list_end,
                "package header",
            )?;
            let raw = read_u32(bytes, package_offset)?;
            let package_len = (raw & 0x00ff_ffff) as usize;
            let package_type = (raw >> 24) as u8;
            if package_len < PACKAGE_HEADER_BYTES {
                return Err(alloc::format!(
                    "list {} package {} length={} is too small",
                    list_index,
                    package_index,
                    package_len
                ));
            }
            let package_end = checked_end(package_offset, package_len, list_end, "HII package")?;
            catalogue.packages.push(PackageRecord {
                list_index,
                package_index,
                package_type,
                offset: u32::try_from(package_offset)
                    .map_err(|_| String::from("package offset exceeds u32"))?,
                length: u32::try_from(package_len)
                    .map_err(|_| String::from("package length exceeds u32"))?,
            });
            let package_bytes = &bytes[package_offset..package_end];
            match package_type {
                HII_STRINGS => {
                    if catalogue.string_packages.len() >= MAX_STRING_PACKAGES {
                        return Err(String::from("HII string-package count exceeds bound"));
                    }
                    match parse_string_package(package_bytes, list_index, package_index) {
                        Ok(package) => {
                            catalogue.stats.decoded_strings = catalogue
                                .stats
                                .decoded_strings
                                .saturating_add(package.strings.len() as u32);
                            catalogue.stats.duplicate_strings = catalogue
                                .stats
                                .duplicate_strings
                                .saturating_add(package.duplicate_blocks);
                            catalogue.stats.unresolved_duplicates = catalogue
                                .stats
                                .unresolved_duplicates
                                .saturating_add(package.unresolved_duplicates);
                            catalogue.stats.skipped_string_ids = catalogue
                                .stats
                                .skipped_string_ids
                                .saturating_add(package.skipped_ids);
                            catalogue.stats.extension_blocks = catalogue
                                .stats
                                .extension_blocks
                                .saturating_add(package.extension_blocks);
                            catalogue.stats.truncated_strings = catalogue
                                .stats
                                .truncated_strings
                                .saturating_add(package.truncated_strings);
                            catalogue.string_packages.push(package);
                        }
                        Err(_) => {
                            catalogue.stats.malformed_packages =
                                catalogue.stats.malformed_packages.saturating_add(1);
                        }
                    }
                }
                HII_FORMS => {
                    if catalogue.form_packages.len() >= MAX_FORM_PACKAGES {
                        return Err(String::from("HII form-package count exceeds bound"));
                    }
                    catalogue.form_packages.push(FormPackageRecord {
                        list_index,
                        package_index,
                        bytes: package_bytes.to_vec(),
                    });
                }
                HII_DEVICE_PATH => {
                    if catalogue.device_path_packages.len() >= MAX_DEVICE_PATH_PACKAGES {
                        return Err(String::from("HII device-path package count exceeds bound"));
                    }
                    catalogue.device_path_packages.push(DevicePathPackageRecord {
                        list_index,
                        package_index,
                        bytes: package_bytes.to_vec(),
                    });
                }
                _ => {}
            }
            package_offset = package_end;
            package_index = package_index.saturating_add(1);
        }
        if package_offset != list_end {
            return Err(alloc::format!(
                "package list {} ended off a package boundary",
                list_index
            ));
        }
        catalogue.lists.push(PackageListRecord {
            guid: EfiGuid::from_uefi_bytes(guid_bytes),
            first_package,
            package_count: catalogue.packages.len() - first_package,
        });
        list_offset = list_end;
    }
    if list_offset != bytes.len() || catalogue.lists.is_empty() {
        return Err(String::from("HII export ended off a package-list boundary"));
    }
    Ok(catalogue)
}

