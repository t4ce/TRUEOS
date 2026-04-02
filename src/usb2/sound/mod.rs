use super::*;
use spin::Mutex;

pub const DEFAULT_RATE_HZ: u32 = 48_000;
pub const DEFAULT_CHANNELS: u16 = 2;
pub const DEMO_ASSET_NAME: &str = "demo.wav";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveAudioStream {
    // Match the HID media-control side by stable_id so composite headset buttons
    // can later adjust this specific UAC sink/source instead of a global volume.
    stable_id: u32,
    slot_id: u32,
    interface_number: u8,
    endpoint_address: u8,
}

static ACTIVE_AUDIO_STREAM: Mutex<Option<ActiveAudioStream>> = Mutex::new(None);

/// Simple PCM format descriptor for sinks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub rate_hz: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
}

#[inline]
pub fn demo_loop_asset_name() -> &'static str {
    DEMO_ASSET_NAME
}

#[inline]
pub fn demo_loop_requested() -> bool {
    AUDIO_STREAM_REQUESTED.load(Ordering::Acquire)
}

#[inline]
pub fn demo_loop_active() -> bool {
    AUDIO_STREAM_ACTIVE.load(Ordering::Acquire)
}

#[inline]
pub fn set_demo_loop_requested(enabled: bool) {
    AUDIO_STREAM_REQUESTED.store(enabled, Ordering::Release);
}

