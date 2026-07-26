//! Minimal HID input-report layout parser for relative mice.
//!
//! Report-protocol mice are not required to use the boot packet layout. In
//! particular, gaming mice commonly place extra button or vendor-defined
//! fields before signed 16-bit X/Y fields. Keep the parsed representation
//! small and decode only the usages consumed by TRUEOS.

extern crate alloc;

use alloc::vec::Vec;

const HID_USAGE_PAGE_GENERIC_DESKTOP: u32 = 0x01;
const HID_USAGE_PAGE_BUTTON: u32 = 0x09;
const HID_USAGE_X: u32 = 0x30;
const HID_USAGE_Y: u32 = 0x31;
const HID_USAGE_WHEEL: u32 = 0x38;
const MAX_BUTTONS: usize = 8;
const MAX_FIELD_BITS: u8 = 32;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ReportField {
    bit_offset: u16,
    bit_size: u8,
    signed: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MouseReportLayout {
    report_id: Option<u8>,
    button_bit_offsets: [u16; MAX_BUTTONS],
    button_mask: u8,
    x: ReportField,
    y: ReportField,
    wheel: Option<ReportField>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedMouseReport {
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
}

impl MouseReportLayout {
    #[inline]
    pub(crate) fn report_id(self) -> Option<u8> {
        self.report_id
    }

    #[inline]
    pub(crate) fn x_bit_offset(self) -> u16 {
        self.x.bit_offset
    }

    #[inline]
    pub(crate) fn y_bit_offset(self) -> u16 {
        self.y.bit_offset
    }

    #[inline]
    pub(crate) fn axis_bits(self) -> (u8, u8) {
        (self.x.bit_size, self.y.bit_size)
    }

    #[inline]
    pub(crate) fn wheel_bit_offset(self) -> Option<u16> {
        self.wheel.map(|field| field.bit_offset)
    }

    pub(crate) fn decode(self, report: &[u8]) -> Option<DecodedMouseReport> {
        let payload = match self.report_id {
            Some(report_id) => {
                if report.first().copied()? != report_id {
                    return None;
                }
                &report[1..]
            }
            None => report,
        };

        let mut buttons = 0u8;
        for usage_index in 0..MAX_BUTTONS {
            let usage_mask = 1u8 << usage_index;
            if (self.button_mask & usage_mask) == 0 {
                continue;
            }
            if read_unsigned_bits(payload, self.button_bit_offsets[usage_index], 1)? != 0 {
                buttons |= usage_mask;
            }
        }

        Some(DecodedMouseReport {
            buttons,
            dx: read_field(payload, self.x)?,
            dy: read_field(payload, self.y)?,
            wheel: match self.wheel {
                Some(field) => read_field(payload, field)?,
                None => 0,
            },
        })
    }
}

#[derive(Copy, Clone, Debug)]
struct GlobalState {
    usage_page: u32,
    logical_minimum: i32,
    report_size: u8,
    report_count: u16,
    report_id: Option<u8>,
}

impl GlobalState {
    const fn new() -> Self {
        Self {
            usage_page: 0,
            logical_minimum: 0,
            report_size: 0,
            report_count: 0,
            report_id: None,
        }
    }
}

#[derive(Clone, Debug)]
struct LocalState {
    usages: Vec<u32>,
    usage_minimum: Option<u32>,
    usage_maximum: Option<u32>,
}

impl LocalState {
    fn new() -> Self {
        Self {
            usages: Vec::new(),
            usage_minimum: None,
            usage_maximum: None,
        }
    }

    fn clear(&mut self) {
        self.usages.clear();
        self.usage_minimum = None;
        self.usage_maximum = None;
    }

    fn usage_at(&self, index: u16) -> Option<u32> {
        if let Some(usage) = self.usages.get(usize::from(index)).copied() {
            return Some(usage);
        }
        if let Some(usage) = self.usages.last().copied() {
            return Some(usage);
        }
        let minimum = self.usage_minimum?;
        let maximum = self.usage_maximum.unwrap_or(minimum);
        Some(minimum.saturating_add(u32::from(index)).min(maximum))
    }
}

#[derive(Clone, Debug)]
struct ReportLayoutBuilder {
    report_id: Option<u8>,
    next_input_bit: u16,
    button_bit_offsets: [u16; MAX_BUTTONS],
    button_mask: u8,
    x: Option<ReportField>,
    y: Option<ReportField>,
    wheel: Option<ReportField>,
}

impl ReportLayoutBuilder {
    const fn new(report_id: Option<u8>) -> Self {
        Self {
            report_id,
            next_input_bit: 0,
            button_bit_offsets: [0; MAX_BUTTONS],
            button_mask: 0,
            x: None,
            y: None,
            wheel: None,
        }
    }

    fn finish(self) -> Option<MouseReportLayout> {
        Some(MouseReportLayout {
            report_id: self.report_id,
            button_bit_offsets: self.button_bit_offsets,
            button_mask: self.button_mask,
            x: self.x?,
            y: self.y?,
            wheel: self.wheel,
        })
    }
}

pub(crate) fn parse_mouse_report_layout(report_descriptor: &[u8]) -> Option<MouseReportLayout> {
    let mut index = 0usize;
    let mut globals = GlobalState::new();
    let mut global_stack = Vec::new();
    let mut locals = LocalState::new();
    let mut reports: Vec<ReportLayoutBuilder> = Vec::new();

    while index < report_descriptor.len() {
        let prefix = report_descriptor[index];
        index += 1;

        if prefix == 0xfe {
            if index + 1 >= report_descriptor.len() {
                break;
            }
            let long_size = usize::from(report_descriptor[index]);
            index = index.saturating_add(2).saturating_add(long_size);
            locals.clear();
            continue;
        }

        let item_size = match prefix & 0x03 {
            0 => 0usize,
            1 => 1usize,
            2 => 2usize,
            _ => 4usize,
        };
        if index + item_size > report_descriptor.len() {
            break;
        }

        let item_data = &report_descriptor[index..index + item_size];
        let value = item_unsigned(item_data);
        index += item_size;

        let item_type = (prefix >> 2) & 0x03;
        let item_tag = (prefix >> 4) & 0x0f;
        match (item_type, item_tag) {
            // Input
            (0, 8) => {
                record_input_fields(&mut reports, globals, &locals, value);
                locals.clear();
            }
            // Other main items also delimit local state.
            (0, _) => locals.clear(),
            // Usage Page
            (1, 0) => globals.usage_page = value,
            // Logical Minimum
            (1, 1) => globals.logical_minimum = item_signed(item_data),
            // Report Size
            (1, 7) => globals.report_size = value.min(u32::from(u8::MAX)) as u8,
            // Report ID
            (1, 8) => {
                globals.report_id = u8::try_from(value).ok().filter(|id| *id != 0);
            }
            // Report Count
            (1, 9) => globals.report_count = value.min(u32::from(u16::MAX)) as u16,
            // Push
            (1, 10) => global_stack.push(globals),
            // Pop
            (1, 11) => {
                if let Some(saved) = global_stack.pop() {
                    globals = saved;
                }
            }
            // Usage
            (2, 0) => locals.usages.push(value),
            // Usage Minimum
            (2, 1) => locals.usage_minimum = Some(value),
            // Usage Maximum
            (2, 2) => locals.usage_maximum = Some(value),
            _ => {}
        }
    }

    reports
        .into_iter()
        .filter_map(ReportLayoutBuilder::finish)
        .next()
}

fn record_input_fields(
    reports: &mut Vec<ReportLayoutBuilder>,
    globals: GlobalState,
    locals: &LocalState,
    input_flags: u32,
) {
    let report_index = match reports
        .iter()
        .position(|report| report.report_id == globals.report_id)
    {
        Some(index) => index,
        None => {
            reports.push(ReportLayoutBuilder::new(globals.report_id));
            reports.len() - 1
        }
    };
    let report = &mut reports[report_index];
    let field_bits = u16::from(globals.report_size);
    let total_bits = field_bits.saturating_mul(globals.report_count);
    let input_start = report.next_input_bit;
    report.next_input_bit = report.next_input_bit.saturating_add(total_bits);

    // Constant fields still consume report bits, but have no HID usages.
    if (input_flags & 1) != 0 || globals.report_size == 0 || globals.report_size > MAX_FIELD_BITS {
        return;
    }

    for field_index in 0..globals.report_count {
        let Some(raw_usage) = locals.usage_at(field_index) else {
            continue;
        };
        let (usage_page, usage) = split_usage(globals.usage_page, raw_usage);
        let bit_offset = input_start.saturating_add(field_bits.saturating_mul(field_index));
        let field = ReportField {
            bit_offset,
            bit_size: globals.report_size,
            signed: globals.logical_minimum < 0,
        };

        if usage_page == HID_USAGE_PAGE_BUTTON && (1..=MAX_BUTTONS as u32).contains(&usage) {
            let button_index = (usage - 1) as usize;
            report.button_bit_offsets[button_index] = bit_offset;
            report.button_mask |= 1u8 << button_index;
            continue;
        }
        if usage_page != HID_USAGE_PAGE_GENERIC_DESKTOP {
            continue;
        }
        match usage {
            HID_USAGE_X if report.x.is_none() => report.x = Some(field),
            HID_USAGE_Y if report.y.is_none() => report.y = Some(field),
            HID_USAGE_WHEEL if report.wheel.is_none() => report.wheel = Some(field),
            _ => {}
        }
    }
}

#[inline]
fn split_usage(default_page: u32, raw_usage: u32) -> (u32, u32) {
    if raw_usage > u32::from(u16::MAX) {
        (raw_usage >> 16, raw_usage & u32::from(u16::MAX))
    } else {
        (default_page, raw_usage)
    }
}

#[inline]
fn item_unsigned(data: &[u8]) -> u32 {
    let mut value = 0u32;
    for (index, byte) in data.iter().enumerate() {
        value |= u32::from(*byte) << (index * 8);
    }
    value
}

#[inline]
fn item_signed(data: &[u8]) -> i32 {
    match data {
        [] => 0,
        [value] => i32::from(i8::from_le_bytes([*value])),
        [lo, hi] => i32::from(i16::from_le_bytes([*lo, *hi])),
        [a, b, c, d] => i32::from_le_bytes([*a, *b, *c, *d]),
        _ => 0,
    }
}

#[inline]
fn read_field(report: &[u8], field: ReportField) -> Option<i32> {
    let raw = read_unsigned_bits(report, field.bit_offset, field.bit_size)?;
    if !field.signed {
        return i32::try_from(raw).ok();
    }
    if field.bit_size == 32 {
        return Some(raw as i32);
    }
    let sign_bit = 1u32 << (field.bit_size - 1);
    if (raw & sign_bit) == 0 {
        Some(raw as i32)
    } else {
        Some((raw | (!0u32 << field.bit_size)) as i32)
    }
}

fn read_unsigned_bits(report: &[u8], bit_offset: u16, bit_size: u8) -> Option<u32> {
    if bit_size == 0 || bit_size > MAX_FIELD_BITS {
        return None;
    }
    let bit_end = usize::from(bit_offset).checked_add(usize::from(bit_size))?;
    if bit_end > report.len().checked_mul(8)? {
        return None;
    }

    let mut value = 0u32;
    for output_bit in 0..bit_size {
        let input_bit = usize::from(bit_offset) + usize::from(output_bit);
        let bit = (report[input_bit / 8] >> (input_bit % 8)) & 1;
        value |= u32::from(bit) << output_bit;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASTOR_DESCRIPTOR: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x07, 0x15, 0x00, 0x25, 0x01, 0x95, 0x07, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x01,
        0x81, 0x01, 0x06, 0x00, 0xff, 0x09, 0x40, 0x95, 0x02, 0x75, 0x08, 0x15, 0x81, 0x25, 0x7f,
        0x81, 0x02, 0x05, 0x01, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x95, 0x01, 0x75, 0x08, 0x81,
        0x06, 0x09, 0x30, 0x09, 0x31, 0x16, 0x00, 0x80, 0x26, 0xff, 0x7f, 0x95, 0x02, 0x75, 0x10,
        0x81, 0x06, 0xc0, 0x06, 0x00, 0xff, 0x09, 0x02, 0x15, 0x00, 0x25, 0x01, 0x75, 0x08, 0x95,
        0x5a, 0xb1, 0x01, 0xc0,
    ];

    const LOGITECH_RECEIVER_DESCRIPTOR: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x95, 0x10, 0x75, 0x01, 0x15,
        0x00, 0x25, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x10, 0x81, 0x02, 0x95, 0x02, 0x75, 0x10,
        0x16, 0x01, 0x80, 0x26, 0xff, 0x7f, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x81, 0x06, 0x95,
        0x01, 0x75, 0x08, 0x15, 0x81, 0x25, 0x7f, 0x09, 0x38, 0x81, 0x06, 0x95, 0x01, 0x05, 0x0c,
        0x0a, 0x38, 0x02, 0x81, 0x06, 0xc0, 0x06, 0x00, 0xff, 0x09, 0xf1, 0x75, 0x08, 0x95, 0x05,
        0x15, 0x00, 0x26, 0xff, 0x00, 0x81, 0x00, 0xc0,
    ];

    const REPORT_ID_MOUSE_DESCRIPTOR: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x85, 0x02, 0x09, 0x01, 0xa1, 0x00, 0x95, 0x10, 0x75,
        0x01, 0x15, 0x00, 0x25, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x10, 0x81, 0x02, 0x95, 0x02,
        0x75, 0x10, 0x16, 0x01, 0x80, 0x26, 0xff, 0x7f, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x81,
        0x06, 0x95, 0x01, 0x75, 0x08, 0x15, 0x81, 0x25, 0x7f, 0x09, 0x38, 0x81, 0x06, 0xc0, 0xc0,
    ];

    #[test]
    fn parses_castor_vendor_prefix_and_signed_axes() {
        let layout = parse_mouse_report_layout(CASTOR_DESCRIPTOR).unwrap();
        assert_eq!(layout.report_id(), None);
        assert_eq!(layout.x_bit_offset(), 32);
        assert_eq!(layout.y_bit_offset(), 48);
        assert_eq!(layout.axis_bits(), (16, 16));
        assert_eq!(layout.wheel_bit_offset(), Some(24));

        let decoded = layout
            .decode(&[0x45, 0xaa, 0x55, 0xff, 0x05, 0x00, 0xfd, 0xff])
            .unwrap();
        assert_eq!(
            decoded,
            DecodedMouseReport {
                buttons: 0x45,
                dx: 5,
                dy: -3,
                wheel: -1,
            }
        );
    }

    #[test]
    fn parses_logitech_extra_buttons_before_signed_axes() {
        let layout = parse_mouse_report_layout(LOGITECH_RECEIVER_DESCRIPTOR).unwrap();
        assert_eq!(layout.report_id(), None);
        assert_eq!(layout.x_bit_offset(), 16);
        assert_eq!(layout.y_bit_offset(), 32);
        assert_eq!(layout.axis_bits(), (16, 16));
        assert_eq!(layout.wheel_bit_offset(), Some(48));

        let decoded = layout
            .decode(&[0x21, 0x80, 0x2c, 0x01, 0x00, 0xfe, 0x01, 0xff])
            .unwrap();
        assert_eq!(
            decoded,
            DecodedMouseReport {
                buttons: 0x21,
                dx: 300,
                dy: -512,
                wheel: 1,
            }
        );
    }

    #[test]
    fn validates_and_strips_the_mouse_report_id() {
        let layout = parse_mouse_report_layout(REPORT_ID_MOUSE_DESCRIPTOR).unwrap();
        assert_eq!(layout.report_id(), Some(2));
        assert_eq!(
            layout.decode(&[2, 1, 0, 7, 0, 0xf7, 0xff, 0]),
            Some(DecodedMouseReport {
                buttons: 1,
                dx: 7,
                dy: -9,
                wheel: 0,
            })
        );
        assert_eq!(layout.decode(&[1, 1, 0, 7, 0, 0xf7, 0xff, 0]), None);
    }
}
