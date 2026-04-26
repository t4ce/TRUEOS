use embassy_time::{Duration as EmbassyDuration, Timer};

const PROBE_PATTERN_NAME: &str = "arp";
const PIANO_PROBE_PATTERN_NAME: &str = "piano-probe";
const PROBE_PATTERN_LOOPS: u32 = 1;
const PROBE_RETRY_DELAY_MS: u64 = 1_000;
const PROBE_LOOP_DELAY_MS: u64 = 250;
const PIANO_NOTE_POLL_DELAY_MS: u64 = 25;
const BASSLINE_IDLE_POLL_DELAY_MS: u64 = 25;

fn piano_claimed() -> bool {
    crate::usb2::midi::piano_connected()
}

fn play_default_probe_pattern() -> Result<&'static str, &'static str> {
    crate::aud::pattern_play(PROBE_PATTERN_NAME, PROBE_PATTERN_LOOPS)?;
    Ok(PROBE_PATTERN_NAME)
}

fn play_piano_probe_pattern(
    note: u8,
    velocity: u8,
    seq: u16,
) -> Result<&'static str, &'static str> {
    crate::log!(
        "intel/hda-probe: piano note-down note={} velocity={} seq={}\n",
        note,
        velocity,
        seq
    );
    crate::aud::pattern_play_piano_probe(note, velocity, PROBE_PATTERN_LOOPS)?;
    Ok(PIANO_PROBE_PATTERN_NAME)
}

fn uptime_ms() -> u64 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = embassy_time_driver::TICK_HZ as u128;
    if hz == 0 {
        0
    } else {
        ((ticks * 1000u128) / hz) as u64
    }
}

fn next_grid_ms(now_ms: u64, quantum_ms: u64) -> u64 {
    if quantum_ms == 0 {
        return now_ms;
    }
    let rem = now_ms % quantum_ms;
    if rem == 0 {
        now_ms
    } else {
        now_ms + (quantum_ms - rem)
    }
}

fn next_running_beat_ms(now_ms: u64, start_ms: u64, beat_ms: u64) -> u64 {
    if beat_ms == 0 || now_ms <= start_ms {
        return now_ms;
    }
    let elapsed = now_ms - start_ms;
    let rem = elapsed % beat_ms;
    if rem == 0 {
        now_ms
    } else {
        now_ms + (beat_ms - rem)
    }
}

#[embassy_executor::task]
pub async fn task() {
    crate::log!(
        "intel/hda-probe: task start pattern={} loops={}\n",
        PROBE_PATTERN_NAME,
        PROBE_PATTERN_LOOPS,
    );

    let mut last_piano_seq: Option<u16> = None;
    let mut last_bassline_toggle_seq = crate::aud::bassline_toggle_seq();
    let mut bassline_active = false;
    let mut bassline_started_ms = 0u64;
    let mut bassline_beat_ms = 60000u64 / 116;
    let mut pending_bassline_target: Option<bool> = None;
    let mut pending_bassline_due_ms = 0u64;

    loop {
        let toggle_seq = crate::aud::bassline_toggle_seq();
        let toggle_delta = toggle_seq.wrapping_sub(last_bassline_toggle_seq);
        if toggle_delta != 0 {
            last_bassline_toggle_seq = toggle_seq;
            if (toggle_delta & 1) != 0 {
                let now = uptime_ms();
                let target = pending_bassline_target
                    .map(|pending| !pending)
                    .unwrap_or(!bassline_active);
                if target == bassline_active {
                    pending_bassline_target = None;
                    crate::log!(
                        "intel/hda-probe: bassline toggle canceled active={}\n",
                        bassline_active
                    );
                } else {
                    pending_bassline_due_ms = if bassline_active {
                        next_running_beat_ms(now, bassline_started_ms, bassline_beat_ms)
                    } else {
                        next_grid_ms(now, bassline_beat_ms)
                    };
                    pending_bassline_target = Some(target);
                    crate::log!(
                        "intel/hda-probe: bassline toggle armed target={} due_ms={} now_ms={}\n",
                        target,
                        pending_bassline_due_ms,
                        now
                    );
                }
            }
        }

        if let Some(target) = pending_bassline_target {
            let now = uptime_ms();
            if now >= pending_bassline_due_ms {
                pending_bassline_target = None;
                if target {
                    match crate::aud::render_retro_bassline() {
                        Ok((samples, bpm, step_ms)) => {
                            bassline_beat_ms = (60000u64 / u64::from(bpm)).max(1);
                            match crate::hda::start_looped_playback(samples.as_slice()) {
                                Ok(()) => {
                                    bassline_active = true;
                                    bassline_started_ms = now;
                                    crate::log!(
                                        "intel/hda-probe: bassline on bpm={} beat_ms={} step_ms={} samples={}\n",
                                        bpm,
                                        bassline_beat_ms,
                                        step_ms,
                                        samples.len()
                                    );
                                }
                                Err(err) => {
                                    crate::log!("intel/hda-probe: bassline start err={}\n", err);
                                }
                            }
                        }
                        Err(err) => {
                            crate::log!("intel/hda-probe: bassline render err={}\n", err);
                        }
                    }
                } else {
                    match crate::aud::stop() {
                        Ok(()) => {
                            bassline_active = false;
                            bassline_started_ms = 0;
                            crate::log!("intel/hda-probe: bassline off\n");
                        }
                        Err(err) => {
                            crate::log!("intel/hda-probe: bassline stop err={}\n", err);
                        }
                    }
                }
            }
        }

        if bassline_active || pending_bassline_target.is_some() {
            Timer::after(EmbassyDuration::from_millis(BASSLINE_IDLE_POLL_DELAY_MS)).await;
            continue;
        }

        if piano_claimed() {
            match crate::usb2::midi::piano_note_snapshot() {
                Some(snapshot) if last_piano_seq != Some(snapshot.seq) => {
                    last_piano_seq = Some(snapshot.seq);
                    match play_piano_probe_pattern(snapshot.note, snapshot.velocity, snapshot.seq) {
                        Ok(pattern_name) => {
                            crate::log!(
                                "intel/hda-probe: pattern ok name={} loops={}\n",
                                pattern_name,
                                PROBE_PATTERN_LOOPS,
                            );
                            Timer::after(EmbassyDuration::from_millis(PIANO_NOTE_POLL_DELAY_MS))
                                .await;
                        }
                        Err(err) => {
                            crate::log!(
                                "intel/hda-probe: pattern err piano_claimed={} err={}\n",
                                true,
                                err,
                            );
                            Timer::after(EmbassyDuration::from_millis(PROBE_RETRY_DELAY_MS)).await;
                        }
                    }
                }
                Some(_) | None => {
                    Timer::after(EmbassyDuration::from_millis(PIANO_NOTE_POLL_DELAY_MS)).await;
                }
            }
        } else {
            last_piano_seq = None;
            match play_default_probe_pattern() {
                Ok(pattern_name) => {
                    crate::log!(
                        "intel/hda-probe: pattern ok name={} loops={}\n",
                        pattern_name,
                        PROBE_PATTERN_LOOPS,
                    );
                    Timer::after(EmbassyDuration::from_millis(PROBE_LOOP_DELAY_MS)).await;
                }
                Err(err) => {
                    crate::log!(
                        "intel/hda-probe: pattern err piano_claimed={} err={}\n",
                        false,
                        err,
                    );
                    Timer::after(EmbassyDuration::from_millis(PROBE_RETRY_DELAY_MS)).await;
                }
            }
        }
    }
}
