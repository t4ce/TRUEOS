use core::fmt::Write;

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use serde_json::{Value, json};

use crate::efi::{EfiGuid, EfiTableHeader};
use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};

const EFI_RUNTIME_SERVICES_SIGNATURE: u64 = 0x5652_4553_544E_5552;
const MAX_RUNTIME_HEADER_BYTES: usize = 4096;
const MAX_CATALOG_PAYLOAD_BYTES: u32 = 16 * 1024 * 1024;
const RUNTIME_SERVICE_NAMES: [&str; 14] = [
    "GetTime",
    "SetTime",
    "GetWakeupTime",
    "SetWakeupTime",
    "SetVirtualAddressMap",
    "ConvertPointer",
    "GetVariable",
    "GetNextVariableName",
    "SetVariable",
    "GetNextHighMonotonicCount",
    "ResetSystem",
    "UpdateCapsule",
    "QueryCapsuleCapabilities",
    "QueryVariableInfo",
];

const EFI_HII_DATABASE_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0xEF9F_C172,
    data2: 0xA1B2,
    data3: 0x4693,
    data4: [0xB3, 0x27, 0x6D, 0x32, 0xFC, 0x41, 0x60, 0x42],
};
const EFI_HII_CONFIG_ROUTING_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x587E_72D7,
    data2: 0xCC50,
    data3: 0x4F79,
    data4: [0x82, 0x09, 0xCA, 0x29, 0x1F, 0xC1, 0xA1, 0x0F],
};
const TRUEOS_BIOS_CATALOG_GUID: EfiGuid = EfiGuid {
    data1: 0x184D_A5DE,
    data2: 0xFA77,
    data3: 0x4A1F,
    data4: [0xB4, 0x27, 0xD4, 0xDB, 0xFC, 0xE6, 0xD7, 0xF7],
};
const BIOS_CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const BIOS_CATALOG_VERSION: u16 = 1;
const BIOS_CATALOG_FLAG_HII_PACKAGES: u32 = 1 << 0;
const BIOS_CATALOG_FLAG_FORMS: u32 = 1 << 1;
const BIOS_CATALOG_FLAG_STRINGS: u32 = 1 << 2;
const BIOS_CATALOG_FLAG_CONFIG: u32 = 1 << 3;
const BIOS_CATALOG_FLAG_PROTOCOLS: u32 = 1 << 4;

const SETUP_KEYWORDS: [&str; 13] = [
    "RAID",
    "RST",
    "VMD",
    "USB",
    "XHCI",
    "SATA",
    "OPROM",
    "GOP",
    "UNDI",
    "SECURE BOOT",
    "TPM",
    "RESIZABLE BAR",
    "ABOVE 4G",
];

#[repr(C)]
#[derive(Clone, Copy)]
struct BiosCatalogHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    flags: u32,
    package_list_count: u32,
    formset_count: u32,
    question_count: u32,
    payload_bytes: u32,
    payload_crc32: u32,
    reserved: u32,
    payload_phys: u64,
}

struct RuntimeServicesSnapshot {
    physical_address: u64,
    revision: u32,
    header_size: usize,
    stored_crc32: u32,
    computed_crc32: u32,
    crc_valid: bool,
    pointers: [Option<usize>; RUNTIME_SERVICE_NAMES.len()],
}

struct StandardHiiExports {
    database: Option<u64>,
    config_routing: Option<u64>,
}

struct CatalogSnapshot {
    table_phys: u64,
    payload_phys: u64,
    flags: u32,
    package_list_count: u32,
    formset_count: u32,
    question_count: u32,
    payload_bytes: u32,
    stored_crc32: u32,
    computed_crc32: u32,
}

enum CatalogProbe {
    Absent,
    Invalid(String),
    Valid(CatalogSnapshot),
}

struct SetupHint {
    type_id: u8,
    type_name: &'static str,
    handle: u16,
    string_index: usize,
    keyword: &'static str,
    text: String,
}

struct FirmwareIdentity {
    vendor: Option<String>,
    version: Option<String>,
    release_date: Option<String>,
    board_vendor: Option<String>,
    board_product: Option<String>,
    board_version: Option<String>,
}

