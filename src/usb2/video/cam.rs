extern crate alloc;

use alloc::vec::Vec;
use core::cmp::min;
use core::future::Future;
use core::task::Poll;

use crab_usb::{USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::usb2::api::{InterfaceEndpointError, claim_interface};

const CAMERA_PREVIEW_READY: u32 =
    crate::r::readiness::UI2_READY | crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY;
const CAMERA_PREVIEW_TEX_ID: u32 = 4_721;
const CAMERA_PREVIEW_X: f32 = 960.0;
const CAMERA_PREVIEW_Y: f32 = 40.0;
const CAMERA_PREVIEW_Z: i16 = 36;
const CAMERA_PREVIEW_W: u32 = 320;
const CAMERA_PREVIEW_H: u32 = 240;
const CAMERA_FRAME_W: usize = 640;
const CAMERA_FRAME_H: usize = 480;
const CAMERA_FRAME_BYTES: usize = CAMERA_FRAME_W * CAMERA_FRAME_H;
const CAMERA_READ_TIMEOUT_MS: u64 = 1000;
const CAMERA_ISO_PACKETS_PER_URB: usize = 8;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CameraTransport {
    BulkIn,
    IsoIn,
}

#[derive(Copy, Clone, Debug)]
struct CameraTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    endpoint_address: u8,
    max_packet_size: u16,
    transport: CameraTransport,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveCameraStream {
    stable_id: u32,
    controller_id: u32,
    slot_id: u32,
    interface_number: u8,
    endpoint_address: u8,
}

#[derive(Default)]
struct UvcFrameAssembler {
    frame_id: Option<u8>,
    bytes: Vec<u8>,
    oversize_logged: bool,
}

static ACTIVE_CAMERA_STREAMS: Mutex<Vec<ActiveCameraStream>> = Mutex::new(Vec::new());

fn pick_camera_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<CameraTarget> {
    let mut best: Option<CameraTarget> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.class != 0x0E || alt.subclass != 0x02 {
                    continue;
                }

                let Some(endpoint) = alt.endpoints.iter().find(|ep| {
                    ep.direction == usb_if::transfer::Direction::In
                        && matches!(
                            ep.transfer_type,
                            usb_if::descriptor::EndpointType::Bulk
                                | usb_if::descriptor::EndpointType::Isochronous
                        )
                }) else {
                    continue;
                };

                let transport = match endpoint.transfer_type {
                    usb_if::descriptor::EndpointType::Bulk => CameraTransport::BulkIn,
                    usb_if::descriptor::EndpointType::Isochronous => CameraTransport::IsoIn,
                    _ => continue,
                };

                let candidate = CameraTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    endpoint_address: endpoint.address,
                    max_packet_size: endpoint.max_packet_size,
                    transport,
                };

                let replace = match best {
                    None => true,
                    Some(current) => {
                        candidate.alternate_setting > current.alternate_setting
                            || (candidate.alternate_setting == current.alternate_setting
                                && candidate.interface_number < current.interface_number)
                            || (candidate.alternate_setting == current.alternate_setting
                                && candidate.interface_number == current.interface_number
                                && matches!(candidate.transport, CameraTransport::BulkIn)
                                && matches!(current.transport, CameraTransport::IsoIn))
                    }
                };

                if replace {
                    best = Some(candidate);
                }
            }
        }
    }

    best
}

fn register_active_camera_stream(stream: ActiveCameraStream) -> bool {
    let mut streams = ACTIVE_CAMERA_STREAMS.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream);
    true
}

