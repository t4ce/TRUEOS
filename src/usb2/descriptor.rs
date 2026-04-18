extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use crab_usb::{Device, USBHost, usb_if};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum UsbDescriptorScope {
    Device,
    Interface(u8),
    Endpoint(u8),
    Other(u16),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum UsbDescriptorType {
    Device,
    Configuration,
    String,
    Interface,
    Endpoint,
    Hid,
    HidReport,
    HidPhysical,
    InterfaceAssociation,
    CsInterface,
    CsEndpoint,
    Bos,
    DeviceCapability,
    Unknown(u8),
}

impl UsbDescriptorType {
    pub(crate) const fn from_u8(value: u8) -> Self {
        match value {
            0x01 => Self::Device,
            0x02 => Self::Configuration,
            0x03 => Self::String,
            0x04 => Self::Interface,
            0x05 => Self::Endpoint,
            0x0B => Self::InterfaceAssociation,
            0x0F => Self::Bos,
            0x10 => Self::DeviceCapability,
            0x21 => Self::Hid,
            0x22 => Self::HidReport,
            0x23 => Self::HidPhysical,
            0x24 => Self::CsInterface,
            0x25 => Self::CsEndpoint,
            other => Self::Unknown(other),
        }
    }

    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Device => 0x01,
            Self::Configuration => 0x02,
            Self::String => 0x03,
            Self::Interface => 0x04,
            Self::Endpoint => 0x05,
            Self::InterfaceAssociation => 0x0B,
            Self::Bos => 0x0F,
            Self::DeviceCapability => 0x10,
            Self::Hid => 0x21,
            Self::HidReport => 0x22,
            Self::HidPhysical => 0x23,
            Self::CsInterface => 0x24,
            Self::CsEndpoint => 0x25,
            Self::Unknown(value) => value,
        }
    }

    pub(crate) const fn short_name(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Configuration => "config",
            Self::String => "string",
            Self::Interface => "interface",
            Self::Endpoint => "endpoint",
            Self::Hid => "hid",
            Self::HidReport => "hid-report",
            Self::HidPhysical => "hid-physical",
            Self::InterfaceAssociation => "iad",
            Self::CsInterface => "cs-interface",
            Self::CsEndpoint => "cs-endpoint",
            Self::Bos => "bos",
            Self::DeviceCapability => "device-cap",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UsbDeviceStrings {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum UsbDescriptorSkipReason {
    QemuBootHidOptionalRead,
}

impl UsbDescriptorSkipReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::QemuBootHidOptionalRead => "qemu-boot-hid-optional-read",
        }
    }
}

pub(crate) const fn endpoint_transfer_type_label(
    transfer_type: usb_if::descriptor::EndpointType,
) -> &'static str {
    match transfer_type {
        usb_if::descriptor::EndpointType::Control => "ctrl",
        usb_if::descriptor::EndpointType::Isochronous => "iso",
        usb_if::descriptor::EndpointType::Bulk => "bulk",
        usb_if::descriptor::EndpointType::Interrupt => "intr",
    }
}

pub(crate) const fn hid_optional_descriptor_skip_reason(
    vendor_id: u16,
    product_id: u16,
) -> Option<UsbDescriptorSkipReason> {
    if vendor_id == 0x0627 && product_id == 0x0001 {
        Some(UsbDescriptorSkipReason::QemuBootHidOptionalRead)
    } else {
        None
    }
}

pub(crate) fn sanitize_usb_identity_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches(|ch: char| ch.is_ascii_whitespace() || ch == '\0');
    if trimmed.is_empty() {
        return None;
    }

    let mut out = String::new();
    for ch in trimmed.chars() {
        if !ch.is_ascii() {
            break;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '+' | ' ') {
            out.push(ch);
            continue;
        }
        if ch == '\0' || ch.is_ascii_control() {
            break;
        }
        break;
    }

    let out: String =
        String::from(out.trim_matches(|ch: char| {
            ch.is_ascii_whitespace() || matches!(ch, '-' | '_' | '.' | ':')
        }));
    if out.len() < 3 || out.len() > 64 || !out.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        None
    } else {
        Some(out)
    }
}

