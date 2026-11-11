//! Bounded HID mouse/touchpad report decoding, independent of the I2C host.
//!
//! Absolute finger positions become relative pointer deltas only while the
//! same confident contact remains down. Touch-down, lift and contact changes
//! establish a new baseline rather than moving the cursor across the screen.

const MAX_REPORTS: usize = 16;
const MAX_CONTACTS: usize = 10;
const MAX_COLLECTIONS: usize = 16;
const MAX_USAGES: usize = 32;
const MAX_GLOBALS: usize = 8;
const MAX_DESCRIPTOR: usize = 4096;
const MAX_REPORT_BITS: u32 = 8192;
const POINTER_UNITS: i64 = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParseError {
    Truncated,
    Malformed,
    TooLarge,
    NoPointer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Pointer {
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
}

/// Location of the Digitizers / Input Mode feature. Report length includes its
/// ID byte when nonzero; bit offsets address the payload after that ID byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Feature {
    pub report_id: u8,
    pub report_len: u16,
    pub bit_offset: u16,
    pub bit_size: u8,
}

impl Feature {
    /// Patch an existing feature report, preserving other fields. A caller can
    /// pass a zeroed report when the feature consists solely of Input Mode.
    pub(crate) fn set_value(self, value: u32, report: &mut [u8]) -> Option<usize> {
        if !(1..=32).contains(&self.bit_size) {
            return None;
        }
        let len = usize::from(self.report_len);
        let report = report.get_mut(..len)?;
        let prefix = usize::from(self.report_id != 0);
        let payload_len = len.checked_sub(prefix)?;
        if usize::from(self.bit_offset) + usize::from(self.bit_size) > payload_len * 8 {
            return None;
        }
        if prefix != 0 {
            *report.first_mut()? = self.report_id;
        }
        if self.bit_size < 32 && value >= (1 << self.bit_size) {
            return None;
        }
        for bit in 0..usize::from(self.bit_size) {
            let index = usize::from(self.bit_offset) + bit;
            let byte = report.get_mut(prefix + index / 8)?;
            let mask = 1u8 << (index % 8);
            *byte = (*byte & !mask) | (u8::from(value & (1 << bit) != 0) * mask);
        }
        Some(len)
    }

    pub(crate) fn report_with_value(self, value: u32, report: &mut [u8]) -> Option<usize> {
        report.get_mut(..usize::from(self.report_len))?.fill(0);
        self.set_value(value, report)
    }
}

#[derive(Clone, Copy, Debug)]
struct Field {
    offset: u16,
    size: u8,
    min: i32,
    max: i32,
}

impl Field {
    fn read(self, payload: &[u8]) -> Option<i32> {
        let mut value = 0u32;
        for bit in 0..usize::from(self.size) {
            let index = usize::from(self.offset) + bit;
            value |= u32::from((payload.get(index / 8)? >> (index % 8)) & 1) << bit;
        }
        if self.min >= 0 && value > i32::MAX as u32 {
            return None;
        }
        Some(if self.min < 0 && self.size < 32 {
            ((value << (32 - self.size)) as i32) >> (32 - self.size)
        } else {
            value as i32
        })
    }

    fn coordinate(self, payload: &[u8]) -> Option<i32> {
        let value = self.read(payload)?;
        (self.max > self.min && (self.min..=self.max).contains(&value)).then_some(value)
    }
}

#[derive(Clone, Copy, Default)]
struct ContactFields {
    id: Option<Field>,
    tip: Option<Field>,
    confidence: Option<Field>,
    x: Option<Field>,
    y: Option<Field>,
}

impl ContactFields {
    fn complete(self) -> bool {
        self.tip.is_some() && self.x.is_some() && self.y.is_some()
    }

