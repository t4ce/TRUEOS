extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use core::future::Future;
use core::task::Poll;

use crab_usb::{Device, USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::usb2::api::{InterfaceEndpointError, claim_interface};

const HID_INTERRUPT_TIMEOUT_MS: u64 = 1000;
const HID_DESC_FALLBACK_REPORT_LEN: u16 = 128;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum HidBootKind {
    Keyboard,
    Mouse,
    Tablet,
    EyeTracker,
}

impl HidBootKind {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            HidBootKind::Keyboard => "keyboard",
            HidBootKind::Mouse => "mouse",
            HidBootKind::Tablet => "tablet",
            HidBootKind::EyeTracker => "eyetracker",
        }
    }
}

#[inline]
fn should_skip_descriptor_logging(vendor_id: u16, product_id: u16, kind: HidBootKind) -> bool {
    // Narrow workaround for QEMU's simple emulated USB boot HID devices, which can
    // wedge on the optional HID descriptor dump path before the interrupt stream starts.
    vendor_id == 0x0627
        && product_id == 0x0001
        && matches!(kind, HidBootKind::Mouse | HidBootKind::Keyboard)
}

#[derive(Copy, Clone, Debug)]
struct HidBootTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    protocol: u8,
    in_endpoint: u8,
    in_max_packet_size: u16,
    report_len: usize,
    kind: HidBootKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveHidStream {
    controller_id: u32,
    stable_id: u32,
    interface_number: u8,
    in_endpoint: u8,
    kind: HidBootKind,
}

static HID_STREAMS_ACTIVE: Mutex<Vec<ActiveHidStream>> = Mutex::new(Vec::new());

#[inline]
fn endpoint_target_from_address(address: u8) -> u32 {
    let ep_num = u32::from(address & 0x0F);
    if ep_num == 0 {
        1
    } else if (address & 0x80) != 0 {
        (ep_num * 2) + 1
    } else {
        ep_num * 2
    }
}

fn pick_hid_boot_targets(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<HidBootTarget> {
    let mut out = Vec::new();

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let kind = match (alt.class, alt.subclass, alt.protocol) {
                    (0x03, 0x01, 0x01) => HidBootKind::Keyboard,
                    (0x03, 0x01, 0x02) => HidBootKind::Mouse,
                    _ if super::eyetracker::matches_interface(
                        alt.class,
                        alt.subclass,
                        alt.protocol,
                    ) =>
                    {
                        HidBootKind::EyeTracker
                    }
                    _ => continue,
                };

                let Some(endpoint) = alt.endpoints.iter().find(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Interrupt
                        && ep.direction == usb_if::transfer::Direction::In
                }) else {
                    continue;
                };

                let report_len = match kind {
                    HidBootKind::Keyboard => 8,
                    HidBootKind::Mouse => 4,
                    HidBootKind::Tablet => super::tablet::report_len(endpoint.max_packet_size),
                    HidBootKind::EyeTracker => {
                        super::eyetracker::report_len(endpoint.max_packet_size)
                    }
                };
                out.push(HidBootTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    protocol: alt.protocol,
                    in_endpoint: endpoint.address,
                    in_max_packet_size: endpoint.max_packet_size,
                    report_len,
                    kind,
                });
            }
        }
    }

    out
}

