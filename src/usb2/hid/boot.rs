extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::task::Poll;

use crab_usb::{USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::usb2::api::{EndpointSubmitExt, InterfaceEndpointError, claim_interface};

const HID_INTERRUPT_TIMEOUT_MS: u64 = 1000;
const LED_VID_JGINYUE: u16 = 0x0416;
const LED_PID_JGINYUE: u16 = 0xA125;
const MOUSE_VID_LAVIEW_CASTOR: u16 = 0x22D4;
const MOUSE_PID_LAVIEW_CASTOR: u16 = 0x1321;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum HidBootKind {
    Keyboard,
    Mouse,
    Tablet,
    EyeTracker,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GenericPointerKind {
    Mouse,
    Tablet,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct GenericPointerInfo {
    kind: GenericPointerKind,
    has_report_id: bool,
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
    let _ = kind;
    super::super::descriptor::hid_optional_descriptor_skip_reason(vendor_id, product_id)
        .map(|reason| {
            let _ = reason.as_str();
            true
        })
        .unwrap_or(false)
}

#[inline]
fn should_skip_qemu_generic_tablet_probe(
    vendor_id: u16,
    product_id: u16,
    kind: HidBootKind,
) -> bool {
    vendor_id == 0x0627 && product_id == 0x0001 && matches!(kind, HidBootKind::Tablet)
}

#[inline]
fn should_skip_known_led_tablet_probe(vendor_id: u16, product_id: u16, kind: HidBootKind) -> bool {
    vendor_id == LED_VID_JGINYUE
        && product_id == LED_PID_JGINYUE
        && matches!(kind, HidBootKind::Tablet)
}

#[inline]
fn should_skip_known_mouse_generic_hid_probe(
    vendor_id: u16,
    product_id: u16,
    kind: HidBootKind,
) -> bool {
    vendor_id == MOUSE_VID_LAVIEW_CASTOR
        && product_id == MOUSE_PID_LAVIEW_CASTOR
        && matches!(kind, HidBootKind::Tablet)
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
    generic_pointer: bool,
    strip_report_id: bool,
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

fn hid_item_data(data: &[u8]) -> u32 {
    let mut value = 0u32;
    for (idx, byte) in data.iter().enumerate() {
        value |= u32::from(*byte) << (idx * 8);
    }
    value
}

fn classify_generic_pointer_report(report_desc: &[u8]) -> Option<GenericPointerInfo> {
    let mut idx = 0usize;
    let mut usage_page = 0u32;
    let mut usage: Option<u32> = None;
    let mut has_report_id = false;
    let mut saw_mouse = false;
    let mut saw_tablet = false;

    while idx < report_desc.len() {
        let prefix = report_desc[idx];
        idx += 1;

        if prefix == 0xFE {
            if idx + 1 >= report_desc.len() {
                break;
            }
            let long_len = usize::from(report_desc[idx]);
            idx = idx.saturating_add(2).saturating_add(long_len);
            usage = None;
            continue;
        }

        let size = match prefix & 0x03 {
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
        let value = hid_item_data(&report_desc[idx..idx + size]);
        idx += size;

        match (item_type, tag) {
            (1, 0) => usage_page = value,
            (1, 8) => has_report_id = true,
            (2, 0) => usage = Some(value),
            (0, 10) => {
                if usage_page == 0x01 && usage == Some(0x02) {
                    saw_mouse = true;
                }
                if usage_page == 0x0D {
                    saw_tablet = true;
                }
                usage = None;
            }
            (0, 12) => usage = None,
            _ => {}
        }
    }

    if saw_tablet {
        Some(GenericPointerInfo {
            kind: GenericPointerKind::Tablet,
            has_report_id,
        })
    } else if saw_mouse {
        Some(GenericPointerInfo {
            kind: GenericPointerKind::Mouse,
            has_report_id,
        })
    } else {
        None
    }
}

async fn read_generic_pointer_info(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    interface_number: u8,
) -> Option<GenericPointerInfo> {
    let mut device = host.open_device(dev_info).await.ok()?;
    let mut hid_desc = [0u8; 9];
    let read_len = device
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
        .ok()?
        .min(hid_desc.len());

    let report_len = if read_len >= 9 && hid_desc[6] == 0x22 {
        u16::from(hid_desc[7]) | (u16::from(hid_desc[8]) << 8)
    } else {
        128
    };
    let mut report_desc = alloc::vec![0u8; usize::from(report_len)];
    let report_read_len = device
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
        .ok()?
        .min(report_desc.len());
    classify_generic_pointer_report(&report_desc[..report_read_len])
}

fn pick_hid_boot_targets(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<HidBootTarget> {
    let mut out = Vec::new();
    let mut generic_tablet_targets = Vec::new();

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let (kind, generic_pointer) = match (alt.class, alt.subclass, alt.protocol) {
                    (0x03, 0x01, 0x01) => (HidBootKind::Keyboard, false),
                    (0x03, 0x01, 0x02) => (HidBootKind::Mouse, false),
                    _ if super::tablet::matches_interface(
                        alt.class,
                        alt.subclass,
                        alt.protocol,
                    ) =>
                    {
                        (HidBootKind::Tablet, true)
                    }
                    _ if super::eyetracker::matches_interface(
                        alt.class,
                        alt.subclass,
                        alt.protocol,
                    ) =>
                    {
                        (HidBootKind::EyeTracker, false)
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
                    HidBootKind::EyeTracker => {
                        super::eyetracker::report_len(endpoint.max_packet_size)
                    }
                    HidBootKind::Tablet => super::tablet::report_len(endpoint.max_packet_size),
                };
                let target = HidBootTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    protocol: alt.protocol,
                    in_endpoint: endpoint.address,
                    in_max_packet_size: endpoint.max_packet_size,
                    report_len,
                    kind,
                    generic_pointer,
                    strip_report_id: false,
                };

                if matches!(kind, HidBootKind::Tablet) {
                    generic_tablet_targets.push(target);
                } else {
                    out.push(target);
                }
            }
        }
    }

    // Generic HID 03/00/00 also appears on combo mice; keep it as tablet-only
    // when the device did not already expose a definite boot mouse interface.
    if !out
        .iter()
        .any(|target| matches!(target.kind, HidBootKind::Mouse))
    {
        out.extend(generic_tablet_targets);
    }

    out
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

    if let Err(err) = device.set_configuration(target.configuration_value).await {
        crate::log_info!(target: "usb";
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
                crate::log_info!(target: "usb";
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

    if matches!(target.kind, HidBootKind::Mouse | HidBootKind::Keyboard) && !target.generic_pointer
    {
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
                crate::log_info!(target: "usb";
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
                crate::log_info!(target: "usb";
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

    if matches!(
        target.kind,
        HidBootKind::Mouse | HidBootKind::Keyboard | HidBootKind::Tablet | HidBootKind::EyeTracker
    ) && (target.generic_pointer
        || matches!(target.kind, HidBootKind::Tablet | HidBootKind::EyeTracker))
    {
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
                crate::log_info!(target: "usb";
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
        Err(InterfaceEndpointError::WrongKind { address, expected }) => {
            crate::log_info!(target: "usb";
                "crabusb: hid {} {:04X}:{:04X} interrupt endpoint kind mismatch ep=0x{:02X} got=0x{:02X} expected={}\n",
                target.kind.as_str(),
                vendor_id,
                product_id,
                target.in_endpoint,
                address,
                expected
            );
            let _ = unregister_active_hid_stream(active_stream);
            return;
        }
        Err(InterfaceEndpointError::Usb(err)) => {
            crate::log_info!(target: "usb";
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

    crate::log_info!(target: "usb";
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
                    crate::log_info!(target: "usb";
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
                        HidBootKind::Mouse => {
                            let mouse_sample = if target.strip_report_id && sample.len() > 1 {
                                &sample[1..]
                            } else {
                                sample
                            };
                            super::handle_mouse_boot_report(
                                controller_id,
                                slot_id,
                                ep_target,
                                mouse_sample,
                            );
                        }
                        HidBootKind::Tablet => {
                            super::handle_tablet_boot_report(
                                controller_id,
                                slot_id,
                                ep_target,
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
                    crate::log_info!(target: "usb";
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
    maybe_start_hid_boot_streams_filtered(
        host,
        dev_info,
        spawner,
        controller_id,
        log_descriptors,
        None,
    )
    .await
}

pub(crate) async fn maybe_start_hid_mouse_streams(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
    log_descriptors: bool,
) -> bool {
    maybe_start_hid_boot_streams_filtered(
        host,
        dev_info,
        spawner,
        controller_id,
        log_descriptors,
        Some(HidBootKind::Mouse),
    )
    .await
}

async fn maybe_start_hid_boot_streams_filtered(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
    log_descriptors: bool,
    only_kind: Option<HidBootKind>,
) -> bool {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let stable_id = dev_info.id() as u32;
    let targets = pick_hid_boot_targets(dev_info.configurations());
    if targets.is_empty() {
        let hid_iface_count = dev_info
            .interface_descriptors()
            .filter(|iface| iface.class == 0x03)
            .count();
        if hid_iface_count != 0 {
            crate::log_info!(target: "usb";
                "crabusb: hid {:04X}:{:04X} found {} HID interface(s) but no stream targets\n",
                vendor_id,
                product_id,
                hid_iface_count
            );
        }
        return false;
    }

    crate::log_info!(target: "usb";
        "crabusb: hid {:04X}:{:04X} candidate targets={}\n",
        vendor_id,
        product_id,
        targets.len()
    );

    let mut started_any = false;
    let mut descriptors_pending = log_descriptors;

    for mut target in targets {
        if should_skip_qemu_generic_tablet_probe(vendor_id, product_id, target.kind) {
            crate::log_info!(target: "usb";
                "crabusb: hid {:04X}:{:04X} skipping generic-tablet target on qemu if#{} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.in_endpoint
            );
            continue;
        }
        if should_skip_known_led_tablet_probe(vendor_id, product_id, target.kind) {
            crate::log_info!(target: "usb";
                "crabusb: hid {:04X}:{:04X} skipping generic-tablet target on known LED controller if#{} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.in_endpoint
            );
            continue;
        }
        if should_skip_known_mouse_generic_hid_probe(vendor_id, product_id, target.kind) {
            crate::log_info!(target: "usb";
                "crabusb: hid {:04X}:{:04X} skipping generic-tablet target on known mouse supplemental HID if#{} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.in_endpoint
            );
            continue;
        }

        if target.generic_pointer {
            let inferred = read_generic_pointer_info(host, dev_info, target.interface_number).await;
            match inferred.map(|info| info.kind) {
                Some(GenericPointerKind::Mouse) => {
                    let has_report_id = inferred.map(|info| info.has_report_id).unwrap_or(false);
                    target.kind = HidBootKind::Mouse;
                    target.strip_report_id = has_report_id;
                    target.report_len =
                        usize::from(target.in_max_packet_size.max(if has_report_id {
                            5
                        } else {
                            4
                        }));
                    crate::log_info!(target: "usb";
                        "crabusb: hid {:04X}:{:04X} generic pointer if#{} classified=mouse report_id={}\n",
                        vendor_id,
                        product_id,
                        target.interface_number,
                        has_report_id
                    );
                }
                Some(GenericPointerKind::Tablet) => {
                    crate::log_info!(target: "usb";
                        "crabusb: hid {:04X}:{:04X} generic pointer if#{} classified=tablet\n",
                        vendor_id,
                        product_id,
                        target.interface_number
                    );
                }
                None => {
                    target.kind = HidBootKind::Mouse;
                    target.report_len = usize::from(target.in_max_packet_size.max(4));
                    crate::log_info!(target: "usb";
                        "crabusb: hid {:04X}:{:04X} generic pointer if#{} classified=mouse fallback=descriptor-unavailable\n",
                        vendor_id,
                        product_id,
                        target.interface_number
                    );
                }
            }
        }

        if only_kind.is_some_and(|kind| target.kind != kind) {
            crate::log_info!(target: "usb";
                "crabusb: hid {:04X}:{:04X} skipping {} target in mouse-only host2 path if#{} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.kind.as_str(),
                target.interface_number,
                target.in_endpoint
            );
            continue;
        }

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

        crate::log_info!(target: "usb";
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
                crate::log_info!(target: "usb";
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
                crate::log_info!(target: "usb";
                    "crabusb: hid {} {:04X}:{:04X} skipping descriptor log for qemu boot hid\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id
                );
            } else {
                crate::usb2::descriptor::log_hid_report_descriptors_on_device(
                    &mut device,
                    dev_info,
                )
                .await;
            }
            descriptors_pending = false;
        }

        match hid_boot_stream_task(device, controller_id, target, active_stream) {
            Ok(token) => {
                spawner.spawn(token);
                started_any = true;
                crate::log_info!(target: "usb";
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
                crate::log_info!(target: "usb";
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
