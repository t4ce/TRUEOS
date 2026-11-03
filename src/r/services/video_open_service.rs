//! File/URL playback admission for Blueprint callers, independent of terminals.
use crate::ui4::VideoPlaybackSession;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

struct BrowserSource { url:String, referrer:Option<String> }
fn valid_browser_url(url:&str)->bool {
    url.starts_with("https://") && url.len()<=3072 && url[8..].split(['/', '?', '#']).next().is_some_and(|host|!host.is_empty()&&!host.contains('@')) && !url.bytes().any(|b|b.is_ascii_control()||b.is_ascii_whitespace())
}
fn browser_source(payload:&[u8])->Option<BrowserSource> {
    let text=core::str::from_utf8(payload).ok()?;
    let (url,referrer)=text.split_once('\0').map_or((text,None),|(u,r)|(u,Some(r)));
    if !valid_browser_url(url)||referrer.is_some_and(|r|!valid_browser_url(r)||r.contains('#')) {return None;}
    Some(BrowserSource{url:url.into(),referrer:referrer.map(String::from)})
}
struct BrowserPlayback {
    owner: u32,
    latest: u32,
    phase: i32,
    queued: Option<BrowserSource>,
    pending: Option<VideoPlaybackSession>,
    active: Option<VideoPlaybackSession>,
    worker: bool,
}
static BROWSERS: Mutex<Vec<BrowserPlayback>> = Mutex::new(Vec::new());
static NEXT_REQUEST: AtomicU32 = AtomicU32::new(1);

/// One active browser playback and one newest candidate per owner. The active
/// session belongs to this service, not the requesting VM's lifetime.
pub(crate) fn browser_request(owner: u32, payload: &[u8]) -> i32 {
    let Some(source)=browser_source(payload) else {return -3;};
    let Some(worker) = crate::workers::pick_background_spawner() else {
        return -4;
    };
    let Ok(id) = NEXT_REQUEST.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
        (id < i32::MAX as u32).then_some(id + 1)
    }) else {
        return -4;
    };
    let mut states = BROWSERS.lock();
    let index = if let Some(index) = states.iter().position(|s| s.owner == owner) {
        index
    } else {
        if states.len() >= crate::allcaps::hv::VM_ID_LIMIT {
            return -4;
        }
        states.push(BrowserPlayback {
            owner,
            latest: 0,
            phase: 0,
            queued: None,
            pending: None,
            active: None,
            worker: false,
        });
        states.len() - 1
    };
    let state = &mut states[index];
    state.latest = id;
    state.phase = 0;
    state.queued = Some(source);
    if let Some(previous) = state.pending.take() {
        previous.cancel();
    }
    if !state.worker {
        match browser_prepare_task(owner) {
            Ok(task) => {
                state.worker = true;
                worker.spawn(task);
            }
            Err(_) => {
                state.queued = None;
                state.phase = -4;
                return -4;
            }
        }
    }
    id as i32
}
/// Pending=0, first frame published=1, ended=2, negative=failed/superseded.
pub(crate) fn browser_status(owner: u32, id: u32) -> i32 {
    BROWSERS
        .lock()
        .iter()
        .find(|s| s.owner == owner)
        .map_or(-1, |s| if s.latest == id { s.phase } else { -125 })
}
fn browser_finished(session: VideoPlaybackSession) {
    let mut states = BROWSERS.lock();
    for state in states.iter_mut() {
        if state.pending == Some(session) {
            state.pending = None;
            state.phase = -2;
        }
        if state.active == Some(session) {
            state.active = None;
            if state.phase == 1 {
                state.phase = 2;
            }
        }
    }
}

/// Serialize supersession against the actual first-frame window publication.
/// Failure leaves the old session untouched. Closing old resources happens
/// outside this lock through the existing display-safe retirement path.
pub(crate) fn publish_browser_frame<T, E>(
    session: VideoPlaybackSession,
    publish: impl FnOnce() -> Result<T, E>,
) -> Option<Result<T, E>> {
    let mut states = BROWSERS.lock();
    if session.is_cancelled() {
        return None;
    }
    let result = publish();
    let mut previous = None;
    if result.is_ok() {
        if let Some(state) = states.iter_mut().find(|s| s.pending == Some(session)) {
            state.pending = None;
            previous = state.active.replace(session);
            state.phase = 1;
        }
    }
    drop(states);
    if let Some(old) = previous {
        old.cancel();
        crate::ui4::stop_decoded_nv12_stream(old, "browser-replaced-after-first-frame");
    }
    Some(result)
}

#[trueos_executor::task(pool_size = 3)]
async fn browser_prepare_task(owner: u32) {
    loop {
        let request = {
            let mut states = BROWSERS.lock();
            let Some(state) = states.iter_mut().find(|s| s.owner == owner) else {
                return;
            };
            match state.queued.take() {
                Some(url) => Some((state.latest, url)),
                None => {
                    state.worker = false;
                    None
                }
            }
        };
        let Some((id, url)) = request else {
            return;
        };
        let started = Instant::now();
        loop {
            if browser_status(owner, id) != 0 {
                break;
            }
            if let Some(session) = crate::ui4::reserve_shell_decoded_video_player() {
                let current = {
                    let mut states = BROWSERS.lock();
                    let state = states.iter_mut().find(|s| s.owner == owner).unwrap();
                    if state.latest == id {
                        state.pending = Some(session);
                        true
                    } else {
                        false
                    }
                };
                if !current {
                    session.finish("browser-candidate-superseded");
                    break;
                }
                let target = if owner & 0x8000_0000 != 0 {
                    crate::hv::blueprint_console_target((owner & 0xff) as u8)
                } else {
                    None
                };
                if let Some(worker) = crate::workers::pick_background_spawner() {
                    if let Ok(task) = playback_task(Source::Browser(url), Session(session), target) {
                        worker.spawn(task);
                    }
                } else {
                    browser_finished(session);
                    session.finish("browser-worker-unavailable");
                }
                break;
            }
            if started.elapsed() > Duration::from_secs(30) {
                let mut states = BROWSERS.lock();
                let state = states.iter_mut().find(|s| s.owner == owner).unwrap();
                if state.latest == id {
                    state.phase = -4;
                }
                break;
            }
            Timer::after(Duration::from_millis(5)).await;
        }
    }
}

