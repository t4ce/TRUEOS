use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::string::String;

use crate::efi::smbios::{self, Structure};
use crate::pci::PciDevice;

const NETWORK_CLASS: u8 = 0x02;
const INTEL_VENDOR_ID: u16 = 0x8086;
const AMT_INFO_SIGNATURE: &[u8; 4] = b"$AMT";

static IMPORTANT_ASSET_LOGGED: AtomicBool = AtomicBool::new(false);

struct AssetEvidence {
    system_uuid: Option<[u8; 16]>,
    manufacturer: Option<String>,
    product: Option<String>,
    version: Option<String>,
    serial: Option<String>,
    inventory_crc32: u32,
    inventory_bytes: usize,
    csme_version: Option<String>,
    csme_sku: Option<String>,
    amt_info_table: bool,
}

struct NetworkEvidence {
    visible: String,
    total: usize,
    intel_functions: usize,
}

pub(crate) fn emit_verified_asset_receipt(dev: PciDevice, bar0: u64) -> bool {
    if IMPORTANT_ASSET_LOGGED.swap(true, Ordering::AcqRel) {
        return false;
    }

    let network = collect_network_evidence();
    match collect_asset_evidence() {
        Ok(asset) => {
            let uuid = asset
                .system_uuid
                .as_ref()
                .map(format_uuid)
                .unwrap_or_else(|| String::from("-"));
            let serial = text_or_dash(asset.serial.as_deref());

            crate::log_important!(
                target: "mei";
                "mei asset tracking: transport=verified bdf={:02X}:{:02X}.{} bar=0x{:016X} system_uuid={} uuid_source=smbios-type1 uuid_quality={} persistence_scope=firmware-not-os os_reinstall_persistence=firmware-dependent csme_upid=not-enumerated system=\"{}|{}|{}\" serial=\"{}\" serial_placeholder={} inventory_crc32=0x{:08X} inventory_bytes={} csme=\"{}\" sku=\"{}\"\n",
                dev.bus,
                dev.slot,
                dev.function,
                bar0,
                uuid,
                asset_uuid_quality(asset.system_uuid.as_ref()),
                text_or_dash(asset.manufacturer.as_deref()),
                text_or_dash(asset.product.as_deref()),
                text_or_dash(asset.version.as_deref()),
                serial,
                yes_no(is_placeholder_text(serial)),
                asset.inventory_crc32,
                asset.inventory_bytes,
                text_or_dash(asset.csme_version.as_deref()),
                text_or_dash(asset.csme_sku.as_deref())
            );

            emit_manageability_posture(&network, asset.amt_info_table, asset.csme_sku.as_deref());
        }
        Err(error) => {
            crate::log_important!(
                target: "mei";
                "mei asset tracking: transport=verified bdf={:02X}:{:02X}.{} bar=0x{:016X} firmware_inventory=unavailable detail=\"{}\" csme_upid=not-enumerated persistence_claim=not-made\n",
                dev.bus,
                dev.slot,
                dev.function,
                bar0,
                error
            );
            emit_manageability_posture(&network, false, None);
        }
    }

    true
}

fn emit_manageability_posture(
    network: &NetworkEvidence,
    amt_info_table: bool,
    csme_sku: Option<&str>,
) {
    let oob_network = if network.intel_functions == 0 {
        "not-evidenced"
    } else {
        "intel-network-function-present-not-verified"
    };
    let remote_state = if csme_sku
        .map(|sku| sku.to_ascii_uppercase().contains("CONSUMER"))
        .unwrap_or(false)
        && network.intel_functions == 0
    {
        "not-evidenced-on-current-platform"
    } else {
        "unverified"
    };

    crate::log_important!(
        target: "mei";
        "mei remote manageability posture: amt_info_table={} csme_sku=\"{}\" amt_remote_state={} hbm=not-initialized clients=not-enumerated oob_network={} intel_network_functions={} visible_network_functions={} visible_network=\"{}\" nic_claim=none\n",
        yes_no(amt_info_table),
        text_or_dash(csme_sku),
        remote_state,
        oob_network,
        network.intel_functions,
        network.total,
        network.visible
    );
}

