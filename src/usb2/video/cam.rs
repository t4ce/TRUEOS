extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::task::Poll;

use crab_usb::{USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::usb2::api::claim_interface;

const CAMERA_CONTROL_TIMEOUT_MS: u64 = 1000;
const UVC_REQ_SET_CUR: u8 = 0x01;
const UVC_REQ_GET_CUR: u8 = 0x81;
const UVC_REQ_GET_LEN: u8 = 0x85;
const UVC_REQ_GET_INFO: u8 = 0x86;
const UVC_REQ_GET_DEF: u8 = 0x87;
const UVC_VS_PROBE_CONTROL: u8 = 0x01;
const UVC_VS_COMMIT_CONTROL: u8 = 0x02;
const UVC_PROBE_LEN_V1: usize = 26;
const UVC_PROBE_LEN_V11: usize = 34;
const UVC_FRAME_INTERVAL_30FPS: u32 = 333_333;

const CAPTURE_WIDTH: usize = 1_920;
const CAPTURE_HEIGHT: usize = 1_080;
const CAPTURE_YUY2_FRAME: usize = CAPTURE_WIDTH * CAPTURE_HEIGHT * 2;
const CAPTURE_BOOT_DELAY_MS: u64 = 10_000;
const CAPTURE_ISO_PACKETS: usize = 128;
const CAPTURE_MAX_URBS: usize = 256;

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

#[derive(Copy, Clone, Debug)]
struct CameraStreamFormat {
    max_frame_bytes: usize,
    max_payload_bytes: usize,
    format_index: u8,
    frame_index: u8,
    frame_interval: u32,
}

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

fn uvc_read_u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    let bytes = buf.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn uvc_write_u16_le(buf: &mut [u8], offset: usize, value: u16) -> bool {
    let Some(dst) = buf.get_mut(offset..offset + 2) else {
        return false;
    };
    dst.copy_from_slice(&value.to_le_bytes());
    true
}

fn uvc_write_u32_le(buf: &mut [u8], offset: usize, value: u32) -> bool {
    let Some(dst) = buf.get_mut(offset..offset + 4) else {
        return false;
    };
    dst.copy_from_slice(&value.to_le_bytes());
    true
}

fn uvc_stream_control_setup(
    request: u8,
    selector: u8,
    interface_number: u8,
) -> usb_if::host::ControlSetup {
    usb_if::host::ControlSetup {
        request_type: usb_if::transfer::RequestType::Class,
        recipient: usb_if::transfer::Recipient::Interface,
        request: usb_if::transfer::Request::Other(request),
        value: u16::from(selector) << 8,
        index: u16::from(interface_number),
    }
}

async fn negotiate_uvc_stream(
    device: &mut crab_usb::Device,
    target: CameraTarget,
    vendor_id: u16,
    product_id: u16,
) -> Option<CameraStreamFormat> {
    let mut tmp = [0u8; 2];
    let probe_len = match with_timeout_or_none(
        device.control_in(
            uvc_stream_control_setup(
                UVC_REQ_GET_LEN,
                UVC_VS_PROBE_CONTROL,
                target.interface_number,
            ),
            &mut tmp,
        ),
        CAMERA_CONTROL_TIMEOUT_MS,
    )
    .await
    {
        Some(Ok(n)) if n >= 2 => {
            let raw = usize::from(u16::from_le_bytes(tmp));
            raw.clamp(UVC_PROBE_LEN_V1, UVC_PROBE_LEN_V11)
        }
        _ => UVC_PROBE_LEN_V1,
    };

    if let Some(Ok(n)) = with_timeout_or_none(
        device.control_in(
            uvc_stream_control_setup(
                UVC_REQ_GET_INFO,
                UVC_VS_PROBE_CONTROL,
                target.interface_number,
            ),
            &mut tmp,
        ),
        CAMERA_CONTROL_TIMEOUT_MS,
    )
    .await
    {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} uvc get-info if#{} bytes={} flags=0x{:02X}\n",
            vendor_id,
            product_id,
            target.interface_number,
            n,
            tmp[0]
        );
    }

    let mut probe = alloc::vec![0u8; probe_len];
    // GET_CUR probe (or GET_DEF fallback)
    let got_probe = match with_timeout_or_none(
        device.control_in(
            uvc_stream_control_setup(
                UVC_REQ_GET_CUR,
                UVC_VS_PROBE_CONTROL,
                target.interface_number,
            ),
            probe.as_mut_slice(),
        ),
        CAMERA_CONTROL_TIMEOUT_MS,
    )
    .await
    {
        Some(Ok(_)) => true,
        _ => {
            match with_timeout_or_none(
                device.control_in(
                    uvc_stream_control_setup(
                        UVC_REQ_GET_DEF,
                        UVC_VS_PROBE_CONTROL,
                        target.interface_number,
                    ),
                    probe.as_mut_slice(),
                ),
                CAMERA_CONTROL_TIMEOUT_MS,
            )
            .await
            {
                Some(Ok(_)) => true,
                _ => false,
            }
        }
    };

    if !got_probe {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} uvc probe read failed if#{}\n",
            vendor_id,
            product_id,
            target.interface_number
        );
        return None;
    }

    // Fill in defaults
    if *probe.get(2).unwrap_or(&0) == 0 {
        probe[2] = 1;
    }
    if *probe.get(3).unwrap_or(&0) == 0 {
        probe[3] = 1;
    }
    if uvc_read_u32_le(&probe, 4).unwrap_or(0) == 0 {
        let _ = uvc_write_u32_le(&mut probe, 4, UVC_FRAME_INTERVAL_30FPS);
    }
    let _ = uvc_write_u16_le(&mut probe, 0, 1);

    let fmt = *probe.get(2).unwrap_or(&0);
    let frm = *probe.get(3).unwrap_or(&0);
    let interval = uvc_read_u32_le(&probe, 4).unwrap_or(0);
    crate::log!(
        "crabusb: camera {:04X}:{:04X} uvc set-cur-probe if#{} fmt={} frame={} interval={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        fmt,
        frm,
        interval
    );

    // SET_CUR probe
    if with_timeout_or_none(
        device.control_out(
            uvc_stream_control_setup(
                UVC_REQ_SET_CUR,
                UVC_VS_PROBE_CONTROL,
                target.interface_number,
            ),
            &probe,
        ),
        CAMERA_CONTROL_TIMEOUT_MS,
    )
    .await
    .and_then(|r| r.ok())
    .is_none()
    {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} uvc set-cur-probe failed if#{}\n",
            vendor_id,
            product_id,
            target.interface_number
        );
        return None;
    }

    // GET_CUR after set (re-read negotiated values)
    let _ = with_timeout_or_none(
        device.control_in(
            uvc_stream_control_setup(
                UVC_REQ_GET_CUR,
                UVC_VS_PROBE_CONTROL,
                target.interface_number,
            ),
            probe.as_mut_slice(),
        ),
        CAMERA_CONTROL_TIMEOUT_MS,
    )
    .await;

    let max_frame = uvc_read_u32_le(&probe, 18).unwrap_or(CAPTURE_YUY2_FRAME as u32);
    let max_payload = uvc_read_u32_le(&probe, 22).unwrap_or(u32::from(target.max_packet_size));
    crate::log!(
        "crabusb: camera {:04X}:{:04X} uvc negotiated if#{} max_frame={} max_payload={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        max_frame,
        max_payload
    );

    // SET_CUR commit
    if with_timeout_or_none(
        device.control_out(
            uvc_stream_control_setup(
                UVC_REQ_SET_CUR,
                UVC_VS_COMMIT_CONTROL,
                target.interface_number,
            ),
            &probe,
        ),
        CAMERA_CONTROL_TIMEOUT_MS,
    )
    .await
    .and_then(|r| r.ok())
    .is_none()
    {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} uvc commit failed if#{}\n",
            vendor_id,
            product_id,
            target.interface_number
        );
        return None;
    }

    Some(CameraStreamFormat {
        max_frame_bytes: max_frame.max(1) as usize,
        max_payload_bytes: max_payload.max(u32::from(target.max_packet_size.max(1))) as usize,
        format_index: *probe.get(2).unwrap_or(&1),
        frame_index: *probe.get(3).unwrap_or(&1),
        frame_interval: uvc_read_u32_le(&probe, 4).unwrap_or(UVC_FRAME_INTERVAL_30FPS),
    })
}

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn yuy2_to_rgba(src: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut rgba = alloc::vec![0u8; width * height * 4];
    let mut si = 0usize;
    let mut di = 0usize;
    let end = width * height * 2;
    while si + 4 <= end && si + 4 <= src.len() && di + 8 <= rgba.len() {
        let y0 = i32::from(src[si]);
        let u = i32::from(src[si + 1]) - 128;
        let y1 = i32::from(src[si + 2]);
        let v = i32::from(src[si + 3]) - 128;
        si += 4;
        for y in [y0, y1] {
            let c = (y - 16).max(0);
            rgba[di] = clamp_u8((298 * c + 516 * u + 128) >> 8);
            rgba[di + 1] = clamp_u8((298 * c - 100 * u - 208 * v + 128) >> 8);
            rgba[di + 2] = clamp_u8((298 * c + 409 * v + 128) >> 8);
            rgba[di + 3] = 0xFF;
            di += 4;
        }
    }
    rgba
}

