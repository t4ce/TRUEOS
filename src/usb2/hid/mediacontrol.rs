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

#[derive(Copy, Clone, Debug)]
struct MediaControlTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    in_endpoint: u8,
    in_max_packet_size: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveMediaControlStream {
    // Composite USB devices expose both UAC and HID interfaces under one stable_id.
    // Keep that identity here so later media-button events can target this device's
    // own audio path instead of applying headset controls globally.
    stable_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    interface_number: u8,
}

static MEDIA_CONTROL_STREAMS_ACTIVE: Mutex<Vec<ActiveMediaControlStream>> = Mutex::new(Vec::new());

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

fn pick_media_control_targets(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<MediaControlTarget> {
    let mut out = Vec::new();

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.class != 0x03 || alt.subclass != 0x00 || alt.protocol != 0x00 {
                    continue;
                }

                let Some(endpoint) = alt.endpoints.iter().find(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Interrupt
                        && ep.direction == usb_if::transfer::Direction::In
                }) else {
                    continue;
                };

                out.push(MediaControlTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    in_endpoint: endpoint.address,
                    in_max_packet_size: endpoint.max_packet_size,
                });
            }
        }
    }

    out
}

fn device_has_audio_interfaces(configs: &[usb_if::descriptor::ConfigurationDescriptor]) -> bool {
    configs.iter().any(|config| {
        config.interfaces.iter().any(|interface| {
            interface.alt_settings.iter().any(|alt| {
                alt.class == 0x01
                    || alt.endpoints.iter().any(|ep| {
                        ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                            && ep.direction == usb_if::transfer::Direction::Out
                    })
            })
        })
    })
}

fn register_active_stream(stream: ActiveMediaControlStream) -> bool {
    let mut streams = MEDIA_CONTROL_STREAMS_ACTIVE.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream);
    true
}

fn unregister_active_stream(stream: ActiveMediaControlStream) {
    let mut streams = MEDIA_CONTROL_STREAMS_ACTIVE.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
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

fn report_changed(prev: &[u8], next: &[u8]) -> bool {
    prev.len() != next.len() || prev.iter().zip(next.iter()).any(|(a, b)| a != b)
}

#[embassy_executor::task(pool_size = 4)]
async fn media_control_task(
    mut device: crab_usb::Device,
    stable_id: u32,
    controller_id: u32,
    target: MediaControlTarget,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot_id = u32::from(device.slot_id());
    let ep_target = endpoint_target_from_address(target.in_endpoint);
    let active_stream = ActiveMediaControlStream {
        stable_id,
        controller_id,
        slot_id,
        ep_target,
        interface_number: target.interface_number,
    };

    if let Err(err) = device
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: hid mediacontrol {:04X}:{:04X} set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
        unregister_active_stream(active_stream);
        return;
    }

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: hid mediacontrol {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_stream(active_stream);
                return;
            }
        };

    let mut interrupt_in = match interface.endpoint_interrupt_in(target.in_endpoint).await {
        Ok(endpoint) => endpoint,
        Err(InterfaceEndpointError::WrongKind { address, expected }) => {
            crate::log!(
                "crabusb: hid mediacontrol {:04X}:{:04X} interrupt endpoint kind mismatch ep=0x{:02X} got=0x{:02X} expected={}\n",
                vendor_id,
                product_id,
                target.in_endpoint,
                address,
                expected
            );
            unregister_active_stream(active_stream);
            return;
        }
        Err(InterfaceEndpointError::Usb(err)) => {
            crate::log!(
                "crabusb: hid mediacontrol {:04X}:{:04X} interrupt open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.in_endpoint,
                err
            );
            unregister_active_stream(active_stream);
            return;
        }
    };

    crate::log!(
        "crabusb: hid mediacontrol {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} int_in=0x{:02X} mps={} ep_target={}\n",
        vendor_id,
        product_id,
        slot_id,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.in_endpoint,
        target.in_max_packet_size,
        ep_target
    );

    let mut report = vec![0u8; usize::from(target.in_max_packet_size.max(1))];
    let mut last_report = Vec::new();
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
                        "crabusb: hid mediacontrol {:04X}:{:04X} interrupt timeout ep=0x{:02X} count={}\n",
                        vendor_id,
                        product_id,
                        target.in_endpoint,
                        timeout_logs
                    );
                }
            }
            Some(Ok(read)) => {
                timeout_logs = 0;
                if read == 0 {
                    continue;
                }
                let sample = &report[..read.min(report.len())];
                if report_changed(&last_report, sample) {
                    crate::log!(
                        "crabusb: hid mediacontrol {:04X}:{:04X} slot={} if#{} ep=0x{:02X} report={:02X?}\n",
                        vendor_id,
                        product_id,
                        slot_id,
                        target.interface_number,
                        target.in_endpoint,
                        sample
                    );
                    last_report.clear();
                    last_report.extend_from_slice(sample);
                }
            }
            Some(Err(err)) => {
                crate::log!(
                    "crabusb: hid mediacontrol {:04X}:{:04X} stream stop ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.in_endpoint,
                    err
                );
                break;
            }
        }
    }

    unregister_active_stream(active_stream);
}

pub(crate) async fn maybe_start_media_control(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    // Keep media-control handling scoped to this physical composite USB device.
    // The HID interface and the UAC audio interfaces share the same stable_id, so
    // later we can route volume/mute/button events to this headset's own audio
    // path instead of treating them as global speaker controls.
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    if !device_has_audio_interfaces(dev_info.configurations()) {
        return false;
    }
    let targets = pick_media_control_targets(dev_info.configurations());
    if targets.is_empty() {
        return false;
    }

    let mut started_any = false;
    for target in targets {
        let device = match host.open_device(dev_info).await {
            Ok(device) => device,
            Err(err) => {
                crate::log!(
                    "crabusb: hid mediacontrol {:04X}:{:04X} open failed: {:?}\n",
                    vendor_id,
                    product_id,
                    err
                );
                break;
            }
        };

        let slot_id = u32::from(device.slot_id());
        let ep_target = endpoint_target_from_address(target.in_endpoint);
        let stable_id = dev_info.id() as u32;
        let active_stream = ActiveMediaControlStream {
            stable_id,
            controller_id,
            slot_id,
            ep_target,
            interface_number: target.interface_number,
        };
        if !register_active_stream(active_stream) {
            continue;
        }

        match media_control_task(device, stable_id, controller_id, target) {
            Ok(token) => {
                spawner.spawn(token);
                started_any = true;
                crate::log!(
                    "crabusb: hid mediacontrol {:04X}:{:04X} handoff if#{} alt={} cfg={} int_in=0x{:02X} mps={}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    target.configuration_value,
                    target.in_endpoint,
                    target.in_max_packet_size
                );
            }
            Err(err) => {
                unregister_active_stream(active_stream);
                crate::log!(
                    "crabusb: hid mediacontrol {:04X}:{:04X} spawn failed if#{} alt={} ep=0x{:02X}: {:?}\n",
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
