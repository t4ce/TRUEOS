//! Read the existing Intel GPIO interrupt level without reprogramming pins.

use crate::efi::acpi::i2c_hid::{MemoryRegion, Polarity, Trigger, tigerlake_lp_gpio_pad};

pub(crate) struct GpioLine {
    register: usize,
    active_low: bool,
}

impl GpioLine {
    pub(crate) fn open(
        controller_hid: &str,
        regions: &[MemoryRegion],
        pin: u16,
        polarity: Polarity,
        trigger: Trigger,
    ) -> Result<Self, &'static str> {
        if !matches!(controller_hid, "INT34C5" | "INTC1055") {
            return Err("unsupported GPIO controller");
        }
        if trigger != Trigger::Level || polarity == Polarity::ActiveBoth {
            return Err("HID input requires a level interrupt");
        }
        let pad = tigerlake_lp_gpio_pad(pin).ok_or("GPIO pin is not exposed")?;
        let region = regions
            .get(pad.community)
            .ok_or("GPIO community is missing")?;
        let length = usize::try_from(region.length).map_err(|_| "GPIO region too large")?;
        if !(0x10..=0x10_0000).contains(&length) || region.base == 0 || region.base & 3 != 0 {
            return Err("invalid GPIO region");
        }
        let mapped = crate::pci::mmio::map_mmio_region_exact(region.base, length)
            .map_err(|_| "GPIO MMIO mapping failed")?;
        let base = mapped.as_ptr() as usize;
        let revision = unsafe { core::ptr::read_volatile(base as *const u32) };
        let padbar = unsafe { core::ptr::read_volatile((base + 0x0c) as *const u32) };
        let offset = pad_offset(revision, padbar, pad.pad_index, length)?;
        let line = Self {
            register: base + offset,
            active_low: polarity == Polarity::ActiveLow,
        };
        line.asserted()?; // Reject an unavailable/muxed/disabled input up front.
        Ok(line)
    }

    pub(crate) fn asserted(&self) -> Result<bool, &'static str> {
        let value = unsafe { core::ptr::read_volatile(self.register as *const u32) };
        input_level(value, self.active_low)
    }
}

fn pad_offset(revision: u32, padbar: u32, pad: u16, length: usize) -> Result<usize, &'static str> {
    if revision == 0 || revision == u32::MAX || padbar == u32::MAX || padbar & 3 != 0 {
        return Err("GPIO controller is unavailable");
    }
    // Intel GPIO revisions with debounce have PADCFG0/1/2 in a 16-byte slot.
    let stride = if revision >> 16 >= 0x92 { 16 } else { 8 };
    let offset = (padbar as usize)
        .checked_add(usize::from(pad) * stride)
        .ok_or("GPIO pad offset overflow")?;
    if padbar < 0x10 || offset.checked_add(4).is_none_or(|end| end > length) {
        return Err("GPIO pad is outside its community");
    }
    Ok(offset)
}

fn input_level(config: u32, active_low: bool) -> Result<bool, &'static str> {
    if config == u32::MAX {
        return Err("GPIO input is unavailable");
    }
    if config & ((0x0f << 10) | (1 << 9)) != 0 || config & (1 << 8) == 0 {
        return Err("GPIO interrupt pin is not configured as an input");
    }
    // RXINV selects interrupt polarity; GPIORXSTATE is the physical pin value.
    Ok((config & (1 << 1) != 0) != active_low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_trackpad_pin_is_active_low_without_double_inversion() {
        assert_eq!(input_level(0x8090_0102, true), Ok(false));
        assert_eq!(input_level(0x8090_0100, true), Ok(true));
        assert_eq!(input_level(0x8090_0102, false), Ok(true));
        assert_eq!(input_level(0x8090_0100, false), Ok(false));
    }

    #[test]
    fn gpio_number_is_translated_before_applying_pad_stride() {
        let pad = tigerlake_lp_gpio_pad(175).unwrap();
        assert_eq!((pad.community, pad.pad_index), (1, 47));
        assert_eq!(
            pad_offset(0x0094_0000, 0x700, pad.pad_index, 0x10000),
            Ok(0x9f0)
        );
        assert_eq!(
            pad_offset(0x0090_0000, 0x700, pad.pad_index, 0x10000),
            Ok(0x878)
        );
        assert_eq!(pad_offset(0x0091_0000, 0x700, 47, 0x10000), Ok(0x878));
        assert_eq!(pad_offset(0x0092_0000, 0x700, 47, 0x10000), Ok(0x9f0));
        assert_eq!(pad_offset(0x0093_0000, 0x700, 47, 0x10000), Ok(0x9f0));
    }

    #[test]
    fn bad_registers_and_non_input_pins_are_rejected() {
        for value in [u32::MAX, 0, 0x302, 0x502] {
            assert!(input_level(value, true).is_err());
        }
        assert!(pad_offset(u32::MAX, 0x700, 47, 0x10000).is_err());
        assert!(pad_offset(0x0094_0000, 0x701, 47, 0x10000).is_err());
        assert!(pad_offset(0x0094_0000, 0x700, 47, 0x900).is_err());
    }

    #[test]
    fn unaligned_firmware_regions_are_rejected_before_register_access() {
        let regions = [MemoryRegion { base: 0x1001, length: 0x10000, writable: true }; 2];
        assert!(matches!(
            GpioLine::open("INT34C5", &regions, 175, Polarity::ActiveLow, Trigger::Level),
            Err("invalid GPIO region")
        ));
    }
}
