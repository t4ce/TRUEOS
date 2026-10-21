//! File/URL playback admission for Blueprint callers, independent of terminals.
use alloc::string::String;
use crate::ui4::VideoPlaybackSession;

struct Session(VideoPlaybackSession);
impl Drop for Session {
    fn drop(&mut self) { self.0.finish("host-video-request-finished"); }
}

enum Source { File(String), Url(String) }

fn source(path: &str, qualified: bool) -> Result<Source, i32> {
    if path.is_empty() || path.as_bytes().contains(&0) { return Err(-1); }
    if qualified && path.starts_with("https://") {
        let authority = path[8..].split(['/', '?', '#']).next().unwrap_or("");
        if path.len() > 8192 || authority.is_empty() || authority.contains('@')
            || path.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) { return Err(-1); }
        return Ok(Source::Url(path.into()));
    }
    if qualified {
        let rest = path.strip_prefix("trueosfs:disc").ok_or(-1)?;
        let (disk, file) = rest.split_once('/').ok_or(-1)?;
        if disk.is_empty() || !disk.bytes().all(|b| b.is_ascii_digit()) || disk.parse::<u32>().is_err() || file.is_empty() { return Err(-1); }
    } else if !path.starts_with('/') { return Err(-1); }
    crate::r::path::FsPath::parse(path, false).map_err(|_| -1)?;
    if !crate::r::io::env::trueosfs_scope_granted() { return Err(-13); }
    let path = crate::r::io::env::resolve_fs_path(path, false).ok_or(-1)?;
    Ok(Source::File(path))
}

pub(crate) fn enqueue(vm: u8, payload: &[u8], qualified: bool) -> i32 {
    let Ok(path) = core::str::from_utf8(payload) else { return -1; };
    let source = match crate::hv::with_guest_broker_context(vm, || source(path, qualified)) {
        Ok(source) => source, Err(code) => return code,
    };
    let Some(worker) = crate::workers::pick_background_spawner() else { return -3; };
    let Some(session) = crate::ui4::reserve_shell_decoded_video_player() else { return -11; };
    let target = crate::hv::blueprint_console_target(vm);
    match playback_task(source, Session(session), target) {
        Ok(task) => { worker.spawn(task); 0 }
        Err(_) => -11,
    }
}

#[trueos_executor::task(pool_size = 3)]
async fn playback_task(source: Source, session: Session, target: Option<crate::shell3::MatrixTarget>) {
    let result = play(source, session.0).await;
    if let Err(error) = result {
        crate::log_os::blueprint_important_line(format_args!("video: {error}\n"));
        if let Some(target) = target {
            crate::shell3::matrix_target_print_line(&target, &alloc::format!("PLY FAILED · {error}"));
        }
    }
}

async fn play(source: Source, session: VideoPlaybackSession) -> Result<(), &'static str> {
    let mut prepared = match &source {
        Source::File(path) => Some(crate::intel::media::hw_vid::prepare_trueosfs_ui4_video(session, path).await?),
        Source::Url(_) => None,
    };
    let extent = prepared.as_ref().map(|p| p.visible_extent())
        .unwrap_or((crate::ui4::DEFAULT_FRAME_WIDTH, crate::ui4::DEFAULT_FRAME_HEIGHT));
    if !crate::ui4::open_shell_decoded_video_player(session, extent.0, extent.1) {
        return Err("UI4 video window unavailable");
    }
    while !session.is_cancelled() {
        match &source {
            Source::File(path) => {
                let asset = match prepared.take() {
                    Some(asset) => asset,
                    None => crate::intel::media::hw_vid::prepare_trueosfs_ui4_video(session, path).await?,
                };
                if asset.visible_extent() != extent { return Err("video resolution changed between loop laps"); }
                crate::intel::media::hw_vid::run_prepared_trueosfs_ui4_video(session, path, asset).await?;
            }
            Source::Url(url) => {
                crate::intel::media::hw_vid::run_resolved_ui4_framed_video_playback(session, url).await?;
                break;
            }
        }
    }
    Ok(())
}
