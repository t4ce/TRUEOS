use core::fmt::Write;

use alloc::string::String;

use crate::efi::smbios;
use crate::pci::PciDevice;

const NCT5585_TOKEN: &str = "NCT5585";
const MEI_FIRMWARE_TOKENS: [&str; 5] = ["$MEI", "MEI1", "MEI2", "MEI3", "MEI4"];

const INTEL_VENDOR_ID: u16 = 0x8086;
const RPL_S_MEI_DEVICE_ID: u16 = 0x7A68;
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

const MEI_H_CSR: usize = 0x04;
const MEI_ME_CSR_HA: usize = 0x0C;
const MEI_STATUS_MAP_BYTES: usize = 0x10;

pub(crate) fn append_dump(out: &mut String) {
    writeln!(out, "=== Platform Mining Hints ===").unwrap();
    writeln!(
        out,
        "capture_policy=read-only firmware/PCI evidence correlation; hints are not claimed devices"
    )
    .unwrap();
    append_smbios_hints(out);
    writeln!(out).unwrap();
    append_csme_pci_snapshot(out);
    writeln!(out).unwrap();
}

fn append_smbios_hints(out: &mut String) {
    let table = match smbios::discover() {
        Ok(table) => table,
        Err(error) => {
            writeln!(
                out,
                "smbios_mining=unavailable reason={} detail={:?}",
                error.label(),
                error
            )
            .unwrap();
            return;
        }
    };

    let mut structures = table.structures();
    let mut nct_hits = 0usize;
    let mut mei_hits = 0usize;

    loop {
        let structure = match structures.next_structure() {
            Ok(Some(structure)) => structure,
            Ok(None) => break,
            Err(error) => {
                writeln!(out, "smbios_mining=parse-stopped detail={:?}", error).unwrap();
                break;
            }
        };

        for (string_index, raw) in structure.strings().enumerate() {
            let text = firmware_text(raw);
            let upper = text.to_ascii_uppercase();
            if upper.contains(NCT5585_TOKEN) {
                nct_hits = nct_hits.saturating_add(1);
                writeln!(
                    out,
                    "smbios_hint kind=nuvoton-superio-candidate type={} ({}) handle=0x{:04X} string={} value=\"{}\"",
                    structure.type_id,
                    structure.type_name(),
                    structure.handle,
                    string_index + 1,
                    text
                )
                .unwrap();
            }

            if MEI_FIRMWARE_TOKENS
                .iter()
                .any(|token| upper.trim() == *token)
            {
                mei_hits = mei_hits.saturating_add(1);
                writeln!(
                    out,
                    "smbios_hint kind=mei-name type={} ({}) handle=0x{:04X} string={} value=\"{}\"",
                    structure.type_id,
                    structure.type_name(),
                    structure.handle,
                    string_index + 1,
                    text
                )
                .unwrap();
            }
        }
    }

    if nct_hits == 0 {
        writeln!(out, "nct5585_candidate=not-seen-in-smbios").unwrap();
    } else {
        writeln!(
            out,
            "nct5585_candidate=present source=smbios confidence=firmware-advertised-only hits={}",
            nct_hits
        )
        .unwrap();
        writeln!(
            out,
            "nct5585_candidate_surfaces=hwmon/thermal fan/tach/PWM PECI GPIO UART LED Port80"
        )
        .unwrap();
        writeln!(
            out,
            "nct5585_next_probe=Super-I/O config identity + logical-device map; requires explicit write-gated config-mode sequence and is not executed by TLB"
        )
        .unwrap();
    }

    if mei_hits == 0 {
        writeln!(out, "mei_firmware_names=not-seen-in-smbios").unwrap();
    } else {
        writeln!(
            out,
            "mei_firmware_names=present source=smbios confidence=naming-only hits={}",
            mei_hits
        )
        .unwrap();
        writeln!(
            out,
            "mei_next_probe=correlate firmware names with PCI MEI/HECI functions; SMBIOS names alone do not establish a transport"
        )
        .unwrap();
    }
}

fn append_csme_pci_snapshot(out: &mut String) {
    writeln!(out, "=== Intel CSME / MEI Discovery ===").unwrap();
    writeln!(
        out,
        "csme_capture_policy=PCI config read-only; no claim, BAR sizing, command writes, reset, DMA, interrupts, HBM, or client traffic"
    )
    .unwrap();

    if crate::pci::with_devices(|devices| devices.is_empty()) {
        crate::pci::enumerate_impl();
    }

    let target = crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|dev| dev.vendor_id == INTEL_VENDOR_ID && dev.device_id == RPL_S_MEI_DEVICE_ID)
    });

    let Some(dev) = target else {
        writeln!(
            out,
            "csme_mei_primary=not-found expected={:04X}:{:04X}",
            INTEL_VENDOR_ID,
            RPL_S_MEI_DEVICE_ID
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

    append_bar0(out, dev);

    writeln!(out, "host_firmware_status:").unwrap();
    for (name, offset) in HFS_REGISTERS {
        let raw = cfg_u32(dev, offset);
        writeln!(out, "  {} cfg+0x{:02X}=0x{:08X}", name, offset, raw).unwrap();
    }
    let hfs1 = cfg_u32(dev, PCI_CFG_HFS_1);
    let hfs3 = cfg_u32(dev, PCI_CFG_HFS_3);
    writeln!(
        out,
        "  conservative_decode HFS1.d0i3={} HFS1.operation_mode_bits=0x{:X} HFS3.fw_sku_bits=0x{:X}",
        yes_no((hfs1 >> 31) & 1 != 0),
        (hfs1 >> 16) & 0xF,
        (hfs3 >> 4) & 0x7
    )
    .unwrap();

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
                if sibling.function == dev.function { " primary" } else { "" }
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
        writeln!(
            out,
            "  {:02X}: {:08X} {:08X} {:08X} {:08X}",
            offset, a, b, c, d
        )
        .unwrap();
    }
    writeln!(
        out,
        "csme_next_probe=HBM/client enumeration only after an explicit MEI transport claim; not part of this observational TLB path"
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

    let h_csr = unsafe {
        core::ptr::read_volatile(mapped.as_ptr().add(MEI_H_CSR) as *const u32)
    };
    let me_csr = unsafe {
        core::ptr::read_volatile(mapped.as_ptr().add(MEI_ME_CSR_HA) as *const u32)
    };

    writeln!(out, "  H_CSR[0x04]=0x{:08X}", h_csr).unwrap();
    append_mei_csr_decode(out, "host", h_csr, false);
    writeln!(out, "  ME_CSR_HA[0x0C]=0x{:08X}", me_csr).unwrap();
    append_mei_csr_decode(out, "firmware", me_csr, true);
}

fn append_mei_csr_decode(out: &mut String, side: &str, raw: u32, firmware: bool) {
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
        "    {} depth={} write_ptr={} read_ptr={} reset={} ready={} int_gen={} int_status={} int_enable={}{}",
        side,
        depth,
        write_ptr,
        read_ptr,
        yes_no(reset),
        yes_no(ready),
        yes_no(interrupt_generate),
        yes_no(interrupt_status),
        yes_no(interrupt_enable),
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
