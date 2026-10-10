//! Persistent Shell3 capture helpers. Copy, navigation, scheduling and recording
//! admission live here; Shell3 only launches the helper and selects its slot.
#[cfg(feature = "trueos_h264_encode_stream")]
mod mux;

use super::{
    helper::{self, Action, Input, Screen},
    tui::{self, Frontend},
};
use crate::shell3::{MatrixTarget};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

const VIDEO: u8 = 1;
const AUDIO: u8 = 2;
const MIN_SHOT_NS: u64 = 250_000_000;
const DURATIONS: [u32; 5] = [3, 30, 60, 300, 900];
const RECORD_LABELS: [&str; 5] = [
    "Record 3 sec",
    "Record 30 sec",
    "Record 1 min",
    "Record 5 min",
    "Record 15 min",
];
const PIC_LABELS: [&str; 3] = [
    "Take one screenshot",
    "Delay by 10 sec",
    "3 by 3 — three pictures, 3 sec apart",
];
const FOOTER: &str = "↑/↓ or j/k select   Enter choose   ←/h back   Esc/q quit   Mouse choose";
static LAST_SHOT: AtomicU64 = AtomicU64::new(0);

fn single_line(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub(crate) fn saved_result(path: &str) -> String {
    format!("Saved: {}", single_line(path.rsplit('/').next().unwrap_or(path)))
}
pub(crate) fn error_result(error: &str) -> String {
    let error = error
        .strip_prefix("film: ")
        .or_else(|| error.strip_prefix("rec: "))
        .unwrap_or(error);
    let error = error.split("; retained ").next().unwrap_or(error);
    format!("Error: {}", single_line(error))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pic,
    Vid,
    Aud,
    Vaud,
}
impl Kind {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "pic" => Self::Pic,
            "vid" => Self::Vid,
            "aud" => Self::Aud,
            "vaud" => Self::Vaud,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Pic => "pic",
            Self::Vid => "vid",
            Self::Aud => "aud",
            Self::Vaud => "vaud",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Pic => "PIC  screen pictures",
            Self::Vid => "VID  screen recording",
            Self::Aud => "AUD  microphone recording",
            Self::Vaud => "VAUD  screen + microphone",
        }
    }
}

/// Both tracks arm before a common start/deadline. A failure wakes/stops its peer.
struct Clock {
    ready: AtomicU8,
    expected: u8,
    epoch: AtomicU64,
    stop: AtomicBool,
    seconds: u32,
}
impl Clock {
    fn new(expected: u8, seconds: u32) -> Arc<Self> {
        Arc::new(Self {
            ready: AtomicU8::new(0),
            expected,
            epoch: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            seconds,
        })
    }
}
#[derive(Default)]
pub(crate) struct RecordingStatus {
    pub finished: bool,
    pub saved: bool,
    pub started_ns: u64,
    pub channels: u8,
    pub message: String,
    // Exact AU byte boundaries and actual capture times preserve pauses caused
    // by disk I/O, rather than assigning nominal FPS to a delayed recording.
    pub frames: Vec<(usize, u64)>,
}
#[derive(Clone)]
pub(crate) struct Recording {
    pub disk: crate::disc::block::DeviceHandle,
    pub path: String,
    pub status: Arc<Mutex<RecordingStatus>>,
    clock: Arc<Clock>,
    track: u8,
}
impl Recording {
    fn new(
        disk: crate::disc::block::DeviceHandle,
        path: String,
        clock: Arc<Clock>,
        track: u8,
    ) -> Self {
        Self {
            disk,
            path,
            status: Arc::new(Mutex::new(RecordingStatus::default())),
            clock,
            track,
        }
    }
    pub(crate) fn stopped(&self) -> bool {
        self.clock.stop.load(Ordering::Acquire)
    }
    pub(crate) fn stop(&self) {
        self.clock.stop.store(true, Ordering::Release);
    }
    pub(crate) async fn ready(&self) -> Option<u64> {
        let ready = self.clock.ready.fetch_or(self.track, Ordering::AcqRel) | self.track;
        if ready == self.clock.expected {
            let epoch = crate::chronos::monotonic_nanos() + 100_000_000;
            let _ =
                self.clock
                    .epoch
                    .compare_exchange(0, epoch, Ordering::AcqRel, Ordering::Acquire);
        }
        let timeout = crate::chronos::monotonic_nanos() + 10_000_000_000;
        loop {
            if self.stopped() {
                return None;
            }
            let now = crate::chronos::monotonic_nanos();
            let epoch = self.clock.epoch.load(Ordering::Acquire);
            if epoch != 0 && now >= epoch {
                return Some(epoch);
            }
            if now >= timeout {
                self.stop();
                return None;
            }
            Timer::after(Duration::from_millis(1)).await;
        }
    }
    pub(crate) fn seconds(&self) -> u32 {
        self.clock.seconds
    }
    pub(crate) fn deadline(&self, epoch: u64) -> u64 {
        epoch + u64::from(self.clock.seconds) * 1_000_000_000
    }
    pub(crate) fn started(&self, ns: u64, channels: u8) {
        let mut status = self.status.lock();
        status.started_ns = ns;
        status.channels = channels;
    }
    pub(crate) fn frame(&self, bytes: usize, ns: u64) {
        if self.clock.expected == VIDEO | AUDIO {
            self.status.lock().frames.push((bytes, ns));
        }
    }
    pub(crate) fn finish(&self, message: &str, saved: bool) {
        let mut status = self.status.lock();
        status.finished = true;
        status.saved = saved;
        status.message = message.into();
        drop(status);
        if !saved {
            self.stop();
        }
        super::service::notify_work();
    }
}

