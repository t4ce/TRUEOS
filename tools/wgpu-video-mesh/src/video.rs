use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread,
};

pub const VIDEO_WIDTH: u32 = 960;
pub const VIDEO_HEIGHT: u32 = 540;
pub const VIDEO_FPS: u32 = 30;

pub struct VideoFrame {
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct PlaybackStats {
    decoded_frames: AtomicU64,
    uploaded_frames: AtomicU64,
    error: Mutex<Option<String>>,
}

impl PlaybackStats {
    pub fn decoded_frames(&self) -> u64 {
        self.decoded_frames.load(Ordering::Relaxed)
    }

    pub fn uploaded_frames(&self) -> u64 {
        self.uploaded_frames.load(Ordering::Relaxed)
    }

    pub fn set_uploaded_frames(&self, value: u64) {
        self.uploaded_frames.store(value, Ordering::Relaxed);
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|error| error.clone())
    }

    fn set_error(&self, message: impl Into<String>) {
        if let Ok(mut error) = self.error.lock() {
            *error = Some(message.into());
        }
    }
}

pub fn spawn_decoder(video_path: PathBuf) -> (Receiver<VideoFrame>, Arc<PlaybackStats>) {
    let (sender, receiver) = sync_channel(1);
    let stats = Arc::new(PlaybackStats::default());
    let thread_stats = Arc::clone(&stats);

    thread::Builder::new()
        .name("ffmpeg-video-decoder".to_owned())
        .spawn(move || decode_forever(video_path, sender, &thread_stats))
        .expect("failed to start decoder thread");

    (receiver, stats)
}

fn decode_forever(video_path: PathBuf, sender: SyncSender<VideoFrame>, stats: &PlaybackStats) {
    if !video_path.is_file() {
        stats.set_error(format!("video file not found: {}", video_path.display()));
        return;
    }

    let filter = format!(
        "fps={VIDEO_FPS},scale={VIDEO_WIDTH}:{VIDEO_HEIGHT}:force_original_aspect_ratio=decrease,\
         pad={VIDEO_WIDTH}:{VIDEO_HEIGHT}:(ow-iw)/2:(oh-ih)/2:color=black"
    );
    let mut child = match Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "fatal",
            "-nostdin",
            "-stream_loop",
            "-1",
            "-re",
            "-i",
        ])
        .arg(&video_path)
        .args([
            "-map", "0:v:0", "-an", "-vf", &filter, "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            stats.set_error(format!(
                "could not start ffmpeg ({error}); install it with: sudo apt install ffmpeg"
            ));
            return;
        }
    };

    let Some(mut stdout) = child.stdout.take() else {
        stats.set_error("ffmpeg did not provide a video output pipe");
        let _ = child.kill();
        return;
    };

    let frame_size = (VIDEO_WIDTH * VIDEO_HEIGHT * 4) as usize;
    loop {
        let mut rgba = vec![0_u8; frame_size];
        if let Err(error) = stdout.read_exact(&mut rgba) {
            stats.set_error(format!("ffmpeg video stream stopped: {error}"));
            break;
        }

        stats.decoded_frames.fetch_add(1, Ordering::Relaxed);
        match sender.try_send(VideoFrame { rgba }) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}
