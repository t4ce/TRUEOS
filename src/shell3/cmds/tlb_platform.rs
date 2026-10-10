use core::fmt::Write;

use alloc::string::String;
use alloc::vec::Vec;

use crate::efi::smbios::{self, Structure};
use crate::pci::PciDevice;

const NCT5585_TOKEN: &str = "NCT5585";
const MEI_FIRMWARE_TOKENS: [&str; 5] = ["$MEI", "MEI1", "MEI2", "MEI3", "MEI4"];

pub(crate) const INTEL_VENDOR_ID: u16 = 0x8086;
pub(crate) const RPL_S_MEI_DEVICE_ID: u16 = 0x7A68;
const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;

const PCI_CFG_HFS_1: u16 = 0x40;
const PCI_CFG_HFS_2: u16 = 0x48;
const PCI_CFG_HFS_3: u16 = 0x60;
const PCI_CFG_HFS_4: u16 = 0x64;
const PCI_CFG_HFS_5: u16 = 0x68;
const PCI_CFG_HFS_6: u16 = 0x6C;
const HFS_REGISTERS: [(&str, u16); 6] = [
    ("HFS1", PCI_CFG_HFS_1),
    ("HFS2", PCI_CFG_HFS_2),
    ("HFS3", PCI_CFG_HFS_3),
    ("HFS4", PCI_CFG_HFS_4),
    ("HFS5", PCI_CFG_HFS_5),
    ("HFS6", PCI_CFG_HFS_6),
];

pub(crate) const MEI_H_CSR: usize = 0x04;
pub(crate) const MEI_ME_CSR_HA: usize = 0x0C;
pub(crate) const MEI_STATUS_MAP_BYTES: usize = 0x10;

#[derive(Clone, Copy)]
struct FirmwareStringHint {
    type_id: u8,
    type_name: &'static str,
    handle: u16,
    string_index: usize,
}

struct TextHint {
    identity: FirmwareStringHint,
    text: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NctManagementHint {
    pub(crate) handle: u16,
    pub(crate) device_type: Option<u8>,
    pub(crate) address: Option<u32>,
    pub(crate) address_type: Option<u8>,
}

struct NamedHandle {
    handle: u16,
    name: String,
}

struct ManagementComponent {
    handle: u16,
    description: Option<String>,
    management_handle: u16,
    component_handle: u16,
    threshold_handle: u16,
}

struct CsmeFirmwareVersion {
    major: u8,
    minor: u8,
    hotfix: u8,
    build: u16,
    sku: Option<String>,
}

#[derive(Default)]
struct PlatformFirmwareHints {
    nct_strings: Vec<TextHint>,
    mei_strings: Vec<TextHint>,
    nct_management: Option<NctManagementHint>,
    cooling_devices: Vec<NamedHandle>,
    temperature_probes: Vec<NamedHandle>,
    management_components: Vec<ManagementComponent>,
    cached_hfs: Option<[u32; 6]>,
    csme_version: Option<CsmeFirmwareVersion>,
    parse_error: Option<String>,
}

pub(crate) fn append_dump(out: &mut String) {
    writeln!(out, "=== Platform Mining Hints ===").unwrap();
    writeln!(
        out,
        "capture_policy=read-only firmware/PCI evidence correlation; hints are not claimed devices"
    )
    .unwrap();

    let hints = match scan_platform_firmware() {
        Ok(hints) => hints,
        Err(error) => {
            writeln!(out, "smbios_mining=unavailable {error}").unwrap();
            writeln!(out).unwrap();
            append_csme_pci_snapshot(out, None, None);
            writeln!(out).unwrap();
            return;
        }
    };

    append_smbios_hints(out, &hints);
    writeln!(out).unwrap();
    append_csme_pci_snapshot(out, hints.cached_hfs, hints.csme_version.as_ref());
    writeln!(out).unwrap();
}

pub(crate) fn nct_management_hint() -> Option<NctManagementHint> {
    scan_platform_firmware()
        .ok()
        .and_then(|hints| hints.nct_management)
}

pub(crate) fn find_primary_mei() -> Option<PciDevice> {
    if crate::pci::with_devices(|devices| devices.is_empty()) {
        crate::pci::enumerate_impl();
    }

    crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|dev| dev.vendor_id == INTEL_VENDOR_ID && dev.device_id == RPL_S_MEI_DEVICE_ID)
    })
}