async fn log_hid_report_descriptors(host: &mut USBHost, dev_info: &crab_usb::DeviceInfo) {
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

#[derive(Copy, Clone)]
enum HidMainItemKind {
    Input,
    Output,
    Feature,
}

impl HidMainItemKind {
    fn as_str(self) -> &'static str {
        match self {
            HidMainItemKind::Input => "input",
            HidMainItemKind::Output => "output",
            HidMainItemKind::Feature => "feature",
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
            (1, 0) => {
                state.usage_page = value;
            }
            (1, 7) => {
                state.report_size_bits = value;
            }
            (1, 8) => {
                state.report_id = value as u8;
            }
            (1, 9) => {
                state.report_count = value;
            }
            (2, 0) => {
                state.usage = Some(value);
            }
            (2, 1) => {
                state.usage_min = Some(value);
            }
            (2, 2) => {
                state.usage_max = Some(value);
            }
            (0, 10) | (0, 12) => {
                state.reset_local();
            }
            _ => {}
        }
    }
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

                let report_len = if read_len >= 9 && hid_desc[6] == 0x22 {
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
                            value: 0x2200,
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

fn register_active_hid_stream(stream: ActiveHidStream) -> bool {
    let mut streams = HID_STREAMS_ACTIVE.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream);
    true
}

fn unregister_active_hid_stream(stream: ActiveHidStream) -> bool {
    let mut streams = HID_STREAMS_ACTIVE.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
    !streams
        .iter()
        .any(|active| active.controller_id == stream.controller_id)
}

async fn with_timeout_or_none<F: Future>(fut: F, timeout_ms: u64) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

#[embassy_executor::task(pool_size = 8)]
async fn hid_boot_stream_task(
    mut device: crab_usb::Device,
    controller_id: u32,
    target: HidBootTarget,
    active_stream: ActiveHidStream,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot_id = u32::from(device.slot_id());
    let ep_target = endpoint_target_from_address(target.in_endpoint);
    let mut boot_protocol_ok = false;
    let mut set_idle_ok = false;

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: hid {} {:04X}:{:04X} set cfg={} failed: {:?}\n",
            target.kind.as_str(),
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
        let _ = unregister_active_hid_stream(active_stream);
        return;
    }

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                let _ = unregister_active_hid_stream(active_stream);
                return;
            }
        };

    if matches!(target.kind, HidBootKind::Mouse | HidBootKind::Keyboard) {
        match interface
            .device()
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x0B),
                    value: 0,
                    index: u16::from(target.interface_number),
                },
                &[],
            )
            .await
        {
            Ok(_) => {
                boot_protocol_ok = true;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} boot protocol if#{} failed (continuing): {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
            }
        }

        match interface
            .device()
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x0A),
                    value: 1 << 8,
                    index: u16::from(target.interface_number),
                },
                &[],
            )
            .await
        {
            Ok(_) => {
                set_idle_ok = true;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} set idle if#{} duration=1 failed (continuing): {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
            }
        }
    }

    if matches!(target.kind, HidBootKind::Tablet | HidBootKind::EyeTracker) {
        match interface
            .device()
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x0A),
                    value: 1 << 8,
                    index: u16::from(target.interface_number),
                },
                &[],
            )
            .await
        {
            Ok(_) => {
                set_idle_ok = true;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} set idle if#{} duration=1 failed (continuing): {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
            }
        }
    }

    let mut interrupt_in = match interface.endpoint_interrupt_in(target.in_endpoint).await {
        Ok(endpoint) => endpoint,
        Err(InterfaceEndpointError::WrongKind { .. }) => {
            crate::log!(
                "crabusb: hid {} {:04X}:{:04X} interrupt endpoint kind mismatch ep=0x{:02X}\n",
                target.kind.as_str(),
                vendor_id,
                product_id,
                target.in_endpoint
            );
            let _ = unregister_active_hid_stream(active_stream);
            return;
        }
        Err(InterfaceEndpointError::Usb(err)) => {
            crate::log!(
                "crabusb: hid {} {:04X}:{:04X} interrupt open failed ep=0x{:02X}: {:?}\n",
                target.kind.as_str(),
                vendor_id,
                product_id,
                target.in_endpoint,
                err
            );
            let _ = unregister_active_hid_stream(active_stream);
            return;
        }
    };

    crate::log!(
        "crabusb: hid {} {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} int_in=0x{:02X} mps={} ep_target={} proto={:02X} boot={} idle={}\n",
        target.kind.as_str(),
        vendor_id,
        product_id,
        slot_id,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.in_endpoint,
        target.in_max_packet_size,
        ep_target,
        target.protocol,
        boot_protocol_ok,
        set_idle_ok
    );

    let mut report = Vec::from_iter(core::iter::repeat_n(
        0u8,
        usize::from(target.in_max_packet_size.max(target.report_len as u16)),
    ));
    let mut timeout_logs = 0u32;

    loop {
        match with_timeout_or_none(
            interrupt_in.submit_and_wait(report.as_mut_slice()),
            HID_INTERRUPT_TIMEOUT_MS,
        )
        .await
        {
            None => {
                timeout_logs = timeout_logs.wrapping_add(1);
                if crate::logflag::USB_LOG_ALL.load(core::sync::atomic::Ordering::Relaxed)
                    && (timeout_logs <= 8 || timeout_logs.is_multiple_of(32))
                {
                    crate::log!(
                        "crabusb: hid {} {:04X}:{:04X} interrupt timeout ep=0x{:02X} count={}\n",
                        target.kind.as_str(),
                        vendor_id,
                        product_id,
                        target.in_endpoint,
                        timeout_logs
                    );
                }
                continue;
            }
            Some(result) => match result {
                Ok(read) => {
                    timeout_logs = 0;
                    if read == 0 {
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                        continue;
                    }

                    let sample = &report[..read.min(report.len())];
                    match target.kind {
                        HidBootKind::Keyboard => super::handle_keyboard_boot_report(
                            controller_id,
                            slot_id,
                            ep_target,
                            sample,
                        ),
                        HidBootKind::Mouse => super::handle_mouse_boot_report(
                            controller_id,
                            slot_id,
                            ep_target,
                            sample,
                        ),
                        HidBootKind::Tablet => {
                            super::tablet::handle_packet(
                                vendor_id,
                                product_id,
                                target.in_endpoint,
                                sample,
                            );
                        }
                        HidBootKind::EyeTracker => {
                            super::eyetracker::handle_packet(
                                vendor_id,
                                product_id,
                                target.in_endpoint,
                                sample,
                            );
                        }
                    }
                }
                Err(err) => {
                    crate::log!(
                        "crabusb: hid {} {:04X}:{:04X} stream stop ep=0x{:02X} err={:?}\n",
                        target.kind.as_str(),
                        vendor_id,
                        product_id,
                        target.in_endpoint,
                        err
                    );
                    break;
                }
            },
        }
    }

    if unregister_active_hid_stream(active_stream) {
        super::remove_hid_slot(controller_id, slot_id);
    }
}

