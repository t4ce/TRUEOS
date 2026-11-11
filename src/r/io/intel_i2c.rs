//! PCI/MMIO admission for the Tiger Lake LPSS I2C controllers.
//!
//! Only the selected function is powered and claimed. Transfers use the
//! polling DesignWare master; bus mastering, DMA and device IRQs stay disabled.

use core::ptr::NonNull;
use trueos_time::{Duration, Instant, Timer};

use super::{designware_i2c, i2c_hid};
use crate::pci::{self, PciDevice};

const OWNER: &str = "input/i2c-hid";
const MAP_BYTES: usize = 0x1000;
const LPSS_RESETS: u16 = 0x204;
const LPSS_FUNCTION_RESETS: u32 = 3;
const PCI_STATUS_CAP_LIST: u16 = 1 << 4;
const PCI_COMMAND_MEMORY: u16 = 1 << 1;
const PCI_COMMAND_MASTER: u16 = 1 << 2;
const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;
const PM_CAP_ID: u8 = 1;
const PMCSR_STATE: u16 = 3;
const PMCSR_PME_STATUS: u16 = 1 << 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Error {
    pub stage: &'static str,
    pub reason: &'static str,
    pub abort_source: u32,
}

fn error(stage: &'static str, reason: &'static str) -> Error {
    Error {
        stage,
        reason,
        abort_source: 0,
    }
}

impl From<designware_i2c::Error> for Error {
    fn from(value: designware_i2c::Error) -> Self {
        Self {
            stage: value.stage,
            reason: value.reason,
            abort_source: value.abort_source,
        }
    }
}

struct Claim {
    device: PciDevice,
}

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = pci::release_device_claim(
            self.device.bus,
            self.device.slot,
            self.device.function,
            OWNER,
        );
    }
}

struct Registers {
    base: NonNull<u8>,
}

impl Registers {
    fn register(&self, offset: u16) -> Option<*mut u32> {
        let offset = usize::from(offset);
        if offset & 3 != 0 || offset.checked_add(4)? > MAP_BYTES {
            return None;
        }
        Some(unsafe { self.base.as_ptr().add(offset).cast::<u32>() })
    }
}

impl designware_i2c::Bus for Registers {
    fn read(&mut self, offset: u16) -> u32 {
        self.register(offset)
            .map(|p| unsafe { core::ptr::read_volatile(p) })
            .unwrap_or(u32::MAX)
    }
    fn write(&mut self, offset: u16, value: u32) {
        if let Some(p) = self.register(offset) {
            unsafe {
                core::ptr::write_volatile(p, value);
            }
        }
    }
    fn now_ms(&self) -> u64 {
        Instant::now().as_millis()
    }
    async fn pause(&mut self) {
        Timer::after(Duration::from_millis(1)).await;
    }
}

/// A claimed controller. Call shutdown before dropping an active controller;
/// Drop releases the PCI claim and never waits on hardware or a task executor.
pub(crate) struct Controller {
    claim: Claim,
    registers: Registers,
    master: designware_i2c::Master,
}