pub(crate) async fn read_optional_string_descriptor(
    device: &mut Device,
    index: Option<core::num::NonZero<u8>>,
) -> Option<String> {
    let idx = index?;
    let text = device.string_descriptor(idx.get()).await.ok()?;
    sanitize_usb_identity_string(text.as_str())
}

pub(crate) async fn read_device_strings(device: &mut Device) -> UsbDeviceStrings {
    let (manufacturer_index, product_index, serial_index) = {
        let desc = device.descriptor();
        (desc.manufacturer_string_index, desc.product_string_index, desc.serial_number_string_index)
    };
    UsbDeviceStrings {
        manufacturer: read_optional_string_descriptor(device, manufacturer_index).await,
        product: read_optional_string_descriptor(device, product_index).await,
        serial: read_optional_string_descriptor(device, serial_index).await,
    }
}

const HID_DESC_FALLBACK_REPORT_LEN: u16 = 128;

#[derive(Copy, Clone)]
enum HidMainItemKind {
    Input,
    Output,
    Feature,
}

impl HidMainItemKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::Feature => "feature",
        }
    }
}

#[derive(Copy, Clone)]
struct HidDecodeState {
    report_id: u8,
    report_size_bits: u32,
    report_count: u32,
    usage_page: u32,
    usage: Option<u32>,
    usage_min: Option<u32>,
    usage_max: Option<u32>,
}

impl HidDecodeState {
    const fn new() -> Self {
        Self {
            report_id: 0,
            report_size_bits: 0,
            report_count: 0,
            usage_page: 0,
            usage: None,
            usage_min: None,
            usage_max: None,
        }
    }

    fn reset_local(&mut self) {
        self.usage = None;
        self.usage_min = None;
        self.usage_max = None;
    }
}

fn hid_item_data(data: &[u8]) -> u32 {
    let mut value = 0u32;
    for (idx, byte) in data.iter().enumerate() {
        value |= u32::from(*byte) << (idx * 8);
    }
    value
}

fn log_hid_report_decode(slot_id: u32, interface_number: u8, report_desc: &[u8]) {
    let mut state = HidDecodeState::new();
    let mut idx = 0usize;

    while idx < report_desc.len() {
        let prefix = report_desc[idx];
        idx += 1;

        if prefix == 0xFE {
            if idx + 1 >= report_desc.len() {
                break;
            }
            let long_len = usize::from(report_desc[idx]);
            idx = idx.saturating_add(2).saturating_add(long_len);
            state.reset_local();
            continue;
        }

        let size_code = prefix & 0x03;
        let size = match size_code {
            0 => 0usize,
            1 => 1usize,
            2 => 2usize,
            _ => 4usize,
        };
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;
        if idx + size > report_desc.len() {
            break;
        }
        let data = &report_desc[idx..idx + size];
        idx += size;
        let value = hid_item_data(data);

        match (item_type, tag) {
            (0, 8) | (0, 9) | (0, 11) => {
                let kind = match tag {
                    8 => HidMainItemKind::Input,
                    9 => HidMainItemKind::Output,
                    _ => HidMainItemKind::Feature,
                };
                let total_bits = state.report_size_bits.saturating_mul(state.report_count);
                let total_bytes = total_bits.div_ceil(8);
                let mut detail = String::new();
                let _ = write!(
                    &mut detail,
                    "page=0x{:04X} bits={} bytes={} count={} size={} flags=0x{:X}",
                    state.usage_page,
                    total_bits,
                    total_bytes,
                    state.report_count,
                    state.report_size_bits,
                    value
                );
                if let Some(usage) = state.usage {
                    let _ = write!(&mut detail, " usage=0x{:X}", usage);
                }
                if let (Some(min), Some(max)) = (state.usage_min, state.usage_max) {
                    let _ = write!(&mut detail, " usage_range=0x{:X}-0x{:X}", min, max);
                }
                crate::log!(
                    "crabusb: hid report decode slot={} if#{} report_id={} kind={} {}\n",
                    slot_id,
                    interface_number,
                    state.report_id,
                    kind.as_str(),
                    detail
                );
                state.reset_local();
            }
            (1, 0) => state.usage_page = value,
            (1, 7) => state.report_size_bits = value,
            (1, 8) => state.report_id = value as u8,
            (1, 9) => state.report_count = value,
            (2, 0) => state.usage = Some(value),
            (2, 1) => state.usage_min = Some(value),
            (2, 2) => state.usage_max = Some(value),
            (0, 10) | (0, 12) => state.reset_local(),
            _ => {}
        }
    }
}

