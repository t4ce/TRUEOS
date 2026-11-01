//! Intel AX201 (CNVi/QuZ) PCI ownership and unified firmware startup.
//!
//! Association and Ethernet readiness are independent of firmware ALIVE.
//! Keep link down until the radio has completed association and key setup;
//! never send AX201 through the legacy iwl4965 or AX211 firmware paths.
//! Reference: Linux drivers/net/wireless/intel/iwlwifi/cfg/22000.c (QuZ/hr).
//! Validated host identity: 8086:a0f0 rev20, subsystem 8086:0070.
//! No emulator-specific policy: discovery always follows the enumerated BDF.

use super::wifi::{WifiDriver, WifiNetwork, WifiState};
use super::{Driver, DriverInfo, DriverStatus, NetworkDriver};
use crate::pci::PciDevice;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[path = "iwl_ax201_mvm.rs"]
mod mvm;

const INTEL_VENDOR: u16 = 0x8086;
const AX201_DEVICE: u16 = 0xA0F0;
const AX201_SUBSYSTEM_VENDOR: u16 = 0x8086;
const AX201_SUBSYSTEM_DEVICE: u16 = 0x0070;
const PCI_CLAIM_OWNER: &str = "net/iwl-ax201";

const CSR_HW_REV: usize = 0x028;
const CSR_HW_RF_ID: usize = 0x09C;

static DRIVER_INFO: DriverInfo = DriverInfo {
    name: "Intel AX201 (iwl-ax201)",
    vendor_ids: &[(INTEL_VENDOR, AX201_DEVICE)],
};

pub struct Ax201Driver {
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    bar0_size: usize,
    mmio: usize,
    runtime: Option<mvm::Runtime>,
    mac: Option<[u8; 6]>,
    start_attempted: bool,
    scan_requested: bool,
    status: DriverStatus,
    wifi_state: WifiState,
}