fn unregister_active_camera_stream(stream: ActiveCameraStream) {
    let mut streams = ACTIVE_CAMERA_STREAMS.lock();
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

fn create_preview_surface() -> Option<crate::r::ui2::Ui2SurfaceWindow> {
    let surface = crate::r::ui2::Ui2SurfaceWindow::new(
        "USB Camera",
        crate::r::ui2::Ui2Rect {
            x: CAMERA_PREVIEW_X,
            y: CAMERA_PREVIEW_Y,
            w: CAMERA_PREVIEW_W as f32,
            h: CAMERA_PREVIEW_H as f32,
        },
        CAMERA_PREVIEW_Z,
        224,
        CAMERA_PREVIEW_TEX_ID,
        true,
        [0x08, 0x0C, 0x10, 0xE0],
    )?;

    let _ = crate::r::ui2::set_window_titlebar_visible(surface.window_id(), false);
    let _ = crate::r::ui2::set_window_bottom_bar_visible(surface.window_id(), false);
    let _ = crate::r::ui2::set_window_hit_test_visible(surface.window_id(), false);
    Some(surface)
}

fn grayscale_frame_to_preview_rgba(frame: &[u8]) -> Vec<u8> {
    let mut rgba = alloc::vec![
        0u8;
        (CAMERA_PREVIEW_W as usize)
            .saturating_mul(CAMERA_PREVIEW_H as usize)
            .saturating_mul(4)
    ];

    for dst_y in 0..(CAMERA_PREVIEW_H as usize) {
        let src_y = dst_y.saturating_mul(CAMERA_FRAME_H) / (CAMERA_PREVIEW_H as usize);
        for dst_x in 0..(CAMERA_PREVIEW_W as usize) {
            let src_x = dst_x.saturating_mul(CAMERA_FRAME_W) / (CAMERA_PREVIEW_W as usize);
            let src_idx = src_y.saturating_mul(CAMERA_FRAME_W).saturating_add(src_x);
            if src_idx >= frame.len() {
                continue;
            }
            let luma = frame[src_idx];
            let dst_idx = (dst_y
                .saturating_mul(CAMERA_PREVIEW_W as usize)
                .saturating_add(dst_x))
            .saturating_mul(4);
            if dst_idx + 4 > rgba.len() {
                continue;
            }
            rgba[dst_idx] = luma;
            rgba[dst_idx + 1] = luma;
            rgba[dst_idx + 2] = luma;
            rgba[dst_idx + 3] = 0xFF;
        }
    }

    rgba
}

fn try_present_frame(
    surface: Option<&crate::r::ui2::Ui2SurfaceWindow>,
    frame: &[u8],
    frame_count: u64,
    vendor_id: u16,
    product_id: u16,
) {
    if frame.len() != CAMERA_FRAME_BYTES {
        if frame_count <= 8 || frame_count.is_multiple_of(120) {
            crate::log!(
                "crabusb: camera {:04X}:{:04X} frame={} size={} expected={} note=waiting-for-raw-640x480x8\n",
                vendor_id,
                product_id,
                frame_count,
                frame.len(),
                CAMERA_FRAME_BYTES
            );
        }
        return;
    }

    let Some(surface) = surface else {
        return;
    };

    let rgba = grayscale_frame_to_preview_rgba(frame);
    if !surface.upload_rgba(rgba.as_slice(), "usb-camera-preview") {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} preview upload failed frame={} tex={} size={}x{}\n",
            vendor_id,
            product_id,
            frame_count,
            surface.tex_id(),
            CAMERA_PREVIEW_W,
            CAMERA_PREVIEW_H
        );
    }
}

