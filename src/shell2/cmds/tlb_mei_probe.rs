use core::fmt::Write;
use core::sync::atomic::{Ordering, compiler_fence};

use alloc::string::String;
use spin::Mutex;

use crate::pci::PciDevice;

const CLAIM_OWNER: &str = "shell2-tlb-mei-probe";
const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

static MEI_PROBE_LOCK: Mutex<()> = Mutex::new(());
static MEI_STATUS_MAPPING: Mutex<Option<(u64, usize)>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct MeiReachabilitySnapshot {
    bar0: u64,
    original_command: u16,
    probe_command: u16,
    command_readback: u16,
    first_host_csr: u32,
    first_firmware_csr: u32,
    second_host_csr: u32,
    second_firmware_csr: u32,
}

impl MeiReachabilitySnapshot {
    fn stable(self) -> bool {
        self.first_host_csr == self.second_host_csr
            && self.first_firmware_csr == self.second_firmware_csr
    }

    fn plausible(self) -> bool {
        csr_plausible(self.first_host_csr) && csr_plausible(self.first_firmware_csr)
    }
}

pub(crate) fn build_probe_text() -> String {
    let mut out = String::new();
    writeln!(out, "Intel MEI status-window reachability probe").unwrap();
    writeln!(
        out,
        "policy=exclusive PCI claim; preserve BAR and every command bit; temporarily enable MEMORY_SPACE only when required; read +0x04/+0x0C only; restore command and release claim"
    )
    .unwrap();
    writeln!(
        out,
        "excluded=bus-master DMA MSI/MSI-X reset host-write(+0x00) firmware-read(+0x08) HBM client traffic"
    )
    .unwrap();

    let _probe_guard = MEI_PROBE_LOCK.lock();
    let Some(dev) = super::tlb_platform::find_primary_mei() else {
        writeln!(
            out,
            "result=not-found expected={:04X}:{:04X}",
            super::tlb_platform::INTEL_VENDOR_ID,
            super::tlb_platform::RPL_S_MEI_DEVICE_ID
        )
        .unwrap();
        return out;
    };

    writeln!(
        out,
        "device={:02X}:{:02X}.{} vid:did={:04X}:{:04X}",
        dev.bus, dev.slot, dev.function, dev.vendor_id, dev.device_id
    )
    .unwrap();

    match crate::pci::claim_device(&dev, CLAIM_OWNER) {
        Ok(()) => writeln!(out, "claim=acquired owner={CLAIM_OWNER}").unwrap(),
        Err(error) => {
            writeln!(out, "result=claim-failed detail={:?}", error).unwrap();
            return out;
        }
    }

    let original_command = read_command(dev);
    let mut command_modified = false;
    let probe_result = run_claimed_probe(dev, original_command, &mut command_modified);

    let command_before_restore = read_command(dev);
    if command_modified {
        write_command(dev, original_command);
    }
    let command_after_restore = read_command(dev);
    let command_restored = command_after_restore == original_command;
    let claim_released =
        crate::pci::release_device_claim(dev.bus, dev.slot, dev.function, CLAIM_OWNER);

    match probe_result {
        Ok(snapshot) => append_snapshot(&mut out, snapshot),
        Err(error) => writeln!(out, "result=probe-failed detail={error}").unwrap(),
    }

    writeln!(
        out,
        "cleanup command_modified={} before_restore=0x{:04X} after_restore=0x{:04X} command_restored={} claim_released={}",
        yes_no(command_modified),
        command_before_restore,
        command_after_restore,
        yes_no(command_restored),
        yes_no(claim_released)
    )
    .unwrap();
    if !command_restored || !claim_released {
        writeln!(out, "cleanup_state=ATTENTION").unwrap();
    } else {
        writeln!(out, "cleanup_state=complete").unwrap();
    }

    out
}