pub fn probe(pci_dev: &PciDevice) -> Option<Box<dyn WifiDriver>> {
    if pci_dev.vendor_id != INTEL_VENDOR || pci_dev.device_id != AX201_DEVICE {
        return None;
    }

    if let Err(error) = crate::pci::claim_device(pci_dev, PCI_CLAIM_OWNER) {
        crate::log_warn!(target: "net";
            "iwl-ax201: PCI claim failed at {:02x}:{:02x}.{} error={:?}\n",
            pci_dev.bus, pci_dev.slot, pci_dev.function, error
        );
        return None;
    }

    crate::log_info!(target: "net";
        "iwl-ax201: stage=pci-owned bdf={:02x}:{:02x}.{}\n",
        pci_dev.bus, pci_dev.slot, pci_dev.function);

    let subsystem_vendor =
        crate::pci::config_read_u16(pci_dev.bus, pci_dev.slot, pci_dev.function, 0x2C);
    let subsystem_device =
        crate::pci::config_read_u16(pci_dev.bus, pci_dev.slot, pci_dev.function, 0x2E);
    let revision = crate::pci::config_read_u8(pci_dev.bus, pci_dev.slot, pci_dev.function, 0x08);

    let Some(bar0_phys) = pci_dev.bar_address(0) else {
        crate::log_warn!(target: "net"; "iwl-ax201: BAR0 is unavailable\n");
        let _ = crate::pci::release_device_claim(
            pci_dev.bus,
            pci_dev.slot,
            pci_dev.function,
            PCI_CLAIM_OWNER,
        );
        return None;
    };
    let Some(bar0_size) =
        crate::pci::bar_size_bytes(pci_dev.bus, pci_dev.slot, pci_dev.function, 0)
    else {
        crate::log_warn!(target: "net"; "iwl-ax201: BAR0 size is unavailable\n");
        let _ = crate::pci::release_device_claim(
            pci_dev.bus,
            pci_dev.slot,
            pci_dev.function,
            PCI_CLAIM_OWNER,
        );
        return None;
    };

    let mapped = (|| {
        let size = usize::try_from(bar0_size).ok()?;
        if size < CSR_HW_RF_ID + 4 {
            return None;
        }
        crate::pci::mmio::map_mmio_region_exact(bar0_phys, size).ok()
    })();
    let Some(mapped) = mapped else {
        crate::log_warn!(target: "net"; "iwl-ax201: stage=bar-map-failed bytes=0x{:x}\n", bar0_size);
        let _ = crate::pci::release_device_claim(
            pci_dev.bus,
            pci_dev.slot,
            pci_dev.function,
            PCI_CLAIM_OWNER,
        );
        return None;
    };
    // Probe only decodes MMIO; DMA stays disabled until queues are implemented.
    let command = crate::pci::config_read_u16(pci_dev.bus, pci_dev.slot, pci_dev.function, 4);
    crate::pci::enable_mem_space_only(pci_dev.bus, pci_dev.slot, pci_dev.function);

    let base = mapped.as_ptr() as usize;
    let hw_rev = unsafe { core::ptr::read_volatile((base + CSR_HW_REV) as *const u32) };
    let hw_rf_id = unsafe { core::ptr::read_volatile((base + CSR_HW_RF_ID) as *const u32) };

    if hw_rev == u32::MAX || hw_rf_id == u32::MAX {
        crate::pci::config_write_u16(pci_dev.bus, pci_dev.slot, pci_dev.function, 4, command);
        let _ = crate::pci::release_device_claim(
            pci_dev.bus,
            pci_dev.slot,
            pci_dev.function,
            PCI_CLAIM_OWNER,
        );
        crate::log_warn!(target: "net"; "iwl-ax201: stage=registers-inaccessible\n");
        return None;
    }

    crate::log_info!(target: "net";
        "iwl-ax201: stage=pci-claimed pci={:04x}:{:04x} subsystem={:04x}:{:04x} rev={:02x} bdf={:02x}:{:02x}.{} bar0=0x{:x} bar0_size=0x{:x}\n",
        pci_dev.vendor_id, pci_dev.device_id,
        subsystem_vendor, subsystem_device, revision,
        pci_dev.bus, pci_dev.slot, pci_dev.function, bar0_phys, bar0_size
    );
    crate::log_info!(target: "net";
        "iwl-ax201: hw_rev=0x{:08x} hw_rf_id=0x{:08x} stage=registers-ready profile=QuZ integrated\n",
        hw_rev, hw_rf_id
    );

    if subsystem_vendor != AX201_SUBSYSTEM_VENDOR || subsystem_device != AX201_SUBSYSTEM_DEVICE {
        crate::log_warn!(target: "net";
            "iwl-ax201: unexpected subsystem expected={:04x}:{:04x}\n",
            AX201_SUBSYSTEM_VENDOR, AX201_SUBSYSTEM_DEVICE
        );
    }

    Some(Box::new(Ax201Driver {
        pci_bus: pci_dev.bus,
        pci_device: pci_dev.slot,
        pci_function: pci_dev.function,
        bar0_size: bar0_size as usize,
        mmio: base,
        runtime: None,
        mac: None,
        start_attempted: false,
        scan_requested: false,
        status: DriverStatus::Loading,
        wifi_state: WifiState::Disconnected,
    }))
}

impl Driver for Ax201Driver {
    fn info(&self) -> &DriverInfo {
        &DRIVER_INFO
    }

    fn probe(&mut self, _pci_dev: &PciDevice) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        if self.runtime.is_some() {
            return Ok(());
        }
        // Repeated UI scan requests must not reuse unretired DMA queues after
        // an uncertain firmware boot. A fresh device reset/probe is required.
        if self.start_attempted {
            return Err("AX201 firmware start already attempted; reset required");
        }
        self.start_attempted = true;
        let firmware = super::iwl_ax201_fw::parse(super::iwl_ax201_fw::EMBEDDED)?;
        crate::log_warn!(target: "net";
            "iwl-ax201: stage=firmware-parsed api={}.{:08x}.{} bytes={} sections={} target_ssid=easyeasy\n",
            firmware.version.major, firmware.version.minor, firmware.version.local,
            super::iwl_ax201_fw::EMBEDDED.len(), firmware.runtime.len());
        let transport = super::iwl_ax201_transport::Transport::new(
            self.mmio,
            self.bar0_size,
            (self.pci_bus, self.pci_device, self.pci_function),
            &firmware,
        )?;
        self.runtime = Some(mvm::Runtime::start(transport, firmware)?);
        Ok(())
    }

    fn status(&self) -> DriverStatus {
        self.status
    }
}