async fn stream_target_audio(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    preferred: PreferredAlt,
    endpoint: IsoOutEndpoint,
    stream_target: Option<UacStreamTarget>,
) {
    const AUDIO_WARMUP_US: usize = 0;

    let Some((wav_data_off, wav_data_len)) = parse_wav_pcm_s16_stereo_48k(DEMO_WAV_EMBEDDED) else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} audio embedded demo.wav unsupported (need pcm s16le stereo 48k)\n",
            vendor_id,
            product_id
        );
        return;
    };
    let wav = &DEMO_WAV_EMBEDDED[wav_data_off..wav_data_off + wav_data_len];

    let endpoint_payload_limit = stream_target
        .map(|target| usize::from(target.max_packet_payload.max(AUDIO_FRAME_BYTES as u16)))
        .unwrap_or_else(|| {
            usize::from((endpoint.max_packet_size & 0x07FF).max(AUDIO_FRAME_BYTES as u16))
        });
    let packet_bytes = choose_audio_packet_bytes(AUDIO_FRAME_BYTES, endpoint_payload_limit);
    let nominal_1ms_bytes = (48_000usize * AUDIO_FRAME_BYTES) / 1_000;
    let packet_duration_us = if packet_bytes >= nominal_1ms_bytes {
        1_000usize
    } else {
        125usize
    };
    let packets_per_request = if packet_bytes >= 160 { 8 } else { 32 };
    let urb_bytes = packet_bytes.saturating_mul(packets_per_request);
    let mut packet_batch = Vec::from_iter(core::iter::repeat_n(0u8, urb_bytes));
    let mut wav_cursor = 0usize;
    let mut logged_probe = false;
    let mut silent_packets_remaining = AUDIO_WARMUP_US / packet_duration_us;
    let mut submit_count = 0u64;
    let mut submitted_bytes = 0u64;
    let mut zero_submit_count = 0u64;
    let mut short_submit_count = 0u64;

    let endpoint_kind = match device.get_endpoint(endpoint.address).await {
        Ok(kind) => kind,
        Err(err) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} ep=0x{:02X} open failed: {:?}\n",
                vendor_id,
                product_id,
                endpoint.address,
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
            preferred.interface_number,
            preferred.alternate_setting,
            endpoint.address
        );
        return;
    };

    crate::log!(
        "crabusb: audio streaming start {:04X}:{:04X} if#{} alt={} ep=0x{:02X} packet={} batch={} sync={}({}) feedback={} source=demo.wav payload_limit={} warmup_packets={}\n",
        vendor_id,
        product_id,
        preferred.interface_number,
        preferred.alternate_setting,
        endpoint.address,
        packet_bytes,
        packets_per_request,
        stream_target.map(|target| target.sync_type).unwrap_or(0xFF),
        uac_sync_type_name(stream_target.map(|target| target.sync_type).unwrap_or(0xFF)),
        stream_target
            .map(|target| target.has_feedback_ep)
            .unwrap_or(false),
        endpoint_payload_limit,
        silent_packets_remaining
    );

    loop {
        if !AUDIO_STREAM_REQUESTED.load(Ordering::Acquire) {
            crate::log!(
                "crabusb: audio streaming stop-request {:04X}:{:04X} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                endpoint.address
            );
            break;
        }

        for packet in packet_batch.chunks_exact_mut(packet_bytes) {
            if silent_packets_remaining > 0 {
                packet.fill(0);
                silent_packets_remaining -= 1;
            } else {
                fill_audio_packet(packet, wav, &mut wav_cursor);
            }
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
                    endpoint.address,
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
    preferred: PreferredAlt,
    endpoint: IsoOutEndpoint,
    stream_target: Option<UacStreamTarget>,
) {
    stream_target_audio(&mut device, vendor_id, product_id, preferred, endpoint, stream_target)
        .await;
    *ACTIVE_AUDIO_STREAM.lock() = None;
    AUDIO_STREAM_ACTIVE.store(false, Ordering::Release);
}

pub(super) async fn maybe_start_target_audio(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &embassy_executor::Spawner,
) {
    if !AUDIO_STREAM_REQUESTED.load(Ordering::Acquire)
        || AUDIO_STREAM_ACTIVE.load(Ordering::Acquire)
    {
        return;
    }

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
            return;
        }
    };

    let configs = device.configurations().to_vec();
    let mut preferred_with_target: Option<(
        PreferredAlt,
        IsoOutEndpoint,
        Option<Vec<u8>>,
        Option<UacStreamTarget>,
    )> = None;

    for (config_index, config) in configs.iter().enumerate() {
        let raw_cfg = fetch_raw_configuration_bytes(&mut device, config_index as u8).await;
        let controls = raw_cfg.as_deref().and_then(parse_uac_audio_controls);
        let playback_link = controls.and_then(|control| control.playback_stream_link);

        if let Some(cfg) = raw_cfg.as_deref() {
            for candidate in parse_uac_stream_candidates(cfg) {
                if crate::logflag::USB_AUDIO_DEBUG_LOGS {
                    crate::log!(
                        "crabusb: audio-route {:04X}:{:04X} cfg={} if#{} alt={} ep=0x{:02X} sync={}({}) feedback={} max_payload={} terminal_link={} playback_link={} fmt={}ch/{}B/{}bit 48k={}\n",
                        vendor_id,
                        product_id,
                        config.configuration_value,
                        candidate.interface_number,
                        candidate.alternate_setting,
                        candidate.endpoint_address,
                        candidate.sync_type,
                        uac_sync_type_name(candidate.sync_type),
                        candidate.has_feedback_ep,
                        candidate.max_packet_payload,
                        candidate.terminal_link.unwrap_or(0),
                        playback_link.unwrap_or(0),
                        candidate.format.map(|fmt| fmt.channels).unwrap_or(0),
                        candidate.format.map(|fmt| fmt.subframe_bytes).unwrap_or(0),
                        candidate.format.map(|fmt| fmt.bit_resolution).unwrap_or(0),
                        candidate
                            .format
                            .map(|fmt| fmt.supports_48k)
                            .unwrap_or(false)
                    );
                }

                let Some(endpoint) = find_iso_out_endpoint(
                    &configs,
                    candidate.interface_number,
                    candidate.alternate_setting,
                ) else {
                    continue;
                };

                let preferred = PreferredAlt {
                    configuration_index: config_index as u8,
                    configuration_value: config.configuration_value,
                    interface_number: candidate.interface_number,
                    alternate_setting: candidate.alternate_setting,
                    class: 0x01,
                    subclass: 0x02,
                    protocol: 0,
                    has_iso_out: true,
                    endpoint_count: 1,
                };

                let score = if candidate.terminal_link.is_some()
                    && candidate.terminal_link == playback_link
                {
                    100u32
                } else {
                    10u32
                };

                let replace = match preferred_with_target {
                    None => true,
                    Some((current, _, _, _)) => {
                        let current_score = if current.interface_number
                            == candidate.interface_number
                            && current.alternate_setting == candidate.alternate_setting
                            && candidate.terminal_link.is_some()
                            && candidate.terminal_link == playback_link
                        {
                            100u32
                        } else {
                            10u32
                        };
                        score > current_score
                            || (score == current_score
                                && candidate.interface_number < current.interface_number)
                            || (score == current_score
                                && candidate.interface_number == current.interface_number
                                && candidate.alternate_setting < current.alternate_setting)
                    }
                };

                if replace {
                    preferred_with_target = Some((
                        preferred,
                        endpoint,
                        raw_cfg.clone(),
                        Some(UacStreamTarget {
                            sync_type: candidate.sync_type,
                            has_feedback_ep: candidate.has_feedback_ep,
                            max_packet_payload: candidate.max_packet_payload,
                            format: candidate.format,
                        }),
                    ));
                }
            }
        }
    }

    let (preferred, endpoint, raw_cfg, stream_target) = if let Some(selected) =
        preferred_with_target
    {
        selected
    } else {
        let Some(preferred) = pick_preferred_alt(&configs) else {
            return;
        };
        let Some(endpoint) = find_iso_out_endpoint(
            &configs,
            preferred.interface_number,
            preferred.alternate_setting,
        ) else {
            crate::log!(
                "crabusb: target {:04X}:{:04X} preferred if#{} alt={} has no iso-out endpoint\n",
                vendor_id,
                product_id,
                preferred.interface_number,
                preferred.alternate_setting
            );
            return;
        };

        let raw_cfg =
            fetch_raw_configuration_bytes(&mut device, preferred.configuration_index).await;
        let stream_target = raw_cfg.as_deref().and_then(|cfg| {
            parse_uac_stream_target(
                cfg,
                preferred.interface_number,
                preferred.alternate_setting,
                endpoint.address,
            )
        });
        (preferred, endpoint, raw_cfg, stream_target)
    };

    if crate::logflag::USB_AUDIO_DEBUG_LOGS {
        if let Some(cfg) = raw_cfg.as_deref() {
            log_uac_topology(cfg, vendor_id, product_id);
        }
        if let Some(target) = stream_target {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio stream sync={} feedback={} max_payload={}\n",
                vendor_id,
                product_id,
                target.sync_type,
                target.has_feedback_ep,
                target.max_packet_payload
            );
        }
    }

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(preferred.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: target {:04X}:{:04X} set cfg={} failed before audio claim: {:?}\n",
            vendor_id,
            product_id,
            preferred.configuration_value,
            err
        );
        return;
    }

    match device
        .claim_interface(preferred.interface_number, preferred.alternate_setting)
        .await
    {
        Ok(()) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio ownership cfg={} if#{} alt={} ep=0x{:02X} interval={}\n",
                vendor_id,
                product_id,
                preferred.configuration_value,
                preferred.interface_number,
                preferred.alternate_setting,
                endpoint.address,
                endpoint.interval
            );
            let rate_bytes = [
                (48_000u32 & 0xFF) as u8,
                ((48_000u32 >> 8) & 0xFF) as u8,
                ((48_000u32 >> 16) & 0xFF) as u8,
            ];
            match device
                .control_out(
                    usb_if::host::ControlSetup {
                        request_type: usb_if::transfer::RequestType::Class,
                        recipient: usb_if::transfer::Recipient::Endpoint,
                        request: usb_if::transfer::Request::Other(0x01),
                        value: 0x0100,
                        index: u16::from(endpoint.address),
                    },
                    &rate_bytes,
                )
                .await
            {
                Ok(_) => crate::log!(
                    "crabusb: target {:04X}:{:04X} audio set-rate ok ep=0x{:02X} hz=48000\n",
                    vendor_id,
                    product_id,
                    endpoint.address
                ),
                Err(err) => crate::log!(
                    "crabusb: target {:04X}:{:04X} audio set-rate failed ep=0x{:02X} hz=48000 err={:?}\n",
                    vendor_id,
                    product_id,
                    endpoint.address,
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
                            index: u16::from(endpoint.address),
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
                            endpoint.address,
                            hz
                        );
                    }
                    Ok(read) => crate::log!(
                        "crabusb: target {:04X}:{:04X} audio set-rate readback short ep=0x{:02X} bytes={}\n",
                        vendor_id,
                        product_id,
                        endpoint.address,
                        read
                    ),
                    Err(err) => crate::log!(
                        "crabusb: target {:04X}:{:04X} audio set-rate readback failed ep=0x{:02X} err={:?}\n",
                        vendor_id,
                        product_id,
                        endpoint.address,
                        err
                    ),
                }
            }

            if let Some(raw_cfg) = raw_cfg.as_deref() {
                configure_uac_playback_controls(&mut device, vendor_id, product_id, raw_cfg).await;
            } else {
                crate::log!(
                    "crabusb: target {:04X}:{:04X} failed to fetch raw cfg#{} for audio controls\n",
                    vendor_id,
                    product_id,
                    preferred.configuration_index
                );
            }
            AUDIO_STREAM_ACTIVE.store(true, Ordering::Release);
            let active_stream = ActiveAudioStream {
                stable_id,
                slot_id: u32::from(device.slot_id()),
                interface_number: preferred.interface_number,
                endpoint_address: endpoint.address,
            };
            *ACTIVE_AUDIO_STREAM.lock() = Some(active_stream);
            match spawner.spawn(audio_stream_task(
                device,
                active_stream,
                vendor_id,
                product_id,
                preferred,
                endpoint,
                stream_target,
            )) {
                Ok(()) => {}
                Err(err) => {
                    *ACTIVE_AUDIO_STREAM.lock() = None;
                    AUDIO_STREAM_ACTIVE.store(false, Ordering::Release);
                    crate::log!(
                        "crabusb: target {:04X}:{:04X} audio spawn failed if#{} alt={}: {:?}\n",
                        vendor_id,
                        product_id,
                        preferred.interface_number,
                        preferred.alternate_setting,
                        err
                    );
                }
            }
        }
        Err(err) => crate::log!(
            "crabusb: target {:04X}:{:04X} audio claim failed if#{} alt={}: {:?}\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            err
        ),
    }
}
