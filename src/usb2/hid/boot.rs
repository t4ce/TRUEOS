extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::task::Poll;

use crab_usb::{USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::usb2::api::{InterfaceEndpointError, claim_interface};

const HID_INTERRUPT_TIMEOUT_MS: u64 = 1000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum HidBootKind {
    Keyboard,
    Mouse,
    Tablet,
}

impl HidBootKind {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            HidBootKind::Keyboard => "keyboard",
            HidBootKind::Mouse => "mouse",
            HidBootKind::Tablet => "tablet",
        }
    }
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
    slot_id: u32,
    ep_target: u32,
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
                    _ if super::tablet::matches_interface(alt.class, alt.subclass, alt.protocol) => {
                        HidBootKind::Tablet
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
    !streams.iter().any(|active| {
        active.controller_id == stream.controller_id && active.slot_id == stream.slot_id
    })
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
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot_id = u32::from(device.slot_id());
    let ep_target = endpoint_target_from_address(target.in_endpoint);
    let active_stream = ActiveHidStream {
        controller_id,
        slot_id,
        ep_target,
        kind: target.kind,
    };
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

    let mut interface = match claim_interface(
        &mut device,
        target.interface_number,
        target.alternate_setting,
    )
    .await
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
                    "crabusb: hid {} {:04X}:{:04X} boot protocol if#{} failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
                let _ = unregister_active_hid_stream(active_stream);
                return;
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
                    "crabusb: hid {} {:04X}:{:04X} set idle if#{} duration=1 failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
                let _ = unregister_active_hid_stream(active_stream);
                return;
            }
        }
    }

    if matches!(target.kind, HidBootKind::Tablet) {
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
                    "crabusb: hid {} {:04X}:{:04X} set idle if#{} duration=1 failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
                let _ = unregister_active_hid_stream(active_stream);
                return;
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
                if timeout_logs <= 8 || timeout_logs.is_multiple_of(32) {
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
                        HidBootKind::Keyboard => {
                            super::handle_keyboard_boot_report(controller_id, slot_id, ep_target, sample)
                        }
                        HidBootKind::Mouse => {
                            super::handle_mouse_boot_report(controller_id, slot_id, ep_target, sample)
                        }
                        HidBootKind::Tablet => {
                            super::tablet::handle_packet(
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
) -> bool {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let targets = pick_hid_boot_targets(dev_info.configurations());
    if targets.is_empty() {
        let hid_iface_count = dev_info
            .interface_descriptors()
            .filter(|iface| iface.class == 0x03)
            .count();
        if hid_iface_count != 0 {
            crate::log!(
                "crabusb: hid {:04X}:{:04X} found {} HID interface(s) but no boot/tablet targets\n",
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

    for target in targets {
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

        let slot_id = u32::from(device.slot_id());
        let ep_target = endpoint_target_from_address(target.in_endpoint);
        let active_stream = ActiveHidStream {
            controller_id,
            slot_id,
            ep_target,
            kind: target.kind,
        };
        if !register_active_hid_stream(active_stream) {
            continue;
        }

        match spawner.spawn(hid_boot_stream_task(device, controller_id, target)) {
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
