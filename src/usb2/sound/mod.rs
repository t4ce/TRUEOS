extern crate alloc;

use alloc::vec::Vec;
use core::cmp::min;
use core::sync::atomic::{AtomicBool, Ordering};

use crab_usb::{EndpointKind, USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const AUDIO_DEMO_ENABLED: bool = true;
const AUDIO_HTTP_LOCAL_DEMO_URLS: [&str; 1] = ["http://192.168.178.112:8080/tools/aud/demo.wav"];
const AUDIO_HTTP_DEMO_TIMEOUT_MS: u32 = 30_000;
const AUDIO_HTTP_DEMO_MAX_BYTES: usize = 32 * 1024 * 1024;
const AUDIO_DEMO_CACHE_PATH: &str = "audio/demo.wav";
const AUDIO_FRAME_BYTES: usize = 4;
const AUDIO_RATE_HZ: u32 = 48_000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveAudioStream {
    stable_id: u32,
    slot_id: u32,
    interface_number: u8,
    endpoint_address: u8,
}

#[derive(Copy, Clone, Debug)]
struct AudioTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    endpoint_address: u8,
    max_packet_size: u16,
    has_feedback_ep: bool,
}

static ACTIVE_AUDIO_STREAM: Mutex<Option<ActiveAudioStream>> = Mutex::new(None);
static AUDIO_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub rate_hz: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
}

#[inline]
pub fn demo_loop_requested() -> bool {
    true
}

#[inline]
pub fn demo_loop_active() -> bool {
    AUDIO_STREAM_ACTIVE.load(Ordering::Acquire)
}

#[inline]
pub fn set_demo_loop_requested(_enabled: bool) {}

fn pick_audio_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<AudioTarget> {
    let mut best: Option<AudioTarget> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.class != 0x01 || alt.subclass != 0x02 {
                    continue;
                }

                let Some(endpoint) = alt.endpoints.iter().find(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                        && ep.direction == usb_if::transfer::Direction::Out
                }) else {
                    continue;
                };

                let candidate = AudioTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    endpoint_address: endpoint.address,
                    max_packet_size: endpoint.max_packet_size,
                    has_feedback_ep: alt.endpoints.iter().any(|ep| {
                        ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                            && ep.direction == usb_if::transfer::Direction::In
                    }),
                };

                let replace = match best {
                    None => true,
                    Some(current) => {
                        candidate.alternate_setting > current.alternate_setting
                            || (candidate.alternate_setting == current.alternate_setting
                                && candidate.max_packet_size > current.max_packet_size)
                            || (candidate.alternate_setting == current.alternate_setting
                                && candidate.max_packet_size == current.max_packet_size
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

async fn fetch_demo_wav_body() -> Option<(&'static str, Vec<u8>)> {
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        match crate::r::fs::trueosfs::file_out_if_index_ready_async(disk, AUDIO_DEMO_CACHE_PATH)
            .await
        {
            Ok(Some(cached)) if !cached.is_empty() => {
                crate::log!(
                    "crabusb: audio cache hit path={} bytes={}\n",
                    AUDIO_DEMO_CACHE_PATH,
                    cached.len()
                );
                return Some((AUDIO_DEMO_CACHE_PATH, cached));
            }
            Ok(None) => {
                crate::log!(
                    "crabusb: audio cache unavailable path={} reason=index-not-ready-or-miss\n",
                    AUDIO_DEMO_CACHE_PATH
                );
            }
            Ok(Some(_)) => {
                crate::log!(
                    "crabusb: audio cache unavailable path={} reason=empty\n",
                    AUDIO_DEMO_CACHE_PATH
                );
            }
            Err(err) => {
                crate::log!(
                    "crabusb: audio cache probe failed path={} err={:?}\n",
                    AUDIO_DEMO_CACHE_PATH,
                    err
                );
            }
        }
    }

    for url in AUDIO_HTTP_LOCAL_DEMO_URLS {
        crate::log!(
            "crabusb: audio fetch try url={} timeout_ms={} max_bytes={}\n",
            url,
            AUDIO_HTTP_DEMO_TIMEOUT_MS,
            AUDIO_HTTP_DEMO_MAX_BYTES
        );
        if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
            match crate::r::net::cli::http_stream::fetch_http_to_file_async(
                url,
                disk,
                AUDIO_DEMO_CACHE_PATH,
                AUDIO_HTTP_DEMO_TIMEOUT_MS,
                AUDIO_HTTP_DEMO_MAX_BYTES,
            )
            .await
            {
                Ok(()) => match crate::r::fs::trueosfs::file_out_async(disk, AUDIO_DEMO_CACHE_PATH)
                    .await
                {
                    Ok(Some(cached)) if !cached.is_empty() => {
                        crate::log!(
                            "crabusb: audio cached path={} url={} bytes={}\n",
                            AUDIO_DEMO_CACHE_PATH,
                            url,
                            cached.len()
                        );
                        return Some((AUDIO_DEMO_CACHE_PATH, cached));
                    }
                    Ok(_) => {
                        crate::log!(
                            "crabusb: audio stream fetch finished but cache empty path={} url={}\n",
                            AUDIO_DEMO_CACHE_PATH,
                            url
                        );
                    }
                    Err(err) => {
                        crate::log!(
                            "crabusb: audio cache read failed path={} url={} err={:?}\n",
                            AUDIO_DEMO_CACHE_PATH,
                            url,
                            err
                        );
                    }
                },
                Err(err) => {
                    crate::log!("crabusb: audio stream fetch failed url={} err={:?}\n", url, err);
                }
            }
        }
        match crate::r::net::cli::http::fetch_http_body(
            url,
            AUDIO_HTTP_DEMO_TIMEOUT_MS,
            AUDIO_HTTP_DEMO_MAX_BYTES,
        )
        .await
        {
            Ok(body) => return Some((url, body)),
            Err(err) => crate::log!("crabusb: audio fetch failed url={} err={:?}\n", url, err),
        }
    }

    None
}

fn le_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let b = bytes.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn le_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let b = bytes.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn parse_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 44 || bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }

    let mut fmt_ok = false;
    let mut data_range = None;
    let mut off = 12usize;

    while off + 8 <= bytes.len() {
        let chunk_id = bytes.get(off..off + 4)?;
        let chunk_len = usize::try_from(le_u32(bytes, off + 4)?).ok()?;
        let chunk_data_off = off + 8;
        let chunk_end = chunk_data_off.checked_add(chunk_len)?;
        if chunk_end > bytes.len() {
            return None;
        }

        if chunk_id == b"fmt " {
            let format_tag = le_u16(bytes, chunk_data_off)?;
            let channels = le_u16(bytes, chunk_data_off + 2)?;
            let sample_rate = le_u32(bytes, chunk_data_off + 4)?;
            let bits_per_sample = le_u16(bytes, chunk_data_off + 14)?;
            fmt_ok = format_tag == 1
                && channels == 2
                && sample_rate == AUDIO_RATE_HZ
                && bits_per_sample == 16;
        } else if chunk_id == b"data" {
            data_range = Some((chunk_data_off, chunk_len));
        }

        let padded_len = (chunk_len + 1) & !1;
        off = chunk_data_off.checked_add(padded_len)?;
    }

    if fmt_ok { data_range } else { None }
}

