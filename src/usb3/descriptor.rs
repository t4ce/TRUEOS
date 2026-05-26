use crab_usb::{Device, DeviceInfo, usb_if};

pub(crate) struct HidDescriptorSkipReason;

impl HidDescriptorSkipReason {
    pub(crate) const fn as_str(&self) -> &'static str {
        "known-optional-descriptor-risk"
    }
}

pub(crate) fn hid_optional_descriptor_skip_reason(
    vendor_id: u16,
    product_id: u16,
) -> Option<HidDescriptorSkipReason> {
    if vendor_id == 0x0627 && product_id == 0x0001 {
        Some(HidDescriptorSkipReason)
    } else {
        None
    }
}

pub(crate) async fn log_hid_report_descriptors_on_device(
    device: &mut Device,
    dev_info: &DeviceInfo,
) {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;

    for interface in dev_info.interface_descriptors().filter(|iface| iface.class == 0x03) {
        let mut hid_desc = [0u8; 9];
        let hid_len = match device
            .control_in(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Standard,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::GetDescriptor,
                    value: 0x2100,
                    index: u16::from(interface.interface_number),
                },
                &mut hid_desc,
            )
            .await
        {
            Ok(len) => len.min(hid_desc.len()),
            Err(err) => {
                crate::log!(
                    "crabusb: hid {:04X}:{:04X} descriptor if#{} hid-desc failed err={:?}\n",
                    vendor_id,
                    product_id,
                    interface.interface_number,
                    err
                );
                continue;
            }
        };

        let report_len = if hid_len >= 9 && hid_desc[6] == 0x22 {
            u16::from(hid_desc[7]) | (u16::from(hid_desc[8]) << 8)
        } else {
            0
        };
        crate::log!(
            "crabusb: hid {:04X}:{:04X} descriptor if#{} class={:02X}:{:02X}:{:02X} hid_len={} report_len={}\n",
            vendor_id,
            product_id,
            interface.interface_number,
            interface.class,
            interface.subclass,
            interface.protocol,
            hid_len,
            report_len
        );

        if report_len == 0 {
            continue;
        }

        let mut report = alloc::vec![0u8; usize::from(report_len).min(512)];
        match device
            .control_in(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Standard,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::GetDescriptor,
                    value: 0x2200,
                    index: u16::from(interface.interface_number),
                },
                &mut report,
            )
            .await
        {
            Ok(len) => {
                let len = len.min(report.len());
                crate::log!(
                    "crabusb: hid {:04X}:{:04X} report-desc if#{} len={} first={:02X?}\n",
                    vendor_id,
                    product_id,
                    interface.interface_number,
                    len,
                    &report[..len.min(16)]
                );
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {:04X}:{:04X} report-desc if#{} failed err={:?}\n",
                    vendor_id,
                    product_id,
                    interface.interface_number,
                    err
                );
            }
        }
    }
}
