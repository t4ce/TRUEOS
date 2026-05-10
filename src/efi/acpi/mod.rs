use core::ptr::NonNull;

use crate::wait;
use acpi::{AcpiTables, Handler as AcpiHandler, PciAddress, PhysicalMapping};
use embassy_time_driver::TICK_HZ;
use spin::Once;

use crate::{
    inb, inl, inw, limine, outb, outl, outw,
    pci::{self, mmio},
};

pub mod bgrt;
pub mod dbg;
pub mod facp;
pub mod hpet;
pub mod madt;
pub mod sleep;
// pub mod fpdt;
// pub mod ssdt;
// pub mod tpm2;
// pub mod dmar;

pub(crate) const SDT_HEADER_LEN: usize = 36;

static ACPI_TABLES: Once<Option<AcpiTables<AcpiIdentityHandler>>> = Once::new();

pub(crate) fn ensure_tables() -> Option<&'static AcpiTables<AcpiIdentityHandler>> {
    ACPI_TABLES.call_once(|| {
        let Some(rsdp_raw) = limine::rsdp_address() else {
            crate::log_trace!("ACPI RSDP MISSING\n");
            return None;
        };
        let Some(rsdp) = limine::try_as_phys_addr(rsdp_raw) else {
            crate::log_trace!("ACPI RSDP 0x{:X} could not be normalized to physical\n", rsdp_raw);
            return None;
        };

        let handler = AcpiIdentityHandler;
        match unsafe { AcpiTables::from_rsdp(handler, rsdp as usize) } {
            Ok(tables) => {
                let mut count = 0usize;
                let mut ssdt_count = 0usize;
                for (_phys, header) in tables.table_headers() {
                    count += 1;
                    let _table_len =
                        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(header.length)) };
                    if header.signature.as_str() == "SSDT" {
                        ssdt_count += 1;
                        continue;
                    }
                }
                if crate::logflag::BOOT_INFO_LOGS && ssdt_count != 0 {
                    crate::log_trace!("ACPI TABLE SSDT count={} (see ssdt::log_once)\n", ssdt_count);
                }
                if crate::logflag::BOOT_INFO_LOGS {
                    crate::log_trace!(
                        "ACPI RSDP raw=0x{:X} phys=0x{:X} tables={}\n",
                        rsdp_raw,
                        rsdp,
                        count
                    );
                }
                Some(tables)
            }
            Err(err) => {
                crate::log_trace!("ACPI RSDP raw=0x{:X} phys=0x{:X} ERROR {:?}\n", rsdp_raw, rsdp, err);
                None
            }
        }
    });

    ACPI_TABLES.get().and_then(|tables| tables.as_ref())
}

pub(crate) fn map_table_bytes(phys: usize) -> Option<&'static [u8]> {
    let phys = limine::try_as_phys_addr(phys as u64).unwrap_or(phys as u64);
    let hdr_ptr = mmio::map_mmio_region_exact(phys, SDT_HEADER_LEN).ok()?;
    let hdr = unsafe { core::slice::from_raw_parts(hdr_ptr.as_ptr(), SDT_HEADER_LEN) };
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    if len < SDT_HEADER_LEN {
        return None;
    }
    let ptr = mmio::map_mmio_region_exact(phys, len).ok()?;
    Some(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), len) })
}

#[derive(Clone, Copy)]
pub(crate) struct AcpiIdentityHandler;

impl AcpiIdentityHandler {
    #[inline(always)]
    fn map_ptr(&self, phys_addr: usize, size: usize) -> NonNull<u8> {
        let normalized = limine::try_as_phys_addr(phys_addr as u64).unwrap_or(phys_addr as u64);
        mmio::map_mmio_region(normalized, size).unwrap_or_else(|err| {
            panic!(
                "ACPI map raw={:x} phys={:x} size {} failed: {:?}",
                phys_addr, normalized, size, err
            )
        })
    }

    #[inline(always)]
    unsafe fn read_phys<T: Copy>(&self, phys_addr: usize) -> T {
        let ptr = self.map_ptr(phys_addr, core::mem::size_of::<T>());
        core::ptr::read_volatile(ptr.as_ptr() as *const T)
    }