fn choose_audio_packet_bytes(endpoint_payload_limit: usize) -> usize {
    let nominal_1ms = (AUDIO_RATE_HZ as usize * AUDIO_FRAME_BYTES) / 1_000;
    let nominal_125us = (AUDIO_RATE_HZ as usize * AUDIO_FRAME_BYTES) / 8_000;

    if endpoint_payload_limit >= nominal_1ms {
        nominal_1ms
    } else if endpoint_payload_limit >= nominal_125us {
        nominal_125us
    } else {
        let rounded = endpoint_payload_limit - (endpoint_payload_limit % AUDIO_FRAME_BYTES);
        rounded.max(AUDIO_FRAME_BYTES)
    }
}

fn fill_audio_packet(packet: &mut [u8], wav: &[u8], wav_cursor: &mut usize) {
    if wav.is_empty() {
        packet.fill(0);
        return;
    }

    let mut written = 0usize;
    while written < packet.len() {
        if *wav_cursor >= wav.len() {
            *wav_cursor = 0;
        }
        let remaining_wav = wav.len() - *wav_cursor;
        let remaining_packet = packet.len() - written;
        let chunk = remaining_wav.min(remaining_packet);
        packet[written..written + chunk].copy_from_slice(&wav[*wav_cursor..*wav_cursor + chunk]);
        *wav_cursor += chunk;
        written += chunk;
    }
}

fn audio_packet_level_probe(packet: &[u8]) -> (u16, u16) {
    let mut peak = 0u16;
    let mut sum = 0u64;
    let mut count = 0u64;

    for frame in packet.chunks_exact(2) {
        let sample = i16::from_le_bytes([frame[0], frame[1]]);
        let magnitude = sample.unsigned_abs();
        peak = peak.max(magnitude);
        sum = sum.saturating_add(u64::from(magnitude));
        count = count.saturating_add(1);
    }

    let mean = if count == 0 { 0 } else { (sum / count) as u16 };
    (peak, mean)
}