pub(crate) fn platform_snapshot_json() -> Value {
    let (identity, hints, smbios_state) = match collect_firmware_identity_and_hints() {
        Ok((identity, hints)) => (identity, hints, "ready"),
        Err(_) => (
            FirmwareIdentity {
                vendor: None,
                version: None,
                release_date: None,
                board_vendor: None,
                board_product: None,
                board_version: None,
            },
            Vec::new(),
            "unavailable",
        ),
    };
    let (processor, memory, smbios_hardware_state) = match smbios_hardware_snapshot() {
        Ok((processor, memory)) => (processor, memory, "ready"),
        Err(_) => (Value::Null, Value::Null, "unavailable"),
    };

    json!({
        "firmware": {
            "vendor": identity.vendor,
            "version": identity.version,
            "date": identity.release_date
        },
        "board": {
            "vendor": identity.board_vendor,
            "product": identity.board_product,
            "version": identity.board_version
        },
        "processor": processor,
        "memory": memory,
        "controllers": platform_controller_snapshot(),
        "setupEvidence": hints.iter().map(setup_hint_json).collect::<Vec<_>>(),
        "sources": {
            "smbios": smbios_state,
            "smbiosHardware": smbios_hardware_state,
            "pci": if crate::pci::with_devices(|devices| devices.is_empty()) {
                "unavailable"
            } else {
                "ready"
            }
        }
    })
}

pub(crate) fn runtime_snapshot_json() -> Value {
    let Some(system_table) = crate::efi::system_table() else {
        return json!({ "state": "unavailable" });
    };
    let boot_services = system_table.boot_services != 0;
    let runtime_services = system_table.runtime_services != 0;
    json!({
        "state": "ready",
        "systemTableVendor": crate::efi::firmware_vendor_string(),
        "firmwareRevision": format!("0x{:08X}", system_table.firmware_revision),
        "uefiRevision": format_uefi_revision(system_table.hdr.revision),
        "configurationTables": system_table.number_of_table_entries,
        "bootServices": boot_services,
        "runtimeServices": runtime_services,
        "handoffPhase": if boot_services {
            "pre-ExitBootServices-or-unusual-handoff"
        } else {
            "post-ExitBootServices"
        }
    })
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let command = args.next();
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    let mut out = String::new();
    match command {
        None | Some("all") => {
            append_status(&mut out);
            writeln!(out).unwrap();
            append_setup_foundation(&mut out);
            writeln!(out).unwrap();
            append_hints(&mut out);
        }
        Some("status") | Some("services") => append_status(&mut out),
        Some("setup") => {
            append_setup_foundation(&mut out);
            writeln!(out).unwrap();
            append_hints(&mut out);
        }
        Some("handoff") => append_handoff(&mut out),
        Some("hints") => append_hints(&mut out),
        Some("help") | Some("-h") | Some("--help") => {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        _ => {
            print_usage(io);
            return ParseOutcome::Handled;
        }
    }

    emit_multiline(io, &out);
    ParseOutcome::Handled
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "bios: usage `bios [all|status|services|setup|handoff|hints]`");
}

fn emit_multiline(io: &'static dyn ShellBackend2, text: &str) {
    for line in text.lines() {
        print_shell_line(io, line.trim_end_matches('\r'));
    }
}

