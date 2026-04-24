use embassy_time::{Duration as EmbassyDuration, Timer};

const PROBE_PATTERN_NAME: &str = "arp";
const PROBE_PATTERN_LOOPS: u32 = 1;
const PROBE_RETRY_DELAY_MS: u64 = 1_000;
const PROBE_LOOP_DELAY_MS: u64 = 250;

#[embassy_executor::task]
pub async fn task() {
    crate::log!(
        "intel/hda-probe: task start pattern={} loops={}\n",
        PROBE_PATTERN_NAME,
        PROBE_PATTERN_LOOPS,
    );

    loop {
        match crate::aud::pattern_play(PROBE_PATTERN_NAME, PROBE_PATTERN_LOOPS) {
            Ok(()) => {
                crate::log!(
                    "intel/hda-probe: pattern ok name={} loops={}\n",
                    PROBE_PATTERN_NAME,
                    PROBE_PATTERN_LOOPS,
                );
                Timer::after(EmbassyDuration::from_millis(PROBE_LOOP_DELAY_MS)).await;
            }
            Err(err) => {
                crate::log!(
                    "intel/hda-probe: pattern err name={} err={}\n",
                    PROBE_PATTERN_NAME,
                    err,
                );
                Timer::after(EmbassyDuration::from_millis(PROBE_RETRY_DELAY_MS)).await;
            }
        }
    }
}