struct Session(VideoPlaybackSession);
impl Drop for Session {
    fn drop(&mut self) {
        browser_finished(self.0);
        self.0.finish("host-video-request-finished");
    }
}

enum Source {
    File(String),
    Url(String),
    Browser(BrowserSource),
}

fn source(path: &str, qualified: bool) -> Result<Source, i32> {
    if path.is_empty() || path.as_bytes().contains(&0) {
        return Err(-1);
    }
    if qualified && path.starts_with("https://") {
        let authority = path[8..].split(['/', '?', '#']).next().unwrap_or("");
        if path.len() > 8192
            || authority.is_empty()
            || authority.contains('@')
            || path
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        {
            return Err(-1);
        }
        return Ok(Source::Url(path.into()));
    }
    if qualified {
        let rest = path.strip_prefix("trueosfs:disc").ok_or(-1)?;
        let (disk, file) = rest.split_once('/').ok_or(-1)?;
        if disk.is_empty()
            || !disk.bytes().all(|b| b.is_ascii_digit())
            || disk.parse::<u32>().is_err()
            || file.is_empty()
        {
            return Err(-1);
        }
    } else if !path.starts_with('/') {
        return Err(-1);
    }
    crate::r::path::FsPath::parse(path, false).map_err(|_| -1)?;
    if !crate::r::io::env::trueosfs_scope_granted() {
        return Err(-13);
    }
    let path = crate::r::io::env::resolve_fs_path(path, false).ok_or(-1)?;
    Ok(Source::File(path))
}

pub(crate) fn enqueue(vm: u8, payload: &[u8], qualified: bool) -> i32 {
    let Ok(path) = core::str::from_utf8(payload) else {
        return -1;
    };
    let source = match crate::hv::with_guest_broker_context(vm, || source(path, qualified)) {
        Ok(source) => source,
        Err(code) => return code,
    };
    if let Source::Url(url) = &source {
        let rc = browser_request(
            crate::r::io::async_fs_cabi::owner_for_vm(vm),
            url.as_bytes(),
        );
        return if rc > 0 { 0 } else { rc };
    }
    let Some(worker) = crate::workers::pick_background_spawner() else {
        return -3;
    };
    let Some(session) = crate::ui4::reserve_shell_decoded_video_player() else {
        return -11;
    };
    let target = crate::hv::blueprint_console_target(vm);
    match playback_task(source, Session(session), target) {
        Ok(task) => {
            worker.spawn(task);
            0
        }
        Err(_) => -11,
    }
}

#[trueos_executor::task(pool_size = 3)]
async fn playback_task(
    source: Source,
    session: Session,
    target: Option<crate::shell3::MatrixTarget>,
) {
    let result = play(source, session.0).await;
    if let Err(error) = result {
        crate::log_os::blueprint_important_line(format_args!("video: {error}\n"));
        if let Some(target) = target {
            crate::shell3::matrix_target_print_line(
                &target,
                &alloc::format!("PLY FAILED · {error}"),
            );
        }
    }
}

async fn play(source: Source, session: VideoPlaybackSession) -> Result<(), &'static str> {
    let mut prepared = match &source {
        Source::File(path) => {
            Some(crate::intel::media::hw_vid::prepare_trueosfs_ui4_video(session, path).await?)
        }
        Source::Url(_) | Source::Browser(_) => None,
    };
    let extent = prepared.as_ref().map(|p| p.visible_extent()).unwrap_or((
        crate::ui4::DEFAULT_FRAME_WIDTH,
        crate::ui4::DEFAULT_FRAME_HEIGHT,
    ));
    if !crate::ui4::open_shell_decoded_video_player(session, extent.0, extent.1) {
        return Err("UI4 video window unavailable");
    }
    while !session.is_cancelled() {
        match &source {
            Source::File(path) => {
                let asset = match prepared.take() {
                    Some(asset) => asset,
                    None => {
                        crate::intel::media::hw_vid::prepare_trueosfs_ui4_video(session, path)
                            .await?
                    }
                };
                if asset.visible_extent() != extent {
                    return Err("video resolution changed between loop laps");
                }
                crate::intel::media::hw_vid::run_prepared_trueosfs_ui4_video(session, path, asset)
                    .await?;
            }
            Source::Browser(source) => {
                crate::intel::media::hw_vid::run_resolved_ui4_framed_video_playback_with_referrer(session,&source.url,source.referrer.as_deref()).await?;
                break;
            }
            Source::Url(url) => {
                crate::intel::media::hw_vid::run_resolved_ui4_framed_video_playback(session, url)
                    .await?;
                break;
            }
        }
    }
    Ok(())
}