fn scan_platform_firmware() -> Result<PlatformFirmwareHints, String> {
    let table = smbios::discover()
        .map_err(|error| alloc::format!("reason={} detail={:?}", error.label(), error))?;
    let mut hints = PlatformFirmwareHints::default();
    let mut structures = table.structures();

    loop {
        let structure = match structures.next_structure() {
            Ok(Some(structure)) => structure,
            Ok(None) => break,
            Err(error) => {
                hints.parse_error = Some(alloc::format!("{:?}", error));
                break;
            }
        };

        collect_string_hints(&mut hints, structure);
        match structure.type_id {
            27 => collect_named_handle(&mut hints.cooling_devices, structure, 0x0E),
            28 => collect_named_handle(&mut hints.temperature_probes, structure, 0x04),
            34 => collect_management_device(&mut hints, structure),
            35 => collect_management_component(&mut hints, structure),
            219 if hints.cached_hfs.is_none() => {
                hints.cached_hfs = parse_type219_cached_hfs(structure)
            }
            221 if hints.csme_version.is_none() => {
                hints.csme_version = parse_type221_csme_version(structure)
            }
            _ => {}
        }
    }

    Ok(hints)
}

fn collect_string_hints(hints: &mut PlatformFirmwareHints, structure: Structure<'_>) {
    for (string_index, raw) in structure.strings().enumerate() {
        let text = firmware_text(raw);
        let upper = text.to_ascii_uppercase();
        let identity = FirmwareStringHint {
            type_id: structure.type_id,
            type_name: structure.type_name(),
            handle: structure.handle,
            string_index: string_index + 1,
        };

        if upper.contains(NCT5585_TOKEN) {
            hints.nct_strings.push(TextHint {
                identity,
                text: text.clone(),
            });
        }
        if MEI_FIRMWARE_TOKENS
            .iter()
            .any(|token| upper.trim() == *token)
        {
            hints.mei_strings.push(TextHint { identity, text });
        }
    }
}

fn collect_named_handle(out: &mut Vec<NamedHandle>, structure: Structure<'_>, offset: usize) {
    let Some(index) = structure.byte(offset) else {
        return;
    };
    let Some(raw) = structure.string_bytes(index) else {
        return;
    };
    out.push(NamedHandle {
        handle: structure.handle,
        name: firmware_text(raw),
    });
}

fn collect_management_device(hints: &mut PlatformFirmwareHints, structure: Structure<'_>) {
    let Some(description_index) = structure.byte(0x04) else {
        return;
    };
    let Some(description) = structure.string_bytes(description_index) else {
        return;
    };
    if !firmware_text(description)
        .to_ascii_uppercase()
        .contains(NCT5585_TOKEN)
    {
        return;
    }

    hints.nct_management = Some(NctManagementHint {
        handle: structure.handle,
        device_type: structure.byte(0x05),
        address: structure.u32(0x06),
        address_type: structure.byte(0x0A),
    });
}

fn collect_management_component(hints: &mut PlatformFirmwareHints, structure: Structure<'_>) {
    let description = structure
        .byte(0x04)
        .and_then(|index| structure.string_bytes(index))
        .map(firmware_text);
    let (Some(management_handle), Some(component_handle), Some(threshold_handle)) =
        (structure.u16(0x05), structure.u16(0x07), structure.u16(0x09))
    else {
        return;
    };

    hints.management_components.push(ManagementComponent {
        handle: structure.handle,
        description,
        management_handle,
        component_handle,
        threshold_handle,
    });
}

