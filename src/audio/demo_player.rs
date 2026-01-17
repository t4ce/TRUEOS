use crate::usb::uac;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

const PREFILL_TARGET: usize = 32;
const PREFILL_BURST: usize = 16;

fn cc_name(cc: u32) -> &'static str {
    match cc {
        1 => "success",
        13 => "short_packet",
        14 => "ring_underrun",
        15 => "ring_overrun",
        4 => "usb_txn_err",
        5 => "trb_err",
        6 => "stall",
        7 => "resource_err",
        8 => "bandwidth_err",
        10 => "invalid_stream",
        11 => "slot_not_enabled",
        12 => "ep_not_enabled",
        _ => "other",
    }
}

#[embassy_executor::task]
pub async fn uac_demo_task() {
    let demo = crate::audio::demo::DEMO;
    let mut rate_hz = demo.sample_rate_hz;
    let mut channels = demo.channels as usize;
    let demo_channels = core::cmp::max(1, demo.channels as usize);
    let samples = demo.samples_interleaved_i16;
    let demo_frames = samples.len() / demo_channels;

    let mut last_log = Instant::now();
    let mut queued: u64 = 0;
    let mut missed: u64 = 0;
    let mut fmt_mismatch: u64 = 0;
    let mut no_runtime: u64 = 0;
    let mut no_device: u64 = 0;
    let mut no_packet: u64 = 0;
    let mut last_status = Instant::now();
    let mut src_pos: u64 = 0;
    let mut src_step: u64 = if rate_hz == 0 {
        0
    } else {
        ((demo.sample_rate_hz as u64) << 32) / (rate_hz as u64)
    };

    'outer: loop {
        if let Some(fmt) = uac::current_format() {
            if fmt.rate_hz != rate_hz && fmt.rate_hz != 0 {
                rate_hz = fmt.rate_hz;
                src_step = ((demo.sample_rate_hz as u64) << 32) / (rate_hz as u64);
            }
            if fmt.channels as usize != channels && fmt.channels != 0 {
                channels = fmt.channels as usize;
            }
        }
        if channels == 0 || demo_frames == 0 || src_step == 0 {
            Timer::after(EmbassyDuration::from_millis(1000)).await;
            continue;
        }

        let mut budget = 1usize;
        let mut in_flight_now = 0usize;
        let mut in_flight_cap = 0usize;
        if let Some((in_flight, cap)) = uac::demo_queue_depth() {
            in_flight_now = in_flight;
            in_flight_cap = cap;
            let target = core::cmp::min(PREFILL_TARGET, cap);
            if in_flight < target {
                budget = core::cmp::min(PREFILL_BURST, target - in_flight);
            }
        }

        let mut queued_this_tick = false;
        let mut tick_us = 1000u64;
        let mut payload_bytes = 0usize;
        let mut max_packet_bytes = 0usize;

        for _ in 0..budget {
            let res = match uac::reserve_demo_packet() {
                Ok(res) => res,
                Err(uac::DemoQueueError::NoDevice) => {
                    no_device = no_device.saturating_add(1);
                    if Instant::now().duration_since(last_status) >= EmbassyDuration::from_secs(2) {
                        crate::log!(
                            "audio: uac demo waiting no_device={} no_runtime={} no_packet={} fmt_mismatch={}\n",
                            no_device,
                            no_runtime,
                            no_packet,
                            fmt_mismatch
                        );
                        last_status = Instant::now();
                    }
                    Timer::after(EmbassyDuration::from_millis(50)).await;
                    continue 'outer;
                }
                Err(uac::DemoQueueError::NoRuntime) => {
                    no_runtime = no_runtime.saturating_add(1);
                    if Instant::now().duration_since(last_status) >= EmbassyDuration::from_secs(2) {
                        crate::log!(
                            "audio: uac demo waiting no_device={} no_runtime={} no_packet={} fmt_mismatch={}\n",
                            no_device,
                            no_runtime,
                            no_packet,
                            fmt_mismatch
                        );
                        last_status = Instant::now();
                    }
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                    continue 'outer;
                }
                Err(uac::DemoQueueError::FormatMismatch) => {
                    fmt_mismatch = fmt_mismatch.saturating_add(1);
                    if Instant::now().duration_since(last_status) >= EmbassyDuration::from_secs(2) {
                        crate::log!(
                            "audio: uac demo waiting no_device={} no_runtime={} no_packet={} fmt_mismatch={}\n",
                            no_device,
                            no_runtime,
                            no_packet,
                            fmt_mismatch
                        );
                        last_status = Instant::now();
                    }
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                    continue 'outer;
                }
                Err(uac::DemoQueueError::NoPacket) => {
                    no_packet = no_packet.saturating_add(1);
                    if Instant::now().duration_since(last_status) >= EmbassyDuration::from_secs(2) {
                        crate::log!(
                            "audio: uac demo waiting no_device={} no_runtime={} no_packet={} fmt_mismatch={}\n",
                            no_device,
                            no_runtime,
                            no_packet,
                            fmt_mismatch
                        );
                        last_status = Instant::now();
                    }
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                    continue 'outer;
                }
            };

            tick_us = res.tick_us;
            payload_bytes = res.payload_bytes;
            max_packet_bytes = res.max_packet_bytes;
            unsafe {
                let out = core::slice::from_raw_parts_mut(res.buf_virt, res.packet_bytes);
                let (payload, pad) = out.split_at_mut(res.payload_bytes);
                let frame_bytes = channels * 2;
                for frame in payload.chunks_exact_mut(frame_bytes) {
                    let src_frame = ((src_pos >> 32) as usize) % demo_frames;
                    let src_base = src_frame * demo_channels;
                    let mut dst_idx = 0usize;

                    while dst_idx < channels {
                        let sample = match (demo_channels, channels) {
                            (1, 1) => samples[src_base],
                            (1, _) => samples[src_base],
                            (2, 1) => {
                                let l = samples[src_base] as i32;
                                let r = samples[src_base + 1] as i32;
                                ((l + r) / 2) as i16
                            }
                            (2, 2) => samples[src_base + dst_idx],
                            _ => samples[src_base + (dst_idx % demo_channels)],
                        };
                        let off = dst_idx * 2;
                        frame[off..off + 2].copy_from_slice(&sample.to_le_bytes());
                        dst_idx += 1;
                    }

                    src_pos = src_pos.wrapping_add(src_step);
                    let max_pos = (demo_frames as u64) << 32;
                    if src_pos >= max_pos {
                        src_pos -= max_pos;
                    }
                }
                pad.fill(0);
            }

            match uac::submit_demo_packet(res) {
                Ok(true) => {
                    queued = queued.saturating_add(1);
                    queued_this_tick = true;
                }
                Ok(false) => missed = missed.saturating_add(1),
                Err(uac::DemoQueueError::NoRuntime) => no_runtime = no_runtime.saturating_add(1),
                Err(uac::DemoQueueError::NoDevice) => {}
                Err(uac::DemoQueueError::FormatMismatch) => {
                    fmt_mismatch = fmt_mismatch.saturating_add(1)
                }
                Err(uac::DemoQueueError::NoPacket) => {}
            }
        }

        if Instant::now().duration_since(last_log) >= EmbassyDuration::from_secs(1) {
            let (evt_ok, evt_err, last_cc) = uac::take_xfer_event_counters();
            let queue_depth = uac::demo_queue_depth();
            crate::log!(
                "audio: uac demo stats queued={} missed={} fmt_mismatch={} no_runtime={} no_device={} no_packet={} tick_us={} bytes={}/{} evt_ok={} evt_err={} last_cc={} ({}) in_flight={:?}\n",
                queued,
                missed,
                fmt_mismatch,
                no_runtime,
                no_device,
                no_packet,
                tick_us,
                payload_bytes,
                max_packet_bytes,
                evt_ok,
                evt_err,
                last_cc,
                cc_name(last_cc),
                queue_depth.map(|(cur, cap)| (cur, cap))
            );
            last_log = Instant::now();
            queued = 0;
            missed = 0;
            fmt_mismatch = 0;
            no_runtime = 0;
            no_device = 0;
            no_packet = 0;
        }

        if in_flight_cap > 0 && in_flight_now < core::cmp::min(PREFILL_TARGET, in_flight_cap) {
            // Fill faster when the queue is under target to avoid underruns.
            Timer::after(EmbassyDuration::from_micros(100)).await;
        } else if queued_this_tick {
            Timer::after(EmbassyDuration::from_micros(tick_us)).await;
        } else {
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        if Instant::now().duration_since(last_status) >= EmbassyDuration::from_secs(2) {
            crate::log!(
                "audio: uac demo heartbeat rate_hz={} ch={} src_rate={} src_ch={}\n",
                rate_hz,
                channels,
                demo.sample_rate_hz,
                demo.channels
            );
            last_status = Instant::now();
        }
    }
}