fn append_status(out: &mut String) {
    writeln!(out, "=== TRUEOS BIOS / UEFI Control-Plane Scout ===").unwrap();
    writeln!(
        out,
        "policy=read-only metadata inspection; no Runtime Service is called; no variable, capsule, reset, HII, device-policy, or flash write"
    )
    .unwrap();

    let Some(st) = crate::efi::system_table() else {
        writeln!(out, "system_table=unavailable").unwrap();
        return;
    };
    let validation = crate::efi::system_table_validation();
    writeln!(
        out,
        "firmware_vendor=\"{}\" firmware_revision=0x{:08X} uefi_revision=0x{:08X}",
        crate::efi::firmware_vendor_string()
            .as_deref()
            .unwrap_or("-"),
        st.firmware_revision,
        st.hdr.revision
    )
    .unwrap();
    if let Some(validation) = validation {
        writeln!(
            out,
            "system_table phys=0x{:016X} header_bytes=0x{:X} crc_valid={} stored=0x{:08X} computed=0x{:08X}",
            validation.physical_address,
            validation.header_size,
            yes_no(validation.crc_valid),
            validation.stored_crc32,
            validation.computed_crc32
        )
        .unwrap();
    } else {
        writeln!(out, "system_table_validation=unavailable").unwrap();
    }

    let boot_services_present = st.boot_services != 0;
    let runtime_services_present = st.runtime_services != 0;
    writeln!(
        out,
        "handoff_phase={} boot_services={} runtime_services={} configuration_tables={}",
        if boot_services_present {
            "pre-ExitBootServices-or-unusual-handoff"
        } else {
            "post-ExitBootServices"
        },
        present_absent(boot_services_present),
        present_absent(runtime_services_present),
        st.number_of_table_entries
    )
    .unwrap();
    writeln!(
        out,
        "setup_browser_state={} protocol_database_access={} note=\"firmware code may remain resident, but that does not keep the Boot Services HII browser callable\"",
        if boot_services_present {
            "possibly-preboot"
        } else {
            "not-a-runtime-interface"
        },
        if boot_services_present {
            "potential"
        } else {
            "ended-by-handoff"
        }
    )
    .unwrap();

    match runtime_services_snapshot() {
        Ok(snapshot) => {
            writeln!(
                out,
                "runtime_table phys=0x{:016X} revision=0x{:08X} header_bytes=0x{:X} crc_valid={} stored=0x{:08X} computed=0x{:08X}",
                snapshot.physical_address,
                snapshot.revision,
                snapshot.header_size,
                yes_no(snapshot.crc_valid),
                snapshot.stored_crc32,
                snapshot.computed_crc32
            )
            .unwrap();
            writeln!(
                out,
                "runtime_call_policy=pointer-presence-only; execution wrapper and runtime memory-map contract not yet established"
            )
            .unwrap();
            for (name, pointer) in RUNTIME_SERVICE_NAMES.iter().zip(snapshot.pointers.iter()) {
                match pointer {
                    Some(pointer) => writeln!(
                        out,
                        "  {:26} ptr=0x{:016X} present={}",
                        name,
                        pointer,
                        yes_no(*pointer != 0)
                    )
                    .unwrap(),
                    None => writeln!(
                        out,
                        "  {:26} ptr=- present=no reason=outside-runtime-table-header",
                        name
                    )
                    .unwrap(),
                }
            }
        }
        Err(error) => writeln!(out, "runtime_table=unavailable detail=\"{}\"", error).unwrap(),
    }
}

fn append_setup_foundation(out: &mut String) {
    writeln!(out, "=== Native Firmware-Settings Editor Foundation ===").unwrap();
    let boot_services_present = crate::efi::system_table()
        .map(|st| st.boot_services != 0)
        .unwrap_or(false);
    writeln!(
        out,
        "editor_state=foundation-only human_schema={} active_write_path=none",
        if boot_services_present {
            "preboot-HII-may-be-reachable"
        } else {
            "runtime-export-or-preboot-capture-required"
        }
    )
    .unwrap();
    writeln!(
        out,
        "hii_truth=HII Database/Config Routing/Form Browser interfaces are Boot Services protocols; firmware may additionally export static HII package/config buffers into the EFI System Configuration Table for OS-present use"
    )
    .unwrap();

    match standard_hii_exports() {
        Ok(exports) => {
            writeln!(
                out,
                "standard_hii_runtime_export database={} database_ptr={} config_routing={} config_ptr={}",
                present_absent(exports.database.is_some()),
                format_optional_address(exports.database),
                present_absent(exports.config_routing.is_some()),
                format_optional_address(exports.config_routing)
            )
            .unwrap();
            if exports.database.is_some() {
                writeln!(
                    out,
                    "standard_hii_editor_path=exported package-list buffer is available for a future bounded parser; this command does not dereference or parse it yet"
                )
                .unwrap();
            } else {
                writeln!(
                    out,
                    "standard_hii_editor_path=not-published-by-this-firmware; use a preboot collector before ExitBootServices"
                )
                .unwrap();
            }
        }
        Err(error) => {
            writeln!(out, "standard_hii_runtime_export=unavailable detail=\"{}\"", error).unwrap()
        }
    }

    writeln!(
        out,
        "local_editor_path=prefer a standard exported HII buffer when present; otherwise export HII package lists + strings + forms + config metadata into a reserved TRUEOS handoff catalog before ExitBootServices"
    )
    .unwrap();
    writeln!(
        out,
        "runtime_path=GetVariable/GetNextVariableName can later read variable-backed state only after TRUEOS implements the required firmware runtime execution environment; SetVariable remains locked"
    )
    .unwrap();
    writeln!(
        out,
        "control_model=question -> formset/question-id -> varstore GUID/name/offset or config-routing callback -> validated transaction -> reboot/effect verification"
    )
    .unwrap();
    append_handoff(out);
}