fn parse_type219_cached_hfs(structure: Structure<'_>) -> Option<[u32; 6]> {
    if structure.formatted_len() < 0x1F {
        return None;
    }
    let has_mei_names = structure.strings().any(|raw| {
        let upper = firmware_text(raw).to_ascii_uppercase();
        matches!(upper.trim(), "MEI1" | "MEI2" | "MEI3" | "MEI4")
    });
    if !has_mei_names {
        return None;
    }

    let mut values = [0u32; 6];
    for (index, value) in values.iter_mut().enumerate() {
        *value = structure.u32(0x07 + index * 4)?;
    }
    Some(values)
}

fn parse_type221_csme_version(structure: Structure<'_>) -> Option<CsmeFirmwareVersion> {
    let count = usize::from(structure.byte(0x04)?);
    for index in 0..count {
        let base = 0x05usize.checked_add(index.checked_mul(7)?)?;
        let label_index = structure.byte(base)?;
        let value_index = structure.byte(base + 1)?;
        let label = structure.string_bytes(label_index).map(firmware_text)?;
        if !label.eq_ignore_ascii_case("ME Firmware Version") {
            continue;
        }

        return Some(CsmeFirmwareVersion {
            major: structure.byte(base + 2)?,
            minor: structure.byte(base + 3)?,
            hotfix: structure.byte(base + 4)?,
            build: structure.u16(base + 5)?,
            sku: structure.string_bytes(value_index).map(firmware_text),
        });
    }
    None
}

fn append_smbios_hints(out: &mut String, hints: &PlatformFirmwareHints) {
    for hint in &hints.nct_strings {
        writeln!(
            out,
            "smbios_hint kind=nuvoton-superio-candidate type={} ({}) handle=0x{:04X} string={} value=\"{}\"",
            hint.identity.type_id,
            hint.identity.type_name,
            hint.identity.handle,
            hint.identity.string_index,
            hint.text
        )
        .unwrap();
    }
    for hint in &hints.mei_strings {
        writeln!(
            out,
            "smbios_hint kind=mei-name type={} ({}) handle=0x{:04X} string={} value=\"{}\"",
            hint.identity.type_id,
            hint.identity.type_name,
            hint.identity.handle,
            hint.identity.string_index,
            hint.text
        )
        .unwrap();
    }

    if let Some(error) = hints.parse_error.as_deref() {
        writeln!(out, "smbios_mining=parse-stopped detail={error}").unwrap();
    }

    if hints.nct_strings.is_empty() {
        writeln!(out, "nct5585_candidate=not-seen-in-smbios").unwrap();
    } else {
        writeln!(
            out,
            "nct5585_candidate=present source=smbios confidence=firmware-advertised hits={}",
            hints.nct_strings.len()
        )
        .unwrap();
        writeln!(
            out,
            "nct5585_candidate_surfaces=hwmon/thermal fan/tach/PWM PECI GPIO UART LED Port80"
        )
        .unwrap();
    }

    if let Some(nct) = hints.nct_management {
        writeln!(
            out,
            "nct5585_management_device handle=0x{:04X} device_type={} address={} address_type={}",
            nct.handle,
            nct.device_type
                .map(|value| alloc::format!("0x{:02X}", value))
                .unwrap_or_else(|| String::from("-")),
            nct.address
                .map(|value| alloc::format!("0x{:08X}", value))
                .unwrap_or_else(|| String::from("-")),
            nct.address_type
                .map(|value| alloc::format!("0x{:02X}({})", value, management_address_type(value)))
                .unwrap_or_else(|| String::from("-"))
        )
        .unwrap();

        for device in &hints.cooling_devices {
            writeln!(
                out,
                "smbios_cooling_device handle=0x{:04X} name=\"{}\"",
                device.handle, device.name
            )
            .unwrap();
        }
        for probe in &hints.temperature_probes {
            writeln!(
                out,
                "smbios_temperature_probe handle=0x{:04X} name=\"{}\"",
                probe.handle, probe.name
            )
            .unwrap();
        }

        let mut associations = 0usize;
        for component in hints
            .management_components
            .iter()
            .filter(|component| component.management_handle == nct.handle)
        {
            associations = associations.saturating_add(1);
            writeln!(
                out,
                "nct5585_component_link type35_handle=0x{:04X} component=0x{:04X} threshold=0x{:04X} description=\"{}\"",
                component.handle,
                component.component_handle,
                component.threshold_handle,
                component.description.as_deref().unwrap_or("-")
            )
            .unwrap();
        }
        writeln!(
            out,
            "nct5585_component_association={} type35_links={}",
            if associations == 0 {
                "unresolved"
            } else {
                "present"
            },
            associations
        )
        .unwrap();
        writeln!(
            out,
            "nct5585_next_probe=run `tlb nct probe` for transient Super-I/O identity/logical-device verification"
        )
        .unwrap();
    }

    if hints.mei_strings.is_empty() {
        writeln!(out, "mei_firmware_names=not-seen-in-smbios").unwrap();
    } else {
        writeln!(
            out,
            "mei_firmware_names=present source=smbios confidence=naming-only hits={}",
            hints.mei_strings.len()
        )
        .unwrap();
    }

    if let Some(cached) = hints.cached_hfs {
        writeln!(out, "smbios_type219_cached_hfs:").unwrap();
        for (index, value) in cached.iter().copied().enumerate() {
            writeln!(out, "  HFS{}=0x{:08X}", index + 1, value).unwrap();
        }
    } else {
        writeln!(out, "smbios_type219_cached_hfs=not-decoded").unwrap();
    }

    if let Some(version) = hints.csme_version.as_ref() {
        writeln!(
            out,
            "csme_firmware_version={}.{}.{}.{} sku=\"{}\" source=smbios-type221",
            version.major,
            version.minor,
            version.hotfix,
            version.build,
            version.sku.as_deref().unwrap_or("-")
        )
        .unwrap();
    } else {
        writeln!(out, "csme_firmware_version=not-decoded-from-smbios").unwrap();
    }
    writeln!(
        out,
        "mei_next_probe=run `tlb mei probe` for a claimed, reversible status-window reachability check"
    )
    .unwrap();
}