fn run_claimed_probe(
    dev: PciDevice,
    original_command: u16,
    command_modified: &mut bool,
) -> Result<MeiReachabilitySnapshot, String> {
    if original_command == u16::MAX {
        return Err(String::from("PCI command register returned all ones"));
    }

    let (bar_lo, _) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, 0);
    if bar_lo == 0 {
        return Err(String::from("BAR0 is unassigned"));
    }
    if bar_lo & 1 != 0 {
        return Err(String::from("BAR0 is an I/O BAR, expected MMIO"));
    }
    let Some(bar0) = dev.bar_address(0) else {
        return Err(String::from("BAR0 could not be decoded"));
    };
    if bar0 == 0 {
        return Err(String::from("BAR0 decoded to zero"));
    }

    let probe_command = original_command | PCI_COMMAND_MEMORY_SPACE;
    if probe_command != original_command {
        *command_modified = true;
        write_command(dev, probe_command);
    }
    let command_readback = read_command(dev);
    if command_readback == u16::MAX {
        return Err(String::from("PCI command readback returned all ones"));
    }
    if command_readback & PCI_COMMAND_MEMORY_SPACE == 0 {
        return Err(alloc::format!(
            "MEMORY_SPACE did not latch, readback=0x{:04X}",
            command_readback
        ));
    }
    if (command_readback & PCI_COMMAND_BUS_MASTER) != (original_command & PCI_COMMAND_BUS_MASTER) {
        return Err(alloc::format!(
            "BUS_MASTER changed unexpectedly, original=0x{:04X} readback=0x{:04X}",
            original_command,
            command_readback
        ));
    }

    let mapped = map_status_window(bar0)?;
    let (first_host_csr, first_firmware_csr) = read_status_pair(mapped);
    compiler_fence(Ordering::SeqCst);
    let (second_host_csr, second_firmware_csr) = read_status_pair(mapped);

    Ok(MeiReachabilitySnapshot {
        bar0,
        original_command,
        probe_command,
        command_readback,
        first_host_csr,
        first_firmware_csr,
        second_host_csr,
        second_firmware_csr,
    })
}

fn append_snapshot(out: &mut String, snapshot: MeiReachabilitySnapshot) {
    writeln!(
        out,
        "bar0=0x{:016X} original_command=0x{:04X} probe_command=0x{:04X} command_readback=0x{:04X}",
        snapshot.bar0, snapshot.original_command, snapshot.probe_command, snapshot.command_readback
    )
    .unwrap();
    writeln!(
        out,
        "command_bits memory_space={} bus_master={} bus_master_preserved={}",
        yes_no(snapshot.command_readback & PCI_COMMAND_MEMORY_SPACE != 0),
        yes_no(snapshot.command_readback & PCI_COMMAND_BUS_MASTER != 0),
        yes_no(
            (snapshot.command_readback & PCI_COMMAND_BUS_MASTER)
                == (snapshot.original_command & PCI_COMMAND_BUS_MASTER)
        )
    )
    .unwrap();

    writeln!(out, "first H_CSR[0x04]=0x{:08X}", snapshot.first_host_csr).unwrap();
    super::tlb_platform::append_mei_csr_decode(out, "host", snapshot.first_host_csr, false);
    writeln!(out, "first ME_CSR_HA[0x0C]=0x{:08X}", snapshot.first_firmware_csr).unwrap();
    super::tlb_platform::append_mei_csr_decode(out, "firmware", snapshot.first_firmware_csr, true);
    writeln!(
        out,
        "second H_CSR=0x{:08X} ME_CSR_HA=0x{:08X} stable={}",
        snapshot.second_host_csr,
        snapshot.second_firmware_csr,
        yes_no(snapshot.stable())
    )
    .unwrap();
    writeln!(
        out,
        "result={} status_window=reachable values_plausible={} values_stable={}",
        if snapshot.plausible() {
            "verified"
        } else {
            "reachable-with-unusual-status"
        },
        yes_no(snapshot.plausible()),
        yes_no(snapshot.stable())
    )
    .unwrap();
}

fn map_status_window(bar0: u64) -> Result<usize, String> {
    let mut cache = MEI_STATUS_MAPPING.lock();
    if let Some((cached_bar, mapped)) = *cache {
        if cached_bar == bar0 {
            return Ok(mapped);
        }
    }

    let mapped =
        crate::pci::mmio::map_mmio_region_exact(bar0, super::tlb_platform::MEI_STATUS_MAP_BYTES)
            .map_err(|error| alloc::format!("MMIO map failed: {:?}", error))?;
    let address = mapped.as_ptr() as usize;
    *cache = Some((bar0, address));
    Ok(address)
}

fn read_status_pair(mapped: usize) -> (u32, u32) {
    let host = unsafe {
        core::ptr::read_volatile((mapped + super::tlb_platform::MEI_H_CSR) as *const u32)
    };
    let firmware = unsafe {
        core::ptr::read_volatile((mapped + super::tlb_platform::MEI_ME_CSR_HA) as *const u32)
    };
    (host, firmware)
}

fn read_command(dev: PciDevice) -> u16 {
    crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04)
}

fn write_command(dev: PciDevice, command: u16) {
    // Status occupies the upper half of this dword and contains RW1C bits.
    // Supplying zero there changes only Command and cannot acknowledge status.
    crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x04, u32::from(command));
}

fn csr_plausible(raw: u32) -> bool {
    raw != 0 && raw != u32::MAX && raw >> 24 != 0
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