    fn read(self, payload: &[u8], fallback_id: usize) -> Option<Contact> {
        if self.tip?.read(payload)? != 1
            || self
                .confidence
                .is_some_and(|field| field.read(payload) != Some(1))
        {
            return None;
        }
        Some(Contact {
            id: match self.id {
                Some(field) => {
                    let value = field.read(payload)?;
                    if !(field.min..=field.max).contains(&value) {
                        return None;
                    }
                    value
                }
                None => fallback_id as i32,
            },
            x: self.x?.coordinate(payload)?,
            y: self.y?.coordinate(payload)?,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: i32,
    x: i32,
    y: i32,
}

#[derive(Clone, Copy)]
struct Report {
    id: u8,
    input_bits: u16,
    feature_bits: u16,
    mouse_x: Option<Field>,
    mouse_y: Option<Field>,
    wheel: Option<Field>,
    buttons: [Option<Field>; 8],
    contacts: [ContactFields; MAX_CONTACTS],
    contact_slots: usize,
    contact_count: Option<Field>,
    input_mode: Option<Field>,
}

impl Report {
    const EMPTY: Self = Self {
        id: 0,
        input_bits: 0,
        feature_bits: 0,
        mouse_x: None,
        mouse_y: None,
        wheel: None,
        buttons: [None; 8],
        contacts: [ContactFields {
            id: None,
            tip: None,
            confidence: None,
            x: None,
            y: None,
        }; MAX_CONTACTS],
        contact_slots: 0,
        contact_count: None,
        input_mode: None,
    };

    fn payload<'a>(self, input: &'a [u8]) -> Option<&'a [u8]> {
        let prefix = usize::from(self.id != 0);
        if prefix != 0 && input.first().copied()? != self.id {
            return None;
        }
        let bytes = usize::from(self.input_bits).div_ceil(8);
        input.get(prefix..prefix + bytes)
    }

    fn buttons(self, payload: &[u8]) -> Option<u8> {
        let mut buttons = 0;
        for (index, field) in self.buttons.into_iter().enumerate() {
            if let Some(field) = field {
                if field.read(payload)? != 0 {
                    buttons |= 1 << index;
                }
            }
        }
        Some(buttons)
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Application {
    #[default]
    Other,
    Mouse,
    Touchpad,
    Configuration,
}

#[derive(Clone, Copy, Default)]
struct Collection {
    app: Application,
    contact: Option<usize>,
}

#[derive(Clone, Copy)]
struct Globals {
    page: u32,
    min: i32,
    max: i32,
    size: u8,
    count: u16,
    report_id: u8,
}

impl Globals {
    const EMPTY: Self = Self {
        page: 0,
        min: 0,
        max: 0,
        size: 0,
        count: 0,
        report_id: 0,
    };
}

struct Locals {
    usages: [u32; MAX_USAGES],
    len: usize,
    min: Option<u32>,
    max: Option<u32>,
}

impl Locals {
    fn new() -> Self {
        Self {
            usages: [0; MAX_USAGES],
            len: 0,
            min: None,
            max: None,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.min = None;
        self.max = None;
    }

    fn usage(&self, index: u16, page: u32) -> Option<u32> {
        let raw = if usize::from(index) < self.len {
            self.usages[usize::from(index)]
        } else if let Some(min) = self.min {
            min.checked_add(u32::from(index))?
                .min(self.max.unwrap_or(min))
        } else {
            *self.usages.get(self.len.checked_sub(1)?)?
        };
        Some(if raw > 0xffff {
            raw
        } else {
            (page << 16) | raw
        })
    }
}

pub(crate) struct Touchpad {
    mouse: Option<Report>,
    touch: Option<Report>,
    input_mode: Option<Feature>,
    previous: Option<Contact>,
    remainder_x: i64,
    remainder_y: i64,
    buttons: u8,
}

impl Touchpad {
    pub(crate) fn from_descriptor(descriptor: &[u8]) -> Result<Self, ParseError> {
        let (reports, count) = parse(descriptor)?;
        let reports = &reports[..count];
        let mouse = reports
            .iter()
            .copied()
            .find(|report| report.mouse_x.is_some() && report.mouse_y.is_some());
        let touch = reports.iter().copied().find(|report| {
            report.contact_slots > 0
                && report.contacts[..report.contact_slots]
                    .iter()
                    .all(|contact| contact.complete())
        });
        if mouse.is_none() && touch.is_none() {
            return Err(ParseError::NoPointer);
        }
        let input_mode = reports.iter().find_map(|report| {
            let field = report.input_mode?;
            Some(Feature {
                report_id: report.id,
                report_len: report.feature_bits.div_ceil(8) + u16::from(report.id != 0),
                bit_offset: field.offset,
                bit_size: field.size,
            })
        });
        Ok(Self {
            mouse,
            touch,
            input_mode,
            previous: None,
            remainder_x: 0,
            remainder_y: 0,
            buttons: 0,
        })
    }

    pub(crate) fn input_mode_feature(&self) -> Option<Feature> {
        self.input_mode
    }

    pub(crate) fn has_precision_reports(&self) -> bool {
        self.touch.is_some()
    }

    /// The I2C input length includes its own two-byte prefix. Padding returned
    /// by a fixed-size I2C read is ignored; reset-complete (zero) is not input.
    pub(crate) fn decode_i2c(&mut self, input: &[u8]) -> Option<Pointer> {
        let len = usize::from(u16::from_le_bytes(input.get(..2)?.try_into().ok()?));
        if len < 2 {
            return None;
        }
        self.decode_hid(input.get(2..len)?)
    }

    pub(crate) fn decode_hid(&mut self, input: &[u8]) -> Option<Pointer> {
        if let Some(report) = self.mouse {
            if let Some(payload) = report.payload(input) {
                self.previous = None;
                self.remainder_x = 0;
                self.remainder_y = 0;
                return self.emit(Pointer {
                    buttons: report.buttons(payload)?,
                    dx: report.mouse_x?.read(payload)?,
                    dy: report.mouse_y?.read(payload)?,
                    wheel: match report.wheel {
                        Some(field) => field.read(payload)?,
                        None => 0,
                    },
                });
            }
        }
        let report = self.touch?;
        let payload = report.payload(input)?;
        let buttons = report.buttons(payload)?;
        let count = match report.contact_count {
            Some(field) => field.read(payload)?,
            None => report.contact_slots as i32,
        };
        let contacts = &report.contacts[..report.contact_slots];
        let mut selected = None;
        if count > 0 && count <= report.contact_slots as i32 {
            for (index, fields) in contacts.iter().copied().enumerate() {
                if let Some(contact) = fields.read(payload, index) {
                    if selected.is_none() || self.previous.is_some_and(|old| old.id == contact.id) {
                        selected = Some((contact, fields));
                    }
                    if self.previous.is_some_and(|old| old.id == contact.id) {
                        break;
                    }
                }
            }
        }
        let mut pointer = Pointer {
            buttons,
            ..Pointer::default()
        };
        if let Some((contact, fields)) = selected {
            if let Some(previous) = self.previous.filter(|old| old.id == contact.id) {
                pointer.dx =
                    normalized_delta(contact.x, previous.x, fields.x?, &mut self.remainder_x);
                pointer.dy =
                    normalized_delta(contact.y, previous.y, fields.y?, &mut self.remainder_y);
            } else {
                self.remainder_x = 0;
                self.remainder_y = 0;
            }
            self.previous = Some(contact);
        } else {
            self.previous = None;
            self.remainder_x = 0;
            self.remainder_y = 0;
        }
        self.emit(pointer)
    }

    /// A transport reset/lost packet must not carry a drag or finger baseline
    /// into the next stream. Return the button release only when needed.
    pub(crate) fn reset(&mut self) -> Option<Pointer> {
        self.previous = None;
        self.remainder_x = 0;
        self.remainder_y = 0;
        self.emit(Pointer::default())
    }

    fn emit(&mut self, pointer: Pointer) -> Option<Pointer> {
        let changed = pointer.buttons != self.buttons
            || pointer.dx != 0
            || pointer.dy != 0
            || pointer.wheel != 0;
        self.buttons = pointer.buttons;
        changed.then_some(pointer)
    }
}

fn normalized_delta(now: i32, old: i32, field: Field, remainder: &mut i64) -> i32 {
    let range = i64::from(field.max) - i64::from(field.min);
    let scaled = (i64::from(now) - i64::from(old)) * POINTER_UNITS + *remainder;
    *remainder = scaled % range;
    (scaled / range) as i32
}

fn report_index(
    reports: &mut [Report; MAX_REPORTS],
    count: &mut usize,
    id: u8,
) -> Result<usize, ParseError> {
    if let Some(index) = reports[..*count].iter().position(|report| report.id == id) {
        return Ok(index);
    }
    if *count == MAX_REPORTS {
        return Err(ParseError::TooLarge);
    }
    let index = *count;
    reports[index].id = id;
    *count += 1;
    Ok(index)
}

fn unsigned(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0, |value, (index, byte)| {
        value | (u32::from(*byte) << (index * 8))
    })
}

fn signed(bytes: &[u8]) -> i32 {
    let value = unsigned(bytes);
    let shift = 32 - bytes.len() * 8;
    if bytes.is_empty() {
        0
    } else {
        ((value << shift) as i32) >> shift
    }
}

fn parse(descriptor: &[u8]) -> Result<([Report; MAX_REPORTS], usize), ParseError> {
    if descriptor.len() > MAX_DESCRIPTOR {
        return Err(ParseError::TooLarge);
    }
    let mut globals = Globals::EMPTY;
    let mut global_stack = [Globals::EMPTY; MAX_GLOBALS];
    let mut global_depth = 0;
    let mut locals = Locals::new();
    let mut collections = [Collection::default(); MAX_COLLECTIONS];
    let mut depth = 0;
    let mut reports = [Report::EMPTY; MAX_REPORTS];
    let mut report_count = 0;
    let mut index = 0;
    while index < descriptor.len() {
        let prefix = descriptor[index];
        index += 1;
        if prefix == 0xfe {
            let header = descriptor
                .get(index..index + 2)
                .ok_or(ParseError::Truncated)?;
            index = index
                .checked_add(2 + usize::from(header[0]))
                .ok_or(ParseError::TooLarge)?;
            if index > descriptor.len() {
                return Err(ParseError::Truncated);
            }
            continue;
        }
        let size = match prefix & 3 {
            3 => 4,
            value => usize::from(value),
        };
        let bytes = descriptor
            .get(index..index + size)
            .ok_or(ParseError::Truncated)?;
        index += size;
        let value = unsigned(bytes);
        let kind = (prefix >> 2) & 3;
        let tag = prefix >> 4;
        match (kind, tag) {
            (1, 0) => {
                if value > 0xffff {
                    return Err(ParseError::TooLarge);
                }
                globals.page = value;
            }
            (1, 1) => globals.min = signed(bytes),
            (1, 2) => {
                globals.max = if globals.min < 0 {
                    signed(bytes)
                } else {
                    value.min(i32::MAX as u32) as i32
                }
            }
            (1, 7) => {
                globals.size = u8::try_from(value).map_err(|_| ParseError::TooLarge)?;
            }
            (1, 8) => {
                globals.report_id = u8::try_from(value)
                    .ok()
                    .filter(|id| *id != 0)
                    .ok_or(ParseError::Malformed)?;
            }
            (1, 9) => {
                globals.count = u16::try_from(value).map_err(|_| ParseError::TooLarge)?;
            }
            (1, 10) => {
                if global_depth == MAX_GLOBALS {
                    return Err(ParseError::TooLarge);
                }
                global_stack[global_depth] = globals;
                global_depth += 1;
            }
            (1, 11) => {
                global_depth = global_depth.checked_sub(1).ok_or(ParseError::Malformed)?;
                globals = global_stack[global_depth];
            }
            (2, 0) => {
                if locals.len == MAX_USAGES {
                    return Err(ParseError::TooLarge);
                }
                locals.usages[locals.len] = value;
                locals.len += 1;
            }
            (2, 1) => locals.min = Some(value),
            (2, 2) => locals.max = Some(value),
            (0, 10) => {
                if depth == MAX_COLLECTIONS {
                    return Err(ParseError::TooLarge);
                }
                let mut collection = if depth == 0 {
                    Collection::default()
                } else {
                    collections[depth - 1]
                };
                let usage = locals.usage(0, globals.page);
                if value == 1 {
                    collection.app = match usage {
                        Some(0x0001_0002) => Application::Mouse,
                        Some(0x000d_0005) => Application::Touchpad,
                        Some(0x000d_000e) => Application::Configuration,
                        _ => Application::Other,
                    };
                    collection.contact = None;
                }
                if collection.app == Application::Touchpad && usage == Some(0x000d_0022) {
                    let index = report_index(&mut reports, &mut report_count, globals.report_id)?;
                    let report = &mut reports[index];
                    if report.contact_slots == MAX_CONTACTS {
                        return Err(ParseError::TooLarge);
                    }
                    collection.contact = Some(report.contact_slots);
                    report.contact_slots += 1;
                }
                collections[depth] = collection;
                depth += 1;
            }
            (0, 12) => {
                depth = depth.checked_sub(1).ok_or(ParseError::Malformed)?;
            }
            (0, 8 | 11) => {
                let report_index =
                    report_index(&mut reports, &mut report_count, globals.report_id)?;
                let report = &mut reports[report_index];
                let bits = u32::from(globals.size)
                    .checked_mul(u32::from(globals.count))
                    .ok_or(ParseError::TooLarge)?;
                let cursor = if tag == 8 {
                    &mut report.input_bits
                } else {
                    &mut report.feature_bits
                };
                let start = *cursor;
                let end = u32::from(start)
                    .checked_add(bits)
                    .ok_or(ParseError::TooLarge)?;
                if end > MAX_REPORT_BITS {
                    return Err(ParseError::TooLarge);
                }
                *cursor = end as u16;
                let collection = if depth == 0 {
                    Collection::default()
                } else {
                    collections[depth - 1]
                };
                // Only variable data fields map independently to usages.
                if value & 3 == 2 && (1..=32).contains(&globals.size) {
                    for field_index in 0..globals.count {
                        let Some(usage) = locals.usage(field_index, globals.page) else {
                            continue;
                        };
                        let field = Field {
                            offset: start + u16::from(globals.size) * field_index,
                            size: globals.size,
                            min: globals.min,
                            max: globals.max,
                        };
                        if tag == 11 {
                            if collection.app == Application::Configuration && usage == 0x000d_0052
                            {
                                report.input_mode = Some(field);
                            }
                            continue;
                        }
                        if matches!(collection.app, Application::Mouse | Application::Touchpad)
                            && (0x0009_0001..=0x0009_0008).contains(&usage)
                        {
                            report.buttons[(usage - 0x0009_0001) as usize] = Some(field);
                        }
                        if collection.app == Application::Mouse && value & 4 != 0 {
                            match usage {
                                0x0001_0030 => report.mouse_x = Some(field),
                                0x0001_0031 => report.mouse_y = Some(field),
                                0x0001_0038 => report.wheel = Some(field),
                                _ => {}
                            }
                        }
                        if collection.app == Application::Touchpad && value & 4 == 0 {
                            if let Some(contact) = collection.contact {
                                match usage {
                                    0x000d_0051 => report.contacts[contact].id = Some(field),
                                    0x000d_0042 => report.contacts[contact].tip = Some(field),
                                    0x000d_0047 => {
                                        report.contacts[contact].confidence = Some(field)
                                    }
                                    0x0001_0030 => report.contacts[contact].x = Some(field),
                                    0x0001_0031 => report.contacts[contact].y = Some(field),
                                    _ => {}
                                }
                            } else if usage == 0x000d_0054 {
                                report.contact_count = Some(field);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if kind == 0 {
            locals.clear();
        }
    }
    if depth != 0 || global_depth != 0 {
        return Err(ParseError::Malformed);
    }
    Ok((reports, report_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from this laptop's SYNA30B8 / 06CB:CE1A under Ubuntu. The
    // descriptor contains the relative mouse, five-contact touchpad and
    // Digitizers Configuration collections, not a hand-written test layout.
    const SYNAPTICS: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x85, 0x02, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19,
        0x01, 0x29, 0x02, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02, 0x95, 0x06,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95,
        0x02, 0x81, 0x06, 0xc0, 0xc0, 0x05, 0x0d, 0x09, 0x05, 0xa1, 0x01, 0x85, 0x03, 0x05, 0x0d,
        0x09, 0x22, 0xa1, 0x02, 0x15, 0x00, 0x25, 0x01, 0x09, 0x47, 0x09, 0x42, 0x95, 0x02, 0x75,
        0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x25, 0x05, 0x09, 0x51, 0x81, 0x02, 0x75, 0x01,
        0x95, 0x03, 0x81, 0x03, 0x05, 0x01, 0x15, 0x00, 0x26, 0x34, 0x05, 0x75, 0x10, 0x55, 0x0e,
        0x65, 0x11, 0x09, 0x30, 0x35, 0x00, 0x46, 0x56, 0x04, 0x95, 0x01, 0x81, 0x02, 0x46, 0x94,
        0x02, 0x26, 0x18, 0x03, 0x09, 0x31, 0x81, 0x02, 0xc0, 0x05, 0x0d, 0x09, 0x22, 0xa1, 0x02,
        0x15, 0x00, 0x25, 0x01, 0x09, 0x47, 0x09, 0x42, 0x95, 0x02, 0x75, 0x01, 0x81, 0x02, 0x95,
        0x01, 0x75, 0x03, 0x25, 0x05, 0x09, 0x51, 0x81, 0x02, 0x75, 0x01, 0x95, 0x03, 0x81, 0x03,
        0x05, 0x01, 0x15, 0x00, 0x26, 0x34, 0x05, 0x75, 0x10, 0x55, 0x0e, 0x65, 0x11, 0x09, 0x30,
        0x35, 0x00, 0x46, 0x56, 0x04, 0x95, 0x01, 0x81, 0x02, 0x46, 0x94, 0x02, 0x26, 0x18, 0x03,
        0x09, 0x31, 0x81, 0x02, 0xc0, 0x05, 0x0d, 0x09, 0x22, 0xa1, 0x02, 0x15, 0x00, 0x25, 0x01,
        0x09, 0x47, 0x09, 0x42, 0x95, 0x02, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x25,
        0x05, 0x09, 0x51, 0x81, 0x02, 0x75, 0x01, 0x95, 0x03, 0x81, 0x03, 0x05, 0x01, 0x15, 0x00,
        0x26, 0x34, 0x05, 0x75, 0x10, 0x55, 0x0e, 0x65, 0x11, 0x09, 0x30, 0x35, 0x00, 0x46, 0x56,
        0x04, 0x95, 0x01, 0x81, 0x02, 0x46, 0x94, 0x02, 0x26, 0x18, 0x03, 0x09, 0x31, 0x81, 0x02,
        0xc0, 0x05, 0x0d, 0x09, 0x22, 0xa1, 0x02, 0x15, 0x00, 0x25, 0x01, 0x09, 0x47, 0x09, 0x42,
        0x95, 0x02, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x25, 0x05, 0x09, 0x51, 0x81,
        0x02, 0x75, 0x01, 0x95, 0x03, 0x81, 0x03, 0x05, 0x01, 0x15, 0x00, 0x26, 0x34, 0x05, 0x75,
        0x10, 0x55, 0x0e, 0x65, 0x11, 0x09, 0x30, 0x35, 0x00, 0x46, 0x56, 0x04, 0x95, 0x01, 0x81,
        0x02, 0x46, 0x94, 0x02, 0x26, 0x18, 0x03, 0x09, 0x31, 0x81, 0x02, 0xc0, 0x05, 0x0d, 0x09,
        0x22, 0xa1, 0x02, 0x15, 0x00, 0x25, 0x01, 0x09, 0x47, 0x09, 0x42, 0x95, 0x02, 0x75, 0x01,
        0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x25, 0x05, 0x09, 0x51, 0x81, 0x02, 0x75, 0x01, 0x95,
        0x03, 0x81, 0x03, 0x05, 0x01, 0x15, 0x00, 0x26, 0x34, 0x05, 0x75, 0x10, 0x55, 0x0e, 0x65,
        0x11, 0x09, 0x30, 0x35, 0x00, 0x46, 0x56, 0x04, 0x95, 0x01, 0x81, 0x02, 0x46, 0x94, 0x02,
        0x26, 0x18, 0x03, 0x09, 0x31, 0x81, 0x02, 0xc0, 0x05, 0x0d, 0x55, 0x0c, 0x66, 0x01, 0x10,
        0x47, 0xff, 0xff, 0x00, 0x00, 0x27, 0xff, 0xff, 0x00, 0x00, 0x75, 0x10, 0x95, 0x01, 0x09,
        0x56, 0x81, 0x02, 0x09, 0x54, 0x25, 0x7f, 0x95, 0x01, 0x75, 0x08, 0x81, 0x02, 0x05, 0x09,
        0x09, 0x01, 0x25, 0x01, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, 0x95, 0x07, 0x81, 0x03, 0x05,
        0x0d, 0x85, 0x08, 0x09, 0x55, 0x09, 0x59, 0x75, 0x04, 0x95, 0x02, 0x25, 0x0f, 0xb1, 0x02,
        0x85, 0x0d, 0x09, 0x60, 0x75, 0x01, 0x95, 0x01, 0x15, 0x00, 0x25, 0x01, 0xb1, 0x02, 0x95,
        0x07, 0xb1, 0x03, 0x85, 0x07, 0x06, 0x00, 0xff, 0x09, 0xc5, 0x15, 0x00, 0x26, 0xff, 0x00,
        0x75, 0x08, 0x96, 0x00, 0x01, 0xb1, 0x02, 0xc0, 0x05, 0x0d, 0x09, 0x0e, 0xa1, 0x01, 0x85,
        0x04, 0x09, 0x22, 0xa1, 0x02, 0x09, 0x52, 0x15, 0x00, 0x25, 0x0a, 0x75, 0x08, 0x95, 0x01,
        0xb1, 0x02, 0xc0, 0x09, 0x22, 0xa1, 0x00, 0x85, 0x06, 0x09, 0x57, 0x09, 0x58, 0x75, 0x01,
        0x95, 0x02, 0x25, 0x01, 0xb1, 0x02, 0x95, 0x06, 0xb1, 0x03, 0xc0, 0xc0, 0x06, 0x00, 0xff,
        0x09, 0x01, 0xa1, 0x01, 0x85, 0x09, 0x09, 0x02, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
        0x95, 0x14, 0x91, 0x02, 0x85, 0x0a, 0x09, 0x03, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
        0x95, 0x14, 0x91, 0x02, 0x85, 0x0b, 0x09, 0x04, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
        0x95, 0x3d, 0x81, 0x02, 0x85, 0x0c, 0x09, 0x05, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
        0x95, 0x3d, 0x81, 0x02, 0x85, 0x0f, 0x09, 0x06, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
        0x95, 0x03, 0xb1, 0x02, 0x85, 0x0e, 0x09, 0x07, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
        0x95, 0x01, 0xb1, 0x02, 0xc0,
    ];

    #[derive(Clone, Copy)]
    struct Finger {
        id: u8,
        x: u16,
        y: u16,
        confident: bool,
        down: bool,
    }

    fn finger(id: u8, x: u16, y: u16) -> Finger {
        Finger {
            id,
            x,
            y,
            confident: true,
            down: true,
        }
    }

    fn precision(fingers: &[Finger], count: u8, buttons: u8) -> [u8; 30] {
        let mut report = [0; 30];
        report[0] = 3;
        for (slot, finger) in fingers.iter().enumerate() {
            let offset = 1 + slot * 5;
            report[offset] =
                (finger.id << 2) | u8::from(finger.confident) | (u8::from(finger.down) << 1);
            report[offset + 1..offset + 3].copy_from_slice(&finger.x.to_le_bytes());
            report[offset + 3..offset + 5].copy_from_slice(&finger.y.to_le_bytes());
        }
        report[28] = count;
        report[29] = buttons;
        report
    }

    #[test]
    fn captured_descriptor_discovers_both_reports_and_input_mode() {
        let pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        assert_eq!(SYNAPTICS.len(), 665);
        assert!(pad.has_precision_reports());
        let mouse = pad.mouse.unwrap();
        assert_eq!((mouse.id, mouse.input_bits), (2, 24));
        let touch = pad.touch.unwrap();
        assert_eq!(
            (touch.id, touch.input_bits, touch.contact_slots),
            (3, 232, 5)
        );
        assert_eq!(touch.contacts[0].id.unwrap().offset, 2);
        assert_eq!(touch.contacts[0].x.unwrap().max, 1332);
        assert_eq!(touch.contacts[0].y.unwrap().max, 792);
        assert_eq!(touch.contact_count.unwrap().offset, 216);
        let feature = pad.input_mode_feature().unwrap();
        assert_eq!(
            feature,
            Feature {
                report_id: 4,
                report_len: 2,
                bit_offset: 0,
                bit_size: 8
            }
        );
        let mut report = [0xff; 8];
        assert_eq!(feature.report_with_value(3, &mut report), Some(2));
        assert_eq!(&report[..2], &[4, 3]);
        assert_eq!(feature.report_with_value(0, &mut report), Some(2));
        assert_eq!(&report[..2], &[4, 0]);
    }

    #[test]
    fn compatibility_mouse_is_signed_and_i2c_padding_is_not_data() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        let frame = [6, 0, 2, 1, (-7i8) as u8, 9, 0xaa, 0xbb];
        assert_eq!(
            pad.decode_i2c(&frame),
            Some(Pointer {
                buttons: 1,
                dx: -7,
                dy: 9,
                wheel: 0
            })
        );
        assert_eq!(pad.decode_hid(&[2, 0, 0, 0]), Some(Pointer::default()));
        assert_eq!(pad.decode_hid(&[2, 0, 0, 0]), None);
    }

    #[test]
    fn relative_mouse_without_report_id_supports_wider_axes() {
        let descriptor = [
            0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, // Mouse application
            0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x03,
            0x81, 0x02, // Three buttons
            0x75, 0x05, 0x95, 0x01, 0x81, 0x01, // Constant padding
            0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x16, 0x00, 0x80, 0x26, 0xff,
            0x7f, // Signed 16-bit range
            0x75, 0x10, 0x95, 0x02, 0x81, 0x06, 0xc0,
        ];
        let mut pad = Touchpad::from_descriptor(&descriptor).unwrap();
        assert!(!pad.has_precision_reports());
        assert_eq!(pad.input_mode_feature(), None);
        let [x0, x1] = (-300i16).to_le_bytes();
        let [y0, y1] = 400i16.to_le_bytes();
        assert_eq!(
            pad.decode_hid(&[5, x0, x1, y0, y1]),
            Some(Pointer {
                buttons: 5,
                dx: -300,
                dy: 400,
                wheel: 0
            })
        );
    }

    #[test]
    fn report_ids_come_from_the_descriptor() {
        let mut descriptor = SYNAPTICS.to_vec();
        for index in 0..descriptor.len() - 1 {
            if descriptor[index] == 0x85 && (2..=4).contains(&descriptor[index + 1]) {
                descriptor[index + 1] += 16;
            }
        }
        let mut pad = Touchpad::from_descriptor(&descriptor).unwrap();
        assert_eq!(pad.input_mode_feature().unwrap().report_id, 20);
        assert_eq!(
            pad.decode_hid(&[18, 0, 3, 4]),
            Some(Pointer {
                dx: 3,
                dy: 4,
                ..Pointer::default()
            })
        );
        let mut report = precision(&[finger(1, 100, 100)], 1, 0);
        report[0] = 19;
        assert_eq!(pad.decode_hid(&report), None);
        report = precision(&[finger(1, 766, 496)], 1, 0);
        report[0] = 19;
        assert_eq!(
            pad.decode_hid(&report),
            Some(Pointer {
                dx: 512,
                dy: 512,
                ..Pointer::default()
            })
        );
    }

    #[test]
    fn touch_down_sets_baseline_and_full_axis_ranges_match_pointer_units() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 100, 100)], 1, 0)),
            None
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 766, 496)], 1, 0)),
            Some(Pointer {
                dx: 512,
                dy: 512,
                ..Pointer::default()
            })
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 766, 496)], 1, 0)),
            None
        );
    }

    #[test]
    fn fractional_movement_accumulates_without_direction_bias() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        pad.decode_hid(&precision(&[finger(1, 100, 100)], 1, 0));
        let mut forward = 0;
        for x in 101..=200 {
            forward += pad
                .decode_hid(&precision(&[finger(1, x, 100)], 1, 0))
                .map(|event| event.dx)
                .unwrap_or(0);
        }
        assert_eq!(forward, 100 * 1024 / 1332);
        let mut backward = 0;
        for x in (100..200).rev() {
            backward += pad
                .decode_hid(&precision(&[finger(1, x, 100)], 1, 0))
                .map(|event| event.dx)
                .unwrap_or(0);
        }
        assert_eq!(forward + backward, 0);
    }

    #[test]
    fn lift_and_recontact_never_jump() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        pad.decode_hid(&precision(&[finger(1, 10, 10)], 1, 0));
        assert_eq!(pad.decode_hid(&precision(&[], 0, 0)), None);
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1200, 700)], 1, 0)),
            None
        );
        let moved = pad
            .decode_hid(&precision(&[finger(1, 1190, 690)], 1, 0))
            .unwrap();
        assert!(moved.dx < 0 && moved.dy < 0);
    }

    #[test]
    fn contact_id_is_stable_across_slot_reordering_and_changes_rebase() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        pad.decode_hid(&precision(&[finger(2, 100, 100)], 1, 0));
        let moved = pad
            .decode_hid(&precision(
                &[finger(3, 1000, 700), finger(2, 120, 100)],
                2,
                0,
            ))
            .unwrap();
        assert_eq!((moved.dx, moved.dy), (20 * 1024 / 1332, 0));
        assert_eq!(
            pad.decode_hid(&precision(&[finger(3, 1000, 700)], 1, 0)),
            None
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(3, 1010, 700)], 1, 0))
                .unwrap()
                .dx,
            10 * 1024 / 1332
        );
    }

    #[test]
    fn nonconfident_palms_are_not_pointer_contacts() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        let mut palm = finger(1, 100, 100);
        palm.confident = false;
        assert_eq!(pad.decode_hid(&precision(&[palm], 1, 0)), None);
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 500, 400)], 1, 0)),
            None
        );
        palm.x = 700;
        assert_eq!(pad.decode_hid(&precision(&[palm], 1, 0)), None);
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 700, 400)], 1, 0)),
            None
        );
    }

    #[test]
    fn physical_clicks_are_independent_of_fingers_and_reset_releases_drag() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        assert_eq!(
            pad.decode_hid(&precision(&[], 0, 1)),
            Some(Pointer {
                buttons: 1,
                ..Pointer::default()
            })
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 100, 100)], 1, 1)),
            None
        );
        assert_eq!(pad.reset(), Some(Pointer::default()));
        assert_eq!(pad.reset(), None);
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1000, 700)], 1, 0)),
            None
        );
    }

    #[test]
    fn zero_contact_count_ignores_stale_tip_bits_and_incomplete_frames_rebase() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        pad.decode_hid(&precision(&[finger(1, 100, 100)], 1, 0));
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1000, 700)], 0, 0)),
            None
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1000, 700)], 1, 0)),
            None
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1100, 700)], 6, 0)),
            None
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1200, 700)], 1, 0)),
            None
        );
    }

    #[test]
    fn out_of_range_contacts_and_unknown_or_truncated_reports_are_ignored() {
        let mut pad = Touchpad::from_descriptor(SYNAPTICS).unwrap();
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 2000, 100)], 1, 0)),
            None
        );
        assert_eq!(
            pad.decode_hid(&precision(&[finger(7, 100, 100)], 1, 0)),
            None
        );
        assert_eq!(pad.decode_hid(&[0x0b, 1, 2, 3]), None);
        assert_eq!(pad.decode_hid(&[2, 1]), None);
        assert_eq!(pad.decode_i2c(&[0, 0]), None);
        assert_eq!(pad.decode_i2c(&[7, 0, 2, 1, 2, 3]), None);
        assert_eq!(pad.decode_i2c(&[1]), None);
        assert_eq!(
            pad.decode_hid(&precision(&[finger(1, 1000, 700)], 1, 0)),
            None
        );
    }

    #[test]
    fn feature_patching_preserves_other_bits_and_checks_bounds() {
        let feature = Feature {
            report_id: 8,
            report_len: 2,
            bit_offset: 4,
            bit_size: 2,
        };
        let mut report = [8, 0xa5];
        assert_eq!(feature.set_value(1, &mut report), Some(2));
        assert_eq!(report, [8, 0x95]);
        assert_eq!(feature.set_value(4, &mut report), None);
        assert_eq!(feature.set_value(1, &mut []), None);
        assert_eq!(
            Feature {
                report_len: 0,
                ..feature
            }
            .set_value(1, &mut report),
            None
        );
        assert_eq!(
            Feature {
                bit_size: 33,
                ..feature
            }
            .set_value(1, &mut report),
            None
        );
    }

    #[test]
    fn malformed_descriptors_fail_boundedly() {
        for descriptor in [
            &[][..],
            &[0x75],
            &[0xfe, 3, 0],
            &[0xc0],
            &[0xb4],
            &[0x85, 0],
            &[0x75, 32, 0x96, 0xff, 0xff, 0x81, 2],
        ] {
            assert!(Touchpad::from_descriptor(descriptor).is_err());
        }
        assert!(matches!(
            Touchpad::from_descriptor(&[0; MAX_DESCRIPTOR + 1]),
            Err(ParseError::TooLarge)
        ));
        for end in 0..SYNAPTICS.len() {
            let _ = Touchpad::from_descriptor(&SYNAPTICS[..end]);
        }
        for index in 0..SYNAPTICS.len() {
            let mut mutated = SYNAPTICS.to_vec();
            mutated[index] ^= 0xff;
            let _ = Touchpad::from_descriptor(&mutated);
        }
    }
}
