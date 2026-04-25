use embassy_time::{Duration as EmbassyDuration, Timer};

const PROBE_PATTERN_NAME: &str = "arp";
const PIANO_PROBE_PATTERN_NAME: &str = "piano-probe";
const PROBE_PATTERN_LOOPS: u32 = 1;
const PROBE_RETRY_DELAY_MS: u64 = 1_000;
const PROBE_LOOP_DELAY_MS: u64 = 250;

fn piano_claimed() -> bool {
    crate::r::readiness::is_set(crate::r::readiness::PIANO_CLAIMED)
}

fn play_probe_pattern() -> Result<&'static str, &'static str> {
    if piano_claimed() {
        let snapshot = crate::usb2::midi::piano_note_snapshot();
        let (note, velocity, seq) = snapshot
            .map(|s| (s.note, s.velocity, s.seq))
            .unwrap_or((60, 88, 0));
        crate::log!(
            "intel/hda-probe: piano claimed note={} velocity={} seq={}\n",
            note,
            velocity,
            seq
        );
        crate::aud::pattern_play_piano_probe(note, velocity, PROBE_PATTERN_LOOPS)?;
        Ok(PIANO_PROBE_PATTERN_NAME)
    } else {
        crate::aud::pattern_play(PROBE_PATTERN_NAME, PROBE_PATTERN_LOOPS)?;
        Ok(PROBE_PATTERN_NAME)
    }
}

#[embassy_executor::task]
pub async fn task() {
    crate::log!(
        "intel/hda-probe: task start pattern={} loops={}\n",
        PROBE_PATTERN_NAME,
        PROBE_PATTERN_LOOPS,
    );

    loop {
        match play_probe_pattern() {
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
                    piano_claimed(),
                    err,
                );
                Timer::after(EmbassyDuration::from_millis(PROBE_RETRY_DELAY_MS)).await;
            }
        }
    }
}
