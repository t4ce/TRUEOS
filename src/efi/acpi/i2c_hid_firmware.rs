//! Read-only metadata extraction for the Intel reference TPD0 AML layout.
//!
//! This intentionally supports a checked subset, not general AML execution.
//! Device/resource names identify the supported firmware layout; addresses,
//! GPIO numbers, speeds and descriptor registers are derived from its objects
//! and current declared NVS fields. Unsupported layouts fail before bus I/O.

extern crate alloc;
use super::i2c_hid::{self, MemoryRegion, Polarity, Trigger};
use alloc::{string::String, vec, vec::Vec};

const MAX_AML_BYTES: usize = 1024 * 1024;
const MAX_NVS_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct TrackpadFirmware {
    pub pci_bus: u8,
    pub pci_slot: u8,
    pub pci_function: u8,
    pub slave_address: u8,
    pub speed_hz: u32,
    pub hid_descriptor_register: u16,
    pub gpio_number: u16,
    pub gpio_polarity: Polarity,
    pub gpio_trigger: Trigger,
    pub gpio_hid: String,
    pub gpio_regions: Vec<MemoryRegion>,
}

type Result<T> = core::result::Result<T, &'static str>;

fn pkg(bytes: &[u8], offset: usize) -> Result<(usize, usize)> {
    let first = *bytes.get(offset).ok_or("AML package truncated")?;
    let following = usize::from(first >> 6);
    let mut length = usize::from(first & if following == 0 { 0x3f } else { 0x0f });
    for index in 0..following {
        length |= usize::from(
            *bytes
                .get(offset + 1 + index)
                .ok_or("AML package truncated")?,
        ) << (4 + index * 8);
    }
    Ok((length, following + 1))
}

fn name(bytes: &[u8], offset: &mut usize) -> Result<[u8; 4]> {
    // A single NameSeg, with optional root/parent prefix, is sufficient for
    // this profile. MultiNamePrefix and method references are never guessed.
    while matches!(bytes.get(*offset), Some(b'\\' | b'^')) {
        *offset += 1;
    }
    let raw = bytes
        .get(*offset..*offset + 4)
        .ok_or("AML name truncated")?;
    if !raw
        .iter()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
    {
        return Err("AML name form unsupported");
    }
    *offset += 4;
    Ok([raw[0], raw[1], raw[2], raw[3]])
}

fn scope_path(bytes: &[u8], offset: &mut usize) -> Result<Vec<[u8; 4]>> {
    // The reference DSDT emits root-level Scope (_SB.PC00.I2C0) without an
    // explicit RootChar. Accept that fully qualified spelling too, but never
    // a parent-relative scope or an abbreviated controller name.
    if bytes.get(*offset) == Some(&b'\\') {
        *offset += 1;
    }
    let count = match bytes.get(*offset) {
        Some(0x2e) => {
            *offset += 1;
            2
        }
        Some(0x2f) => {
            *offset += 1;
            let count = usize::from(*bytes.get(*offset).ok_or("AML scope path truncated")?);
            *offset += 1;
            count
        }
        _ => 1,
    };
    if count == 0 || count > 8 {
        return Err("AML scope depth unsupported");
    }
    let mut path = Vec::new();
    for _ in 0..count {
        path.push(name(bytes, offset)?);
    }
    if path.first() != Some(b"_SB_") {
        return Err("AML scope path unsupported");
    }
    Ok(path)
}

fn in_scope(all: &[u8], wanted: &[[u8; 4]], object: &[u8]) -> bool {
    let object_start = object.as_ptr() as usize - all.as_ptr() as usize;
    let object_end = object_start + object.len();
    for offset in 0..all.len() {
        if all[offset] != 0x10 {
            continue;
        }
        let start = offset + 1;
        let Ok((length, width)) = pkg(all, start) else {
            continue;
        };
        let Some(end) = start.checked_add(length).filter(|end| *end <= all.len()) else {
            continue;
        };
        let mut cursor = start + width;
        if scope_path(all, &mut cursor).ok().as_deref() == Some(wanted)
            && cursor <= object_start
            && object_end <= end
        {
            return true;
        }
    }
    false
}

fn integer(bytes: &[u8], offset: &mut usize) -> Result<u64> {
    let opcode = *bytes.get(*offset).ok_or("AML integer truncated")?;
    *offset += 1;
    let size = match opcode {
        0 => return Ok(0),
        1 => return Ok(1),
        0xff => return Ok(u64::MAX),
        0x0a => 1,
        0x0b => 2,
        0x0c => 4,
        0x0e => 8,
        _ => return Err("AML integer expression unsupported"),
    };
    let raw = bytes
        .get(*offset..*offset + size)
        .ok_or("AML integer truncated")?;
    *offset += size;
    Ok(raw.iter().enumerate().fold(0, |value, (index, byte)| {
        value | (u64::from(*byte) << (index * 8))
    }))
}

fn unique<T>(target: &mut Option<T>, value: T) -> Result<()> {
    if target.is_some() {
        return Err("AML metadata ambiguous");
    }
    *target = Some(value);
    Ok(())
}

