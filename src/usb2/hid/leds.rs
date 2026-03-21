use crab_usb::{Device, EndpointBulkOut, EndpointInterruptOut, err::TransferError, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

use super::super::api::claim_interface;

const LED_VID_JGINYUE: u16 = 0x0416;
const LED_PID_JGINYUE: u16 = 0xA125;
const LED_VID_MSI: u16 = 0x1462;
const LED_PID_MSI_MYSTIC_LIGHT: u16 = 0x7E03;
const LED_RUNTIME_POLL_MS: u64 = 2_000;
const MAX_LED_RUNTIMES: usize = 4;
const MAX_ACTIVE_STREAMS: usize = 4;

#[derive(Copy, Clone, Debug)]
enum LedOutEndpointKind {
    Interrupt,
    Bulk,
}

#[derive(Copy, Clone, Debug)]
struct LedEndpointInfo {
    address: u8,
    max_packet_size: u16,
    kind: LedOutEndpointKind,
}

#[derive(Copy, Clone, Debug)]
struct LedTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    protocol: u8,
    out_endpoint: LedEndpointInfo,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveLedStream {
    controller_id: u32,
    slot_id: u32,
}

enum LedOutEndpoint {
    Interrupt(EndpointInterruptOut),
    Bulk(EndpointBulkOut),
}

impl LedOutEndpoint {
    async fn submit_and_wait(&mut self, buff: &[u8]) -> Result<usize, TransferError> {
        match self {
            LedOutEndpoint::Interrupt(ep) => ep.submit_and_wait(buff).await,
            LedOutEndpoint::Bulk(ep) => ep.submit_and_wait(buff).await,
        }
    }
}

struct LedRuntime {
    controller_id: u32,
    slot_id: u32,
    vendor_id: u16,
    product_id: u16,
    device: Device,
    interface_number: u8,
    out_report_id: u8,
    out_report_total_len: u16,
    out_endpoint: LedOutEndpoint,
}

unsafe impl Send for LedRuntime {}

static ACTIVE_LED_STREAMS: Mutex<Vec<ActiveLedStream, MAX_ACTIVE_STREAMS>> = Mutex::new(Vec::new());
static LED_RUNTIMES: Mutex<Vec<LedRuntime, MAX_LED_RUNTIMES>> = Mutex::new(Vec::new());

pub fn is_supported_led_controller(vid: u16, pid: u16) -> bool {
    (vid == LED_VID_JGINYUE && pid == LED_PID_JGINYUE)
        || (vid == LED_VID_MSI && pid == LED_PID_MSI_MYSTIC_LIGHT)
}

#[inline]
fn default_report_shape(vid: u16, pid: u16) -> (u8, u16) {
    if vid == LED_VID_MSI && pid == LED_PID_MSI_MYSTIC_LIGHT {
        (1, 64)
    } else {
        (0, 64)
    }
}

fn register_active_led_stream(stream: ActiveLedStream) -> bool {
    let mut streams = ACTIVE_LED_STREAMS.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream).is_ok()
}

fn unregister_active_led_stream(stream: ActiveLedStream) {
    let mut streams = ACTIVE_LED_STREAMS.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
}

fn register_runtime(rt: LedRuntime) {
    let mut runtimes = LED_RUNTIMES.lock();
    if let Some(existing) = runtimes.iter_mut().find(|existing| {
        existing.controller_id == rt.controller_id && existing.slot_id == rt.slot_id
    }) {
        *existing = rt;
        return;
    }
    let _ = runtimes.push(rt);
}

fn take_runtime(controller_id: u32, slot_id: u32) -> Option<LedRuntime> {
    let mut runtimes = LED_RUNTIMES.lock();
    let idx = runtimes
        .iter()
        .position(|rt| rt.controller_id == controller_id && rt.slot_id == slot_id)?;
    Some(runtimes.remove(idx))
}

fn first_runtime_key() -> Option<(u32, u32)> {
    let guard = LED_RUNTIMES.lock();
    guard.first().map(|rt| (rt.controller_id, rt.slot_id))
}

async fn send_hid_set_report(
    device: &mut Device,
    interface_number: u8,
    report_id: u8,
    data: &[u8],
) -> Result<(), TransferError> {
    device
        .control_out(
            usb_if::host::ControlSetup {
                request_type: usb_if::transfer::RequestType::Class,
                recipient: usb_if::transfer::Recipient::Interface,
                request: usb_if::transfer::Request::Other(0x09),
                value: ((2u16) << 8) | u16::from(report_id),
                index: u16::from(interface_number),
            },
            data,
        )
        .await
        .map(|_| ())
}