pub(crate) async fn log_hid_report_descriptors(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
) {
    let interface_numbers: Vec<u8> = dev_info
        .interface_descriptors()
        .filter(|iface| iface.class == 0x03)
        .map(|iface| iface.interface_number)
        .collect();

    if interface_numbers.is_empty() {
        return;
    }

    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: hid {:04X}:{:04X} descriptor-open failed: {:?}\n",
                dev_info.vendor_id(),
                dev_info.product_id(),
                err
            );
            return;
        }
    };

    log_hid_report_descriptors_on_device(&mut device, dev_info).await;
}

pub(crate) async fn log_hid_report_descriptors_on_device(
    device: &mut Device,
    dev_info: &crab_usb::DeviceInfo,
) {
    let interface_numbers: Vec<u8> = dev_info
        .interface_descriptors()
        .filter(|iface| iface.class == 0x03)
        .map(|iface| iface.interface_number)
        .collect();

    if interface_numbers.is_empty() {
        return;
    }

    let slot_id = u32::from(device.slot_id());
    for interface_number in interface_numbers {
        let mut hid_desc = [0u8; 9];
        match device
            .control_in(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Standard,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::GetDescriptor,
                    value: 0x2100,
                    index: u16::from(interface_number),
                },
                &mut hid_desc,
            )
            .await
        {
            Ok(read_len) => {
                let read_len = read_len.min(hid_desc.len());
                crate::log!(
                    "crabusb: hid desc slot={} if#{} bytes={:02X?}\n",
                    slot_id,
                    interface_number,
                    &hid_desc[..read_len]
                );

                let report_len =
                    if read_len >= 9 && hid_desc[6] == UsbDescriptorType::HidReport.code() {
                        u16::from(hid_desc[7]) | (u16::from(hid_desc[8]) << 8)
                    } else {
                        HID_DESC_FALLBACK_REPORT_LEN
                    };

                let mut report_desc = alloc::vec![0u8; usize::from(report_len)];
                match device
                    .control_in(
                        usb_if::host::ControlSetup {
                            request_type: usb_if::transfer::RequestType::Standard,
                            recipient: usb_if::transfer::Recipient::Interface,
                            request: usb_if::transfer::Request::GetDescriptor,
                            value: u16::from(UsbDescriptorType::HidReport.code()) << 8,
                            index: u16::from(interface_number),
                        },
                        &mut report_desc,
                    )
                    .await
                {
                    Ok(report_read_len) => {
                        let report_read_len = report_read_len.min(report_desc.len());
                        crate::log!(
                            "crabusb: hid report desc slot={} if#{} len={} bytes={:02X?}\n",
                            slot_id,
                            interface_number,
                            report_read_len,
                            &report_desc[..report_read_len]
                        );
                        log_hid_report_decode(
                            slot_id,
                            interface_number,
                            &report_desc[..report_read_len],
                        );
                    }
                    Err(err) => {
                        crate::log!(
                            "crabusb: hid report desc slot={} if#{} read failed len={} err={:?}\n",
                            slot_id,
                            interface_number,
                            report_len,
                            err
                        );
                    }
                }
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid desc slot={} if#{} read failed err={:?}\n",
                    slot_id,
                    interface_number,
                    err
                );
            }
        }
    }
}
