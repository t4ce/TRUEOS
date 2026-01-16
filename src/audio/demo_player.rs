use crate::usb::uac;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

const DEMO_SAMPLES: &[i16] = &[];

#[embassy_executor::task]
pub async fn uac_demo_task() {
    let samples = DEMO_SAMPLES;
    if samples.is_empty() {
        loop {
            Timer::after(EmbassyDuration::from_millis(1000)).await;
        }
    }

    let mut sample_idx: usize = 0;

    let mut last_log = Instant::now();
    let mut queued: u64 = 0;
    let mut missed: u64 = 0;
    let mut fmt_mismatch: u64 = 0;
    let mut no_runtime: u64 = 0;

    loop {
        let res = match uac::reserve_demo_packet() {
            Ok(res) => res,
            Err(uac::DemoQueueError::NoDevice) => {
                Timer::after(EmbassyDuration::from_millis(50)).await;
                continue;
            }
            Err(uac::DemoQueueError::NoRuntime) => {
                no_runtime = no_runtime.saturating_add(1);
                Timer::after(EmbassyDuration::from_millis(10)).await;
                continue;
            }
            Err(uac::DemoQueueError::FormatMismatch) => {
                fmt_mismatch = fmt_mismatch.saturating_add(1);
                Timer::after(EmbassyDuration::from_millis(10)).await;
                continue;
            }
            Err(uac::DemoQueueError::NoPacket) => {
                Timer::after(EmbassyDuration::from_millis(10)).await;
                continue;
            }
        };

        let tick_us = res.tick_us;
        unsafe {
            let out = core::slice::from_raw_parts_mut(res.buf_virt, res.packet_bytes);
            let (payload, pad) = out.split_at_mut(res.payload_bytes);
            for dst in payload.chunks_exact_mut(2) {
                let s = samples[sample_idx];
                sample_idx += 1;
                if sample_idx >= samples.len() {
                    sample_idx = 0;
                }
                dst.copy_from_slice(&s.to_le_bytes());
            }
            pad.fill(0);
        }

        let mut queued_this_tick = false;
        match uac::submit_demo_packet(res) {
            Ok(true) => {
                queued = queued.saturating_add(1);
                queued_this_tick = true;
            }
            Ok(false) => missed = missed.saturating_add(1),
            Err(uac::DemoQueueError::NoRuntime) => no_runtime = no_runtime.saturating_add(1),
            Err(uac::DemoQueueError::NoDevice) => {}
            Err(uac::DemoQueueError::FormatMismatch) => fmt_mismatch = fmt_mismatch.saturating_add(1),
            Err(uac::DemoQueueError::NoPacket) => {}
        }

        if Instant::now().duration_since(last_log) >= EmbassyDuration::from_secs(1) {
            let (evt_ok, evt_err, last_cc) = uac::take_xfer_event_counters();
            crate::log!(
                "audio: uac demo stats queued={} missed={} fmt_mismatch={} no_runtime={} tick_us={} bytes={}/{} evt_ok={} evt_err={} last_cc={}\n",
                queued,
                missed,
                fmt_mismatch,
                no_runtime,
                tick_us,
                res.payload_bytes,
                res.max_packet_bytes,
                evt_ok,
                evt_err,
                last_cc
            );
            last_log = Instant::now();
            queued = 0;
            missed = 0;
            fmt_mismatch = 0;
            no_runtime = 0;
        }

        if queued_this_tick {
            Timer::after(EmbassyDuration::from_micros(tick_us)).await;
        } else {
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
}