fn build_report_bytes(report_id: u8, total_len: u16, payload: &[u8]) -> heapless::Vec<u8, 64> {
    let mut report = heapless::Vec::<u8, 64>::new();
    if report_id != 0 {
        let _ = report.push(report_id);
    }

    let target_total = if total_len != 0 {
        core::cmp::min(total_len as usize, 64)
    } else {
        0
    };
    let target_payload = if target_total != 0 {
        target_total.saturating_sub(report.len())
    } else {
        64usize.saturating_sub(report.len())
    };
    let take = core::cmp::min(payload.len(), target_payload);
    let _ = report.extend_from_slice(&payload[..take]);
    if target_total != 0 {
        while report.len() < target_total {
            let _ = report.push(0);
        }
    }

    report
}

async fn send_output_report(
    controller_id: u32,
    slot_id: u32,
    report_id: u8,
    payload: &[u8],
) -> Result<(), ()> {
    let mut rt = take_runtime(controller_id, slot_id).ok_or(())?;
    let report = build_report_bytes(report_id, rt.out_report_total_len, payload);
    let control_payload = if report_id != 0 {
        report.get(1..).unwrap_or(&[])
    } else {
        report.as_slice()
    };

    let result = match rt.out_endpoint.submit_and_wait(report.as_slice()).await {
        Ok(_) => Ok(()),
        Err(err) => {
            crate::log!(
                "crabusb: leds {:04X}:{:04X} out transfer failed slot={} err={:?}; falling back to set-report\n",
                rt.vendor_id,
                rt.product_id,
                rt.slot_id,
                err
            );
            send_hid_set_report(
                &mut rt.device,
                rt.interface_number,
                report_id,
                control_payload,
            )
            .await
            .map_err(|_| ())
        }
    };

    register_runtime(rt);
    result
}

pub fn is_online() -> bool {
    !LED_RUNTIMES.lock().is_empty()
}

pub async fn send_output_report_first(report_id: u8, payload: &[u8]) -> Result<(), ()> {
    let Some((controller_id, slot_id)) = first_runtime_key() else {
        return Err(());
    };
    send_output_report(controller_id, slot_id, report_id, payload).await
}

pub async fn send_output_report_for_handle(
    controller_id: usize,
    slot_id: u32,
    report_id: u8,
    payload: &[u8],
) -> Result<(), ()> {
    send_output_report(controller_id as u32, slot_id, report_id, payload).await
}

pub async fn send_preferred_output_report_first(payload: &[u8]) -> Result<(), ()> {
    let Some((controller_id, slot_id)) = first_runtime_key() else {
        return Err(());
    };
    send_preferred_output_report_for_handle(controller_id as usize, slot_id, payload).await
}

pub async fn send_preferred_output_report_for_handle(
    controller_id: usize,
    slot_id: u32,
    payload: &[u8],
) -> Result<(), ()> {
    let (report_id, total_len) = {
        let guard = LED_RUNTIMES.lock();
        let Some(rt) = guard
            .iter()
            .find(|rt| rt.controller_id == controller_id as u32 && rt.slot_id == slot_id)
        else {
            return Err(());
        };
        (rt.out_report_id, rt.out_report_total_len)
    };

    let want_total = core::cmp::min(total_len as usize, 64);
    let want_payload = if want_total == 0 {
        payload.len()
    } else {
        want_total.saturating_sub((report_id != 0) as usize)
    };
    let take = core::cmp::min(payload.len(), want_payload);
    send_output_report(controller_id as u32, slot_id, report_id, &payload[..take]).await
}

fn pick_led_target(configs: &[usb_if::descriptor::ConfigurationDescriptor]) -> Option<LedTarget> {
    let mut best: Option<(u8, LedTarget)> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.alternate_setting != 0 {
                    continue;
                }

                let out_endpoint = alt.endpoints.iter().find_map(|ep| {
                    if ep.direction != usb_if::transfer::Direction::Out {
                        return None;
                    }
                    let kind = match ep.transfer_type {
                        usb_if::descriptor::EndpointType::Interrupt => {
                            Some(LedOutEndpointKind::Interrupt)
                        }
                        usb_if::descriptor::EndpointType::Bulk => Some(LedOutEndpointKind::Bulk),
                        _ => None,
                    }?;
                    Some(LedEndpointInfo {
                        address: ep.address,
                        max_packet_size: ep.max_packet_size,
                        kind,
                    })
                });

                let Some(out_endpoint) = out_endpoint else {
                    continue;
                };

                let score = ((alt.class == 0x03) as u8) * 4
                    + matches!(out_endpoint.kind, LedOutEndpointKind::Interrupt) as u8 * 2
                    + (alt.protocol != 0) as u8;

                let candidate = LedTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    protocol: alt.protocol,
                    out_endpoint,
                };

                match best {
                    None => best = Some((score, candidate)),
                    Some((best_score, _)) if score > best_score => best = Some((score, candidate)),
                    _ => {}
                }
            }
        }
    }

    best.map(|(_, target)| target)
}