pub(crate) async fn maybe_start_hid_boot_streams(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
    log_descriptors: bool,
) -> bool {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let stable_id = dev_info.stable_id().raw();
    let targets = pick_hid_boot_targets(dev_info.configurations());
    if targets.is_empty() {
        let hid_iface_count = dev_info
            .interface_descriptors()
            .filter(|iface| iface.class == 0x03)
            .count();
        if hid_iface_count != 0 {
            crate::log!(
                "crabusb: hid {:04X}:{:04X} found {} HID interface(s) but no boot targets\n",
                vendor_id,
                product_id,
                hid_iface_count
            );
        }
        return false;
    }

    crate::log!(
        "crabusb: hid {:04X}:{:04X} candidate targets={}\n",
        vendor_id,
        product_id,
        targets.len()
    );

    let mut started_any = false;
    let mut descriptors_pending = log_descriptors;

    for target in targets {
        let active_stream = ActiveHidStream {
            controller_id,
            stable_id,
            interface_number: target.interface_number,
            in_endpoint: target.in_endpoint,
            kind: target.kind,
        };
        if !register_active_hid_stream(active_stream) {
            continue;
        }

        crate::log!(
            "crabusb: hid {:04X}:{:04X} target kind={} if#{} alt={} cfg={} int_in=0x{:02X} mps={} proto={:02X}\n",
            vendor_id,
            product_id,
            target.kind.as_str(),
            target.interface_number,
            target.alternate_setting,
            target.configuration_value,
            target.in_endpoint,
            target.in_max_packet_size,
            target.protocol
        );
        let device = match host.open_device(dev_info).await {
            Ok(device) => device,
            Err(err) => {
                let _ = unregister_active_hid_stream(active_stream);
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} open failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    err
                );
                continue;
            }
        };
        let mut device = device;

        if descriptors_pending {
            if should_skip_descriptor_logging(vendor_id, product_id, target.kind) {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} skipping descriptor log for qemu boot hid\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id
                );
            } else {
                log_hid_report_descriptors_on_device(&mut device, dev_info).await;
            }
            descriptors_pending = false;
        }

        match spawner.spawn(hid_boot_stream_task(device, controller_id, target, active_stream)) {
            Ok(()) => {
                started_any = true;
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} handoff if#{} alt={} cfg={} int_in=0x{:02X} mps={} proto={:02X}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    target.configuration_value,
                    target.in_endpoint,
                    target.in_max_packet_size,
                    target.protocol
                );
            }
            Err(err) => {
                let _ = unregister_active_hid_stream(active_stream);
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} spawn failed if#{} alt={} ep=0x{:02X}: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    target.in_endpoint,
                    err
                );
            }
        }
    }

    started_any
}
