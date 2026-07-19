//! Feature-gated Full-HD H.264 software-encoder bring-up service.
//!
//! This proof deliberately stops at a verified Annex-B access unit. It does
//! not retain the encoded bytes, touch TRUEOSFS, or create a network socket.

use core::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

use embassy_time::{Duration, Timer};

const PROBE_START_DELAY_MS: u64 = 1_000;
const DEFERRED_UDP_PORT: u16 = 8921;

static STATE: AtomicU8 = AtomicU8::new(H264EncodeProbeState::Waiting as u8);
static ENCODE_US: AtomicU64 = AtomicU64::new(0);
static SOURCE_BYTES: AtomicUsize = AtomicUsize::new(0);
static ENCODED_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum H264EncodeProbeState {
    Waiting = 0,
    Encoding = 1,
    Verified = 2,
    Failed = 3,
}

impl H264EncodeProbeState {
    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Encoding,
            2 => Self::Verified,
            3 => Self::Failed,
            _ => Self::Waiting,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct H264EncodeProbeSnapshot {
    pub(crate) state: H264EncodeProbeState,
    pub(crate) encode_us: u64,
    pub(crate) source_bytes: usize,
    pub(crate) encoded_bytes: usize,
}

pub(crate) fn h264_encode_probe_snapshot() -> H264EncodeProbeSnapshot {
    H264EncodeProbeSnapshot {
        state: H264EncodeProbeState::from_raw(STATE.load(Ordering::Acquire)),
        encode_us: ENCODE_US.load(Ordering::Acquire),
        source_bytes: SOURCE_BYTES.load(Ordering::Acquire),
        encoded_bytes: ENCODED_BYTES.load(Ordering::Acquire),
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_probe_task() {
    let carrier_slot = crate::percpu::current_slot();
    crate::log_info!(target: "ui4/h264-encode";
        "ui4/h264-encode: service online carrier=ap1-ui-core expected_slot={} current_slot={} feature=trueos_h264_encode_probe source=synthetic-i420-fullhd output=discard transport=none udp_port_8921=deferred start_delay_ms={}\n",
        crate::workers::AP1_UI_SERVICE_SLOT,
        carrier_slot,
        PROBE_START_DELAY_MS,
    );
    if carrier_slot != crate::workers::AP1_UI_SERVICE_SLOT as usize {
        STATE.store(H264EncodeProbeState::Failed as u8, Ordering::Release);
        crate::log_error!(target: "ui4/h264-encode";
            "ui4/h264-encode: proof rejected reason=wrong-carrier expected_slot={} current_slot={} socket_calls=0\n",
            crate::workers::AP1_UI_SERVICE_SLOT,
            carrier_slot,
        );
        park().await;
    }

    Timer::after(Duration::from_millis(PROBE_START_DELAY_MS)).await;
    STATE.store(H264EncodeProbeState::Encoding as u8, Ordering::Release);
    let started_ns = crate::chronos::monotonic_nanos();
    let result = trueos_h264_encode_probe::encode_full_hd_diagnostic_idr();
    let encode_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
    ENCODE_US.store(encode_us, Ordering::Release);

    match result {
        Ok(proof) => {
            let metrics = proof.metrics;
            SOURCE_BYTES.store(metrics.source_bytes, Ordering::Release);
            ENCODED_BYTES.store(metrics.encoded_bytes, Ordering::Release);
            let ratio_permille = metrics
                .encoded_bytes
                .saturating_mul(1_000)
                .checked_div(metrics.source_bytes.max(1))
                .unwrap_or(0);
            let estimated_30fps_kbit = metrics
                .encoded_bytes
                .saturating_mul(8)
                .saturating_mul(30)
                .checked_div(1_000)
                .unwrap_or(usize::MAX);
            crate::log_info!(target: "ui4/h264-encode";
                "ui4/h264-encode: proof accepted=1 codec=h264 profile=baseline frame=idr macroblock_mode=i_pcm compression=lossless-none visible={}x{} coded={}x{} macroblocks={} source_bytes={} annexb_bytes={} sps_bytes={} pps_bytes={} idr_bytes={} size_permille={} estimated_30fps_kbit={} encode_us={} source_fnv1a32=0x{:08X} encoded_fnv1a32=0x{:08X} nals=7,8,5\n",
                metrics.visible_width,
                metrics.visible_height,
                metrics.coded_width,
                metrics.coded_height,
                metrics.macroblocks,
                metrics.source_bytes,
                metrics.encoded_bytes,
                metrics.sps_bytes,
                metrics.pps_bytes,
                metrics.idr_bytes,
                ratio_permille,
                estimated_30fps_kbit,
                encode_us,
                metrics.source_fnv1a32,
                metrics.encoded_fnv1a32,
            );
            drop(proof);
            STATE.store(H264EncodeProbeState::Verified as u8, Ordering::Release);
            crate::log_info!(target: "ui4/h264-encode";
                "ui4/h264-encode: result discarded=1 retained_bytes=0 filesystem_writes=0 socket_calls=0 udp_port={} status=verified service=parked next_step=independent-compressed-encoder-or-transport\n",
                DEFERRED_UDP_PORT,
            );
        }
        Err(error) => {
            STATE.store(H264EncodeProbeState::Failed as u8, Ordering::Release);
            crate::log_error!(target: "ui4/h264-encode";
                "ui4/h264-encode: proof accepted=0 error={:?} code={} encode_us={} filesystem_writes=0 socket_calls=0 udp_port={} status=failed service=parked\n",
                error,
                error.code(),
                encode_us,
                DEFERRED_UDP_PORT,
            );
        }
    }
    let snapshot = h264_encode_probe_snapshot();
    crate::log_info!(target: "ui4/h264-encode";
        "ui4/h264-encode: snapshot state={:?} encode_us={} source_bytes={} encoded_bytes={}\n",
        snapshot.state,
        snapshot.encode_us,
        snapshot.source_bytes,
        snapshot.encoded_bytes,
    );
    park().await;
}

async fn park() -> ! {
    loop {
        Timer::after(Duration::from_secs(3_600)).await;
    }
}