pub(super) fn recognizes(name: &str) -> bool {
    Kind::parse(name).is_some()
}
pub(super) fn start(name: &str, frontend: Frontend) -> Result<(), String> {
    let kind = Kind::parse(name).ok_or("capture: unknown helper")?;
    let Some((target, spawner)) = helper::admit(name, frontend)? else {
        return Ok(());
    };
    let token = match menu_task(kind, target.clone()) {
        Ok(token) => token,
        Err(_) => {
            tui::cancel_native_attach(&target);
            return Err("capture: helper task pool is full".into());
        }
    };
    spawner.spawn(token);
    Ok(())
}

struct Pictures {
    remaining: u8,
    next: u64,
}
struct Menu {
    kind: Kind,
    selected: usize,
    input: Input,
    pictures: Option<Pictures>,
    pending_pictures: usize,
    work: Option<helper::Work>,
    video: Option<Recording>,
    audio: Option<Recording>,
    message: String,
    result: String,
    seconds: u32,
    muxing: Option<Arc<Mutex<Option<Result<String, String>>>>>,
}
impl Menu {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            selected: 0,
            input: Input::default(),
            pictures: None,
            pending_pictures: 0,
            work: None,
            video: None,
            audio: None,
            message: String::new(),
            result: String::new(),
            seconds: 0,
            muxing: None,
        }
    }
    fn busy(&self) -> bool {
        self.video.is_some()
            || self.audio.is_some()
            || self.pictures.is_some()
            || self.muxing.is_some()
    }
    fn working(&self) -> bool {
        self.busy() || self.pending_pictures != 0
    }
    fn sync_work(&mut self, target: &MatrixTarget) {
        if self.working() {
            if self.work.is_none() {
                self.work = Some(helper::Work::new(target));
            }
        } else {
            self.work = None;
        }
    }
    fn labels(&self) -> Vec<&'static str> {
        if self.busy() {
            alloc::vec![
                if self.pictures.is_some() {
                    "Cancel pictures"
                } else if self.muxing.is_some() {
                    "Saving combined file…"
                } else {
                    "Stop and save"
                },
                "Return"
            ]
        } else {
            let mut labels = if self.kind == Kind::Pic {
                PIC_LABELS.to_vec()
            } else {
                RECORD_LABELS.to_vec()
            };
            labels.push("Return");
            labels
        }
    }
    fn layout(&self, rows: usize) -> (usize, usize, usize) {
        let first = if rows >= 12 { 4 } else { 1 };
        let visible = rows
            .saturating_sub(first + 3)
            .max(1)
            .min(self.labels().len());
        let offset = self.selected.saturating_sub(visible - 1);
        (first, visible, offset)
    }
    fn action(&mut self, action: Action, target: &MatrixTarget, rows: usize, now: u64) -> bool {
        let labels = self.labels();
        self.selected = self.selected.min(labels.len() - 1);
        match action {
            Action::Up => self.selected = (self.selected + labels.len() - 1) % labels.len(),
            Action::Down => self.selected = (self.selected + 1) % labels.len(),
            Action::Quit => {
                tui::native_return(target);
                return true;
            }
            Action::Click(row) => {
                if row == rows - 1 {
                    tui::native_return(target);
                    return true;
                }
                let (first, visible, offset) = self.layout(rows);
                if !(first..first + visible).contains(&row) {
                    return false;
                }
                self.selected = offset + row - first;
                if self.selected == labels.len() - 1 {
                    tui::native_return(target);
                    return true;
                }
                self.choose(target, now);
            }
            Action::Choose => {
                if self.selected == labels.len() - 1 {
                    tui::native_return(target);
                    return true;
                }
                self.choose(target, now);
            }
        }
        false
    }
    fn choose(&mut self, target: &MatrixTarget, now: u64) {
        if self.selected == self.labels().len() - 1 {
            tui::native_return(target);
            return;
        }
        if self.muxing.is_some() {
            return;
        }
        if self.busy() {
            let pictures_cancelled = self.pictures.take().is_some();
            for recording in self.video.iter().chain(self.audio.iter()) {
                recording.stop();
            }
            self.message = if pictures_cancelled {
                "Pictures cancelled."
            } else {
                "Stopping and saving…"
            }
            .into();
            return;
        }
        if self.kind == Kind::Pic {
            self.pictures = Some(Pictures {
                remaining: if self.selected == 2 { 3 } else { 1 },
                next: now
                    + if self.selected == 1 {
                        10_000_000_000
                    } else {
                        0
                    },
            });
            self.message = "Picture scheduled.".into();
            self.selected = 0;
        } else {
            let seconds = DURATIONS[self.selected];
            match self.record(target, seconds) {
                Ok(()) => self.message = "Arming recording…".into(),
                Err(error) => self.problem(error),
            }
            self.selected = 0;
        }
    }
    fn problem(&mut self, error: &str) {
        self.result = error_result(error);
        self.message.clear();
    }
    fn notice(&mut self, notice: &str) {
        self.pending_pictures = self.pending_pictures.saturating_sub(1);
        self.result = single_line(notice);
        if self.pictures.is_none() && self.pending_pictures == 0 {
            self.message.clear();
        }
    }
    fn result_line(&self, cols: usize) -> String {
        let chars: Vec<char> = self.result.chars().collect();
        if chars.len() <= cols {
            return self.result.clone();
        }
        if cols == 0 {
            return String::new();
        }
        let available = cols - 1;
        if self.result.starts_with("Saved: ") {
            // Keep both the filename's identifying prefix and its extension.
            let tail = available / 2;
            chars[..available - tail]
                .iter()
                .chain(core::iter::once(&'…'))
                .chain(chars[chars.len() - tail..].iter())
                .copied()
                .collect()
        } else {
            chars[..available]
                .iter()
                .copied()
                .chain(core::iter::once('…'))
                .collect()
        }
    }
    fn record(&mut self, target: &MatrixTarget, seconds: u32) -> Result<(), &'static str> {
        let disk = crate::ui4::writable_capture_root_handle()
            .ok_or("No writable TRUEOSFS root is mounted.")?;
        let expected = match self.kind {
            Kind::Vid => VIDEO,
            Kind::Aud => AUDIO,
            _ => VIDEO | AUDIO,
        };
        let clock = Clock::new(expected, seconds);
        let wall = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
        let now = crate::chronos::monotonic_nanos();
        let name = format!("{}-{wall}-{now}", self.kind.name());
        // Temporary proven outputs are retained if either capture or mux fails.
        if expected & VIDEO != 0 {
            #[cfg(feature = "trueos_h264_encode_stream")]
            {
                let recording =
                    Recording::new(disk, format!("screenfilms/{name}.h264"), clock.clone(), VIDEO);
                crate::ui4::request_capture_film(target.clone(), recording.clone())?;
                self.video = Some(recording);
            }
            #[cfg(not(feature = "trueos_h264_encode_stream"))]
            return Err("Hardware encoder support is disabled in this build.");
        }
        if expected & AUDIO != 0 {
            let recording = Recording::new(disk, format!("recordings/{name}.wav"), clock, AUDIO);
            if let Err(error) =
                crate::shell2::cmds::rec::request_capture(target.clone(), recording.clone())
            {
                if let Some(video) = &self.video {
                    video.stop();
                }
                return Err(error);
            }
            self.audio = Some(recording);
        }
        self.seconds = seconds;
        Ok(())
    }
    fn frame(&self, cols: usize, rows: usize, now: u64) -> Vec<String> {
        let labels = self.labels();
        let (first, visible, offset) = self.layout(rows);
        let mut lines = alloc::vec![String::new(); rows];
        lines[0] = self.kind.title().into();
        if first == 4 {
            lines[2] = "Choose an operation.".into();
        }
        for i in 0..visible {
            let index = i + offset;
            lines[first + i] =
                format!("  {} {}", if index == self.selected { "›" } else { " " }, labels[index]);
        }
        // Progress never replaces the last saved filename or failure.
        let status_row = rows - 3;
        let status = if let Some(pictures) = &self.pictures {
            format!(
                "{} picture(s) remaining; next in {} sec. {}",
                pictures.remaining,
                pictures.next.saturating_sub(now).div_ceil(1_000_000_000),
                self.message
            )
        } else if self.video.is_some() || self.audio.is_some() {
            let recording = self.video.as_ref().or(self.audio.as_ref()).unwrap();
            let epoch = recording.clock.epoch.load(Ordering::Acquire);
            if epoch == 0 {
                self.message.clone()
            } else {
                format!(
                    "{} / {} sec{}  {}",
                    now.saturating_sub(epoch) / 1_000_000_000,
                    self.seconds,
                    if recording.stopped() {
                        " · saving"
                    } else {
                        ""
                    },
                    self.message
                )
            }
        } else {
            self.message.clone()
        };
        lines[status_row] = if self.working() {
            format!("{} {}", helper::spinner(now), status)
        } else {
            status
        };
        lines[rows - 2] = self.result_line(cols);
        if rows >= 16 {
            for (index, recording) in self.video.iter().chain(self.audio.iter()).enumerate() {
                let status = recording.status.lock();
                lines[rows - 5 + index] = if status.message.is_empty() {
                    format!("trueosfs:/{}", recording.path)
                } else {
                    status.message.clone()
                };
            }
        }
        lines[rows - 1] = if cols < 70 {
            "↑/↓ j/k  Enter choose  Esc/q Return".into()
        } else {
            FOOTER.into()
        };
        lines
    }

    fn pictures(&mut self, target: &MatrixTarget, now: u64) {
        let Some(pictures) = self.pictures.as_mut() else {
            return;
        };
        if now < pictures.next {
            return;
        }
        if crate::ui4::writable_capture_root_handle().is_none() {
            self.pictures = None;
            self.problem("No writable TRUEOSFS root is mounted.");
            return;
        }
        let previous = LAST_SHOT.load(Ordering::Acquire);
        if previous != 0 && now.saturating_sub(previous - 1) < MIN_SHOT_NS {
            self.pictures = None;
            self.problem("Please wait 250 ms between pictures.");
            return;
        }
        if LAST_SHOT
            .compare_exchange(previous, now.saturating_add(1), Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        match crate::ui4::request_wd_postblend_capture(target.clone()) {
            Ok(()) => {
                self.pending_pictures += 1;
                self.message = "Picture armed; waiting for capture and save…".into();
                pictures.remaining -= 1;
                pictures.next = now + 3_000_000_000;
                if pictures.remaining == 0 {
                    self.pictures = None;
                }
            }
            Err(error) => {
                self.pictures = None;
                self.problem(error);
            }
        }
    }
    fn recordings(&mut self, target: &MatrixTarget) {
        if let Some(muxing) = &self.muxing {
            let Some(result) = muxing.lock().take() else {
                return;
            };
            self.message = match result {
                Ok(path) => {
                    self.result = saved_result(&path);
                    format!("Saved trueosfs:/{path}")
                }
                Err(error) => {
                    self.result = error_result(&error);
                    format!("{error}; source H.264/WAV retained.")
                }
            };
            self.muxing = None;
        } else {
            if self.video.is_none() && self.audio.is_none() {
                return;
            }
            if self
                .video
                .iter()
                .chain(self.audio.iter())
                .any(|r| !r.status.lock().finished)
            {
                return;
            }
            if self.kind == Kind::Vaud && self.video.is_some() && self.audio.is_some() {
                let video = self.video.as_ref().unwrap();
                let audio = self.audio.as_ref().unwrap();
                if video.status.lock().saved && audio.status.lock().saved {
                    let status = Arc::new(Mutex::new(None));
                    if let Some(spawner) = crate::workers::pick_background_spawner() {
                        if let Ok(token) = mux_task(video.clone(), audio.clone(), status.clone()) {
                            spawner.spawn(token);
                            self.muxing = Some(status);
                            self.message = "Combining video and audio into one MKV…".into();
                            return;
                        }
                    }
                    self.message = "Cannot start mux; source H.264/WAV retained.".into();
                    self.result = error_result("Cannot start combined-file save.");
                } else {
                    let failed = self
                        .video
                        .iter()
                        .chain(self.audio.iter())
                        .find(|recording| !recording.status.lock().saved)
                        .unwrap();
                    self.result = error_result(&failed.status.lock().message);
                    self.message =
                        "Capture failed; available source files and recovery chunks retained."
                            .into();
                }
            } else {
                let recording = self.video.as_ref().or(self.audio.as_ref()).unwrap();
                let status = recording.status.lock();
                self.result = if status.saved {
                    saved_result(&recording.path)
                } else {
                    error_result(&status.message)
                };
                self.message = status.message.clone();
            }
        }
        shell2::print_matrix_target_line(target, &self.message);
        self.message.clear();
        self.video = None;
        self.audio = None;
        self.selected = 0;
    }
}