fn named_package<'a>(bytes: &'a [u8], opcode: &[u8], wanted: &[u8; 4]) -> Result<&'a [u8]> {
    let mut found = None;
    for (offset, window) in bytes.windows(opcode.len()).enumerate() {
        if window != opcode {
            continue;
        }
        let start = offset + opcode.len();
        let Ok((length, width)) = pkg(bytes, start) else {
            continue;
        };
        if length < width {
            continue;
        }
        let Some(end) = start.checked_add(length).filter(|end| *end <= bytes.len()) else {
            continue;
        };
        let mut cursor = start + width;
        if name(bytes, &mut cursor).ok().as_ref() == Some(wanted) {
            unique(
                &mut found,
                bytes.get(cursor..end).ok_or("AML object truncated")?,
            )?;
        }
    }
    found.ok_or("AML metadata object missing")
}

fn method<'a>(bytes: &'a [u8], wanted: &[u8; 4]) -> Result<&'a [u8]> {
    let body = named_package(bytes, &[0x14], wanted)?;
    if body.first().is_none_or(|flags| flags & 7 != 0) {
        return Err("AML method arguments unsupported");
    }
    Ok(&body[1..])
}

fn named_value<'a>(bytes: &'a [u8], wanted: &[u8; 4]) -> Result<&'a [u8]> {
    let mut found = None;
    for offset in 0..bytes.len() {
        if bytes[offset] != 0x08 {
            continue;
        }
        let mut cursor = offset + 1;
        if name(bytes, &mut cursor).ok().as_ref() == Some(wanted) {
            unique(&mut found, &bytes[cursor..])?;
        }
    }
    found.ok_or("AML named constant missing")
}

fn named_integer(bytes: &[u8], wanted: &[u8; 4]) -> Result<u64> {
    integer(named_value(bytes, wanted)?, &mut 0)
}

fn integer_or_name(bytes: &[u8], cursor: &mut usize, all: &[u8]) -> Result<u64> {
    let initial = *cursor;
    if let Ok(value) = integer(bytes, cursor) {
        return Ok(value);
    }
    *cursor = initial;
    named_integer(all, &name(bytes, cursor)?)
}

fn literal_buffer(bytes: &[u8], wanted: &[u8; 4]) -> Result<Vec<u8>> {
    let value = named_value(bytes, wanted)?;
    if value.first() != Some(&0x11) {
        return Err("AML buffer expression unsupported");
    }
    let (length, width) = pkg(value, 1)?;
    let end = 1usize
        .checked_add(length)
        .filter(|end| *end <= value.len())
        .ok_or("AML buffer truncated")?;
    let mut cursor = 1 + width;
    let size = usize::try_from(integer(value, &mut cursor)?).map_err(|_| "AML buffer too large")?;
    if size > 16 * 1024 || cursor.checked_add(size) != Some(end) {
        return Err("AML buffer size unsupported");
    }
    Ok(value[cursor..end].to_vec())
}

fn stored_integer(bytes: &[u8], wanted: &[u8; 4]) -> Result<u64> {
    let mut found = None;
    for offset in 0..bytes.len() {
        if bytes[offset] != 0x70 {
            continue;
        }
        let mut cursor = offset + 1;
        let Ok(value) = integer(bytes, &mut cursor) else {
            continue;
        };
        if name(bytes, &mut cursor).ok().as_ref() == Some(wanted) {
            unique(&mut found, value)?;
        }
    }
    found.ok_or("AML initialization constant missing")
}

fn region(all: &[u8], wanted: &[u8; 4]) -> Result<(u64, usize)> {
    let mut found = None;
    for offset in 0..all.len().saturating_sub(1) {
        if all[offset..offset + 2] != [0x5b, 0x80] {
            continue;
        }
        let mut cursor = offset + 2;
        if name(all, &mut cursor).ok().as_ref() != Some(wanted) {
            continue;
        }
        if all.get(cursor) != Some(&0) {
            return Err("AML NVS region is not SystemMemory");
        }
        cursor += 1;
        let base = integer_or_name(all, &mut cursor, all)?;
        let length = usize::try_from(integer_or_name(all, &mut cursor, all)?)
            .map_err(|_| "AML NVS region too large")?;
        if length == 0 || length > MAX_NVS_BYTES || base.checked_add(length as u64).is_none() {
            return Err("AML NVS region range invalid");
        }
        unique(&mut found, (base, length))?;
    }
    found.ok_or("AML NVS region missing")
}

fn field_offset(all: &[u8], region_name: &[u8; 4], wanted: &[u8; 4]) -> Result<(usize, usize)> {
    let fields = named_package(all, &[0x5b, 0x81], region_name)?;
    let mut cursor = 1; // FieldFlags.
    let mut bits = 0usize;
    while cursor < fields.len() {
        match fields[cursor] {
            0 => {
                let (length, width) = pkg(fields, cursor + 1)?;
                bits = bits
                    .checked_add(length)
                    .ok_or("AML field offset overflow")?;
                cursor += 1 + width;
            }
            1 => cursor += 3, // AccessField.
            3 => cursor += 4, // ExtendedAccessField.
            2 => return Err("AML connected NVS field unsupported"),
            _ => {
                let field = name(fields, &mut cursor)?;
                let (length, width) = pkg(fields, cursor)?;
                cursor += width;
                if &field == wanted {
                    if bits % 8 != 0 || length == 0 || length > 64 || length % 8 != 0 {
                        return Err("AML NVS field alignment unsupported");
                    }
                    return Ok((bits / 8, length / 8));
                }
                bits = bits
                    .checked_add(length)
                    .ok_or("AML field offset overflow")?;
            }
        }
    }
    Err("AML NVS field missing")
}