fn append_csme_pci_snapshot(
    out: &mut String,
    cached_hfs: Option<[u32; 6]>,
    version: Option<&CsmeFirmwareVersion>,
) {
    writeln!(out, "=== Intel CSME / MEI Discovery ===").unwrap();
    writeln!(
        out,
        "csme_capture_policy=PCI config read-only; no claim, BAR sizing, command writes, reset, DMA, interrupts, HBM, or client traffic"
    )
    .unwrap();

    let Some(dev) = find_primary_mei() else {
        writeln!(
            out,
            "csme_mei_primary=not-found expected={:04X}:{:04X}",
            INTEL_VENDOR_ID, RPL_S_MEI_DEVICE_ID
        )
        .unwrap();
        return;
    };

    let revision = cfg_u8(dev, 0x08);
    let command = cfg_u16(dev, 0x04);
    let status = cfg_u16(dev, 0x06);
    let subsystem_vendor = cfg_u16(dev, 0x2C);
    let subsystem_device = cfg_u16(dev, 0x2E);
    writeln!(
        out,
        "csme_mei_primary={:02X}:{:02X}.{} vid:did={:04X}:{:04X} rev={:02X} class={:02X}/{:02X}/{:02X}",
        dev.bus,
        dev.slot,
        dev.function,
        dev.vendor_id,
        dev.device_id,
        revision,
        dev.class,
        dev.subclass,
        dev.prog_if
    )
    .unwrap();
    writeln!(
        out,
        "pci_command=0x{:04X} memory_space={} pci_status=0x{:04X} subsystem={:04X}:{:04X}",
        command,
        yes_no(command & PCI_COMMAND_MEMORY_SPACE != 0),
        status,
        subsystem_vendor,
        subsystem_device
    )
    .unwrap();

    if let Some(version) = version {
        writeln!(
            out,
            "firmware={}.{}.{}.{} sku=\"{}\" source=smbios-type221",
            version.major,
            version.minor,
            version.hotfix,
            version.build,
            version.sku.as_deref().unwrap_or("-")
        )
        .unwrap();
    }

    append_bar0(out, dev);

    let mut live_hfs = [0u32; 6];
    writeln!(out, "host_firmware_status:").unwrap();
    for (index, (name, offset)) in HFS_REGISTERS.iter().copied().enumerate() {
        let raw = cfg_u32(dev, offset);
        live_hfs[index] = raw;
        writeln!(out, "  {} cfg+0x{:02X}=0x{:08X}", name, offset, raw).unwrap();
    }
    let hfs1 = live_hfs[0];
    let hfs3 = live_hfs[2];
    writeln!(
        out,
        "  conservative_decode HFS1.d0i3={} HFS1.operation_mode_bits=0x{:X} HFS3.fw_sku_bits=0x{:X}",
        yes_no((hfs1 >> 31) & 1 != 0),
        (hfs1 >> 16) & 0xF,
        (hfs3 >> 4) & 0x7
    )
    .unwrap();

    if let Some(cached) = cached_hfs {
        writeln!(out, "firmware_to_live_hfs_diff:").unwrap();
        for index in 0..live_hfs.len() {
            writeln!(
                out,
                "  HFS{} cached=0x{:08X} live=0x{:08X} xor=0x{:08X} state={}",
                index + 1,
                cached[index],
                live_hfs[index],
                cached[index] ^ live_hfs[index],
                if cached[index] == live_hfs[index] {
                    "same"
                } else {
                    "changed"
                }
            )
            .unwrap();
        }
    }

    append_mei_mmio_status(out, dev, command);

    writeln!(out, "same_slot_functions:").unwrap();
    crate::pci::with_devices(|devices| {
        for sibling in devices
            .iter()
            .filter(|sibling| sibling.bus == dev.bus && sibling.slot == dev.slot)
        {
            writeln!(
                out,
                "  {:02X}:{:02X}.{} {:04X}:{:04X} class={:02X}/{:02X}/{:02X}{}",
                sibling.bus,
                sibling.slot,
                sibling.function,
                sibling.vendor_id,
                sibling.device_id,
                sibling.class,
                sibling.subclass,
                sibling.prog_if,
                if sibling.function == dev.function {
                    " primary"
                } else {
                    ""
                }
            )
            .unwrap();
        }
    });

    writeln!(out, "pci_config_256:").unwrap();
    for offset in (0u16..0x100u16).step_by(16) {
        let a = cfg_u32(dev, offset);
        let b = cfg_u32(dev, offset + 4);
        let c = cfg_u32(dev, offset + 8);
        let d = cfg_u32(dev, offset + 12);
        writeln!(out, "  {:02X}: {:08X} {:08X} {:08X} {:08X}", offset, a, b, c, d).unwrap();
    }
    writeln!(
        out,
        "csme_next_probe=HBM/client enumeration only after a persistent MEI transport claim; explicit status reachability is available via `tlb mei probe`"
    )
    .unwrap();
}