impl Controller {
    pub(crate) async fn open(device: PciDevice, bus_hz: u32) -> Result<Self, Error> {
        if device.vendor_id != 0x8086
            || !matches!(device.device_id, 0xa0e8 | 0xa0e9)
            || device.class != 0x0c
            || device.subclass != 0x80
        {
            return Err(error("admission", "unsupported Intel I2C controller"));
        }
        let identity = pci::config_read_u32(device.bus, device.slot, device.function, 0);
        if identity != (u32::from(device.device_id) << 16) | 0x8086 {
            return Err(error("admission", "PCI identity changed or disappeared"));
        }
        pci::claim_device(&device, OWNER)
            .map_err(|_| error("claim", "PCI function already owned or claim registry full"))?;
        let claim = Claim { device };
        // Every subsequent error drops this guard, including an async error
        // during DesignWare configuration, so no failed probe leaks a claim.
        // Quiesce DMA/INTx before D0, rather than allowing a firmware-owned
        // engine to run during the D3hot recovery interval. D0 can reset PCI
        // command state, so apply and verify the final decode policy again.
        configure_pci_command(device, false)?;
        power_on(device).await?;
        configure_pci_command(device, true)?;

        let (bar_low, _) = pci::read_bar_raw(device.bus, device.slot, device.function, 0);
        if bar_low == u32::MAX || bar_low & 1 != 0 || (bar_low >> 1) & 3 == 3 {
            return Err(error("bar0", "BAR0 is not a valid memory BAR"));
        }
        let physical = device
            .bar_address(0)
            .filter(|&base| base != 0 && base & 0xfff == 0)
            .ok_or(error("bar0", "BAR0 is absent or not page aligned"))?;
        physical
            .checked_add(MAP_BYTES as u64)
            .ok_or(error("bar0", "BAR0 address overflows"))?;
        // This sizing probe is confined to the exclusively admitted function,
        // with DMA disabled; the PCI helper restores BARs and decode state.
        let size = pci::bar_size_bytes(device.bus, device.slot, device.function, 0)
            .ok_or(error("bar0", "BAR0 size unavailable"))?;
        if size < MAP_BYTES as u64 {
            return Err(error("bar0", "BAR0 does not contain LPSS registers"));
        }
        let base = pci::mmio::map_mmio_region_exact(physical, MAP_BYTES)
            .map_err(|_| error("map", "BAR0 MMIO mapping failed"))?;
        let mut registers = Registers { base };
        use designware_i2c::Bus;
        let resets = registers.read(LPSS_RESETS);
        if resets == u32::MAX {
            return Err(error("lpss-reset", "LPSS registers unavailable"));
        }
        // Release APB and functional reset while preserving the independent
        // iDMA reset bit. I2C's fixed root clock needs no write at offset 0x200.
        registers.write(LPSS_RESETS, resets | LPSS_FUNCTION_RESETS);
        let deadline = Instant::now().as_millis().saturating_add(20);
        let mut released = false;
        for _ in 0..32 {
            let observed = registers.read(LPSS_RESETS);
            if observed == u32::MAX {
                return Err(error("lpss-reset", "LPSS registers disappeared"));
            }
            if observed & LPSS_FUNCTION_RESETS == LPSS_FUNCTION_RESETS {
                released = true;
                break;
            }
            if Instant::now().as_millis() >= deadline {
                break;
            }
            Timer::after(Duration::from_millis(1)).await;
        }
        if !released {
            return Err(error("lpss-reset", "functional reset release timed out"));
        }
        let master =
            designware_i2c::initialize(&mut registers, designware_i2c::Timing::intel_lpss(bus_hz))
                .await?;
        Ok(Self {
            claim,
            registers,
            master,
        })
    }

    pub(crate) fn device(&self) -> PciDevice {
        self.claim.device
    }

    pub(crate) fn fifo_depths(&self) -> (u16, u16) {
        (self.master.tx_depth, self.master.rx_depth)
    }

    pub(crate) async fn transfer(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Error> {
        self.master
            .transfer(&mut self.registers, u16::from(address), write, read)
            .await
            .map_err(Into::into)
    }

    pub(crate) fn target(&mut self, address: u8) -> Target<'_> {
        Target {
            controller: self,
            address,
        }
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), Error> {
        self.master
            .shutdown(&mut self.registers)
            .await
            .map_err(Into::into)
    }
}

pub(crate) struct Target<'a> {
    controller: &'a mut Controller,
    address: u8,
}

impl i2c_hid::Transport for Target<'_> {
    type Error = Error;
    async fn transfer(&mut self, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error> {
        self.controller.transfer(self.address, write, read).await
    }
}

/// Find PM through the standard capability chain instead of assuming 0x80.
/// A visited-bit mask makes malformed firmware chains bounded and detectable.
fn power_capability(
    status: u16,
    first: u8,
    mut read: impl FnMut(u16) -> u8,
) -> Result<Option<u16>, Error> {
    if status & PCI_STATUS_CAP_LIST == 0 {
        return Ok(None);
    }
    let mut position = u16::from(first & !3);
    let mut visited = 0u64;
    for _ in 0..48 {
        if position == 0 {
            return Ok(None);
        }
        if !(0x40..0x100).contains(&position) {
            return Err(error("power", "invalid PCI capability pointer"));
        }
        let bit = 1u64 << (position / 4);
        if visited & bit != 0 {
            return Err(error("power", "cyclic PCI capability chain"));
        }
        visited |= bit;
        let id = read(position);
        if id == u8::MAX {
            return Err(error("power", "PCI capability unavailable"));
        }
        if id == PM_CAP_ID {
            if position > 0xf8 {
                return Err(error("power", "truncated power capability"));
            }
            return Ok(Some(position));
        }
        position = u16::from(read(position + 1) & !3);
    }
    Err(error("power", "PCI capability chain exceeded bound"))
}

