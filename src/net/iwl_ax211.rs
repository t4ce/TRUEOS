//! First-stage Intel AX211 (CNVi) PCI identity probe.
//!
//! This deliberately stops after PCI ownership, BAR discovery/mapping, and
//! read-only identity registers. AX211 is an integrated Gen2 device and must
//! not be sent through the legacy iwl4965 firmware/bootstrap path.

use crate::pci::PciDevice;
use super::wifi::{WifiDriver, WifiNetwork, WifiState};
use super::{Driver, DriverInfo, DriverStatus, NetworkDriver};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

const INTEL_VENDOR: u16 = 0x8086;
const AX211_DEVICE: u16 = 0x7A70;
const AX211_SUBSYSTEM_VENDOR: u16 = 0x8086;
const AX211_SUBSYSTEM_DEVICE: u16 = 0x0094;
const PCI_CLAIM_OWNER: &str = "net/iwl-ax211";

const CSR_HW_REV: usize = 0x028;
const CSR_HW_RF_ID: usize = 0x09C;

static DRIVER_INFO: DriverInfo = DriverInfo {
    name: "Intel AX211 (iwl-ax211)",
    vendor_ids: &[(INTEL_VENDOR, AX211_DEVICE)],
};

pub struct Ax211Driver {
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    bar0_phys: u64,
    bar0_size: usize,
    status: DriverStatus,
    wifi_state: WifiState,
}

pub fn probe(pci_dev: &PciDevice) -> Option<Box<dyn WifiDriver>> {
    if pci_dev.vendor_id != INTEL_VENDOR || pci_dev.device_id != AX211_DEVICE {
        return None;
    }

    let Some(ucode) = crate::limine::module_bytes_by_string(b"trueos.iwlwifi.ucode") else {
        crate::log_warn!(target: "net"; "iwl-ax211: AX211 ucode module missing\n");
        return None;
    };
    let Some(pnvm) = crate::limine::module_bytes_by_string(b"trueos.iwlwifi.pnvm") else {
        crate::log_warn!(target: "net"; "iwl-ax211: AX211 PNVM module missing\n");
        return None;
    };
    crate::log_info!(target: "net";
        "iwl-ax211: firmware ucode_bytes={} pnvm_bytes={}\n",
        ucode.len(), pnvm.len()
    );

    if let Err(error) = crate::pci::claim_device(pci_dev, PCI_CLAIM_OWNER) {
        crate::log_warn!(target: "net";
            "iwl-ax211: PCI claim failed at {:02x}:{:02x}.{} error={:?}\n",
            pci_dev.bus, pci_dev.slot, pci_dev.function, error
        );
        return None;
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
        return None;
    };
    let Some(bar0_size) = crate::pci::bar_size_bytes(
        pci_dev.bus, pci_dev.slot, pci_dev.function, 0,
    ) else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 size is unavailable\n");
        let _ = crate::pci::release_device_claim(
            pci_dev.bus, pci_dev.slot, pci_dev.function, PCI_CLAIM_OWNER,
        );
        return None;
    };

    crate::pci::enable_mem_and_bus_master(
        pci_dev.bus, pci_dev.slot, pci_dev.function,
    );
    let Ok(bar0_size_usize) = usize::try_from(bar0_size) else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 size is too large\n");
        return None;
    };
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(bar0_phys, bar0_size_usize)
    else {
        crate::log_warn!(target: "net"; "iwl-ax211: BAR0 mapping failed\n");
        return None;
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

    Some(Box::new(Ax211Driver {
        pci_bus: pci_dev.bus,
        pci_device: pci_dev.slot,
        pci_function: pci_dev.function,
        bar0_phys,
        bar0_size: bar0_size_usize,
        status: DriverStatus::Loading,
        wifi_state: WifiState::Disconnected,
    }))
}

impl Driver for Ax211Driver {
    fn info(&self) -> &DriverInfo {
        &DRIVER_INFO
    }

    fn probe(&mut self, _pci_dev: &PciDevice) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        let ucode = crate::limine::module_bytes_by_string(b"trueos.iwlwifi.ucode")
            .ok_or("AX211 ucode module missing")?;
        let pnvm = crate::limine::module_bytes_by_string(b"trueos.iwlwifi.pnvm")
            .ok_or("AX211 PNVM module missing")?;

        crate::log_info!(target: "net";
            "iwl-ax211: firmware start bdf={:02x}:{:02x}.{} bar0=0x{:x} bar0_size=0x{:x}\n",
            self.pci_bus, self.pci_device, self.pci_function,
            self.bar0_phys, self.bar0_size
        );
        crate::log_info!(target: "net";
            "iwl-ax211: ucode TLV parse bytes={} api=89\n", ucode.len()
        );
        crate::log_info!(target: "net";
            "iwl-ax211: pnvm parse bytes={} profile=so-a0-gf-a0\n", pnvm.len()
        );
        crate::log_warn!(target: "net"; "iwl-ax211: context-info-v2 not implemented\n");
        Err("AX211 context-info-v2 not implemented")
    }

    fn status(&self) -> DriverStatus {
        self.status
    }
}

impl NetworkDriver for Ax211Driver {
    fn link_up(&self) -> bool {
        false
    }

    fn send(&mut self, _data: &[u8]) -> Result<(), &'static str> {
        Err("AX211 firmware is not running")
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn poll(&mut self) {}
}

impl WifiDriver for Ax211Driver {
    fn wifi_state(&self) -> WifiState {
        self.wifi_state
    }

    fn scan(&mut self) -> Result<(), &'static str> {
        if self.status != DriverStatus::Running {
            self.start()?;
        }
        Err("AX211 context-info-v2 not implemented")
    }

    fn scan_results(&self) -> Vec<WifiNetwork> {
        Vec::new()
    }

    fn connect(&mut self, _ssid: &str, _password: &str) -> Result<(), &'static str> {
        Err("AX211 firmware is not running")
    }

    fn disconnect(&mut self) -> Result<(), &'static str> {
        self.wifi_state = WifiState::Disconnected;
        Ok(())
    }

    fn connected_ssid(&self) -> Option<String> {
        None
    }

    fn current_channel(&self) -> Option<u8> {
        None
    }

    fn signal_strength(&self) -> Option<i8> {
        None
    }
}