fn present_yuy2_snapshot(raw: &[u8], vendor_id: u16, product_id: u16) {
    let stride = CAPTURE_WIDTH * 2;
    let lines = (raw.len() / stride).min(CAPTURE_HEIGHT);
    if lines == 0 {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} snapshot too small bytes={}\n",
            vendor_id,
            product_id,
            raw.len()
        );
        return;
    }
    let rgba = yuy2_to_rgba(&raw[..lines * stride], CAPTURE_WIDTH, lines);
    // Pad to full height so overlay surface stays fixed size
    let full_len = CAPTURE_WIDTH * CAPTURE_HEIGHT * 4;
    let full = if lines == CAPTURE_HEIGHT {
        rgba
    } else {
        let mut buf = alloc::vec![0u8; full_len];
        let n = lines * CAPTURE_WIDTH * 4;
        buf[..n].copy_from_slice(&rgba[..n]);
        buf
    };
    crate::log!(
        "crabusb: camera {:04X}:{:04X} snapshot present lines={}/{} bytes={}\n",
        vendor_id,
        product_id,
        lines,
        CAPTURE_HEIGHT,
        raw.len()
    );
    crate::intel::present_rgba_overlay_top_right(
        &full,
        CAPTURE_WIDTH as u32,
        CAPTURE_HEIGHT as u32,
        CAPTURE_WIDTH * 4,
    );
}