fn d0_value(pmcsr: u16) -> u16 {
    // PME status is write-one-to-clear. Preserve it by writing zero, while
    // retaining unrelated controls such as PME enable and data selection.
    pmcsr & !(PMCSR_STATE | PMCSR_PME_STATUS)
}

fn configure_pci_command(device: PciDevice, memory: bool) -> Result<(), Error> {
    let command = pci::config_read_u16(device.bus, device.slot, device.function, 0x04);
    if command == u16::MAX {
        return Err(error("pci-command", "PCI configuration unavailable"));
    }
    let required = PCI_COMMAND_INTX_DISABLE | if memory { PCI_COMMAND_MEMORY } else { 0 };
    let command = (command & !(PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER)) | required;
    pci::config_write_u16(device.bus, device.slot, device.function, 0x04, command);
    let observed = pci::config_read_u16(device.bus, device.slot, device.function, 0x04);
    if observed == u16::MAX
        || observed & (PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER | PCI_COMMAND_INTX_DISABLE)
            != required
    {
        return Err(error(
            "pci-command",
            "memory-only polling configuration rejected",
        ));
    }
    Ok(())
}

async fn power_on(device: PciDevice) -> Result<(), Error> {
    let status = pci::config_read_u16(device.bus, device.slot, device.function, 0x06);
    if status == u16::MAX {
        return Err(error("power", "PCI function unavailable"));
    }
    let first = pci::config_read_u8(device.bus, device.slot, device.function, 0x34);
    let capability = power_capability(status, first, |offset| {
        pci::config_read_u8(device.bus, device.slot, device.function, offset)
    })?
    .ok_or(error("power", "PCI power capability missing"))?;
    let offset = capability + 4;
    let pmcsr = pci::config_read_u16(device.bus, device.slot, device.function, offset);
    if pmcsr == u16::MAX {
        return Err(error("power", "PMCSR unavailable"));
    }
    let previous_state = pmcsr & PMCSR_STATE;
    if previous_state != 0 {
        pci::config_write_u16(
            device.bus,
            device.slot,
            device.function,
            offset,
            d0_value(pmcsr),
        );
        // D3hot may internally reset the function. Do not access BAR MMIO until
        // the mandatory recovery interval and D0 readback have completed.
        Timer::after(Duration::from_millis(if previous_state == 3 {
            10
        } else {
            1
        }))
        .await;
    }
    let observed = pci::config_read_u16(device.bus, device.slot, device.function, offset);
    if observed == u16::MAX || observed & PMCSR_STATE != 0 {
        return Err(error("power", "PCI function did not enter D0"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_capability_can_follow_other_capabilities_at_arbitrary_offsets() {
        let mut configuration = [0u8; 256];
        configuration[0x50] = 5;
        configuration[0x51] = 0xa3; // low reserved pointer bits are ignored
        configuration[0xa0] = 0x10;
        configuration[0xa1] = 0x70;
        configuration[0x70] = 1;
        assert_eq!(
            power_capability(PCI_STATUS_CAP_LIST, 0x51, |offset| configuration
                [usize::from(offset)]),
            Ok(Some(0x70))
        );
    }

    #[test]
    fn malformed_or_truncated_capability_chains_never_escape_config_space() {
        let mut reads = 0;
        assert_eq!(
            power_capability(PCI_STATUS_CAP_LIST, 0x40, |offset| {
                reads += 1;
                if offset == 0x40 { 5 } else { 0x40 }
            }),
            Err(error("power", "cyclic PCI capability chain"))
        );
        assert_eq!(reads, 2);
        assert_eq!(
            power_capability(PCI_STATUS_CAP_LIST, 0x20, |_| panic!(
                "must reject before access"
            )),
            Err(error("power", "invalid PCI capability pointer"))
        );
        assert_eq!(
            power_capability(PCI_STATUS_CAP_LIST, 0xfc, |_| 1),
            Err(error("power", "truncated power capability"))
        );
        assert_eq!(
            power_capability(0, 0x40, |_| panic!("capability list absent")),
            Ok(None)
        );
    }

    #[test]
    fn d0_transition_preserves_controls_without_acknowledging_pme_status() {
        // D3hot + PME status + PME enabled + data-select controls. The state
        // changes to D0, and the W1C status is not acknowledged accidentally.
        assert_eq!(d0_value(0x9b03), 0x1b00);
        assert_eq!(d0_value(0x0100), 0x0100);
        assert_eq!(d0_value(0x0002), 0x0000);
    }
}