fn append_handoff(out: &mut String) {
    writeln!(out, "standard_hii_database_guid={}", EFI_HII_DATABASE_PROTOCOL_GUID.fmt_canonical())
        .unwrap();
    writeln!(
        out,
        "standard_hii_config_routing_guid={}",
        EFI_HII_CONFIG_ROUTING_PROTOCOL_GUID.fmt_canonical()
    )
    .unwrap();
    writeln!(out, "fallback_preboot_catalog_guid={}", TRUEOS_BIOS_CATALOG_GUID.fmt_canonical())
        .unwrap();
    writeln!(
        out,
        "fallback_preboot_catalog_contract magic=TRBIOS1 version={} flags=hii-packages/forms/strings/config/protocols payload=reserved-physical-memory crc32=required max_payload_bytes={}",
        BIOS_CATALOG_VERSION,
        MAX_CATALOG_PAYLOAD_BYTES
    )
    .unwrap();

    match probe_catalog() {
        CatalogProbe::Absent => {
            writeln!(
                out,
                "fallback_preboot_catalog=absent collector=not-installed next=\"FirmwareScout.efi or a bootloader hook must run before ExitBootServices when the standard runtime HII export is absent\""
            )
            .unwrap();
        }
        CatalogProbe::Invalid(error) => {
            writeln!(out, "fallback_preboot_catalog=invalid detail=\"{}\"", error).unwrap();
        }
        CatalogProbe::Valid(snapshot) => {
            writeln!(
                out,
                "fallback_preboot_catalog=valid table_phys=0x{:016X} payload_phys=0x{:016X} payload_bytes={} crc_valid={} stored=0x{:08X} computed=0x{:08X}",
                snapshot.table_phys,
                snapshot.payload_phys,
                snapshot.payload_bytes,
                yes_no(snapshot.stored_crc32 == snapshot.computed_crc32),
                snapshot.stored_crc32,
                snapshot.computed_crc32
            )
            .unwrap();
            writeln!(
                out,
                "  flags=0x{:08X} hii_packages={} forms={} strings={} config={} protocols={}",
                snapshot.flags,
                yes_no(snapshot.flags & BIOS_CATALOG_FLAG_HII_PACKAGES != 0),
                yes_no(snapshot.flags & BIOS_CATALOG_FLAG_FORMS != 0),
                yes_no(snapshot.flags & BIOS_CATALOG_FLAG_STRINGS != 0),
                yes_no(snapshot.flags & BIOS_CATALOG_FLAG_CONFIG != 0),
                yes_no(snapshot.flags & BIOS_CATALOG_FLAG_PROTOCOLS != 0)
            )
            .unwrap();
            writeln!(
                out,
                "  package_lists={} formsets={} questions={}",
                snapshot.package_list_count, snapshot.formset_count, snapshot.question_count
            )
            .unwrap();
        }
    }
}

fn append_hints(out: &mut String) {
    writeln!(out, "=== Firmware Setup Evidence ===").unwrap();
    writeln!(
        out,
        "evidence_policy=human-readable candidates only; a string or live controller is not yet a writable setup question"
    )
    .unwrap();

    match collect_firmware_identity_and_hints() {
        Ok((identity, hints)) => {
            writeln!(
                out,
                "firmware vendor=\"{}\" version=\"{}\" date=\"{}\"",
                text_or_dash(identity.vendor.as_deref()),
                text_or_dash(identity.version.as_deref()),
                text_or_dash(identity.release_date.as_deref())
            )
            .unwrap();
            writeln!(
                out,
                "board vendor=\"{}\" product=\"{}\" version=\"{}\"",
                text_or_dash(identity.board_vendor.as_deref()),
                text_or_dash(identity.board_product.as_deref()),
                text_or_dash(identity.board_version.as_deref())
            )
            .unwrap();
            if hints.is_empty() {
                writeln!(out, "setup_string_hints=none").unwrap();
            } else {
                writeln!(out, "setup_string_hints={}:", hints.len()).unwrap();
                for hint in hints {
                    writeln!(
                        out,
                        "  type={} ({}) handle=0x{:04X} string={} keyword={} text=\"{}\"",
                        hint.type_id,
                        hint.type_name,
                        hint.handle,
                        hint.string_index,
                        hint.keyword,
                        hint.text
                    )
                    .unwrap();
                }
            }
        }
        Err(error) => {
            writeln!(out, "smbios_setup_evidence=unavailable detail=\"{}\"", error).unwrap();
        }
    }

    append_live_controller_hints(out);
    writeln!(
        out,
        "next_schema_step=parse a standard or TRUEOS-captured HII package buffer, then map labels such as RAID/RST/VMD/USB to exact formsets, question IDs, storage backends, valid options, defaults, and reset requirements"
    )
    .unwrap();
}

