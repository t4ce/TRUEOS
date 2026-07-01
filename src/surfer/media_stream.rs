extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

const MEDIA_CANDIDATE_CAP: usize = 32;
const MEDIA_CANDIDATE_WAIT_POLL_MS: u64 = 25;

#[derive(Clone, Debug)]
pub(crate) struct BrowserMediaCandidate {
    pub(crate) browser_instance_id: u32,
    pub(crate) generation: u32,
    pub(crate) tag: String,
    pub(crate) url: String,
    pub(crate) kind: String,
}

#[derive(Default)]
struct BrowserMediaStreams {
    candidates: VecDeque<BrowserMediaCandidate>,
}

static BROWSER_MEDIA_STREAMS: Mutex<Option<BrowserMediaStreams>> = Mutex::new(None);

fn with_media_streams<R>(f: impl FnOnce(&mut BrowserMediaStreams) -> R) -> R {
    let mut guard = BROWSER_MEDIA_STREAMS.lock();
    let streams = guard.get_or_insert_with(BrowserMediaStreams::default);
    f(streams)
}

pub(crate) fn is_stream_candidate(kind: &str, url: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    if url.contains("/initplayback") || url.contains("initplayback?") {
        return false;
    }
    kind.contains("video")
        || kind.contains("media")
        || kind.contains("mp4")
        || kind.contains("h264")
        || kind.contains("avc")
        || kind.contains("mpegurl")
        || kind.contains("m3u8")
        || url.contains("mime=video")
        || url.contains("videoplayback")
        || url.ends_with(".mp4")
        || url.contains(".mp4?")
        || url.ends_with(".m3u8")
        || url.contains(".m3u8?")
}

pub(crate) fn push_candidate(candidate: BrowserMediaCandidate) -> usize {
    let len = with_media_streams(|streams| {
        while streams.candidates.len() >= MEDIA_CANDIDATE_CAP {
            let _ = streams.candidates.pop_front();
        }
        streams.candidates.push_back(candidate.clone());
        streams.candidates.len()
    });

    crate::log!(
        "surfer-media: stream candidate browser={} generation={} tag={} kind={} queued={} action=queue-url-only no_download=1 no_file=1 url={}\n",
        candidate.browser_instance_id,
        candidate.generation,
        candidate.tag,
        candidate.kind,
        len,
        candidate.url
    );
    len
}

pub(crate) fn begin_browser_generation(browser_instance_id: u32, generation: u32) -> usize {
    let dropped = with_media_streams(|streams| {
        let before = streams.candidates.len();
        streams.candidates.clear();
        before
    });
    crate::log!(
        "surfer-media: browser generation begin browser={} generation={} dropped_candidates={} action=clear-latest-media-queue\n",
        browser_instance_id,
        generation,
        dropped
    );
    dropped
}

pub(crate) fn latest_candidate() -> Option<BrowserMediaCandidate> {
    with_media_streams(|streams| {
        streams
            .candidates
            .iter()
            .enumerate()
            .max_by_key(|(index, candidate)| {
                (
                    browser_media_candidate_rank(candidate.kind.as_str(), candidate.url.as_str()),
                    *index,
                )
            })
            .map(|(_, candidate)| candidate.clone())
    })
}

fn browser_media_candidate_rank(kind: &str, url: &str) -> u8 {
    let kind = kind.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    if kind.contains("mp4")
        || kind.contains("h264")
        || kind.contains("avc")
        || url.contains("mime=video/mp4")
        || url.ends_with(".mp4")
        || url.contains(".mp4?")
    {
        100
    } else if kind.contains("youtube-innertube") || url.starts_with("innertube://player?") {
        60
    } else if kind.contains("sabr") || url.contains("sabr=1") || url.contains("sabr%3d1") {
        20
    } else if kind.contains("video") || url.contains("videoplayback") {
        40
    } else {
        0
    }
}

pub(crate) fn candidate_count() -> usize {
    with_media_streams(|streams| streams.candidates.len())
}

pub(crate) async fn wait_latest_candidate(timeout_ms: u64) -> Option<BrowserMediaCandidate> {
    if let Some(candidate) = latest_candidate() {
        return Some(candidate);
    }
    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    loop {
        if let Some(candidate) = latest_candidate() {
            return Some(candidate);
        }
        if Instant::now() >= deadline {
            return None;
        }
        Timer::after(EmbassyDuration::from_millis(MEDIA_CANDIDATE_WAIT_POLL_MS)).await;
    }
}