/// Capture one frame from an isochronous endpoint then stop.
async fn capture_one_iso_frame(
    iso_in: &mut crab_usb::EndpointIsoIn,
    target: CameraTarget,
    sf: CameraStreamFormat,
    vendor_id: u16,
    product_id: u16,
) {
    let pkt_bytes = sf
        .max_payload_bytes
        .max(usize::from(target.max_packet_size.max(1)));
    let mut rx = alloc::vec![0u8; pkt_bytes * CAPTURE_ISO_PACKETS];
    let mut frame_buf = Vec::with_capacity(sf.max_frame_bytes.min(CAPTURE_YUY2_FRAME + 4096));
    let mut prev_fid: Option<u8> = None;
    let mut got_first_fid = false;

    for urb_idx in 0..CAPTURE_MAX_URBS {
        let read = match iso_in
            .submit_and_wait(rx.as_mut_slice(), CAPTURE_ISO_PACKETS)
            .await
        {
            Ok(n) => n,
            Err(err) => {
                if urb_idx < 8 {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} capture iso err urb={} err={:?}\n",
                        vendor_id,
                        product_id,
                        urb_idx,
                        err
                    );
                }
                continue; // iso errors are normal, skip
            }
        };
        if read == 0 {
            continue;
        }

        for pkt in rx[..read.min(rx.len())].chunks(pkt_bytes.max(1)) {
            if pkt.len() < 2 {
                continue;
            }
            let hdr_len = pkt[0] as usize;
            if hdr_len < 2 || hdr_len > pkt.len() {
                continue;
            }
            let flags = pkt[1];
            if (flags & 0x40) != 0 {
                continue;
            } // error bit
            let fid = flags & 0x01;
            let eof = (flags & 0x02) != 0;
            let payload = &pkt[hdr_len..];

            // Wait for the first FID transition so we start at a frame boundary
            if !got_first_fid {
                if prev_fid.is_some() && prev_fid != Some(fid) {
                    got_first_fid = true;
                    frame_buf.clear();
                }
                prev_fid = Some(fid);
                if !got_first_fid {
                    continue;
                }
            }

            // FID toggled = new frame; if we already have data, that's our frame
            if prev_fid != Some(fid) && !frame_buf.is_empty() {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} capture done urb={} bytes={} reason=fid-toggle\n",
                    vendor_id,
                    product_id,
                    urb_idx,
                    frame_buf.len()
                );
                present_yuy2_snapshot(&frame_buf, vendor_id, product_id);
                return;
            }
            prev_fid = Some(fid);

            if !payload.is_empty() {
                let room = sf.max_frame_bytes.saturating_sub(frame_buf.len());
                let take = payload.len().min(room);
                frame_buf.extend_from_slice(&payload[..take]);
            }

            if eof && !frame_buf.is_empty() {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} capture done urb={} bytes={} reason=eof\n",
                    vendor_id,
                    product_id,
                    urb_idx,
                    frame_buf.len()
                );
                present_yuy2_snapshot(&frame_buf, vendor_id, product_id);
                return;
            }
        }
    }

    // Didn't get a clean frame boundary — present whatever we have
    if !frame_buf.is_empty() {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} capture partial bytes={} urbs={}\n",
            vendor_id,
            product_id,
            frame_buf.len(),
            CAPTURE_MAX_URBS
        );
        present_yuy2_snapshot(&frame_buf, vendor_id, product_id);
    } else {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} capture got 0 bytes after {} urbs\n",
            vendor_id,
            product_id,
            CAPTURE_MAX_URBS
        );
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn camera_snapshot_task(
    mut device: crab_usb::Device,
    controller_id: u32,
    target: CameraTarget,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;

    // Wait for boot to settle
    Timer::after(EmbassyDuration::from_millis(CAPTURE_BOOT_DELAY_MS)).await;

    crate::log!(
        "crabusb: camera {:04X}:{:04X} snapshot begin ctrl={} if#{} alt={} ep=0x{:02X}\n",
        vendor_id,
        product_id,
        controller_id,
        target.interface_number,
        target.alternate_setting,
        target.endpoint_address
    );

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
        return;
    }

    let Some(sf) = negotiate_uvc_stream(&mut device, target, vendor_id, product_id).await else {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} uvc negotiation failed if#{}\n",
            vendor_id,
            product_id,
            target.interface_number
        );
        return;
    };

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(i) => i,
            Err(err) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                return;
            }
        };

    crate::log!(
        "crabusb: camera {:04X}:{:04X} snapshot capture start payload={} frame={}\n",
        vendor_id,
        product_id,
        sf.max_payload_bytes,
        sf.max_frame_bytes
    );

    match target.transport {
        CameraTransport::IsoIn => {
            match interface
                .endpoint_isochronous_in(target.endpoint_address)
                .await
            {
                Ok(mut iso_in) => {
                    capture_one_iso_frame(&mut iso_in, target, sf, vendor_id, product_id).await;
                }
                Err(err) => {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} iso open failed ep=0x{:02X}: {:?}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address,
                        err
                    );
                }
            }
        }
        CameraTransport::BulkIn => {
            crate::log!(
                "crabusb: camera {:04X}:{:04X} snapshot bulk not implemented\n",
                vendor_id,
                product_id
            );
        }
    }

    crate::log!("crabusb: camera {:04X}:{:04X} snapshot task done\n", vendor_id, product_id);
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

    match camera_snapshot_task(device, controller_id, target) {
        Ok(token) => {
            spawner.spawn(token);
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
            crate::log!(
                "crabusb: camera {:04X}:{:04X} spawn failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
        }
    }

    true
}