fn runtime_services_snapshot() -> Result<RuntimeServicesSnapshot, String> {
    let st = crate::efi::system_table().ok_or_else(|| String::from("system table unavailable"))?;
    let raw = st.runtime_services as u64;
    if raw == 0 {
        return Err(String::from("runtime-services pointer is zero"));
    }
    let physical_address = crate::limine::try_as_phys_addr(raw)
        .ok_or_else(|| alloc::format!("runtime-services address is not mappable: 0x{raw:X}"))?;
    if !crate::limine::memmap_contains_phys_range(
        physical_address,
        core::mem::size_of::<EfiTableHeader>(),
    ) {
        return Err(String::from("runtime-services header is outside one memory-map range"));
    }

    let header_mapping = crate::pci::mmio::map_limine_struct::<EfiTableHeader>(physical_address)
        .map_err(|error| alloc::format!("runtime header map failed: {error:?}"))?;
    let header = unsafe { *header_mapping.as_ref() };
    if header.signature != EFI_RUNTIME_SERVICES_SIGNATURE {
        return Err(alloc::format!("runtime signature mismatch: 0x{:016X}", header.signature));
    }
    let header_size = header.header_size as usize;
    if header_size < core::mem::size_of::<EfiTableHeader>()
        || header_size > MAX_RUNTIME_HEADER_BYTES
    {
        return Err(alloc::format!("runtime header size is invalid: 0x{header_size:X}"));
    }
    if !crate::limine::memmap_contains_phys_range(physical_address, header_size) {
        return Err(String::from("complete runtime-services table crosses memory-map range"));
    }

    let mapped = crate::pci::mmio::map_mmio_region_exact(physical_address, header_size)
        .map_err(|error| alloc::format!("runtime table map failed: {error:?}"))?;
    let bytes = unsafe { core::slice::from_raw_parts(mapped.as_ptr(), header_size) };
    let mut crc_bytes = Vec::from(bytes);
    let crc_field = crc_bytes
        .get_mut(16..20)
        .ok_or_else(|| String::from("runtime CRC field is outside table"))?;
    crc_field.fill(0);
    let computed_crc32 = crc32fast::hash(&crc_bytes);

    let mut pointers = [None; RUNTIME_SERVICE_NAMES.len()];
    let pointer_base = core::mem::size_of::<EfiTableHeader>();
    let pointer_bytes = core::mem::size_of::<usize>();
    for (index, slot) in pointers.iter_mut().enumerate() {
        let offset = pointer_base
            .checked_add(index.saturating_mul(pointer_bytes))
            .ok_or_else(|| String::from("runtime pointer offset overflow"))?;
        let end = offset
            .checked_add(pointer_bytes)
            .ok_or_else(|| String::from("runtime pointer end overflow"))?;
        if end > header_size {
            continue;
        }
        let value =
            unsafe { core::ptr::read_unaligned(mapped.as_ptr().add(offset) as *const usize) };
        *slot = Some(value);
    }

    Ok(RuntimeServicesSnapshot {
        physical_address,
        revision: header.revision,
        header_size,
        stored_crc32: header.crc32,
        computed_crc32,
        crc_valid: computed_crc32 == header.crc32,
        pointers,
    })
}

fn standard_hii_exports() -> Result<StandardHiiExports, String> {
    let tables = crate::efi::configuration_tables()
        .map_err(|error| alloc::format!("config tables: {error:?}"))?;
    let mut database = None;
    let mut config_routing = None;

    for entry in tables {
        if guid_eq(&entry.vendor_guid, &EFI_HII_DATABASE_PROTOCOL_GUID) {
            database = (entry.vendor_table != 0).then_some(entry.vendor_table as u64);
        } else if guid_eq(&entry.vendor_guid, &EFI_HII_CONFIG_ROUTING_PROTOCOL_GUID) {
            config_routing = (entry.vendor_table != 0).then_some(entry.vendor_table as u64);
        }
    }

    Ok(StandardHiiExports {
        database,
        config_routing,
    })
}