#[embassy_executor::task(pool_size = 4)]
pub async fn led_controller_task(mut device: Device, controller_id: u32, target: LedTarget) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot_id = u32::from(device.slot_id());
    let active_stream = ActiveLedStream {
        controller_id,
        slot_id,
    };

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: leds {:04X}:{:04X} set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
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
                "crabusb: leds {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                err
            );
            unregister_active_led_stream(active_stream);
            return;
        }
    };

    let out_endpoint = match target.out_endpoint.kind {
        LedOutEndpointKind::Interrupt => match interface
            .endpoint_interrupt_out(target.out_endpoint.address)
            .await
        {
            Ok(ep) => LedOutEndpoint::Interrupt(ep),
            Err(err) => {
                crate::log!(
                    "crabusb: leds {:04X}:{:04X} interrupt_out open failed ep=0x{:02X}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.out_endpoint.address,
                    err
                );
                unregister_active_led_stream(active_stream);
                return;
            }
        },
        LedOutEndpointKind::Bulk => match interface
            .endpoint_bulk_out(target.out_endpoint.address)
            .await
        {
            Ok(ep) => LedOutEndpoint::Bulk(ep),
            Err(err) => {
                crate::log!(
                    "crabusb: leds {:04X}:{:04X} bulk_out open failed ep=0x{:02X}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.out_endpoint.address,
                    err
                );
                unregister_active_led_stream(active_stream);
                return;
            }
        },
    };

    let (out_report_id, out_report_total_len) = default_report_shape(vendor_id, product_id);

    register_runtime(LedRuntime {
        controller_id,
        slot_id,
        vendor_id,
        product_id,
        device,
        interface_number: target.interface_number,
        out_report_id,
        out_report_total_len,
        out_endpoint,
    });

    crate::log!(
        "crabusb: leds {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} out_ep=0x{:02X} kind={} mps={} rid={} total_len={} proto={:02X}\n",
        vendor_id,
        product_id,
        slot_id,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.out_endpoint.address,
        match target.out_endpoint.kind {
            LedOutEndpointKind::Interrupt => "int",
            LedOutEndpointKind::Bulk => "bulk",
        },
        target.out_endpoint.max_packet_size,
        out_report_id,
        out_report_total_len,
        target.protocol
    );

    loop {
        Timer::after(EmbassyDuration::from_millis(LED_RUNTIME_POLL_MS)).await;
    }
}

pub(crate) async fn maybe_start_led_controller(
    host: &mut crab_usb::USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    if !is_supported_led_controller(vendor_id, product_id) {
        return false;
    }

    let Some(target) = pick_led_target(dev_info.configurations()) else {
        crate::log!(
            "crabusb: leds {:04X}:{:04X} no suitable out interface found\n",
            vendor_id,
            product_id
        );
        return true;
    };

    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: leds {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return true;
        }
    };

    let active_stream = ActiveLedStream {
        controller_id,
        slot_id: u32::from(device.slot_id()),
    };
    if !register_active_led_stream(active_stream) {
        return true;
    }

    match spawner.spawn(led_controller_task(device, controller_id, target)) {
        Ok(()) => {
            crate::log!(
                "crabusb: leds {:04X}:{:04X} handoff if#{} alt={} cfg={} out_ep=0x{:02X} kind={} mps={} proto={:02X}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.configuration_value,
                target.out_endpoint.address,
                match target.out_endpoint.kind {
                    LedOutEndpointKind::Interrupt => "int",
                    LedOutEndpointKind::Bulk => "bulk",
                },
                target.out_endpoint.max_packet_size,
                target.protocol
            );
        }
        Err(err) => {
            unregister_active_led_stream(active_stream);
            crate::log!(
                "crabusb: leds {:04X}:{:04X} spawn failed if#{} alt={}: {:?}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                err
            );
        }
    }

    true
}