fn field_value(all: &[u8], region_name: &[u8; 4], wanted: &[u8; 4], data: &[u8]) -> Result<u64> {
    let (offset, size) = field_offset(all, region_name, wanted)?;
    let raw = data
        .get(offset..offset + size)
        .ok_or("AML NVS field outside region")?;
    Ok(raw.iter().enumerate().fold(0, |value, (index, byte)| {
        value | (u64::from(*byte) << (index * 8))
    }))
}

fn constant_return(bytes: &[u8]) -> Result<u64> {
    if bytes.first() != Some(&0xa4) {
        return Err("AML constant return unsupported");
    }
    let mut cursor = 1;
    let value = integer(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err("AML constant return has extra operations");
    }
    Ok(value)
}

fn field_binding(bytes: &[u8], buffer: &[u8; 4], offset: usize, opcode: u8) -> Result<[u8; 4]> {
    let mut found = None;
    for start in 0..bytes.len() {
        if bytes[start] != opcode {
            continue;
        }
        let mut cursor = start + 1;
        if name(bytes, &mut cursor).ok().as_ref() != Some(buffer) {
            continue;
        }
        let Ok(index) = integer(bytes, &mut cursor) else {
            continue;
        };
        if index == offset as u64 {
            unique(&mut found, name(bytes, &mut cursor)?)?;
        }
    }
    found.ok_or("AML resource field binding missing")
}

fn added_offset(bytes: &[u8], target: &[u8; 4]) -> Result<u64> {
    let mut found = None;
    for offset in 0..bytes.len().saturating_sub(1) {
        if bytes[offset..offset + 2] != [0x70, 0x72] {
            continue;
        }
        let mut cursor = offset + 2;
        if name(bytes, &mut cursor).ok().as_ref() != Some(b"SBRG") {
            continue;
        }
        let delta = integer(bytes, &mut cursor)?;
        if bytes.get(cursor) != Some(&0) {
            return Err("AML add result target unsupported");
        }
        cursor += 1;
        if name(bytes, &mut cursor).ok().as_ref() == Some(target) {
            unique(&mut found, delta)?;
        }
    }
    found.ok_or("AML GPIO region expression missing")
}

fn gpio_base_table(all: &[u8]) -> Result<Vec<(u64, u64)>> {
    let bytes = named_value(all, b"GPCL")?;
    if bytes.first() != Some(&0x12) {
        return Err("AML GPIO group package missing");
    }
    let (length, width) = pkg(bytes, 1)?;
    let end = 1 + length;
    if end > bytes.len() {
        return Err("AML GPIO group package truncated");
    }
    let mut cursor = 1 + width;
    let count = usize::from(*bytes.get(cursor).ok_or("AML GPIO group count missing")?);
    cursor += 1;
    if count > 64 {
        return Err("AML GPIO group count too large");
    }
    let mut groups = Vec::new();
    for _ in 0..count {
        if bytes.get(cursor) != Some(&0x12) {
            return Err("AML GPIO group entry unsupported");
        }
        cursor += 1;
        let (length, width) = pkg(bytes, cursor)?;
        let entry_end = cursor + length;
        cursor += width;
        if entry_end > end || bytes.get(cursor) != Some(&7) {
            return Err("AML GPIO group entry size unsupported");
        }
        cursor += 1;
        let mut values = [0u64; 7];
        for value in &mut values {
            *value = integer(bytes, &mut cursor)?;
        }
        if cursor != entry_end {
            return Err("AML GPIO group has extra operations");
        }
        groups.push((values[1], values[6])); // Silicon group size and Windows GPIO base.
    }
    if cursor != end {
        return Err("AML GPIO package has extra operations");
    }
    Ok(groups)
}

const REFERENCE_HELPERS: [(&[u8; 4], &[u8]); 5] = [
    (b"GNUM", b"\x01\x70GNMB\x68\x60\x70GGRP\x68\x61\xa4\x72GINF\x61\x0a\x06\x60\x00"),
    (b"GNMB", b"\x09\xa4\x7b\x68\x0b\xff\xff\x00"),
    (b"GGRP", b"\x09\x7a\x7b\x68\x0c\x00\x00\xff\x00\x00\x0a\x10\x60\xa4\x60"),
    (b"GINF", b"\x02\xa0\x17\x93PCHSPCHL\xa4\x83\x88\x83\x88GPCL\x68\x00\x69\x00\xa1\x0e\xa4\x83\x88\x83\x88GPCH\x68\x00\x69\x00"),
    (b"HIDD", b"\x0d\xa0\x1d\x93\x68HIDG\xa0\x0e\x93\x6a\x00\xa0\x09\x93\x69\x01\xa4\x11\x03\x01\x03\xa0\x06\x93\x6a\x01\xa4\x6c\xa4\x11\x03\x01\x00"),
];