fn probe_catalog() -> CatalogProbe {
    let tables = match crate::efi::configuration_tables() {
        Ok(tables) => tables,
        Err(error) => return CatalogProbe::Invalid(alloc::format!("config tables: {error:?}")),
    };
    let Some(entry) = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
    else {
        return CatalogProbe::Absent;
    };
    if entry.vendor_table == 0 {
        return CatalogProbe::Invalid(String::from("catalog table pointer is zero"));
    }
    let Some(table_phys) = crate::limine::try_as_phys_addr(entry.vendor_table as u64) else {
        return CatalogProbe::Invalid(alloc::format!(
            "catalog table address is not mappable: 0x{:X}",
            entry.vendor_table
        ));
    };
    if !crate::limine::memmap_contains_phys_range(
        table_phys,
        core::mem::size_of::<BiosCatalogHeader>(),
    ) {
        return CatalogProbe::Invalid(String::from("catalog header is outside memory map"));
    }
    let mapped = match crate::pci::mmio::map_limine_struct::<BiosCatalogHeader>(table_phys) {
        Ok(mapped) => mapped,
        Err(error) => {
            return CatalogProbe::Invalid(alloc::format!("catalog header map failed: {error:?}"));
        }
    };
    let header = unsafe { *mapped.as_ref() };
    if header.magic != BIOS_CATALOG_MAGIC {
        return CatalogProbe::Invalid(String::from("catalog magic mismatch"));
    }
    if header.version != BIOS_CATALOG_VERSION {
        return CatalogProbe::Invalid(alloc::format!(
            "catalog version {} is unsupported",
            header.version
        ));
    }
    if usize::from(header.header_bytes) < core::mem::size_of::<BiosCatalogHeader>() {
        return CatalogProbe::Invalid(alloc::format!(
            "catalog header_bytes={} is too small",
            header.header_bytes
        ));
    }
    if header.payload_bytes == 0 {
        return CatalogProbe::Invalid(String::from("catalog payload is empty"));
    }
    if header.payload_bytes > MAX_CATALOG_PAYLOAD_BYTES {
        return CatalogProbe::Invalid(alloc::format!(
            "catalog payload_bytes={} exceeds limit {}",
            header.payload_bytes,
            MAX_CATALOG_PAYLOAD_BYTES
        ));
    }
    let Some(payload_phys) = crate::limine::try_as_phys_addr(header.payload_phys) else {
        return CatalogProbe::Invalid(alloc::format!(
            "catalog payload address is not mappable: 0x{:X}",
            header.payload_phys
        ));
    };
    let payload_bytes = header.payload_bytes as usize;
    if !crate::limine::memmap_contains_phys_range(payload_phys, payload_bytes) {
        return CatalogProbe::Invalid(String::from("catalog payload crosses memory-map range"));
    }
    let payload = match crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_bytes) {
        Ok(payload) => payload,
        Err(error) => {
            return CatalogProbe::Invalid(alloc::format!("catalog payload map failed: {error:?}"));
        }
    };
    let bytes = unsafe { core::slice::from_raw_parts(payload.as_ptr(), payload_bytes) };
    let computed_crc32 = crc32fast::hash(bytes);
    if computed_crc32 != header.payload_crc32 {
        return CatalogProbe::Invalid(alloc::format!(
            "catalog payload CRC mismatch stored=0x{:08X} computed=0x{:08X}",
            header.payload_crc32,
            computed_crc32
        ));
    }

    CatalogProbe::Valid(CatalogSnapshot {
        table_phys,
        payload_phys,
        flags: header.flags,
        package_list_count: header.package_list_count,
        formset_count: header.formset_count,
        question_count: header.question_count,
        payload_bytes: header.payload_bytes,
        stored_crc32: header.payload_crc32,
        computed_crc32,
    })
}

