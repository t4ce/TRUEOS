use core::{hint, ptr::NonNull};

use acpi::{AcpiTables, Handler as AcpiHandler, PciAddress, PhysicalMapping};
use embassy_time_driver::TICK_HZ;
use spin::Once;

use crate::{
    inb, inl, inw, limine, outb, outl, outw,
    pci::{self, mmio},
};

pub mod bgrt;
pub mod dbg;
pub mod dmar;
pub mod facp;
pub mod fpdt;
pub mod hpet;
pub mod madt;
pub mod ssdt;
pub mod tpm2;

static ACPI_TABLES: Once<Option<AcpiTables<AcpiIdentityHandler>>> = Once::new();

pub(crate) fn ensure_tables() -> Option<&'static AcpiTables<AcpiIdentityHandler>> {
    ACPI_TABLES.call_once(|| {
        let Some(rsdp) = limine::rsdp_address() else {
            crate::log!("ACPI RSDP MISSING\n");
            return None;
        };

        let handler = AcpiIdentityHandler;
        match unsafe { AcpiTables::from_rsdp(handler, rsdp as usize) } {
            Ok(tables) => {
                let mut count = 0usize;
                for (phys, header) in tables.table_headers() {
                    count += 1;
                    let table_len =
                        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(header.length)) };
                    crate::log!(
                        "ACPI TABLE {} @0x{:X} len=0x{:X}\n",
                        header.signature.as_str(),
                        phys,
                        table_len
                    );
                }
                crate::log!("ACPI RSDP 0x{:X} tables={}\n", rsdp, count);
                Some(tables)
            }
            Err(err) => {
                crate::log!("ACPI RSDP 0x{:X} ERROR {:?}\n", rsdp, err);
                None
            }
        }
    });

    ACPI_TABLES.get().and_then(|tables| tables.as_ref())
}

#[derive(Clone, Copy)]
pub(crate) struct AcpiIdentityHandler;

impl AcpiIdentityHandler {
    #[inline(always)]
    fn map_ptr(&self, phys_addr: usize, size: usize) -> NonNull<u8> {
        mmio::map_mmio_region(phys_addr as u64, size).unwrap_or_else(|err| {
            panic!("ACPI map {:x} size {} failed: {:?}", phys_addr, size, err)
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
            panic!(
                "PCI segment {} unsupported by legacy config access",
                segment
            );
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
            hint::spin_loop();
        }
    }

    fn sleep(&self, milliseconds: u64) {
        self.stall(milliseconds.saturating_mul(1_000));
    }
}