fn collect_asset_evidence() -> Result<AssetEvidence, String> {
    let table = smbios::discover()
        .map_err(|error| alloc::format!("reason={} detail={:?}", error.label(), error))?;
    let mut system_uuid = None;
    let mut manufacturer = None;
    let mut product = None;
    let mut version = None;
    let mut serial = None;
    let mut csme_version = None;
    let mut csme_sku = None;
    let mut amt_info_table = false;
    let mut structures = table.structures();

    loop {
        let structure = match structures.next_structure() {
            Ok(Some(structure)) => structure,
            Ok(None) => break,
            Err(error) => return Err(alloc::format!("SMBIOS parse stopped: {:?}", error)),
        };

        match structure.type_id {
            1 => {
                manufacturer = text_field(structure, 0x04);
                product = text_field(structure, 0x05);
                version = text_field(structure, 0x06);
                serial = text_field(structure, 0x07);
                if let Some(bytes) = structure.bytes(0x08, 16) {
                    let mut uuid = [0u8; 16];
                    uuid.copy_from_slice(bytes);
                    system_uuid = Some(uuid);
                }
            }
            130 => {
                amt_info_table |= structure
                    .bytes(0x04, AMT_INFO_SIGNATURE.len())
                    .map(|bytes| bytes == AMT_INFO_SIGNATURE)
                    .unwrap_or(false);
            }
            221 if csme_version.is_none() => {
                if let Some((found_version, found_sku)) = parse_type221_csme_version(structure) {
                    csme_version = Some(found_version);
                    csme_sku = found_sku;
                }
            }
            127 => break,
            _ => {}
        }
    }

    Ok(AssetEvidence {
        system_uuid,
        manufacturer,
        product,
        version,
        serial,
        inventory_crc32: crc32fast::hash(table.bytes()),
        inventory_bytes: table.bytes().len(),
        csme_version,
        csme_sku,
        amt_info_table,
    })
}

fn parse_type221_csme_version(structure: Structure<'_>) -> Option<(String, Option<String>)> {
    let count = usize::from(structure.byte(0x04)?);
    for index in 0..count {
        let base = 0x05usize.checked_add(index.checked_mul(7)?)?;
        let (Some(label_index), Some(value_index)) =
            (structure.byte(base), structure.byte(base + 1))
        else {
            continue;
        };
        let Some(label) = structure.string_bytes(label_index).map(firmware_text) else {
            continue;
        };
        if !label.eq_ignore_ascii_case("ME Firmware Version") {
            continue;
        }

        let (Some(major), Some(minor), Some(hotfix), Some(build)) = (
            structure.byte(base + 2),
            structure.byte(base + 3),
            structure.byte(base + 4),
            structure.u16(base + 5),
        ) else {
            continue;
        };
        return Some((
            alloc::format!("{}.{}.{}.{}", major, minor, hotfix, build),
            structure.string_bytes(value_index).map(firmware_text),
        ));
    }
    None
}

fn collect_network_evidence() -> NetworkEvidence {
    if crate::pci::with_devices(|devices| devices.is_empty()) {
        crate::pci::enumerate_impl();
    }

    crate::pci::with_devices(|devices| {
        let mut visible = String::new();
        let mut total = 0usize;
        let mut intel_functions = 0usize;

        for dev in devices.iter().filter(|dev| dev.class == NETWORK_CLASS) {
            if total != 0 {
                visible.push(',');
            }
            write!(
                visible,
                "{:02X}:{:02X}.{}={:04X}:{:04X}",
                dev.bus, dev.slot, dev.function, dev.vendor_id, dev.device_id
            )
            .unwrap();
            total = total.saturating_add(1);
            if dev.vendor_id == INTEL_VENDOR_ID {
                intel_functions = intel_functions.saturating_add(1);
            }
        }
        if visible.is_empty() {
            visible.push('-');
        }

        NetworkEvidence {
            visible,
            total,
            intel_functions,
        }
    })
}

fn text_field(structure: Structure<'_>, offset: usize) -> Option<String> {
    structure
        .string_bytes(structure.byte(offset)?)
        .map(firmware_text)
}

fn firmware_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        if byte.is_ascii_graphic() || byte == b' ' {
            out.push(byte as char);
        } else {
            out.push('?');
        }
    }
    out
}

fn asset_uuid_quality(uuid: Option<&[u8; 16]>) -> &'static str {
    let Some(uuid) = uuid else {
        return "missing";
    };
    if uuid.iter().all(|byte| *byte == 0) {
        return "invalid-zero";
    }
    if uuid.iter().all(|byte| *byte == 0xFF) {
        return "invalid-ff";
    }
    if patterned_uuid(uuid) {
        return "placeholder-suspected";
    }
    "firmware-unverified"
}

fn patterned_uuid(uuid: &[u8; 16]) -> bool {
    let zero_high_bytes = (0..16).step_by(2).all(|index| uuid[index] == 0);
    let ascending_low_bytes = (1..15)
        .step_by(2)
        .all(|index| uuid[index + 2] == uuid[index].wrapping_add(1));
    zero_high_bytes && ascending_low_bytes
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    alloc::format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[3],
        bytes[2],
        bytes[1],
        bytes[0],
        bytes[5],
        bytes[4],
        bytes[7],
        bytes[6],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn is_placeholder_text(value: &str) -> bool {
    let upper = value.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "-" | "DEFAULT STRING" | "TO BE FILLED BY O.E.M." | "UNKNOWN" | "NONE" | "N/A"
    )
}

fn text_or_dash(value: Option<&str>) -> &str {
    match value {
        Some(value) if !value.is_empty() => value,
        _ => "-",
    }
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