fn collect_firmware_identity_and_hints() -> Result<(FirmwareIdentity, Vec<SetupHint>), String> {
    let table = crate::efi::smbios::discover()
        .map_err(|error| alloc::format!("reason={} detail={error:?}", error.label()))?;
    let mut identity = FirmwareIdentity {
        vendor: None,
        version: None,
        release_date: None,
        board_vendor: None,
        board_product: None,
        board_version: None,
    };
    let mut hints = Vec::new();
    let mut seen = BTreeSet::new();
    let mut structures = table.structures();

    loop {
        let structure = match structures.next_structure() {
            Ok(Some(structure)) => structure,
            Ok(None) => break,
            Err(error) => return Err(alloc::format!("SMBIOS parse stopped: {error:?}")),
        };

        match structure.type_id {
            0 => {
                identity.vendor = structure_text(&structure, 0x04);
                identity.version = structure_text(&structure, 0x05);
                identity.release_date = structure_text(&structure, 0x08);
            }
            2 => {
                identity.board_vendor = structure_text(&structure, 0x04);
                identity.board_product = structure_text(&structure, 0x05);
                identity.board_version = structure_text(&structure, 0x06);
            }
            _ => {}
        }

        for (string_index, bytes) in structure.strings().enumerate() {
            let text = firmware_text(bytes);
            let upper = text.to_ascii_uppercase();
            let Some(keyword) = SETUP_KEYWORDS
                .iter()
                .copied()
                .find(|keyword| upper.contains(*keyword))
            else {
                continue;
            };
            if !seen.insert(text.clone()) {
                continue;
            }
            hints.push(SetupHint {
                type_id: structure.type_id,
                type_name: structure.type_name(),
                handle: structure.handle,
                string_index: string_index + 1,
                keyword,
                text,
            });
            if hints.len() == 32 {
                break;
            }
        }
        if hints.len() == 32 {
            break;
        }
    }

    Ok((identity, hints))
}

fn append_live_controller_hints(out: &mut String) {
    if crate::pci::with_devices(|devices| devices.is_empty()) {
        writeln!(out, "live_policy_controllers=unavailable reason=pci-registry-empty").unwrap();
        return;
    }

    writeln!(out, "live_policy_controllers:").unwrap();
    let mut count = 0usize;
    crate::pci::with_devices(|devices| {
        for dev in devices
            .iter()
            .filter(|dev| dev.class == 0x01 || (dev.class == 0x0C && dev.subclass == 0x03))
        {
            let role = if dev.class == 0x01 {
                match dev.subclass {
                    0x06 => "storage-sata",
                    0x08 => "storage-nvme",
                    0x04 => "storage-raid",
                    _ => "storage-other",
                }
            } else {
                match dev.prog_if {
                    0x30 => "usb-xhci",
                    0x20 => "usb-ehci",
                    0x10 => "usb-ohci",
                    _ => "usb-controller",
                }
            };
            writeln!(
                out,
                "  {:02X}:{:02X}.{} {:04X}:{:04X} class={:02X}/{:02X}/{:02X} role={}",
                dev.bus,
                dev.slot,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class,
                dev.subclass,
                dev.prog_if,
                role
            )
            .unwrap();
            count = count.saturating_add(1);
        }
    });
    if count == 0 {
        writeln!(out, "  none").unwrap();
    }
}

fn platform_controller_snapshot() -> Vec<Value> {
    crate::pci::with_devices(|devices| {
        devices
            .iter()
            .filter_map(|device| {
                let role = match (device.class, device.subclass, device.prog_if) {
                    (0x01, 0x06, _) => "SATA",
                    (0x01, 0x08, _) => "NVMe",
                    (0x01, 0x04, _) => "RAID",
                    (0x0C, 0x03, 0x30) => "xHCI",
                    (0x02, _, _) => "Network",
                    _ => return None,
                };
                Some(json!({
                    "role": role,
                    "address": format!("{:02X}:{:02X}.{}", device.bus, device.slot, device.function),
                    "vendorId": format!("{:04X}", device.vendor_id),
                    "deviceId": format!("{:04X}", device.device_id),
                    "name": pci_controller_name(device.vendor_id, device.device_id)
                }))
            })
            .collect()
    })
}

fn setup_hint_json(hint: &SetupHint) -> Value {
    json!({
        "keyword": hint.keyword,
        "text": hint.text,
        "source": "smbios-string-evidence",
        "setting": false,
        "smbiosType": hint.type_id,
        "smbiosTypeName": hint.type_name,
        "handle": format!("0x{:04X}", hint.handle),
        "stringIndex": hint.string_index
    })
}