#[trueos_executor::task(pool_size = 4)]
async fn menu_task(kind: Kind, target: MatrixTarget) {
    let mut menu = Menu::new(kind);
    let mut screen = Screen::default();
    while let Some((bytes, notices)) = tui::native_read(&target) {
        let now = crate::chronos::monotonic_nanos();
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let mut actions = menu.input.feed(&bytes, now);
        if let Some(action) = menu.input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            if menu.action(action, &target, surface.rows as usize, now) {
                break;
            }
        }
        for notice in notices {
            menu.notice(&notice);
        }
        menu.pictures(&target, now);
        menu.recordings(&target);
        menu.sync_work(&target);
        let frame = menu.frame(surface.cols as usize, surface.rows as usize, now);
        screen.paint(&target, &frame, surface.cols as usize, surface.rows as usize);
        Timer::after(Duration::from_millis(20)).await;
    }
    for recording in menu.video.iter().chain(menu.audio.iter()) {
        recording.stop();
    }
}

#[trueos_executor::task(pool_size = 1)]
async fn mux_task(
    video: Recording,
    audio: Recording,
    status: Arc<Mutex<Option<Result<String, String>>>>,
) {
    #[cfg(feature = "trueos_h264_encode_stream")]
    let result = mux::save(&video, &audio).await.map_err(String::from);
    #[cfg(not(feature = "trueos_h264_encode_stream"))]
    let result = {
        let _ = (video, audio);
        Err(String::from("Hardware encoder support is disabled in this build."))
    };
    *status.lock() = Some(result);
    super::service::notify_work();
}