fn ingest_uvc_payload(
    assembler: &mut UvcFrameAssembler,
    packet: &[u8],
    frame_count: &mut u64,
    surface: Option<&crate::r::ui2::Ui2SurfaceWindow>,
    vendor_id: u16,
    product_id: u16,
) {
    if packet.len() < 2 {
        return;
    }

    let header_len = packet[0] as usize;
    if header_len < 2 || header_len > packet.len() {
        return;
    }

    let header_flags = packet[1];
    if (header_flags & 0x40) != 0 {
        return;
    }

    let frame_id = header_flags & 0x01;
    let end_of_frame = (header_flags & 0x02) != 0;
    if assembler.frame_id != Some(frame_id) && !assembler.bytes.is_empty() {
        assembler.bytes.clear();
    }
    assembler.frame_id = Some(frame_id);

    let payload = &packet[header_len..];
    if !payload.is_empty() {
        let remaining = CAMERA_FRAME_BYTES
            .saturating_mul(4)
            .saturating_sub(assembler.bytes.len());
        if remaining == 0 {
            if !assembler.oversize_logged {
                assembler.oversize_logged = true;
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} dropping oversized frame chunk len={} cap={}\n",
                    vendor_id,
                    product_id,
                    payload.len(),
                    CAMERA_FRAME_BYTES.saturating_mul(4)
                );
            }
        } else {
            assembler
                .bytes
                .extend_from_slice(&payload[..min(payload.len(), remaining)]);
        }
    }

    if end_of_frame {
        *frame_count = frame_count.wrapping_add(1);
        try_present_frame(surface, assembler.bytes.as_slice(), *frame_count, vendor_id, product_id);
        assembler.bytes.clear();
        assembler.oversize_logged = false;
    }
}

async fn stream_bulk_camera(
    bulk_in: &mut crab_usb::EndpointBulkIn,
    target: CameraTarget,
    surface: Option<&crate::r::ui2::Ui2SurfaceWindow>,
    vendor_id: u16,
    product_id: u16,
) {
    let read_len = usize::from(target.max_packet_size.max(512)).saturating_mul(16);
    let mut rx = alloc::vec![0u8; read_len];
    let mut assembler = UvcFrameAssembler {
        frame_id: None,
        bytes: Vec::with_capacity(CAMERA_FRAME_BYTES),
        oversize_logged: false,
    };
    let mut frame_count = 0u64;
    let mut timeout_logs = 0u32;

    loop {
        match with_timeout_or_none(
            bulk_in.submit_and_wait(rx.as_mut_slice()),
            CAMERA_READ_TIMEOUT_MS,
        )
        .await
        {
            None => {
                timeout_logs = timeout_logs.wrapping_add(1);
                if timeout_logs <= 8 || timeout_logs.is_multiple_of(32) {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} bulk timeout ep=0x{:02X} count={}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address,
                        timeout_logs
                    );
                }
            }
            Some(Ok(read)) => {
                timeout_logs = 0;
                if read == 0 {
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                    continue;
                }
                ingest_uvc_payload(
                    &mut assembler,
                    &rx[..read.min(rx.len())],
                    &mut frame_count,
                    surface,
                    vendor_id,
                    product_id,
                );
            }
            Some(Err(err)) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} bulk stop ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.endpoint_address,
                    err
                );
                break;
            }
        }
    }
}

