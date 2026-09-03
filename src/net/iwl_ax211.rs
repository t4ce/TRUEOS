//! First-stage Intel AX211 (CNVi) PCI identity probe.
//!
//! This deliberately stops after PCI ownership, BAR discovery/mapping, and
//! read-only identity registers. AX211 is an integrated Gen2 device and must
//! not be sent through the legacy iwl4965 firmware/bootstrap path.

use crate::pci::PciDevice;

const INTEL_VENDOR: u16 = 0x8086;
const AX211_DEVICE: u16 = 0x7A70;
const AX211_SUBSYSTEM_VENDOR: u16 = 0x8086;
const AX211_SUBSYSTEM_DEVICE: u16 = 0x0094;
const PCI_CLAIM_OWNER: &str = "net/iwl-ax211";

const CSR_HW_REV: usize = 0x028;
const CSR_HW_RF_ID: usize = 0x09C;

pub fn probe(pci_dev: &PciDevice) -> bool {
    if pci_dev.vendor_id != INTEL_VENDOR || pci_dev.device_id != AX211_DEVICE {
        return false;
    }

    if let Err(error) = crate::pci::claim_device(pci_dev, PCI_CLAIM_OWNER) {
        crate::log_warn!(target: "net";
            "iwl-ax211: PCI claim failed at {:02x}:{:02x}.{} error={:?}\n",
            pci_dev.bus, pci_dev.slot, pci_dev.function, error
        );
        return false;
    }

    let subsystem_vendor = crate::pci::config_read_u16(
        pci_dev.bus, pci_dev.slot, pci_dev.function, 0x2C,
    );
    let subsystem_device = crate::pci::config_read_u16(
        pci_dev.bus, pci_dev.slot, pci_dev.function, 0x2E,
    );
    let revision = crate::pci::config_read_u8(
        pci_dev.bus, pci_dev.slot, pci_dev.function, 0x08,
    );

    let Some(bar0_phys) = pci_dev.bar_address(0) else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 is unavailable\n");
        let _ = crate::pci::release_device_claim(
            pci_dev.bus, pci_dev.slot, pci_dev.function, PCI_CLAIM_OWNER,
        );
        return false;
    };
    let Some(bar0_size) = crate::pci::bar_size_bytes(
        pci_dev.bus, pci_dev.slot, pci_dev.function, 0,
    ) else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 size is unavailable\n");
        let _ = crate::pci::release_device_claim(
            pci_dev.bus, pci_dev.slot, pci_dev.function, PCI_CLAIM_OWNER,
        );
        return false;
    };

    crate::pci::enable_mem_and_bus_master(
        pci_dev.bus, pci_dev.slot, pci_dev.function,
    );
    let Ok(bar0_size_usize) = usize::try_from(bar0_size) else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 size is too large\n");
        return false;
    };
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(bar0_phys, bar0_size_usize)
    else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 mapping failed\n");
        return false;
    };

    let base = mapped.as_ptr() as usize;
    let hw_rev = unsafe { core::ptr::read_volatile((base + CSR_HW_REV) as *const u32) };
    let hw_rf_id = unsafe { core::ptr::read_volatile((base + CSR_HW_RF_ID) as *const u32) };

    crate::log_info!(target: "net";
        "iwl-ax211: pci={:04x}:{:04x} subsystem={:04x}:{:04x} rev={:02x} bdf={:02x}:{:02x}.{} bar0=0x{:x} bar0_size=0x{:x}\n",
        pci_dev.vendor_id, pci_dev.device_id,
        subsystem_vendor, subsystem_device, revision,
        pci_dev.bus, pci_dev.slot, pci_dev.function, bar0_phys, bar0_size
    );
    crate::log_info!(target: "net";
        "iwl-ax211: hw_rev=0x{:08x} hw_rf_id=0x{:08x} profile=so-a0-gf-a0 gen2 integrated imr\n",
        hw_rev, hw_rf_id
    );

    if subsystem_vendor != AX211_SUBSYSTEM_VENDOR || subsystem_device != AX211_SUBSYSTEM_DEVICE {
        crate::log_warn!(target: "net";
            "iwl-ax211: unexpected subsystem expected={:04x}:{:04x}\n",
            AX211_SUBSYSTEM_VENDOR, AX211_SUBSYSTEM_DEVICE
        );
    }
    true
}
