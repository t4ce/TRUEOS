extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::task::Poll;

use crab_usb::{USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::usb2::api::{EndpointBulkIn, EndpointIsoIn, EndpointSubmitExt, claim_interface};

const CAMERA_CONTROL_TIMEOUT_MS: u64 = 1000;
const CAMERA_BOOT_DELAY_MS: u64 = 2500;
const CAMERA_MAX_EMPTY_READS: u32 = 512;

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

const FULLHD_WIDTH: u16 = 1920;
const FULLHD_HEIGHT: u16 = 1080;
const FULLHD_YUY2_BYTES: usize = FULLHD_WIDTH as usize * FULLHD_HEIGHT as usize * 2;
const MAX_REASONABLE_FRAME_BYTES: usize = 16 * 1024 * 1024;
const ISO_PACKETS_PER_READ: usize = 64;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CameraTransport {
    BulkIn,
    IsoIn,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FrameEncoding {
    Mjpeg,
    Yuy2,
    Unknown,
}

impl FrameEncoding {
    const fn name(self) -> &'static str {
        match self {
            Self::Mjpeg => "mjpeg",
            Self::Yuy2 => "yuy2",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct CameraTarget {
    configuration_index: u8,
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    endpoint_address: u8,
    max_packet_size: u16,
    packets_per_microframe: usize,
    transport: CameraTransport,
}

#[derive(Copy, Clone, Debug)]
struct UvcStreamChoice {
    encoding: FrameEncoding,
    format_index: u8,
    frame_index: u8,
    width: u16,
    height: u16,
    frame_interval: u32,
    max_frame_bytes: usize,
}

#[derive(Copy, Clone, Debug)]
struct CameraStreamFormat {
    encoding: FrameEncoding,
    width: u16,
    height: u16,
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

    for (config_index, config) in configs.iter().enumerate() {
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
                let bytes_per_service = usize::from(endpoint.max_packet_size.max(1))
                    .saturating_mul(endpoint.packets_per_microframe.max(1));
                let candidate = CameraTarget {
                    configuration_index: config_index.min(u8::MAX as usize) as u8,
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    endpoint_address: endpoint.address,
                    max_packet_size: endpoint.max_packet_size,
                    packets_per_microframe: endpoint.packets_per_microframe,
                    transport,
                };

                let replace = match best {
                    None => true,
                    Some(current) => {
                        let current_bytes = usize::from(current.max_packet_size.max(1))
                            .saturating_mul(current.packets_per_microframe.max(1));
                        bytes_per_service > current_bytes
                            || (bytes_per_service == current_bytes
                                && candidate.transport == CameraTransport::BulkIn
                                && current.transport != CameraTransport::BulkIn)
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

fn read_u16_le(buf: &[u8], offset: usize) -> Option<u16> {
    let bytes = buf.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    let bytes = buf.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) -> bool {
    let Some(dst) = buf.get_mut(offset..offset + 2) else {
        return false;
    };
    dst.copy_from_slice(&value.to_le_bytes());
    true
}

fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) -> bool {
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

async fn read_raw_configuration_descriptor(
    device: &mut crab_usb::Device,
    config_index: u8,
) -> Option<Vec<u8>> {
    let mut header = [0u8; 9];
    if device
        .ctrl_ep_mut()
        .get_descriptor(
            usb_if::descriptor::DescriptorType::CONFIGURATION,
            config_index,
            0,
            &mut header,
        )
        .await
        .is_err()
    {
        return None;
    }
    let total_len = usize::from(read_u16_le(&header, 2)?);
    if total_len < header.len() || total_len > 4096 {
        return None;
    }
    let mut raw = alloc::vec![0u8; total_len];
    device
        .ctrl_ep_mut()
        .get_descriptor(
            usb_if::descriptor::DescriptorType::CONFIGURATION,
            config_index,
            0,
            raw.as_mut_slice(),
        )
        .await
        .ok()?;
    Some(raw)
}

fn guid_is_yuy2(guid: &[u8]) -> bool {
    guid.len() >= 16
        && &guid[0..4] == b"YUY2"
        && guid[4..16] == [0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]
}

fn parse_uvc_stream_choices(raw: &[u8], interface_number: u8) -> Vec<UvcStreamChoice> {
    let mut choices = Vec::new();
    let mut off = 0usize;
    let mut current_if = None;
    let mut current_encoding = FrameEncoding::Unknown;
    let mut current_format_index = 0u8;

    while off + 2 <= raw.len() {
        let len = raw[off] as usize;
        if len < 2 || off + len > raw.len() {
            break;
        }
        let desc = &raw[off..off + len];
        match desc[1] {
            0x04 if len >= 9 => {
                current_if = Some(desc[2]);
                current_encoding = FrameEncoding::Unknown;
                current_format_index = 0;
            }
            0x24 if current_if == Some(interface_number) && len >= 3 => match desc[2] {
                0x04 if len >= 27 => {
                    current_format_index = desc[3];
                    current_encoding = if guid_is_yuy2(&desc[5..21]) {
                        FrameEncoding::Yuy2
                    } else {
                        FrameEncoding::Unknown
                    };
                }
                0x05 if len >= 26 && current_format_index != 0 => {
                    let width = read_u16_le(desc, 5).unwrap_or(0);
                    let height = read_u16_le(desc, 7).unwrap_or(0);
                    let max_frame = read_u32_le(desc, 17).unwrap_or(0) as usize;
                    let interval = read_u32_le(desc, 21).unwrap_or(UVC_FRAME_INTERVAL_30FPS);
                    choices.push(UvcStreamChoice {
                        encoding: current_encoding,
                        format_index: current_format_index,
                        frame_index: desc[3],
                        width,
                        height,
                        frame_interval: if interval == 0 {
                            UVC_FRAME_INTERVAL_30FPS
                        } else {
                            interval
                        },
                        max_frame_bytes: max_frame,
                    });
                }
                0x06 if len >= 11 => {
                    current_format_index = desc[3];
                    current_encoding = FrameEncoding::Mjpeg;
                }
                0x07 if len >= 26 && current_format_index != 0 => {
                    let width = read_u16_le(desc, 5).unwrap_or(0);
                    let height = read_u16_le(desc, 7).unwrap_or(0);
                    let max_frame = read_u32_le(desc, 17).unwrap_or(0) as usize;
                    let interval = read_u32_le(desc, 21).unwrap_or(UVC_FRAME_INTERVAL_30FPS);
                    choices.push(UvcStreamChoice {
                        encoding: current_encoding,
                        format_index: current_format_index,
                        frame_index: desc[3],
                        width,
                        height,
                        frame_interval: if interval == 0 {
                            UVC_FRAME_INTERVAL_30FPS
                        } else {
                            interval
                        },
                        max_frame_bytes: max_frame,
                    });
                }
                _ => {}
            },
            _ => {}
        }
        off += len;
    }

    choices
}

fn choose_stream(choices: &[UvcStreamChoice]) -> Option<UvcStreamChoice> {
    fn score(choice: UvcStreamChoice) -> u32 {
        let fullhd = choice.width == FULLHD_WIDTH && choice.height == FULLHD_HEIGHT;
        let encoding = match choice.encoding {
            FrameEncoding::Mjpeg => 400,
            FrameEncoding::Yuy2 => 300,
            FrameEncoding::Unknown => 0,
        };
        let size = if fullhd {
            10_000
        } else {
            u32::from(choice.width).saturating_mul(u32::from(choice.height)) / 1000
        };
        size + encoding
    }
    choices
        .iter()
        .copied()
        .filter(|c| c.encoding != FrameEncoding::Unknown && c.width != 0 && c.height != 0)
        .max_by_key(|c| score(*c))
}

async fn negotiate_uvc_stream(
    device: &mut crab_usb::Device,
    target: CameraTarget,
    choice: UvcStreamChoice,
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
            usize::from(u16::from_le_bytes(tmp)).clamp(UVC_PROBE_LEN_V1, UVC_PROBE_LEN_V11)
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
        _ => with_timeout_or_none(
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
        .is_some_and(|r| r.is_ok()),
    };
    if !got_probe {
        return None;
    }

    let _ = write_u16_le(&mut probe, 0, 1);
    if probe.len() > 2 {
        probe[2] = choice.format_index;
    }
    if probe.len() > 3 {
        probe[3] = choice.frame_index;
    }
    let _ = write_u32_le(&mut probe, 4, choice.frame_interval);

    crate::log!(
        "crabusb: camera {:04X}:{:04X} uvc probe request if#{} fmt={} frame={} {}x{} enc={} interval={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        choice.format_index,
        choice.frame_index,
        choice.width,
        choice.height,
        choice.encoding.name(),
        choice.frame_interval
    );

    with_timeout_or_none(
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
    .and_then(|r| r.ok())?;

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

    with_timeout_or_none(
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
    .and_then(|r| r.ok())?;

    let negotiated_frame = read_u32_le(&probe, 18)
        .map(|n| n as usize)
        .filter(|n| *n != 0)
        .unwrap_or(choice.max_frame_bytes)
        .clamp(1, MAX_REASONABLE_FRAME_BYTES);
    let negotiated_payload = read_u32_le(&probe, 22)
        .map(|n| n as usize)
        .filter(|n| *n != 0)
        .unwrap_or_else(|| usize::from(target.max_packet_size.max(1)))
        .max(usize::from(target.max_packet_size.max(1)));

    Some(CameraStreamFormat {
        encoding: choice.encoding,
        width: choice.width,
        height: choice.height,
        max_frame_bytes: negotiated_frame,
        max_payload_bytes: negotiated_payload,
        format_index: probe.get(2).copied().unwrap_or(choice.format_index),
        frame_index: probe.get(3).copied().unwrap_or(choice.frame_index),
        frame_interval: read_u32_le(&probe, 4).unwrap_or(choice.frame_interval),
    })
}

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn yuy2_to_rgba(src: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let need = width.checked_mul(height)?.checked_mul(2)?;
    if src.len() < need {
        return None;
    }
    let mut rgba = alloc::vec![0u8; width.checked_mul(height)?.checked_mul(4)?];
    let mut si = 0usize;
    let mut di = 0usize;
    while si + 4 <= need && di + 8 <= rgba.len() {
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
    Some(rgba)
}

fn present_stream_frame(
    frame: &[u8],
    sf: CameraStreamFormat,
    vendor_id: u16,
    product_id: u16,
) -> bool {
    let decoded = match sf.encoding {
        FrameEncoding::Mjpeg => match crate::gfx::jpeg_codec::decode_jpeg_rgba(frame) {
            Ok(decoded) => (decoded.width, decoded.height, decoded.rgba),
            Err(err) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} mjpeg decode failed code={} bytes={}\n",
                    vendor_id,
                    product_id,
                    err.code(),
                    frame.len()
                );
                return false;
            }
        },
        FrameEncoding::Yuy2 => {
            let width = u32::from(sf.width);
            let height = u32::from(sf.height);
            let Some(rgba) = yuy2_to_rgba(frame, width as usize, height as usize) else {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} yuy2 frame too small bytes={} need={}\n",
                    vendor_id,
                    product_id,
                    frame.len(),
                    FULLHD_YUY2_BYTES
                );
                return false;
            };
            (width, height, rgba)
        }
        FrameEncoding::Unknown => return false,
    };

    crate::intel::present_rgba_overlay_top_right(
        decoded.2.as_slice(),
        decoded.0,
        decoded.1,
        (decoded.0 as usize).saturating_mul(4),
    );
    true
}

fn accept_uvc_payload(
    packet: &[u8],
    frame_buf: &mut Vec<u8>,
    prev_fid: &mut Option<u8>,
    started: &mut bool,
    max_frame_bytes: usize,
) -> Option<bool> {
    if packet.len() < 2 {
        return None;
    }
    let hdr_len = usize::from(packet[0]);
    if hdr_len < 2 || hdr_len > packet.len() {
        return None;
    }
    let flags = packet[1];
    if (flags & 0x40) != 0 {
        frame_buf.clear();
        return Some(false);
    }
    let fid = flags & 0x01;
    let eof = (flags & 0x02) != 0;
    let payload = &packet[hdr_len..];

    if !*started {
        if prev_fid.is_some() && *prev_fid != Some(fid) {
            *started = true;
            frame_buf.clear();
        }
        *prev_fid = Some(fid);
        if !*started {
            return Some(false);
        }
    }

    if *prev_fid != Some(fid) && !frame_buf.is_empty() {
        *prev_fid = Some(fid);
        return Some(true);
    }
    *prev_fid = Some(fid);

    if !payload.is_empty() {
        let room = max_frame_bytes.saturating_sub(frame_buf.len());
        let take = payload.len().min(room);
        frame_buf.extend_from_slice(&payload[..take]);
    }
    Some(eof && !frame_buf.is_empty())
}

async fn stream_iso(
    iso_in: &mut EndpointIsoIn,
    target: CameraTarget,
    sf: CameraStreamFormat,
    vendor_id: u16,
    product_id: u16,
) {
    let packet_bytes = sf
        .max_payload_bytes
        .max(usize::from(target.max_packet_size.max(1)))
        .min(64 * 1024);
    let mut rx = alloc::vec![0u8; packet_bytes.saturating_mul(ISO_PACKETS_PER_READ)];
    let mut frame_buf = Vec::with_capacity(sf.max_frame_bytes.min(MAX_REASONABLE_FRAME_BYTES));
    let mut prev_fid = None;
    let mut started = false;
    let mut frames = 0u64;

    loop {
        let read = match iso_in
            .submit_iso_in_and_wait(rx.as_mut_slice(), ISO_PACKETS_PER_READ)
            .await
        {
            Ok(n) => n.min(rx.len()),
            Err(err) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} iso read err={:?} frames={}\n",
                    vendor_id,
                    product_id,
                    err,
                    frames
                );
                Timer::after(EmbassyDuration::from_millis(10)).await;
                continue;
            }
        };
        for packet in rx[..read].chunks(packet_bytes.max(1)) {
            if accept_uvc_payload(
                packet,
                &mut frame_buf,
                &mut prev_fid,
                &mut started,
                sf.max_frame_bytes,
            )
            .unwrap_or(false)
            {
                if present_stream_frame(frame_buf.as_slice(), sf, vendor_id, product_id) {
                    frames = frames.saturating_add(1);
                    if frames == 1 || frames % 120 == 0 {
                        crate::log!(
                            "crabusb: camera {:04X}:{:04X} stream frames={} bytes={} enc={}\n",
                            vendor_id,
                            product_id,
                            frames,
                            frame_buf.len(),
                            sf.encoding.name()
                        );
                    }
                }
                frame_buf.clear();
            }
        }
    }
}

async fn stream_bulk(
    bulk_in: &mut EndpointBulkIn,
    target: CameraTarget,
    sf: CameraStreamFormat,
    vendor_id: u16,
    product_id: u16,
) {
    let read_bytes = sf
        .max_payload_bytes
        .max(usize::from(target.max_packet_size.max(1)))
        .min(1024 * 1024);
    let mut rx = alloc::vec![0u8; read_bytes];
    let mut frame_buf = Vec::with_capacity(sf.max_frame_bytes.min(MAX_REASONABLE_FRAME_BYTES));
    let mut prev_fid = None;
    let mut started = false;
    let mut frames = 0u64;
    let mut empty_reads = 0u32;

    loop {
        let read = match bulk_in.submit_and_wait(rx.as_mut_slice()).await {
            Ok(0) => {
                empty_reads = empty_reads.saturating_add(1);
                if empty_reads > CAMERA_MAX_EMPTY_READS {
                    Timer::after(EmbassyDuration::from_millis(5)).await;
                    empty_reads = 0;
                }
                continue;
            }
            Ok(n) => {
                empty_reads = 0;
                n.min(rx.len())
            }
            Err(err) => {
                crate::log!(
                    "crabusb: camera {:04X}:{:04X} bulk read err={:?} frames={}\n",
                    vendor_id,
                    product_id,
                    err,
                    frames
                );
                Timer::after(EmbassyDuration::from_millis(10)).await;
                continue;
            }
        };
        if accept_uvc_payload(
            &rx[..read],
            &mut frame_buf,
            &mut prev_fid,
            &mut started,
            sf.max_frame_bytes,
        )
        .unwrap_or(false)
        {
            if present_stream_frame(frame_buf.as_slice(), sf, vendor_id, product_id) {
                frames = frames.saturating_add(1);
                if frames == 1 || frames % 120 == 0 {
                    crate::log!(
                        "crabusb: camera {:04X}:{:04X} stream frames={} bytes={} enc={}\n",
                        vendor_id,
                        product_id,
                        frames,
                        frame_buf.len(),
                        sf.encoding.name()
                    );
                }
            }
            frame_buf.clear();
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn camera_stream_task(
    mut device: crab_usb::Device,
    controller_id: u32,
    target: CameraTarget,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;

    Timer::after(EmbassyDuration::from_millis(CAMERA_BOOT_DELAY_MS)).await;

    let raw_config =
        read_raw_configuration_descriptor(&mut device, target.configuration_index).await;
    let choices = raw_config
        .as_deref()
        .map(|raw| parse_uvc_stream_choices(raw, target.interface_number))
        .unwrap_or_default();
    let Some(choice) = choose_stream(choices.as_slice()) else {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} no supported uvc stream if#{} choices={}\n",
            vendor_id,
            product_id,
            target.interface_number,
            choices.len()
        );
        return;
    };

    crate::log!(
        "crabusb: camera {:04X}:{:04X} stream begin ctrl={} if#{} alt={} ep=0x{:02X} transport={:?} choice fmt={} frame={} {}x{} {}\n",
        vendor_id,
        product_id,
        controller_id,
        target.interface_number,
        target.alternate_setting,
        target.endpoint_address,
        target.transport,
        choice.format_index,
        choice.frame_index,
        choice.width,
        choice.height,
        choice.encoding.name()
    );

    if let Err(err) = device.set_configuration(target.configuration_value).await {
        crate::log!(
            "crabusb: camera {:04X}:{:04X} set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
        return;
    }

    let Some(sf) = negotiate_uvc_stream(&mut device, target, choice, vendor_id, product_id).await
    else {
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
        "crabusb: camera {:04X}:{:04X} stream ready fmt={} frame={} {}x{} enc={} interval={} payload={} frame_bytes={}\n",
        vendor_id,
        product_id,
        sf.format_index,
        sf.frame_index,
        sf.width,
        sf.height,
        sf.encoding.name(),
        sf.frame_interval,
        sf.max_payload_bytes,
        sf.max_frame_bytes
    );

    match target.transport {
        CameraTransport::IsoIn => match interface
            .endpoint_isochronous_in(target.endpoint_address)
            .await
        {
            Ok(mut iso_in) => stream_iso(&mut iso_in, target, sf, vendor_id, product_id).await,
            Err(err) => crate::log!(
                "crabusb: camera {:04X}:{:04X} iso open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.endpoint_address,
                err
            ),
        },
        CameraTransport::BulkIn => {
            match interface.endpoint_bulk_in(target.endpoint_address).await {
                Ok(mut bulk_in) => {
                    stream_bulk(&mut bulk_in, target, sf, vendor_id, product_id).await
                }
                Err(err) => crate::log!(
                    "crabusb: camera {:04X}:{:04X} bulk open failed ep=0x{:02X}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.endpoint_address,
                    err
                ),
            }
        }
    }
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
    let stable_id = dev_info.id() as u32;

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

    match camera_stream_task(device, controller_id, target) {
        Ok(token) => {
            spawner.spawn(token);
            crate::log!(
                "crabusb: camera {:04X}:{:04X} handoff stream if#{} alt={} cfg={} ep=0x{:02X} transport={:?} packet={} stable_id={}\n",
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
