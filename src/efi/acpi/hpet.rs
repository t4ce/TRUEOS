use core::ptr::NonNull;

use acpi::sdt::hpet::HpetInfo;
use spin::Once;

use crate::pci::mmio::{self, MapError as MmioMapError};

use super::ensure_tables;

const HPET_MMIO_SIZE: usize = 0x1000;
const CAPABILITIES_OFFSET: usize = 0x000;
const CONFIG_OFFSET: usize = 0x010;
const MAIN_COUNTER_OFFSET: usize = 0x0F0;
const FEMTOSECONDS_PER_SECOND: u128 = 1_000_000_000_000_000;

static HPET_INSTANCE: Once<Option<Hpet>> = Once::new();

#[derive(Debug)]
pub struct Hpet {
    regs: NonNull<u8>,
    info: HpetInfo,
    period_fs: u32,
    frequency_hz: u64,
}

unsafe impl Send for Hpet {}
unsafe impl Sync for Hpet {}

impl Hpet {
    unsafe fn read_reg64(&self, offset: usize) -> u64 {
        core::ptr::read_volatile(self.regs.as_ptr().add(offset) as *const u64)
    }

    unsafe fn write_reg64(&self, offset: usize, value: u64) {
        core::ptr::write_volatile(self.regs.as_ptr().add(offset) as *mut u64, value);
    }

    unsafe fn configure(&self, enable: bool, legacy: bool) {
        let mut config = self.read_reg64(CONFIG_OFFSET);
        config &= !0b11;
        if legacy {
            config |= 1 << 1;
        }
        if enable {
            config |= 1;
        }
        self.write_reg64(CONFIG_OFFSET, config);
    }

    #[inline]
    fn counter_mask(&self) -> u64 {
        if self.info.main_counter_is_64bits {
            u64::MAX
        } else {
            u32::MAX as u64
        }
    }

    #[inline]
    pub fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }

    #[inline]
    pub fn main_counter(&self) -> u64 {
        let raw = unsafe { self.read_reg64(MAIN_COUNTER_OFFSET) };
        raw & self.counter_mask()
    }

    #[inline]
    pub fn counter_delta(&self, start: u64, end: u64) -> u64 {
        let mask = self.counter_mask();
        end.wrapping_sub(start) & mask
    }
}

pub fn ensure() -> Option<&'static Hpet> {
    HPET_INSTANCE.call_once(init_hpet);
    HPET_INSTANCE.get().and_then(|hpet| hpet.as_ref())
}

fn init_hpet() -> Option<Hpet> {
    let tables = ensure_tables()?;
    let info = match HpetInfo::new(tables) {
        Ok(info) => info,
        Err(err) => {
            crate::log!("HPET table error: {:?}\n", err);
            return None;
        }
    };

    let regs = match map_hpet_regs(info.base_address) {
        Ok(regs) => regs,
        Err(err) => {
            crate::log!("HPET map failed @0x{:X}: {:?}\n", info.base_address, err);
            return None;
        }
    };

    let mut hpet = Hpet {
        regs,
        info,
        period_fs: 0,
        frequency_hz: 0,
    };

    let cap = unsafe { hpet.read_reg64(CAPABILITIES_OFFSET) };
    let clk_period_fs = ((cap >> 32) & 0xFFFF_FFFF) as u32;
    if clk_period_fs == 0 {
        crate::log!("HPET invalid period (cap=0x{:X})\n", cap);
        return None;
    }

    let freq_hz = (FEMTOSECONDS_PER_SECOND / u128::from(clk_period_fs)) as u64;

    hpet.period_fs = clk_period_fs;
    hpet.frequency_hz = freq_hz;

    unsafe {
        hpet.configure(false, false);
        hpet.write_reg64(MAIN_COUNTER_OFFSET, 0);
        hpet.configure(true, false);
    }

    crate::log!(
        "HPET @0x{:X} freq={}Hz comps={} counter_{}bit legacy_capable={}\n",
        hpet.info.base_address,
        hpet.frequency_hz,
        hpet.info.num_comparators,
        if hpet.info.main_counter_is_64bits {
            64
        } else {
            32
        },
        hpet.info.legacy_irq_capable,
    );

    Some(hpet)
}

fn map_hpet_regs(phys_base: usize) -> Result<NonNull<u8>, MmioMapError> {
    mmio::map_mmio_region_exact(phys_base as u64, HPET_MMIO_SIZE)
}