async fn stream_iso_camera(
    iso_in: &mut crab_usb::EndpointIsoIn,
    target: CameraTarget,
    surface: Option<&crate::r::ui2::Ui2SurfaceWindow>,
    vendor_id: u16,
    product_id: u16,
) {
    let packet_bytes = usize::from(target.max_packet_size.max(1));
    let mut rx = alloc::vec![0u8; packet_bytes.saturating_mul(CAMERA_ISO_PACKETS_PER_URB)];
    let mut assembler = UvcFrameAssembler {
        frame_id: None,
        bytes: Vec::with_capacity(CAMERA_FRAME_BYTES),
        oversize_logged: false,
    };
    let mut frame_count = 0u64;

    loop {
        match iso_in
            .submit_and_wait(rx.as_mut_slice(), CAMERA_ISO_PACKETS_PER_URB)
            .await
        {
            Ok(read) => {
                if read == 0 {
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                    continue;
                }
                for packet in rx[..read.min(rx.len())].chunks(packet_bytes.max(1)) {
                    ingest_uvc_payload(
                        &mut assembler,
                        packet,
                        &mut frame_count,
                        surface,
                        vendor_id,
                        product_id,
                    );
                }
            }
            Err(err) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} iso stop ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.endpoint_address,
                    err
                );
                break;
            }
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn camera_stream_task(
    mut device: crab_usb::Device,
    controller_id: u32,
    target: CameraTarget,
    active_stream: ActiveCameraStream,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
        unregister_active_camera_stream(active_stream);
        return;
    }

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_camera_stream(active_stream);
                return;
            }
        };

    crate::r::readiness::wait_for(CAMERA_PREVIEW_READY).await;
    let surface = create_preview_surface();
    if let Some(surface) = surface.as_ref() {
        let _ = crate::r::ui2::set_window_title(
            surface.window_id(),
            if matches!(target.transport, CameraTransport::BulkIn) {
                "USB Camera Bulk"
            } else {
                "USB Camera Iso"
            },
        );
    }

    crate::log!(
        "crabusb: camera {:04X}:{:04X} stream start ctrl={} if#{} alt={} ep=0x{:02X} transport={:?} packet={} preview={}x{}\n",
        vendor_id,
        product_id,
        controller_id,
        target.interface_number,
        target.alternate_setting,
        target.endpoint_address,
        target.transport,
        target.max_packet_size,
        CAMERA_PREVIEW_W,
        CAMERA_PREVIEW_H
    );

    match target.transport {
        CameraTransport::BulkIn => {
            let mut bulk_in = match interface.endpoint_bulk_in(target.endpoint_address).await {
                Ok(endpoint) => endpoint,
                Err(InterfaceEndpointError::WrongKind { .. }) => {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} bulk kind mismatch ep=0x{:02X}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address
                    );
                    unregister_active_camera_stream(active_stream);
                    return;
                }
                Err(InterfaceEndpointError::Usb(err)) => {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} bulk open failed ep=0x{:02X}: {:?}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address,
                        err
                    );
                    unregister_active_camera_stream(active_stream);
                    return;
                }
            };
            stream_bulk_camera(&mut bulk_in, target, surface.as_ref(), vendor_id, product_id).await;
        }
        CameraTransport::IsoIn => {
            let mut iso_in = match interface
                .endpoint_isochronous_in(target.endpoint_address)
                .await
            {
                Ok(endpoint) => endpoint,
                Err(InterfaceEndpointError::WrongKind { .. }) => {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} iso kind mismatch ep=0x{:02X}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address
                    );
                    unregister_active_camera_stream(active_stream);
                    return;
                }
                Err(InterfaceEndpointError::Usb(err)) => {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} iso open failed ep=0x{:02X}: {:?}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address,
                        err
                    );
                    unregister_active_camera_stream(active_stream);
                    return;
                }
            };
            stream_iso_camera(&mut iso_in, target, surface.as_ref(), vendor_id, product_id).await;
        }
    }

    unregister_active_camera_stream(active_stream);
}

pub(crate) async fn maybe_start_camera(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let Some(target) = pick_camera_target(dev_info.configurations()) else {
        return false;
    };

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let stable_id = dev_info.stable_id().raw();

    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: camera {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return true;
        }
    };

    let active_stream = ActiveCameraStream {
        stable_id,
        controller_id,
        slot_id: u32::from(device.slot_id()),
        interface_number: target.interface_number,
        endpoint_address: target.endpoint_address,
    };
    if !register_active_camera_stream(active_stream) {
        return true;
    }

    match spawner.spawn(camera_stream_task(device, controller_id, target, active_stream)) {
        Ok(()) => {
            crate::log!(
                "crabusb: camera {:04X}:{:04X} handoff if#{} alt={} cfg={} ep=0x{:02X} transport={:?} packet={} stable_id={}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.configuration_value,
                target.endpoint_address,
                target.transport,
                target.max_packet_size,
                stable_id
            );
        }
        Err(err) => {
            unregister_active_camera_stream(active_stream);
            crate::log!(
                "crabusb: camera {:04X}:{:04X} spawn failed if#{} alt={}: {:?}\n",
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