fn append_bar0(out: &mut String, dev: PciDevice) {
    let (lo, hi) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, 0);
    let is_io = lo & 1 != 0;
    let is_64 = !is_io && ((lo >> 1) & 0x3) == 0x2;
    let decoded = dev.bar_address(0);
    writeln!(
        out,
        "bar0 raw_lo=0x{:08X} raw_hi={} kind={} width={} decoded={}",
        lo,
        hi.map(|value| alloc::format!("0x{:08X}", value))
            .unwrap_or_else(|| String::from("-")),
        if is_io { "io" } else { "mmio" },
        if is_64 { "64" } else { "32" },
        decoded
            .map(|base| alloc::format!("0x{:016X}", base))
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
}

fn append_mei_mmio_status(out: &mut String, dev: PciDevice, command: u16) {
    writeln!(out, "mei_mmio_status:").unwrap();
    writeln!(
        out,
        "  policy=status-only reads at +0x04/+0x0C; +0x00 host-write and +0x08 firmware-read data windows intentionally untouched"
    )
    .unwrap();

    let (bar_lo, _) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, 0);
    if bar_lo & 1 != 0 {
        writeln!(out, "  state=skipped reason=BAR0-is-io").unwrap();
        return;
    }
    if command & PCI_COMMAND_MEMORY_SPACE == 0 {
        writeln!(out, "  state=skipped reason=PCI-memory-space-disabled").unwrap();
        return;
    }
    let Some(bar0) = dev.bar_address(0) else {
        writeln!(out, "  state=skipped reason=BAR0-unavailable").unwrap();
        return;
    };
    if bar0 == 0 {
        writeln!(out, "  state=skipped reason=BAR0-zero").unwrap();
        return;
    }

    let mapped = match crate::pci::mmio::map_mmio_region_exact(bar0, MEI_STATUS_MAP_BYTES) {
        Ok(mapped) => mapped,
        Err(error) => {
            writeln!(out, "  state=map-failed detail={:?}", error).unwrap();
            return;
        }
    };

    let h_csr = unsafe { core::ptr::read_volatile(mapped.as_ptr().add(MEI_H_CSR) as *const u32) };
    let me_csr =
        unsafe { core::ptr::read_volatile(mapped.as_ptr().add(MEI_ME_CSR_HA) as *const u32) };

    writeln!(out, "  H_CSR[0x04]=0x{:08X}", h_csr).unwrap();
    append_mei_csr_decode(out, "host", h_csr, false);
    writeln!(out, "  ME_CSR_HA[0x0C]=0x{:08X}", me_csr).unwrap();
    append_mei_csr_decode(out, "firmware", me_csr, true);
}

