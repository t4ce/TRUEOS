extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use spin::Mutex;

const MEDIA_CANDIDATE_CAP: usize = 32;

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
    kind.contains("video")
        || kind.contains("media")
        || kind.contains("mp4")
        || kind.contains("h264")
        || kind.contains("avc")
        || kind.contains("mpegurl")
        || kind.contains("m3u8")
        || url.contains("mime=video")
        || url.contains("videoplayback")
        || url.contains("googlevideo.com")
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

pub(crate) fn latest_candidate() -> Option<BrowserMediaCandidate> {
    with_media_streams(|streams| streams.candidates.back().cloned())
}