fn validate_reference_helpers(all: &[u8]) -> Result<()> {
    // These are operation shapes, not board data. In this profile GNUM adds
    // GPCL[group][6] to the low 16 bits of GPDI. Verify all four helpers before
    // interpreting those fields rather than assuming their names imply that
    // behavior. HIDD implements revision 1/function 1 of the HID-I2C UUID.
    for (name, expected) in REFERENCE_HELPERS {
        if named_package(all, &[0x14], name)? != expected {
            return Err("AML reference helper implementation unsupported");
        }
    }
    Ok(())
}

/// `read` must accept only the declared ACPI NVS RAM ranges. The caller owns
/// physical mapping policy; this reader never writes RAM or device registers.
/// It derives configured wiring without executing the OSYS compatibility branch
/// in `_CRS`; presence, GPIO mode and the controller layout are checked directly.
pub(crate) fn discover(
    dsdt: &[u8],
    mut read: impl FnMut(u64, &mut [u8]) -> Result<()>,
) -> Result<TrackpadFirmware> {
    if dsdt.len() < 36 || dsdt.len() > MAX_AML_BYTES || &dsdt[..4] != b"DSDT" {
        return Err("AML DSDT size/signature invalid");
    }
    let length = u32::from_le_bytes(dsdt[4..8].try_into().unwrap()) as usize;
    if length != dsdt.len() || dsdt.iter().fold(0u8, |a, b| a.wrapping_add(*b)) != 0 {
        return Err("AML DSDT length/checksum invalid");
    }
    let all = &dsdt[36..];
    validate_reference_helpers(all)?;
    let trackpad = named_package(all, &[0x5b, 0x82], b"TPD0")?;
    if !in_scope(all, &[*b"_SB_", *b"PC00", *b"I2C0"], trackpad) {
        return Err("AML trackpad controller scope unsupported");
    }
    if !named_value(trackpad, b"_CID")?.starts_with(b"\x0dPNP0C50\0") {
        return Err("AML trackpad does not declare HID-over-I2C");
    }
    if constant_return(method(trackpad, b"_STA")?)? != 0x0f {
        return Err("AML trackpad is not present/enabled");
    }
    let ini = method(trackpad, b"_INI")?;
    let hid_descriptor_register = u16::try_from(stored_integer(ini, b"HID2")?)
        .map_err(|_| "HID descriptor register out of range")?;
    let slave_address =
        u8::try_from(stored_integer(ini, b"BADR")?).map_err(|_| "I2C address out of range")?;
    if !(8..=0x77).contains(&slave_address) {
        return Err("I2C target is reserved/not 7-bit");
    }
    let dsm = named_package(trackpad, &[0x14], b"_DSM")?;
    if dsm.first().is_none_or(|flags| flags & 7 != 4)
        || !dsm.windows(12).any(|window| window == b"HIDDhijkHID2")
        || !literal_buffer(all, b"HIDG")?
            .as_slice()
            .eq(&i2c_hid::HID_I2C_DSM_UUID_LE)
    {
        return Err("AML HID descriptor method layout unsupported");
    }

    let mut bus_resource = literal_buffer(trackpad, b"SBFB")?;
    if bus_resource.len() < 18 || bus_resource[0] != 0x8e || bus_resource[5] != 1 {
        return Err("AML I2C resource template unsupported");
    }
    if field_binding(trackpad, b"SBFB", 16, 0x8b)? != *b"BADR"
        || field_binding(trackpad, b"SBFB", 12, 0x8a)? != *b"SPED"
    {
        return Err("AML I2C template field binding unsupported");
    }
    let speed_hz = u32::from_le_bytes(bus_resource[12..16].try_into().unwrap());
    if speed_hz == 0 || bus_resource[7] & 1 != 0 {
        return Err("AML I2C speed/addressing unsupported");
    }
    bus_resource[16..18].copy_from_slice(&u16::from(slave_address).to_le_bytes());
    let bus_resources =
        i2c_hid::parse_resources(&bus_resource).map_err(|_| "AML I2C template invalid")?;
    if bus_resources.i2c.len() != 1 {
        return Err("AML I2C connection count unsupported");
    }

    let mut nvs = Vec::new();
    for region_name in [b"PNVA", b"GNVS"] {
        let (base, length) = region(all, region_name)?;
        let mut data = vec![0; length];
        read(base, &mut data)?;
        nvs.push(data);
    }
    let pch = field_value(all, b"PNVA", b"PCHS", &nvs[0])?;
    if pch != named_integer(all, b"PCHL")? {
        return Err("AML GPIO controller is not supported Tiger Lake-LP layout");
    }
    if field_value(all, b"PNVA", b"GPHD", &nvs[0])? != 0 {
        return Err("AML GPIO controller is hidden");
    }
    if field_value(all, b"GNVS", b"TPDM", &nvs[1])? != 0 {
        return Err("AML trackpad does not use GPIO interrupt mode");
    }
    let sideband_base = field_value(all, b"PNVA", b"SBRG", &nvs[0])?;
    if sideband_base == 0 || sideband_base & 0xffff != 0 {
        return Err("AML sideband register base invalid");
    }
    let encoded_gpio = field_value(all, b"GNVS", b"GPDI", &nvs[1])?;
    let groups = gpio_base_table(all)?;
    let group = ((encoded_gpio >> 16) & 0xff) as usize;
    let pin = encoded_gpio & 0xffff;
    let (count, gpio_base) = *groups.get(group).ok_or("AML GPIO group out of range")?;
    if pin >= count || gpio_base == 0xffff {
        return Err("AML GPIO pin not mapped");
    }
    let gpio_number = u16::try_from(
        gpio_base
            .checked_add(pin)
            .ok_or("AML GPIO number overflow")?,
    )
    .map_err(|_| "AML GPIO number out of range")?;
    if i2c_hid::tigerlake_lp_gpio_pad(gpio_number).is_none() {
        return Err("AML GPIO number not supported");
    }

    let mut gpio_resource = literal_buffer(trackpad, b"SBFG")?;
    if field_binding(trackpad, b"SBFG", 23, 0x8b)? != *b"INT1"
        || !ini.windows(13).any(|window| window == b"\x70GNUMGPDIINT1")
    {
        return Err("AML GPIO pin initialization unsupported");
    }
    gpio_resource
        .get_mut(23..25)
        .ok_or("AML GPIO template too short")?
        .copy_from_slice(&gpio_number.to_le_bytes());
    let gpio = i2c_hid::parse_resources(&gpio_resource).map_err(|_| "AML GPIO resource invalid")?;
    let interrupt = gpio
        .gpio_interrupts
        .first()
        .ok_or("AML GPIO interrupt missing")?;
    if gpio.gpio_interrupts.len() != 1
        || interrupt.pins.len() != 1
        || interrupt.controller != "\\_SB.GPI0"
    {
        return Err("AML GPIO interrupt controller unsupported");
    }
    let gpio_device = named_package(all, &[0x5b, 0x82], b"GPI0")?;
    if !in_scope(all, &[*b"_SB_"], gpio_device) {
        return Err("AML GPIO controller scope unsupported");
    }
    if !method(gpio_device, b"_HID")?
        .windows(8)
        .any(|w| w == b"INT34C5\0")
    {
        return Err("AML GPIO device identity unsupported");
    }
    let gpio_crs = method(gpio_device, b"_CRS")?;
    let mut gpio_regions_buffer = literal_buffer(gpio_crs, b"RBFL")?;
    let mut offset = 0;
    let mut patched = 0;
    while offset < gpio_regions_buffer.len() {
        let tag = gpio_regions_buffer[offset];
        let size = if tag & 0x80 != 0 {
            let len = gpio_regions_buffer
                .get(offset + 1..offset + 3)
                .ok_or("AML GPIO region resource truncated")?;
            usize::from(u16::from_le_bytes([len[0], len[1]])) + 3
        } else {
            usize::from(tag & 7) + 1
        };
        if offset + size > gpio_regions_buffer.len() {
            return Err("AML GPIO region resource length invalid");
        }
        if tag == 0x86 {
            if size != 12 {
                return Err("AML GPIO region memory format unsupported");
            }
            let field = field_binding(gpio_crs, b"RBFL", offset + 4, 0x8a)?;
            let base = sideband_base
                .checked_add(added_offset(gpio_crs, &field)?)
                .ok_or("AML GPIO region base overflow")?;
            let base = u32::try_from(base).map_err(|_| "AML GPIO region outside 32-bit range")?;
            gpio_regions_buffer[offset + 4..offset + 8].copy_from_slice(&base.to_le_bytes());
            patched += 1;
        }
        offset += size;
    }
    let gpio_regions = i2c_hid::parse_resources(&gpio_regions_buffer)
        .map_err(|_| "AML GPIO controller resources invalid")?
        .memory;
    if patched != 4 || gpio_regions.len() != 4 {
        return Err("AML GPIO community resources unsupported");
    }

    let root = named_package(all, &[0x5b, 0x82], b"PC00")?;
    let bbn = method(root, b"_BBN")?;
    if bbn != b"\xa4BN00" {
        return Err("AML PCI bus-number expression unsupported");
    }
    let pci_bus = u8::try_from(constant_return(method(root, b"BN00")?)?)
        .map_err(|_| "AML PCI bus out of range")?;
    // Intel tables extend PC00 through later Scope objects rather than placing
    // every device bytecode inside the original root Device package.
    let controller = named_package(all, &[0x5b, 0x82], b"I2C0")?;
    if !in_scope(all, &[*b"_SB_", *b"PC00"], controller) {
        return Err("AML I2C PCI controller scope unsupported");
    }
    let address = constant_return(method(controller, b"_ADR")?)?;
    let pci_slot = u8::try_from(address >> 16).map_err(|_| "AML PCI slot out of range")?;
    let pci_function =
        u8::try_from(address & 0xffff).map_err(|_| "AML PCI function out of range")?;
    if pci_slot > 31 || pci_function > 7 {
        return Err("AML PCI address invalid");
    }

    Ok(TrackpadFirmware {
        pci_bus,
        pci_slot,
        pci_function,
        slave_address,
        speed_hz,
        hid_descriptor_register,
        gpio_number,
        gpio_polarity: interrupt.polarity,
        gpio_trigger: interrupt.trigger,
        gpio_hid: String::from("INT34C5"),
        gpio_regions,
    })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn length(value: usize) -> Vec<u8> {
        if value < 64 {
            vec![value as u8]
        } else if value < 4096 {
            vec![0x40 | (value & 15) as u8, (value >> 4) as u8]
        } else {
            vec![
                0x80 | (value & 15) as u8,
                (value >> 4) as u8,
                (value >> 12) as u8,
            ]
        }
    }

    fn package(opcode: &[u8], body: &[u8]) -> Vec<u8> {
        let width = if body.len() < 63 {
            1
        } else if body.len() < 4094 {
            2
        } else {
            3
        };
        let mut out = opcode.to_vec();
        out.extend(length(body.len() + width));
        out.extend(body);
        out
    }

    fn int(value: u64) -> Vec<u8> {
        let mut out = vec![0x0e];
        out.extend(value.to_le_bytes());
        out
    }

    fn named(wanted: &[u8; 4], value: &[u8]) -> Vec<u8> {
        let mut out = vec![8];
        out.extend(wanted);
        out.extend(value);
        out
    }

    fn object(opcode: &[u8], wanted: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut payload = wanted.to_vec();
        payload.extend(body);
        package(opcode, &payload)
    }

    fn method0(wanted: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut payload = vec![0];
        payload.extend(body);
        object(&[0x14], wanted, &payload)
    }

    fn buffer(wanted: &[u8; 4], contents: &[u8]) -> Vec<u8> {
        let mut payload = int(contents.len() as u64);
        payload.extend(contents);
        named(wanted, &package(&[0x11], &payload))
    }

    fn scope(parts: &[&[u8; 4]], body: &[u8]) -> Vec<u8> {
        let mut payload = vec![0x2f, parts.len() as u8];
        for part in parts {
            payload.extend(*part);
        }
        payload.extend(body);
        package(&[0x10], &payload)
    }

    fn binding(opcode: u8, source: &[u8; 4], offset: u64, target: &[u8; 4]) -> Vec<u8> {
        let mut out = vec![opcode];
        out.extend(source);
        out.extend(int(offset));
        out.extend(target);
        out
    }

    fn op_region(wanted: &[u8; 4], base: &[u8], size: &[u8]) -> Vec<u8> {
        let mut out = vec![0x5b, 0x80];
        out.extend(wanted);
        out.push(0);
        out.extend(base);
        out.extend(size);
        out
    }

    fn fields(region: &[u8; 4], reserved: usize, entries: &[(&[u8; 4], usize)]) -> Vec<u8> {
        let mut out = vec![0, 0]; // FieldFlags and reserved field.
        out.extend(length(reserved));
        for (name, bits) in entries {
            out.extend(*name);
            out.extend(length(*bits));
        }
        object(&[0x5b, 0x81], region, &out)
    }

    fn checksum(dsdt: &mut [u8]) {
        let size = dsdt.len() as u32;
        dsdt[4..8].copy_from_slice(&size.to_le_bytes());
        dsdt[9] = 0;
        dsdt[9] = 0u8.wrapping_sub(dsdt.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    }

    fn fixture() -> Vec<u8> {
        let mut all = Vec::new();
        for (name, helper) in REFERENCE_HELPERS {
            all.extend(object(&[0x14], name, helper));
        }
        all.extend(buffer(b"HIDG", &i2c_hid::HID_I2C_DSM_UUID_LE));
        all.extend(named(b"PNVB", &int(0x1234_5000)));
        all.extend(named(b"PNVL", &int(64)));
        all.extend(op_region(b"PNVA", b"PNVB", b"PNVL"));
        all.extend(op_region(b"GNVS", &int(0x2345_6000), &int(96)));
        all.extend(fields(
            b"PNVA",
            24,
            &[(b"PCHS", 16), (b"SBRG", 32), (b"GPHD", 8)],
        ));
        all.extend(fields(b"GNVS", 40, &[(b"TPDM", 8), (b"GPDI", 32)]));
        all.extend(named(b"PCHL", &int(2)));
        let mut entry = vec![7];
        for value in [0, 21, 0, 0, 0, 0, 160] {
            entry.extend(int(value));
        }
        let mut table = vec![1];
        table.extend(package(&[0x12], &entry));
        all.extend(named(b"GPCL", &package(&[0x12], &table)));

        let mut pc00 = method0(b"_BBN", b"\xa4BN00");
        let mut ret = vec![0xa4];
        ret.extend(int(3));
        pc00.extend(method0(b"BN00", &ret));
        all.extend(object(&[0x5b, 0x82], b"PC00", &pc00));
        let mut adr = vec![0xa4];
        adr.extend(int(20 << 16 | 1));
        all.extend(scope(
            &[b"_SB_", b"PC00"],
            &object(&[0x5b, 0x82], b"I2C0", &method0(b"_ADR", &adr)),
        ));

        let mut tpd0 = named(b"_CID", b"\x0dPNP0C50\0");
        tpd0.extend(method0(b"_STA", b"\xa4\x0a\x0f"));
        let mut ini = b"\x70GNUMGPDIINT1\x70".to_vec();
        ini.extend(int(0x42));
        ini.extend(b"HID2\x70");
        ini.extend(int(0x34));
        ini.extend(b"BADR");
        tpd0.extend(method0(b"_INI", &ini));
        tpd0.extend(object(&[0x14], b"_DSM", b"\x04\xa4HIDDhijkHID2"));
        let mut bus = vec![0x8e, 20, 0, 2, 0, 1, 2, 0, 0, 1, 6, 0];
        bus.extend(100_000u32.to_le_bytes());
        bus.extend(0u16.to_le_bytes());
        bus.extend(b"NULL\0\x79\0");
        tpd0.extend(buffer(b"SBFB", &bus));
        tpd0.extend(binding(0x8b, b"SBFB", 16, b"BADR"));
        tpd0.extend(binding(0x8a, b"SBFB", 12, b"SPED"));
        let mut gpio = vec![
            0x8c,
            0,
            0,
            1,
            0,
            1,
            0,
            2 | 8,
            0,
            0,
            0,
            0,
            0,
            0,
            23,
            0,
            0,
            25,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        gpio.extend(b"\\_SB.GPI0\0");
        let gpio_length = gpio.len() as u16 - 3;
        gpio[1..3].copy_from_slice(&gpio_length.to_le_bytes());
        gpio.extend([0x79, 0]);
        tpd0.extend(buffer(b"SBFG", &gpio));
        tpd0.extend(binding(0x8b, b"SBFG", 23, b"INT1"));
        all.extend(scope(
            &[b"_SB_", b"PC00", b"I2C0"],
            &object(&[0x5b, 0x82], b"TPD0", &tpd0),
        ));

        let mut memory = Vec::new();
        for _ in 0..4 {
            memory.extend([0x86, 9, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0]);
        }
        memory.extend([0x79, 0]);
        let mut crs = buffer(b"RBFL", &memory);
        for index in 0..4u8 {
            let target = [b'C', b'M', b'L', b'0' + index];
            crs.extend(binding(0x8a, b"RBFL", u64::from(index) * 12 + 4, &target));
            crs.extend(b"\x70\x72SBRG");
            crs.extend(int((u64::from(index) + 1) * 0x100000));
            crs.push(0);
            crs.extend(target);
        }
        let mut gpi0 = method0(b"_HID", b"\xa4\x0dINT34C5\0");
        gpi0.extend(method0(b"_CRS", &crs));
        all.extend(scope(&[b"_SB_"], &object(&[0x5b, 0x82], b"GPI0", &gpi0)));
        let mut dsdt = vec![0; 36];
        dsdt[..4].copy_from_slice(b"DSDT");
        dsdt.extend(all);
        checksum(&mut dsdt);
        dsdt
    }

    fn synthetic_nvs(dsdt: &[u8], region_name: &[u8; 4]) -> Vec<u8> {
        let all = &dsdt[36..];
        let (_, size) = region(all, region_name).unwrap();
        let mut nvs = vec![0; size];
        if region_name == b"PNVA" {
            set_field(all, region_name, b"PCHS", &mut nvs, 2);
            set_field(all, region_name, b"SBRG", &mut nvs, 0xfb00_0000);
        } else {
            set_field(all, region_name, b"GPDI", &mut nvs, 14);
        }
        nvs
    }

    fn set_field(all: &[u8], region: &[u8; 4], wanted: &[u8; 4], data: &mut [u8], value: u64) {
        let (offset, size) = field_offset(all, region, wanted).unwrap();
        data[offset..offset + size].copy_from_slice(&value.to_le_bytes()[..size]);
    }

    #[test]
    fn derives_alternate_wiring_from_aml_and_bounded_current_nvs() {
        let dsdt = fixture();
        let pnva = synthetic_nvs(&dsdt, b"PNVA");
        let gnvs = synthetic_nvs(&dsdt, b"GNVS");
        let mut reads = Vec::new();
        let firmware = discover(&dsdt, |base, out| {
            reads.push((base, out.len()));
            match base {
                0x1234_5000 => out.copy_from_slice(&pnva),
                0x2345_6000 => out.copy_from_slice(&gnvs),
                _ => panic!("not a declared NVS region"),
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(reads, [(0x1234_5000, 64), (0x2345_6000, 96)]);
        assert_eq!(
            (firmware.pci_bus, firmware.pci_slot, firmware.pci_function),
            (3, 20, 1)
        );
        assert_eq!(
            (
                firmware.slave_address,
                firmware.speed_hz,
                firmware.hid_descriptor_register
            ),
            (0x34, 100_000, 0x42)
        );
        assert_eq!(firmware.gpio_number, 174);
        assert_eq!(firmware.gpio_polarity, Polarity::ActiveLow);
        assert_eq!(firmware.gpio_trigger, Trigger::Level);
        assert_eq!(
            firmware
                .gpio_regions
                .iter()
                .map(|region| (region.base, region.length))
                .collect::<Vec<_>>(),
            [
                (0xfb10_0000, 65536),
                (0xfb20_0000, 65536),
                (0xfb30_0000, 65536),
                (0xfb40_0000, 65536)
            ]
        );
    }

    #[test]
    fn nvs_policy_errors_propagate_and_inactive_modes_are_rejected() {
        let dsdt = fixture();
        assert!(matches!(
            discover(&dsdt, |_, _| Err("NVS mapping denied")),
            Err("NVS mapping denied")
        ));
        for (region_name, field, value) in [
            (b"PNVA", b"PCHS", 1),
            (b"PNVA", b"GPHD", 1),
            (b"GNVS", b"TPDM", 1),
            (b"GNVS", b"GPDI", 255),
            (b"PNVA", b"SBRG", 1),
        ] {
            let mut pnva = synthetic_nvs(&dsdt, b"PNVA");
            let mut gnvs = synthetic_nvs(&dsdt, b"GNVS");
            set_field(
                &dsdt[36..],
                region_name,
                field,
                if region_name == b"PNVA" {
                    &mut pnva
                } else {
                    &mut gnvs
                },
                value,
            );
            assert!(
                discover(&dsdt, |base, out| {
                    out.copy_from_slice(if base == 0x1234_5000 { &pnva } else { &gnvs });
                    Ok(())
                })
                .is_err()
            );
        }
    }

    #[test]
    fn corrupt_headers_ambiguous_names_and_unsupported_helpers_fail_before_reads() {
        let valid = fixture();
        let mut cases = Vec::new();
        for size in [0, 1, 35, 36, valid.len() - 1] {
            cases.push(valid[..size].to_vec());
        }
        let mut checksum_bad = valid.clone();
        checksum_bad[9] ^= 1;
        cases.push(checksum_bad);
        let mut ambiguous = valid.clone();
        ambiguous.extend(named(b"PNVB", &int(0x3456_7000)));
        checksum(&mut ambiguous);
        cases.push(ambiguous);
        let mut wrong_helper = valid.clone();
        let pos = wrong_helper
            .windows(4)
            .position(|window| window == b"GNMB")
            .unwrap();
        wrong_helper[pos] = b'X';
        checksum(&mut wrong_helper);
        cases.push(wrong_helper);
        for dsdt in cases {
            assert!(discover(&dsdt, |_, _| panic!("invalid AML caused physical read")).is_err());
        }
    }

    #[test]
    fn every_truncated_package_and_field_read_remains_bounded() {
        let dsdt = fixture();
        for size in 36..dsdt.len() {
            let mut truncated = dsdt[..size].to_vec();
            checksum(&mut truncated);
            let _ = discover(&truncated, |_, _| Err("bounded test read"));
        }
        for size in 0..32 {
            let bytes = vec![0xff; size];
            let _ = pkg(&bytes, 0);
            let _ = field_offset(&bytes, b"GNVS", b"GPDI");
            let _ = gpio_base_table(&bytes);
        }
    }

    #[test]
    fn captured_firmware_metadata_uses_current_declared_nvs_ranges() {
        // The optional private hardware capture stays outside the repository.
        // NVS contents below are a synthetic snapshot, not stored board data.
        let Ok(path) = std::env::var("TRUEOS_I2C_TEST_DSDT") else {
            return;
        };
        let dsdt = std::fs::read(path).unwrap();
        let all = &dsdt[36..];
        let (pnva_base, pnva_length) = region(all, b"PNVA").unwrap();
        let (gnvs_base, gnvs_length) = region(all, b"GNVS").unwrap();
        let mut pnva = vec![0; pnva_length];
        let mut gnvs = vec![0; gnvs_length];
        set_field(
            all,
            b"PNVA",
            b"PCHS",
            &mut pnva,
            named_integer(all, b"PCHL").unwrap(),
        );
        set_field(all, b"PNVA", b"SBRG", &mut pnva, 0xfd00_0000);
        set_field(all, b"PNVA", b"GPHD", &mut pnva, 0);
        set_field(all, b"GNVS", b"TPDM", &mut gnvs, 0);
        let groups = gpio_base_table(all).unwrap();
        let (group, &(count, base)) = groups
            .iter()
            .enumerate()
            .find(|(_, (count, base))| *base <= 175 && 175 < *base + *count)
            .unwrap();
        let pin = 175 - base;
        assert!(pin < count);
        set_field(all, b"GNVS", b"GPDI", &mut gnvs, (group as u64) << 16 | pin);
        let mut reads = Vec::new();
        let firmware = discover(&dsdt, |base, out| {
            reads.push((base, out.len()));
            match base {
                value if value == pnva_base => out.copy_from_slice(&pnva),
                value if value == gnvs_base => out.copy_from_slice(&gnvs),
                _ => panic!("read outside declared NVS ranges"),
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(reads, [(pnva_base, pnva_length), (gnvs_base, gnvs_length)]);
        assert_eq!(
            (firmware.pci_bus, firmware.pci_slot, firmware.pci_function),
            (0, 21, 0)
        );
        assert_eq!(
            (
                firmware.slave_address,
                firmware.speed_hz,
                firmware.hid_descriptor_register
            ),
            (0x2c, 400_000, 0x20)
        );
        assert_eq!(firmware.gpio_number, 175);
        assert_eq!(firmware.gpio_polarity, Polarity::ActiveLow);
        assert_eq!(firmware.gpio_trigger, Trigger::Level);
        assert_eq!(firmware.gpio_hid, "INT34C5");
        assert_eq!(
            firmware
                .gpio_regions
                .iter()
                .map(|region| region.base)
                .collect::<Vec<_>>(),
            [0xfd6e_0000, 0xfd6d_0000, 0xfd6a_0000, 0xfd69_0000]
        );
    }
}