pub(crate) fn append_mei_csr_decode(out: &mut String, side: &str, raw: u32, firmware: bool) {
    let depth = (raw >> 24) & 0xFF;
    let write_ptr = (raw >> 16) & 0xFF;
    let read_ptr = (raw >> 8) & 0xFF;
    let reset = raw & 0x10 != 0;
    let ready = raw & 0x08 != 0;
    let interrupt_generate = raw & 0x04 != 0;
    let interrupt_status = raw & 0x02 != 0;
    let interrupt_enable = raw & 0x01 != 0;
    writeln!(
        out,
        "    {} depth={} write_ptr={} read_ptr={} reset={} ready={} int_gen={} int_status={} int_enable={} sanity={}{}",
        side,
        depth,
        write_ptr,
        read_ptr,
        yes_no(reset),
        yes_no(ready),
        yes_no(interrupt_generate),
        yes_no(interrupt_status),
        yes_no(interrupt_enable),
        mei_csr_sanity(raw),
        if firmware && raw & 0x40 != 0 {
            " pg-isolation-capable=yes"
        } else if firmware {
            " pg-isolation-capable=no"
        } else {
            ""
        }
    )
    .unwrap();
}

fn mei_csr_sanity(raw: u32) -> &'static str {
    if raw == u32::MAX {
        "all-ones"
    } else if raw == 0 {
        "zero"
    } else if raw >> 24 == 0 {
        "depth-zero"
    } else {
        "plausible"
    }
}

fn management_address_type(value: u8) -> &'static str {
    match value {
        0x01 => "other",
        0x02 => "unknown",
        0x03 => "io-port",
        0x04 => "memory",
        0x05 => "smbus",
        _ => "reserved",
    }
}

fn cfg_u8(dev: PciDevice, offset: u16) -> u8 {
    crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, offset)
}

fn cfg_u16(dev: PciDevice, offset: u16) -> u16 {
    crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, offset)
}

fn cfg_u32(dev: PciDevice, offset: u16) -> u32 {
    crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, offset)
}

fn firmware_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\"' => out.push_str("\\\""),
            value if value.is_ascii_graphic() || value == b' ' => out.push(value as char),
            value => {
                write!(out, "\\x{:02X}", value).unwrap();
            }
        }
    }
    out
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