impl NetworkDriver for Ax201Driver {
    fn link_up(&self) -> bool {
        self.status == DriverStatus::Running
            && self
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.link_up())
    }

    fn send(&mut self, data: &[u8]) -> Result<(), &'static str> {
        self.runtime
            .as_mut()
            .ok_or("AX201 firmware not ready")?
            .send(data)
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        self.runtime.as_mut().and_then(|runtime| runtime.receive())
    }

    fn poll(&mut self) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        match runtime.poll() {
            Ok(true) => {
                if self.status != DriverStatus::Running {
                    self.mac = hardware_mac(self.mmio);
                    self.status = DriverStatus::Running;
                    crate::log_warn!(target: "net";
                        "iwl-ax201: stage=firmware-ready mac={:?} link=down\n", self.mac);
                }
                if self.scan_requested {
                    self.scan_requested = false;
                    let result = self
                        .mac
                        .ok_or("AX201 hardware MAC unavailable")
                        .and_then(|mac| runtime.begin_scan(mac));
                    if let Err(reason) = result {
                        self.wifi_state = WifiState::Failed;
                        crate::log_warn!(target: "net"; "iwl-ax201: stage=scan-error reason={}\n", reason);
                    }
                }
                if let Some(state) = runtime.connection_state() {
                    self.wifi_state = state;
                } else if runtime.scan_finished() {
                    self.wifi_state = WifiState::Disconnected;
                }
            }
            Ok(false) => {}
            Err(reason) => {
                self.status = DriverStatus::Loading;
                self.wifi_state = WifiState::Failed;
                crate::log_warn!(target: "net"; "iwl-ax201: stage=firmware-error reason={}\n", reason);
            }
        }
    }
}

impl WifiDriver for Ax201Driver {
    fn mac_address(&self) -> Option<[u8; 6]> {
        self.mac
    }

    // NIC inventory may exist while disconnected. Carrier and TX remain
    // controlled by association plus confirmed WPA2 key exchange.
    fn ethernet_data_path(&self) -> bool {
        self.status == DriverStatus::Running && self.mac.is_some()
    }

    fn last_error(&self) -> Option<&'static str> {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.connection_error())
    }

    fn wifi_state(&self) -> WifiState {
        self.wifi_state
    }

    fn scan(&mut self) -> Result<(), &'static str> {
        self.start()?;
        self.scan_requested = true;
        self.wifi_state = WifiState::Scanning;
        Ok(())
    }

    fn scan_results(&self) -> Vec<WifiNetwork> {
        self.runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.scan_results())
    }

    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), &'static str> {
        let mac = self.mac.ok_or("AX201 hardware MAC not ready")?;
        self.runtime
            .as_mut()
            .ok_or("AX201 firmware not ready")?
            .connect(ssid, password.as_bytes(), mac)?;
        self.wifi_state = WifiState::Connecting;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), &'static str> {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.disconnect();
        }
        self.wifi_state = WifiState::Disconnected;
        Ok(())
    }

    fn connected_ssid(&self) -> Option<String> {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.connected_ssid())
    }

    fn signal_strength(&self) -> Option<i8> {
        self.runtime.as_ref().and_then(|runtime| runtime.signal())
    }
}

// 22000/QuZ mac_addr_from_csr=0x380, Linux iwl-nvm-parse.c.
// STRAP wins over OTP only when it contains a valid unicast hardware address.
fn hardware_mac(mmio: usize) -> Option<[u8; 6]> {
    for offset in [0x388usize, 0x380] {
        let low = csr_word(mmio + offset);
        let high = csr_word(mmio + offset + 4);
        let mac = [
            (low >> 24) as u8,
            (low >> 16) as u8,
            (low >> 8) as u8,
            low as u8,
            (high >> 8) as u8,
            high as u8,
        ];
        if mac != [0; 6] && mac[0] & 1 == 0 {
            return Some(mac);
        }
    }
    None
}

fn csr_word(address: usize) -> u32 {
    let word: u32;
    unsafe {
        core::arch::asm!("mov {word:e}, dword ptr [{address}]",
            word = out(reg) word, address = in(reg) address,
            options(nostack, preserves_flags, readonly));
    }
    word
}
