//! Bounded decoding of the ACPI resources needed by HID-over-I2C devices.
//!
//! This module does not execute AML or touch hardware. Its input must be the
//! evaluated `_CRS` buffer, rather than bytes guessed out of a DSDT/SSDT. The
//! descriptor layouts follow ACPICA's `amlresrc.h` and `rsserial.c`.

extern crate alloc;

use alloc::{string::String, vec::Vec};

const MAX_RESOURCE_BYTES: usize = 16 * 1024;
const MAX_DESCRIPTORS: usize = 128;
const MAX_PINS: usize = 64;
const MAX_PATH_BYTES: usize = 256;

/// HID-over-I2C `_DSM` GUID, in the ACPI UUID buffer byte order. Invoke with
/// revision 1, function 1, and an empty package; the integer result is the HID
/// descriptor register address. It is not the I2C slave address.
pub(crate) const HID_I2C_DSM_UUID_LE: [u8; 16] = [
    0xf7, 0xf6, 0xdf, 0x3c, 0x67, 0x42, 0x55, 0x45, 0xad, 0x05, 0xb3, 0x0a, 0x3d, 0x89, 0x38, 0xde,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Polarity {
    ActiveHigh,
    ActiveLow,
    ActiveBoth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Trigger {
    Level,
    Edge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct I2cConnection {
    pub slave_address: u8,
    pub speed_hz: u32,
    pub controller: String,
    pub source_index: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpioInterrupt {
    pub pins: Vec<u16>,
    pub controller: String,
    pub source_index: u8,
    pub polarity: Polarity,
    pub trigger: Trigger,
    pub shared: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Interrupt {
    pub vectors: Vec<u32>,
    pub polarity: Polarity,
    pub trigger: Trigger,
    pub shared: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub writable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Resources {
    pub i2c: Vec<I2cConnection>,
    pub gpio_interrupts: Vec<GpioInterrupt>,
    pub irqs: Vec<Interrupt>,
    pub memory: Vec<MemoryRegion>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    TooLarge,
    Truncated,
    BadLength,
    UnsupportedRevision,
    UnsupportedAddressing,
    UnsupportedSlaveMode,
    InvalidAddress,
    InvalidSpeed,
    InvalidPath,
    InvalidOffset,
    InvalidPolarity,
    InvalidMemoryRange,
    MissingEndTag,
    TrailingData,
    BadChecksum,
}

fn word(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let value = bytes.get(offset..offset + 2).ok_or(Error::Truncated)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn dword(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let value = bytes.get(offset..offset + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn path(bytes: &[u8]) -> Result<String, Error> {
    let length = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(Error::InvalidPath)?;
    if length == 0 || length > MAX_PATH_BYTES {
        return Err(Error::InvalidPath);
    }
    let bytes = &bytes[..length];
    if !bytes.iter().all(|&byte| {
        byte.is_ascii_uppercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'\\' | b'^' | b'.')
    }) {
        return Err(Error::InvalidPath);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| Error::InvalidPath)
}

fn i2c_descriptor(bytes: &[u8]) -> Result<Option<I2cConnection>, Error> {
    // Header + nine-byte serial preamble + six-byte I2C payload.
    if bytes.len() < 12 {
        return Err(Error::BadLength);
    }
    if bytes[5] != 1 {
        return Ok(None); // Another GenericSerialBus type, e.g. SPI or UART.
    }
    if !matches!(bytes[3], 1 | 2) || bytes[9] != 1 {
        return Err(Error::UnsupportedRevision);
    }
    let type_length = usize::from(word(bytes, 10)?);
    if type_length < 6 {
        return Err(Error::BadLength);
    }
    let source_offset = 12usize.checked_add(type_length).ok_or(Error::BadLength)?;
    if source_offset >= bytes.len() || bytes.len() < 18 {
        return Err(Error::BadLength);
    }
    if bytes[6] & 1 != 0 {
        return Err(Error::UnsupportedSlaveMode);
    }
    if word(bytes, 7)? & 1 != 0 {
        return Err(Error::UnsupportedAddressing); // The first transport supports 7-bit targets.
    }
    let slave = word(bytes, 16)?;
    if !(0x08..=0x77).contains(&slave) {
        return Err(Error::InvalidAddress); // Exclude reserved/general-call addresses too.
    }
    let speed_hz = dword(bytes, 12)?;
    if speed_hz == 0 {
        return Err(Error::InvalidSpeed);
    }
    Ok(Some(I2cConnection {
        slave_address: slave as u8,
        speed_hz,
        controller: path(&bytes[source_offset..])?,
        source_index: bytes[4],
    }))
}

fn gpio_descriptor(bytes: &[u8]) -> Result<Option<GpioInterrupt>, Error> {
    if bytes.len() < 23 {
        return Err(Error::BadLength);
    }
    if bytes[3] != 1 {
        return Err(Error::UnsupportedRevision);
    }
    if bytes[4] != 0 {
        return Ok(None); // GpioIo is not the device interrupt connection.
    }
    let flags = word(bytes, 7)?;
    let polarity = match (flags >> 1) & 3 {
        0 => Polarity::ActiveHigh,
        1 => Polarity::ActiveLow,
        2 => Polarity::ActiveBoth,
        _ => return Err(Error::InvalidPolarity),
    };
    let pin_offset = usize::from(word(bytes, 14)?);
    let source_offset = usize::from(word(bytes, 17)?);
    let vendor_offset = usize::from(word(bytes, 19)?);
    let vendor_length = usize::from(word(bytes, 21)?);
    if pin_offset < 23
        || pin_offset >= source_offset
        || source_offset >= bytes.len()
        || (source_offset - pin_offset) % 2 != 0
    {
        return Err(Error::InvalidOffset);
    }
    let pin_count = (source_offset - pin_offset) / 2;
    if pin_count > MAX_PINS {
        return Err(Error::TooLarge);
    }
    let source_end = if vendor_offset == 0 && vendor_length == 0 {
        bytes.len()
    } else {
        let vendor_end = vendor_offset
            .checked_add(vendor_length)
            .ok_or(Error::InvalidOffset)?;
        if vendor_offset <= source_offset || vendor_end > bytes.len() {
            return Err(Error::InvalidOffset);
        }
        vendor_offset
    };
    let mut pins = Vec::with_capacity(pin_count);
    for offset in (pin_offset..source_offset).step_by(2) {
        pins.push(word(bytes, offset)?);
    }
    Ok(Some(GpioInterrupt {
        pins,
        controller: path(&bytes[source_offset..source_end])?,
        source_index: bytes[16],
        polarity,
        trigger: if flags & 1 != 0 {
            Trigger::Edge
        } else {
            Trigger::Level
        },
        shared: flags & 8 != 0,
    }))
}

fn extended_irq(bytes: &[u8]) -> Result<Interrupt, Error> {
    if bytes.len() < 5 {
        return Err(Error::BadLength);
    }
    let count = usize::from(bytes[4]);
    if count == 0 || count > MAX_PINS {
        return Err(Error::BadLength);
    }
    if bytes.len() < 5 + count * 4 {
        return Err(Error::BadLength);
    }
    let mut vectors = Vec::with_capacity(count);
    for offset in (5..5 + count * 4).step_by(4) {
        vectors.push(dword(bytes, offset)?);
    }
    Ok(Interrupt {
        vectors,
        polarity: if bytes[3] & 4 != 0 {
            Polarity::ActiveLow
        } else {
            Polarity::ActiveHigh
        },
        trigger: if bytes[3] & 2 != 0 {
            Trigger::Edge
        } else {
            Trigger::Level
        },
        shared: bytes[3] & 8 != 0,
    })
}

/// Parse an evaluated resource template. Unknown descriptor types are skipped
/// by their checked lengths; malformed supported descriptors are errors.
pub(crate) fn parse_resources(bytes: &[u8]) -> Result<Resources, Error> {
    if bytes.len() > MAX_RESOURCE_BYTES {
        return Err(Error::TooLarge);
    }
    let mut resources = Resources::default();
    let mut offset = 0usize;
    for _ in 0..MAX_DESCRIPTORS {
        let tag = *bytes.get(offset).ok_or(Error::MissingEndTag)?;
        let length = if tag & 0x80 != 0 {
            usize::from(word(bytes, offset + 1)?) + 3
        } else {
            usize::from(tag & 7) + 1
        };
        let end = offset.checked_add(length).ok_or(Error::BadLength)?;
        let descriptor = bytes.get(offset..end).ok_or(Error::Truncated)?;
        if tag & 0xf8 == 0x78 {
            if length != 2 {
                return Err(Error::BadLength);
            }
            // Zero checksum explicitly means no checksum was generated.
            if descriptor[1] != 0 && bytes[..end].iter().fold(0u8, |a, b| a.wrapping_add(*b)) != 0 {
                return Err(Error::BadChecksum);
            }
            if bytes[end..].iter().any(|&byte| byte != 0) {
                return Err(Error::TrailingData);
            }
            return Ok(resources);
        }
        match tag {
            0x8e => {
                if let Some(connection) = i2c_descriptor(descriptor)? {
                    resources.i2c.push(connection);
                }
            }
            0x8c => {
                if let Some(interrupt) = gpio_descriptor(descriptor)? {
                    resources.gpio_interrupts.push(interrupt);
                }
            }
            0x89 => resources.irqs.push(extended_irq(descriptor)?),
            0x86 => {
                if descriptor.len() != 12 {
                    return Err(Error::BadLength);
                }
                let base = u64::from(dword(descriptor, 4)?);
                let length = u64::from(dword(descriptor, 8)?);
                if length == 0 || base + length > 0x1_0000_0000 {
                    return Err(Error::InvalidMemoryRange);
                }
                resources.memory.push(MemoryRegion {
                    base,
                    length,
                    writable: descriptor[3] & 1 != 0,
                });
            }
            0x22 | 0x23 => {
                let mask = word(descriptor, 1)?;
                let flags = if tag == 0x23 { descriptor[3] } else { 1 };
                let vectors = (0..16u32).filter(|&bit| mask & (1 << bit) != 0).collect();
                resources.irqs.push(Interrupt {
                    vectors,
                    polarity: if flags & 8 != 0 {
                        Polarity::ActiveLow
                    } else {
                        Polarity::ActiveHigh
                    },
                    trigger: if flags & 1 != 0 {
                        Trigger::Edge
                    } else {
                        Trigger::Level
                    },
                    shared: flags & 0x10 != 0,
                });
            }
            _ => {}
        }
        offset = end;
    }
    Err(Error::TooLarge)
}

/// A GPIO number from INT34C5/INTC1055 is not a dense silicon pin index. These
/// Tiger Lake-LP mappings follow `pinctrl-tigerlake.c`'s GPP gpio_base ranges.
/// `community` indexes the four controller memory resources; `pad_index` is
/// relative to that community. Read its PADBAR to locate PADCFG0. This helper
/// deliberately performs no GPIO configuration writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpioPad {
    pub community: usize,
    pub pad_index: u16,
}

pub(crate) fn tigerlake_lp_gpio_pad(gpio: u16) -> Option<GpioPad> {
    // (GPIO base, silicon base, count, memory-resource index, community base).
    const GROUPS: [(u16, u16, u16, usize, u16); 12] = [
        (0, 0, 26, 0, 0),
        (32, 26, 16, 0, 0),
        (64, 42, 25, 0, 0),
        (96, 67, 8, 1, 67),
        (128, 75, 24, 1, 67),
        (160, 99, 21, 1, 67),
        (192, 120, 24, 1, 67),
        (224, 144, 27, 1, 67),
        (256, 171, 24, 2, 171),
        (288, 195, 25, 2, 171),
        (320, 226, 25, 2, 171),
        (352, 260, 8, 3, 260),
    ];
    GROUPS.iter().find_map(
        |&(gpio_base, silicon_base, count, community, community_base)| {
            let relative = gpio.checked_sub(gpio_base)?;
            (relative < count).then_some(GpioPad {
                community,
                pad_index: silicon_base - community_base + relative,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn large(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![tag];
        bytes.extend_from_slice(&(body.len() as u16).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    fn finish(mut bytes: Vec<u8>) -> Vec<u8> {
        bytes.extend_from_slice(&[0x79, 0]);
        bytes
    }

    fn i2c(vendor: &[u8]) -> Vec<u8> {
        let mut bytes = vec![2, 0, 1, 2, 0, 0, 1];
        bytes.extend_from_slice(&(6u16 + vendor.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&400_000u32.to_le_bytes());
        bytes.extend_from_slice(&0x2cu16.to_le_bytes());
        bytes.extend_from_slice(vendor);
        bytes.extend_from_slice(b"\\_SB.PC00.I2C0\0");
        large(0x8e, &bytes)
    }

    fn gpio() -> Vec<u8> {
        let source = b"\\_SB.PC00.GPI0\0";
        let mut bytes = vec![0; 25 + source.len()];
        bytes[0] = 0x8c;
        let length = (bytes.len() - 3) as u16;
        bytes[1..3].copy_from_slice(&length.to_le_bytes());
        bytes[3] = 1;
        bytes[5] = 1;
        bytes[7] = 2 | 8;
        bytes[14..16].copy_from_slice(&23u16.to_le_bytes());
        bytes[17..19].copy_from_slice(&25u16.to_le_bytes());
        bytes[23..25].copy_from_slice(&145u16.to_le_bytes());
        bytes[25..].copy_from_slice(source);
        bytes
    }

    #[test]
    fn parses_i2c_serialbus_v2_and_gpio_interrupt() {
        let mut bytes = i2c(&[]);
        bytes.extend(gpio());
        let resources = parse_resources(&finish(bytes)).unwrap();
        assert_eq!(
            resources.i2c[0],
            I2cConnection {
                slave_address: 0x2c,
                speed_hz: 400_000,
                controller: String::from("\\_SB.PC00.I2C0"),
                source_index: 0,
            }
        );
        assert_eq!(
            resources.gpio_interrupts[0],
            GpioInterrupt {
                pins: vec![145],
                controller: String::from("\\_SB.PC00.GPI0"),
                source_index: 0,
                polarity: Polarity::ActiveLow,
                trigger: Trigger::Level,
                shared: true,
            }
        );
    }

    #[test]
    fn serialbus_source_follows_declared_vendor_length() {
        let resources = parse_resources(&finish(i2c(&[0xaa, 0xbb, 0xcc]))).unwrap();
        assert_eq!(resources.i2c[0].controller, "\\_SB.PC00.I2C0");
    }

    #[test]
    fn rejects_ten_bit_slave_mode_and_reserved_addresses() {
        for (offset, value, expected) in [
            (7, 1, Error::UnsupportedAddressing),
            (6, 3, Error::UnsupportedSlaveMode),
            (16, 0, Error::InvalidAddress),
            (16, 0x78, Error::InvalidAddress),
        ] {
            let mut bytes = i2c(&[]);
            bytes[offset] = value;
            assert_eq!(parse_resources(&finish(bytes)), Err(expected));
        }
    }

    #[test]
    fn malformed_lengths_and_paths_are_rejected_without_panics() {
        for length in 0..i2c(&[]).len() {
            assert!(parse_resources(&i2c(&[])[..length]).is_err());
        }
        let mut bytes = i2c(&[]);
        *bytes.last_mut().unwrap() = b'X';
        assert_eq!(parse_resources(&finish(bytes)), Err(Error::InvalidPath));
        let mut bytes = i2c(&[]);
        bytes[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(parse_resources(&finish(bytes)), Err(Error::BadLength));
    }

    #[test]
    fn gpio_offsets_cannot_escape_the_descriptor_or_overlap_header() {
        for (offset, value) in [(14, 4), (14, 24), (17, 500), (19, 1)] {
            let mut bytes = gpio();
            bytes[offset..offset + 2].copy_from_slice(&u16::to_le_bytes(value));
            assert_eq!(parse_resources(&finish(bytes)), Err(Error::InvalidOffset));
        }
    }

    #[test]
    fn fixed_memory_and_extended_irq_are_retained_in_resource_order() {
        let mut bytes = large(0x86, &[1, 0, 0, 0x6a, 0xfd, 0, 0, 1, 0]);
        bytes.extend(large(0x89, &[0x0d, 1, 0x2b, 0, 0, 0]));
        let resources = parse_resources(&finish(bytes)).unwrap();
        assert_eq!(
            resources.memory,
            [MemoryRegion {
                base: 0xfd6a_0000,
                length: 0x10000,
                writable: true
            }]
        );
        assert_eq!(
            resources.irqs,
            [Interrupt {
                vectors: vec![43],
                polarity: Polarity::ActiveLow,
                trigger: Trigger::Level,
                shared: true
            }]
        );
    }

    #[test]
    fn unknown_resources_are_skipped_and_end_tag_checks_are_bounded() {
        let mut bytes = large(0x84, &[1, 2, 3]);
        bytes.extend(i2c(&[]));
        assert_eq!(parse_resources(&finish(bytes)).unwrap().i2c.len(), 1);
        assert_eq!(parse_resources(&[0x79, 1]), Err(Error::BadChecksum));
        assert_eq!(parse_resources(&[0x79, 0, 1]), Err(Error::TrailingData));
        assert_eq!(
            parse_resources(&[0x79, 0, 0, 0]).unwrap(),
            Resources::default()
        );
        assert_eq!(
            parse_resources(&vec![0; MAX_RESOURCE_BYTES + 1]),
            Err(Error::TooLarge)
        );
    }

    #[test]
    fn fixed_memory_cannot_wrap_its_32_bit_address_space() {
        for (base, length) in [(0u32, 0u32), (0xffff_f000, 0x2000)] {
            let mut body = vec![1];
            body.extend(base.to_le_bytes());
            body.extend(length.to_le_bytes());
            assert_eq!(
                parse_resources(&finish(large(0x86, &body))),
                Err(Error::InvalidMemoryRange)
            );
        }
    }

    #[test]
    fn truncated_gpio_and_irq_descriptors_never_escape_bounds() {
        for descriptor in [gpio(), large(0x89, &[0x0d, 1, 0x2b, 0, 0, 0])] {
            for length in 0..descriptor.len() {
                assert!(parse_resources(&descriptor[..length]).is_err());
            }
        }
        let mut bytes = gpio();
        bytes[7] = 6; // Polarity encoding 3 is reserved.
        assert_eq!(parse_resources(&finish(bytes)), Err(Error::InvalidPolarity));
    }

    #[test]
    fn gpio_number_mapping_respects_gaps_and_community_bases() {
        assert_eq!(
            tigerlake_lp_gpio_pad(0),
            Some(GpioPad {
                community: 0,
                pad_index: 0
            })
        );
        assert_eq!(
            tigerlake_lp_gpio_pad(128),
            Some(GpioPad {
                community: 1,
                pad_index: 8
            })
        );
        assert_eq!(
            tigerlake_lp_gpio_pad(145),
            Some(GpioPad {
                community: 1,
                pad_index: 25
            })
        );
        assert_eq!(
            tigerlake_lp_gpio_pad(320),
            Some(GpioPad {
                community: 2,
                pad_index: 55
            })
        );
        assert_eq!(
            tigerlake_lp_gpio_pad(352),
            Some(GpioPad {
                community: 3,
                pad_index: 0
            })
        );
        for gap in [
            26, 31, 48, 63, 89, 127, 152, 191, 216, 255, 280, 319, 345, 360,
        ] {
            assert_eq!(tigerlake_lp_gpio_pad(gap), None, "GPIO {gap}");
        }
    }
}