fn smbios_hardware_snapshot() -> Result<(Value, Value), String> {
    let table = crate::efi::smbios::discover()
        .map_err(|error| alloc::format!("reason={} detail={error:?}", error.label()))?;
    let mut structures = table.structures();
    let mut processors = Vec::<Value>::new();
    let mut modules = Vec::<Value>::new();
    let mut slots = 0usize;
    let mut installed_devices = 0usize;
    let mut installed_bytes = 0u64;
    let mut capacity_unknown = false;
    let mut speed_min = u32::MAX;
    let mut speed_max = 0u32;

    loop {
        let structure = match structures.next_structure() {
            Ok(Some(structure)) => structure,
            Ok(None) => break,
            Err(error) => return Err(alloc::format!("SMBIOS parse stopped: {error:?}")),
        };

        if structure.type_id == 4 {
            let status = structure.byte(0x18).unwrap_or(0);
            processors.push(json!({
                "socket": structure_text(&structure, 0x04),
                "manufacturer": structure_text(&structure, 0x07),
                "model": structure_text(&structure, 0x10),
                "maxSpeedMHz": structure.u16(0x14).filter(|value| *value != 0),
                "currentSpeedMHz": structure.u16(0x16).filter(|value| *value != 0),
                "socketPopulated": status & 0x40 != 0,
                "statusCode": status & 0x07,
                "cores": processor_count_field(structure, 0x23, 0x2A),
                "enabledCores": processor_count_field(structure, 0x24, 0x2C),
                "threads": processor_count_field(structure, 0x25, 0x2E)
            }));
            continue;
        }

        let Some(device) = structure.memory_device() else {
            continue;
        };
        slots = slots.saturating_add(1);
        let (size_state, size_bytes) = match device.size {
            crate::efi::smbios::MemoryDeviceSize::NotInstalled => ("empty", None),
            crate::efi::smbios::MemoryDeviceSize::Unknown => {
                installed_devices = installed_devices.saturating_add(1);
                capacity_unknown = true;
                ("unknown", None)
            }
            crate::efi::smbios::MemoryDeviceSize::Bytes(bytes) => {
                installed_devices = installed_devices.saturating_add(1);
                installed_bytes = installed_bytes.saturating_add(bytes);
                ("installed", Some(bytes))
            }
        };
        let speed = device.configured_speed_mt_s.or(device.speed_mt_s);
        if size_state != "empty" {
            if let Some(speed) = speed {
                speed_min = speed_min.min(speed);
                speed_max = speed_max.max(speed);
            }
        }
        modules.push(json!({
            "handle": format!("0x{:04X}", device.handle),
            "locator": device.locator.map(firmware_text),
            "bankLocator": device.bank_locator.map(firmware_text),
            "sizeState": size_state,
            "sizeBytes": size_bytes,
            "speedMtS": device.speed_mt_s,
            "configuredSpeedMtS": device.configured_speed_mt_s,
            "manufacturer": structure_text(&structure, 0x17),
            "partNumber": structure_text(&structure, 0x1A)
        }));
    }

    let processor = processors
        .iter()
        .find(|processor| {
            processor
                .get("socketPopulated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| processors.first().cloned())
        .unwrap_or(Value::Null);
    let memory = if slots == 0 {
        Value::Null
    } else {
        json!({
            "installedBytes": installed_bytes,
            "capacityComplete": !capacity_unknown,
            "installedDevices": installed_devices,
            "slots": slots,
            "speedMinMtS": (speed_min != u32::MAX).then_some(speed_min),
            "speedMaxMtS": (speed_max != 0).then_some(speed_max),
            "devices": modules
        })
    };

    Ok((processor, memory))
}

fn processor_count_field(
    structure: crate::efi::smbios::Structure<'_>,
    legacy_offset: usize,
    extended_offset: usize,
) -> Option<u16> {
    match structure.byte(legacy_offset)? {
        0 => None,
        0xFF => structure
            .u16(extended_offset)
            .filter(|value| *value != 0 && *value != 0xFFFF),
        value => Some(u16::from(value)),
    }
}

fn pci_controller_name(vendor_id: u16, device_id: u16) -> String {
    if vendor_id == 0x8086 {
        format!("Intel {device_id:04X}")
    } else {
        format!("PCI {vendor_id:04X}:{device_id:04X}")
    }
}

fn format_uefi_revision(revision: u32) -> String {
    format!("{}.{:02}", revision >> 16, revision & 0xFFFF)
}

fn structure_text(structure: &crate::efi::smbios::Structure<'_>, offset: usize) -> Option<String> {
    structure
        .string_bytes(structure.byte(offset)?)
        .map(firmware_text)
}

fn firmware_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            value if value.is_ascii_graphic() || value == b' ' => out.push(value as char),
            value => {
                write!(out, "\\x{:02X}", value).unwrap();
            }
        }
    }
    out
}

fn guid_eq(left: &EfiGuid, right: &EfiGuid) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

fn format_optional_address(value: Option<u64>) -> String {
    value
        .map(|address| alloc::format!("0x{address:016X}"))
        .unwrap_or_else(|| String::from("-"))
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

const fn present_absent(value: bool) -> &'static str {
    if value { "present" } else { "absent" }
}