async fn stream_target_audio(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    target: AudioTarget,
) {
    let (source_url, body) = match fetch_demo_wav_body().await {
        Some(payload) => payload,
        None => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio fetch exhausted sources={:?}\n",
                vendor_id,
                product_id,
                AUDIO_HTTP_LOCAL_DEMO_URLS
            );
            return;
        }
    };

    let Some((wav_data_off, wav_data_len)) = parse_wav_pcm_s16_stereo_48k(body.as_slice()) else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} audio wav unsupported url={} bytes={} need=pcm_s16le_stereo_48k\n",
            vendor_id,
            product_id,
            source_url,
            body.len()
        );
        return;
    };
    let wav = &body[wav_data_off..wav_data_off + wav_data_len];

    let endpoint_kind = match device.get_endpoint(target.endpoint_address).await {
        Ok(kind) => kind,
        Err(err) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} ep=0x{:02X} open failed: {:?}\n",
                vendor_id,
                product_id,
                target.endpoint_address,
                err
            );
            return;
        }
    };

    let EndpointKind::IsochronousOut(mut iso_out) = endpoint_kind else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} if#{} alt={} ep=0x{:02X} is not iso-out\n",
            vendor_id,
            product_id,
            target.interface_number,
            target.alternate_setting,
            target.endpoint_address
        );
        return;
    };

    let endpoint_payload_limit = usize::from(target.max_packet_size & 0x07FF);
    let packet_bytes = choose_audio_packet_bytes(endpoint_payload_limit);
    let packets_per_request = if packet_bytes >= 160 { 8 } else { 32 };
    let urb_bytes = packet_bytes.saturating_mul(packets_per_request);
    let mut packet_batch = Vec::from_iter(core::iter::repeat_n(0u8, urb_bytes));
    let mut wav_cursor = 0usize;
    let mut logged_probe = false;
    let mut submit_count = 0u64;
    let mut submitted_bytes = 0u64;
    let mut zero_submit_count = 0u64;
    let mut short_submit_count = 0u64;

    crate::log!(
        "crabusb: audio streaming start {:04X}:{:04X} if#{} alt={} ep=0x{:02X} packet={} batch={} feedback={} source_url={} wav_bytes={} payload_limit={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.endpoint_address,
        packet_bytes,
        packets_per_request,
        target.has_feedback_ep,
        source_url,
        wav.len(),
        endpoint_payload_limit
    );

    loop {
        for packet in packet_batch.chunks_exact_mut(packet_bytes) {
            fill_audio_packet(packet, wav, &mut wav_cursor);
        }

        if crate::logflag::USB_AUDIO_DEBUG_LOGS && !logged_probe {
            let probe_len = min(packet_bytes, 16);
            let probe = &packet_batch[..probe_len];
            let non_zero = probe.iter().filter(|b| **b != 0).count();
            crate::log!(
                "crabusb: audio probe first_packet bytes={} non_zero={} head={:02X?}\n",
                probe_len,
                non_zero,
                probe
            );
            logged_probe = true;
        }

        match iso_out
            .submit_and_wait(packet_batch.as_slice(), packets_per_request)
            .await
        {
            Ok(sent) => {
                submit_count = submit_count.wrapping_add(1);
                if sent == 0 {
                    zero_submit_count = zero_submit_count.wrapping_add(1);
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                } else {
                    submitted_bytes = submitted_bytes.saturating_add(sent as u64);
                    if sent != urb_bytes {
                        short_submit_count = short_submit_count.wrapping_add(1);
                    }
                }

                if crate::logflag::USB_AUDIO_DEBUG_LOGS
                    && (submit_count <= 4 || submit_count.is_multiple_of(1024))
                {
                    let first_packet = &packet_batch[..packet_bytes];
                    let (peak, mean_abs) = audio_packet_level_probe(first_packet);
                    crate::log!(
                        "crabusb: audio heartbeat {:04X}:{:04X} submits={} bytes={} last_sent={} zero_submits={} short_submits={} first_peak={} first_mean_abs={}\n",
                        vendor_id,
                        product_id,
                        submit_count,
                        submitted_bytes,
                        sent,
                        zero_submit_count,
                        short_submit_count,
                        peak,
                        mean_abs
                    );
                }
            }
            Err(err) => {
                crate::log!(
                    "crabusb: audio streaming stopped {:04X}:{:04X} ep=0x{:02X} err={:?}\n",
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
async fn audio_stream_task(
    mut device: crab_usb::Device,
    active_stream: ActiveAudioStream,
    vendor_id: u16,
    product_id: u16,
    target: AudioTarget,
) {
    stream_target_audio(&mut device, vendor_id, product_id, target).await;
    let mut guard = ACTIVE_AUDIO_STREAM.lock();
    if guard.as_ref() == Some(&active_stream) {
        *guard = None;
    }
    AUDIO_STREAM_ACTIVE.store(false, Ordering::Release);
}

pub(crate) async fn maybe_start_target_audio(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
) -> bool {
    if !AUDIO_DEMO_ENABLED {
        return false;
    }

    if AUDIO_STREAM_ACTIVE.load(Ordering::Acquire) {
        return false;
    }

    let Some(target) = pick_audio_target(dev_info.configurations()) else {
        return false;
    };

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let stable_id = dev_info.stable_id().raw();

    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return true;
        }
    };

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: target {:04X}:{:04X} set cfg={} failed before audio claim: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
        return true;
    }

    match device
        .claim_interface(target.interface_number, target.alternate_setting)
        .await
    {
        Ok(()) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio ownership cfg={} if#{} alt={} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.configuration_value,
                target.interface_number,
                target.alternate_setting,
                target.endpoint_address
            );

            let rate_bytes = [
                (AUDIO_RATE_HZ & 0xFF) as u8,
                ((AUDIO_RATE_HZ >> 8) & 0xFF) as u8,
                ((AUDIO_RATE_HZ >> 16) & 0xFF) as u8,
            ];
            match device
                .control_out(
                    usb_if::host::ControlSetup {
                        request_type: usb_if::transfer::RequestType::Class,
                        recipient: usb_if::transfer::Recipient::Endpoint,
                        request: usb_if::transfer::Request::Other(0x01),
                        value: 0x0100,
                        index: u16::from(target.endpoint_address),
                    },
                    &rate_bytes,
                )
                .await
            {
                Ok(_) => crate::log!(
                    "crabusb: target {:04X}:{:04X} audio set-rate ok ep=0x{:02X} hz={}\n",
                    vendor_id,
                    product_id,
                    target.endpoint_address,
                    AUDIO_RATE_HZ
                ),
                Err(err) => crate::log!(
                    "crabusb: target {:04X}:{:04X} audio set-rate failed ep=0x{:02X} hz={} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.endpoint_address,
                    AUDIO_RATE_HZ,
                    err
                ),
            }

            if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                let mut rate_readback = [0u8; 3];
                match device
                    .control_in(
                        usb_if::host::ControlSetup {
                            request_type: usb_if::transfer::RequestType::Class,
                            recipient: usb_if::transfer::Recipient::Endpoint,
                            request: usb_if::transfer::Request::Other(0x81),
                            value: 0x0100,
                            index: u16::from(target.endpoint_address),
                        },
                        &mut rate_readback,
                    )
                    .await
                {
                    Ok(read) if read >= 3 => {
                        let hz = u32::from(rate_readback[0])
                            | (u32::from(rate_readback[1]) << 8)
                            | (u32::from(rate_readback[2]) << 16);
                        crate::log!(
                            "crabusb: target {:04X}:{:04X} audio set-rate readback ep=0x{:02X} hz={}\n",
                            vendor_id,
                            product_id,
                            target.endpoint_address,
                            hz
                        );
                    }
                    Ok(read) => crate::log!(
                        "crabusb: target {:04X}:{:04X} audio set-rate readback short ep=0x{:02X} bytes={}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address,
                        read
                    ),
                    Err(err) => crate::log!(
                        "crabusb: target {:04X}:{:04X} audio set-rate readback failed ep=0x{:02X} err={:?}\n",
                        vendor_id,
                        product_id,
                        target.endpoint_address,
                        err
                    ),
                }
            }

            AUDIO_STREAM_ACTIVE.store(true, Ordering::Release);
            let active_stream = ActiveAudioStream {
                stable_id,
                slot_id: u32::from(device.slot_id()),
                interface_number: target.interface_number,
                endpoint_address: target.endpoint_address,
            };
            *ACTIVE_AUDIO_STREAM.lock() = Some(active_stream);

            match audio_stream_task(device, active_stream, vendor_id, product_id, target) {
                Ok(token) => {
                    spawner.spawn(token);
                    crate::log!(
                        "crabusb: audio handoff {:04X}:{:04X} if#{} alt={} ep=0x{:02X} stable_id={}\n",
                        vendor_id,
                        product_id,
                        target.interface_number,
                        target.alternate_setting,
                        target.endpoint_address,
                        stable_id
                    );
                }
                Err(err) => {
                    *ACTIVE_AUDIO_STREAM.lock() = None;
                    AUDIO_STREAM_ACTIVE.store(false, Ordering::Release);
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio spawn failed if#{} alt={}: {:?}\n",
                        vendor_id,
                        product_id,
                        target.interface_number,
                        target.alternate_setting,
                        err
                    );
                }
            }
        }
        Err(err) => crate::log!(
            "crabusb: target {:04X}:{:04X} audio claim failed if#{} alt={}: {:?}\n",
            vendor_id,
            product_id,
            target.interface_number,
            target.alternate_setting,
            err
        ),
    }

    true
}