    #[inline(always)]
    unsafe fn write_phys<T>(&self, phys_addr: usize, value: T) {
        let ptr = self.map_ptr(phys_addr, core::mem::size_of::<T>());
        core::ptr::write_volatile(ptr.as_ptr() as *mut T, value);
    }

    fn split_pci_address(address: PciAddress) -> (u8, u8, u8) {
        let segment = address.segment();
        if segment != 0 {
            panic!("PCI segment {} unsupported by legacy config access", segment);
        }
        (address.bus(), address.device(), address.function())
    }
}

impl AcpiHandler for AcpiIdentityHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let mapped = self.map_ptr(physical_address, size);
        let ptr = NonNull::new(mapped.as_ptr() as *mut T).expect("ACPI mapping null");
        PhysicalMapping {
            physical_start: physical_address,
            virtual_start: ptr,
            region_length: size,
            mapped_length: size,
            handler: *self,
        }
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}

    fn read_u8(&self, address: usize) -> u8 {
        unsafe { self.read_phys::<u8>(address) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        unsafe { self.read_phys::<u16>(address) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        unsafe { self.read_phys::<u32>(address) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        unsafe { self.read_phys::<u64>(address) }
    }

    fn write_u8(&self, address: usize, value: u8) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u16(&self, address: usize, value: u16) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u32(&self, address: usize, value: u32) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u64(&self, address: usize, value: u64) {
        unsafe { self.write_phys(address, value) };
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        unsafe { inb(port) }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        unsafe { inw(port) }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        unsafe { inl(port) }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        unsafe { outb(port, value) };
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        unsafe { outw(port, value) };
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        unsafe { outl(port, value) };
    }

    fn read_pci_u8(&self, address: PciAddress, offset: u16) -> u8 {
        let (bus, slot, function) = Self::split_pci_address(address);
        pci::config_read_u8(bus, slot, function, offset)
    }

    fn read_pci_u16(&self, address: PciAddress, offset: u16) -> u16 {
        let (bus, slot, function) = Self::split_pci_address(address);
        pci::config_read_u16(bus, slot, function, offset)
    }

    fn read_pci_u32(&self, address: PciAddress, offset: u16) -> u32 {
        let (bus, slot, function) = Self::split_pci_address(address);
        pci::config_read_u32(bus, slot, function, offset)
    }

    fn write_pci_u8(&self, address: PciAddress, offset: u16, value: u8) {
        let (bus, slot, function) = Self::split_pci_address(address);
        pci::config_write_u8(bus, slot, function, offset, value);
    }

    fn write_pci_u16(&self, address: PciAddress, offset: u16, value: u16) {
        let (bus, slot, function) = Self::split_pci_address(address);
        pci::config_write_u16(bus, slot, function, offset, value);
    }

    fn write_pci_u32(&self, address: PciAddress, offset: u16, value: u32) {
        let (bus, slot, function) = Self::split_pci_address(address);
        pci::config_write_u32(bus, slot, function, offset, value);
    }

    fn nanos_since_boot(&self) -> u64 {
        let ticks = embassy_time_driver::now();
        let hz = u128::from(TICK_HZ).max(1);
        ((ticks as u128) * 1_000_000_000u128 / hz) as u64
    }

    fn stall(&self, microseconds: u64) {
        let target = self
            .nanos_since_boot()
            .saturating_add(microseconds.saturating_mul(1_000));
        while self.nanos_since_boot() < target {
            wait::spin_step();
        }
    }

    fn sleep(&self, milliseconds: u64) {
        self.stall(milliseconds.saturating_mul(1_000));
    }

    fn create_mutex(&self) -> acpi::Handle {
        unsafe { core::mem::transmute(0u32) }
    }

    fn acquire(&self, _handle: acpi::Handle, _timeout: u16) -> Result<(), acpi::aml::AmlError> {
        Ok(())
    }

    fn release(&self, _handle: acpi::Handle) {}
}
