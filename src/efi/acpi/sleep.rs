use alloc::boxed::Box;
use core::ptr::read_unaligned;

use acpi::sdt::fadt::Fadt;
use aml::{AmlContext, AmlName, AmlValue, DebugVerbosity, value::Args};
use spin::Once;

use crate::{
    inb, inl, inw, outb, outl, outw,
    pci::{self, mmio},
};

use super::ensure_tables;

#[derive(Clone, Copy, Debug)]
pub struct SleepType {
    pub pm1a: u8,
    pub pm1b: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
struct SleepTypeCache {
    s1: Option<SleepType>,
    s2: Option<SleepType>,
    s3: Option<SleepType>,
    s4: Option<SleepType>,
    s5: Option<SleepType>,
}

impl SleepTypeCache {
    fn get(self, state: u8) -> Option<SleepType> {
        match state {
            1 => self.s1,
            2 => self.s2,
            3 => self.s3,
            4 => self.s4,
            5 => self.s5,
            _ => None,
        }
    }
}

static SLEEP_TYPES: Once<Option<SleepTypeCache>> = Once::new();

pub fn sleep_type_for_state(state: u8) -> Option<SleepType> {
    SLEEP_TYPES
        .call_once(resolve_sleep_types)
        .as_ref()
        .and_then(|cache| cache.get(state))
}

fn resolve_sleep_types() -> Option<SleepTypeCache> {
    let tables = ensure_tables()?;
    let mut ctx = AmlContext::new(Box::new(AmlRuntimeHandler), DebugVerbosity::None);

    let fadt = tables.find_table::<Fadt>()?;
    let fadt = unsafe { fadt.virtual_start.as_ref() };
    fadt.validate().ok()?;

    let dsdt_phys = fadt.dsdt_address().ok()?;
    if parse_definition_block(&mut ctx, dsdt_phys, "DSDT").is_err() {
        return None;
    }

    for (phys, header) in tables.table_headers() {
        if header.signature.as_str() != "SSDT" {
            continue;
        }
        if parse_definition_block(&mut ctx, phys, "SSDT").is_err() {
            crate::log!("ACPI SSDT: parse failed phys=0x{:X}\n", phys);
        }
    }

    Some(SleepTypeCache {
        s1: resolve_sx(&mut ctx, "\\_S1"),
        s2: resolve_sx(&mut ctx, "\\_S2"),
        s3: resolve_sx(&mut ctx, "\\_S3"),
        s4: resolve_sx(&mut ctx, "\\_S4"),
        s5: resolve_sx(&mut ctx, "\\_S5"),
    })
}

fn parse_definition_block(ctx: &mut AmlContext, phys: usize, label: &str) -> Result<(), ()> {
    let Some(bytes) = map_table_bytes(phys) else {
        crate::log!("ACPI {}: map failed phys=0x{:X}\n", label, phys);
        return Err(());
    };

    if bytes.len() < 36 {
        crate::log!("ACPI {}: short table len={}\n", label, bytes.len());
        return Err(());
    }

    let aml = &bytes[36..];
    if let Err(err) = ctx.parse_table(aml) {
        crate::log!("ACPI {}: AML parse error {:?}\n", label, err);
        return Err(());
    }
    Ok(())
}

fn map_table_bytes(phys: usize) -> Option<&'static [u8]> {
    let hdr_ptr = mmio::map_mmio_region_exact(phys as u64, 36).ok()?;
    let hdr = unsafe { core::slice::from_raw_parts(hdr_ptr.as_ptr(), 36) };
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    if len < 36 {
        return None;
    }
    let ptr = mmio::map_mmio_region_exact(phys as u64, len).ok()?;
    Some(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), len) })
}

fn resolve_sx(ctx: &mut AmlContext, path: &str) -> Option<SleepType> {
    let name = AmlName::from_str(path).ok()?;
    let value = ctx.invoke_method(&name, Args::default()).ok()?;
    let AmlValue::Package(elements) = value else {
        return None;
    };
    if elements.len() < 2 {
        return None;
    }

    let pm1a = elements[0]
        .as_integer(ctx)
        .ok()
        .and_then(|v| u8::try_from(v).ok())?;
    let pm1b = elements[1]
        .as_integer(ctx)
        .ok()
        .and_then(|v| u8::try_from(v).ok());
    Some(SleepType { pm1a, pm1b })
}

#[derive(Clone, Copy)]
struct AmlRuntimeHandler;

impl AmlRuntimeHandler {
    #[inline(always)]
    fn map_ptr(&self, phys_addr: usize, size: usize) -> core::ptr::NonNull<u8> {
        mmio::map_mmio_region(phys_addr as u64, size)
            .unwrap_or_else(|err| panic!("AML map {:x} size {} failed: {:?}", phys_addr, size, err))
    }

    #[inline(always)]
    unsafe fn read_phys<T: Copy>(&self, phys_addr: usize) -> T {
        let ptr = self.map_ptr(phys_addr, core::mem::size_of::<T>());
        read_unaligned(ptr.as_ptr() as *const T)
    }

    #[inline(always)]
    unsafe fn write_phys<T>(&self, phys_addr: usize, value: T) {
        let ptr = self.map_ptr(phys_addr, core::mem::size_of::<T>());
        core::ptr::write_volatile(ptr.as_ptr() as *mut T, value);
    }
}

impl aml::Handler for AmlRuntimeHandler {
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

    fn write_u8(&mut self, address: usize, value: u8) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u16(&mut self, address: usize, value: u16) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u32(&mut self, address: usize, value: u32) {
        unsafe { self.write_phys(address, value) };
    }

    fn write_u64(&mut self, address: usize, value: u64) {
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

    fn read_pci_u8(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
        if segment != 0 {
            return 0xFF;
        }
        pci::config_read_u8(bus, device, function, offset)
    }

    fn read_pci_u16(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
        if segment != 0 {
            return 0xFFFF;
        }
        pci::config_read_u16(bus, device, function, offset)
    }

    fn read_pci_u32(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
        if segment != 0 {
            return 0xFFFF_FFFF;
        }
        pci::config_read_u32(bus, device, function, offset)
    }

    fn write_pci_u8(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u8,
    ) {
        if segment != 0 {
            return;
        }
        pci::config_write_u8(bus, device, function, offset, value);
    }

    fn write_pci_u16(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u16,
    ) {
        if segment != 0 {
            return;
        }
        pci::config_write_u16(bus, device, function, offset, value);
    }

    fn write_pci_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u32,
    ) {
        if segment != 0 {
            return;
        }
        pci::config_write_u32(bus, device, function, offset, value);
    }
}
