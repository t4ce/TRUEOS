//! Persistent Shell3 capture helpers. Copy, navigation, scheduling and recording
//! admission live here; Shell3 only launches the helper and selects its slot.
#[cfg(feature = "trueos_h264_encode_stream")]
mod mux;

use super::{
    capture::helper::{Action, Input, Screen},
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
            super::MatrixSlots::drop_slot(Some(name));
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
                audio::request_capture(target.clone(), recording.clone())
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
        print_line(target, &self.message);
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

/// Capture status belongs to the exact slot lifetime.
pub(crate) fn print_line(target: &MatrixTarget, text: &str) {
    super::MatrixSlots::echo_output(&super::matrix_target_slot_lease(target), text.into());
}
pub(crate) fn interrupted(target: &MatrixTarget) -> bool {
    !super::matrix_target::matrix_slot_is_live(&super::matrix_target_slot_lease(target))
}
mod helper {
use super::super::tui::{self, Frontend};
use crate::shell3::{MatrixTarget};
use alloc::{format, string::String, vec::Vec};

const GO: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
pub(super) fn spinner(now: u64) -> char {
    GO[(now / 100_000_000 % GO.len() as u64) as usize]
}

/// Count finite work, rather than the lifetime or selection of its menu.
pub(super) struct Work;
impl Work { pub(super) fn new(_: &MatrixTarget) -> Self { Self } }

/// A parked helper is reused; only a new lease needs a worker task.
pub(super) fn admit(
    name: &str,
    frontend: Frontend,
) -> Result<Option<(MatrixTarget, crate::workers::WorkerSpawner)>, String> {
    if tui::native_slot(name) {
        tui::request(frontend, name).map_err(String::from)?;
        return Ok(None);
    }
    let spawner =
        crate::workers::pick_background_spawner().ok_or("No background worker is ready.")?;
    if super::super::matrix_slots().lock().ids.iter().any(|id| id == name) { return Err("Capture slot is occupied.".into()); }
    let generation = super::super::MatrixSlots::ensure_named(name);
    let target = MatrixTarget::from_lease(super::super::MatrixSlotLease::from_identity(name.into(), generation));
    if let Err(error) = tui::attach_native(frontend, &target) {
        super::super::MatrixSlots::drop_slot(Some(name));
        return Err(error);
    }
    Ok(Some((target, spawner)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Up,
    Down,
    Choose,
    Quit,
    Click(usize),
}
#[derive(Default)]
pub(super) struct Input {
    sequence: Vec<u8>,
    escape_at: u64,
    after_cr: bool,
}
impl Input {
    pub(super) fn in_sequence(&self) -> bool {
        !self.sequence.is_empty()
    }
    pub(super) fn feed(&mut self, bytes: &[u8], now: u64) -> Vec<Action> {
        self.feed_cells(bytes, now)
            .into_iter()
            .map(|(action, _)| action)
            .collect()
    }
    /// Column coordinates let a row expose a separate action tag.
    pub(super) fn feed_cells(&mut self, bytes: &[u8], now: u64) -> Vec<(Action, Option<usize>)> {
        let mut actions = Vec::new();
        for &byte in bytes {
            if !self.sequence.is_empty() {
                if self.sequence.len() == 1 && !matches!(byte, b'[' | b'O') {
                    self.sequence.clear();
                    actions.push((Action::Quit, None));
                    continue;
                }
                self.sequence.push(byte);
                if self.sequence.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    if let Some(action) = decode_cell_sequence(&self.sequence) {
                        actions.push(action);
                    }
                    self.sequence.clear();
                } else if self.sequence.len() > 64 {
                    self.sequence.clear();
                }
                continue;
            }
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            let action = match byte {
                27 => {
                    self.sequence.push(byte);
                    self.escape_at = now;
                    None
                }
                b'k' => Some(Action::Up),
                b'j' => Some(Action::Down),
                b'\r' | b'\n' => Some(Action::Choose),
                b'q' | b'h' | 3 => Some(Action::Quit),
                _ => None,
            };
            if let Some(action) = action {
                actions.push((action, None));
            }
        }
        actions
    }
    pub(super) fn timeout(&mut self, now: u64) -> Option<Action> {
        if !self.sequence.is_empty() && now.saturating_sub(self.escape_at) >= 75_000_000 {
            let lone_escape = self.sequence.len() == 1;
            self.sequence.clear();
            return lone_escape.then_some(Action::Quit);
        }
        None
    }
}
fn decode_cell_sequence(bytes: &[u8]) -> Option<(Action, Option<usize>)> {
    match bytes {
        b"\x1b[A" | b"\x1bOA" => Some((Action::Up, None)),
        b"\x1b[B" | b"\x1bOB" => Some((Action::Down, None)),
        b"\x1b[D" | b"\x1bOD" => Some((Action::Quit, None)),
        _ => {
            let report = bytes.strip_prefix(b"\x1b[<")?;
            if report.last() != Some(&b'M') {
                return None;
            } // no release/double trigger
            let text = core::str::from_utf8(&report[..report.len() - 1]).ok()?;
            let mut fields = text.split(';');
            let button = fields.next()?.parse::<u16>().ok()?;
            let col = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
            let row = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
            if fields.next().is_some() {
                return None;
            }
            match button & !28 {
                0 => Some((Action::Click(row), Some(col))),
                64 => Some((Action::Up, None)),
                65 => Some((Action::Down, None)),
                _ => None,
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Screen {
    previous: Vec<Vec<char>>,
    geometry: (usize, usize),
}
impl Screen {
    pub(super) fn invalidate(&mut self) {
        for row in &mut self.previous {
            row.fill(' ');
        }
        self.geometry = (0, 0);
    }
    /// Blank padding erases shrinking text. ANSI positions use glyph columns,
    /// so UTF-8 bars and arrows never turn byte offsets into cursor positions.
    pub(super) fn diff(&mut self, lines: &[String], cols: usize, rows: usize) -> String {
        let resized = self.geometry != (cols, rows);
        let mut output = String::new();
        if resized {
            output.push_str("\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[0m\x1b[2J");
            self.previous = alloc::vec![alloc::vec![' '; cols]; rows];
            self.geometry = (cols, rows);
        }
        for row in 0..rows {
            let mut current = alloc::vec![' '; cols];
            if let Some(line) = lines.get(row) {
                for (col, ch) in line.chars().take(cols).enumerate() {
                    current[col] = if ch.is_control() { ' ' } else { ch };
                }
            }
            let previous = &self.previous[row];
            let mut col = 0;
            while col < cols {
                if current[col] == previous[col] {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < cols && current[col] != previous[col] {
                    col += 1;
                }
                output.push_str(&format!("\x1b[{};{}H", row + 1, start + 1));
                output.extend(current[start..col].iter().copied());
            }
            self.previous[row] = current;
        }
        output
    }
    pub(super) fn paint(
        &mut self,
        target: &MatrixTarget,
        lines: &[String],
        cols: usize,
        rows: usize,
    ) {
        let bytes = self.diff(lines, cols, rows);
        if !bytes.is_empty() {
            tui::native_write(target, bytes.as_bytes());
        }
    }
}

}

mod audio {
// Independent consecutive HDA PCM recording adapter.
use crate::r::fs::trueosfs as fs;
use crate::r::services::hda_capture_lane as capture;
use crate::shell3::MatrixTarget;
use alloc::{format, string::String, vec, vec::Vec};
use spin::Mutex;
use trueos_time::{Duration, Timer};

const DIRECTORY: &str = "recordings";
const CHUNK_BYTES: usize = 192 * 1024;
const COPY_BYTES: usize = 64 * 1024;
const PROGRESS_NS: u64 = 5_000_000_000;
// Classic RIFF has a 32-bit length. Finish before reaching its limit.
const MAX_PCM_BYTES: u64 = u32::MAX as u64 - 36 - 4;
struct Control {
    busy: bool,
    stop: bool,
}
static CONTROL: Mutex<Control> = Mutex::new(Control {
    busy: false,
    stop: false,
});

struct RecordRequest {
    capture: Option<crate::shell3::capture::Recording>,
    minutes: Option<u8>,
    disk: crate::disc::block::DeviceHandle,
    path: String,
    target: MatrixTarget,
    origin: MatrixTarget,
}

pub(crate) fn request_capture(origin: MatrixTarget, recording: crate::shell3::capture::Recording) -> Result<(), &'static str> {
    request_recording_inner(None, origin, Some(recording))
}
fn request_recording_inner(minutes: Option<u8>, origin: MatrixTarget, capture: Option<crate::shell3::capture::Recording>) -> Result<(), &'static str> {
    let mut control = CONTROL.lock();
    if control.busy {
        return Err("microphone recording is already running; use rec stop or §rec§");
    }
    let spawner =
        crate::workers::pick_background_spawner().ok_or("no background worker is ready")?;
    let disk = match &capture {Some(recording) => recording.disk, None => crate::ui4::writable_capture_root_handle().ok_or("no writable TRUEOSFS root is mounted")?};
    let target = origin.clone();
    let now = crate::chronos::monotonic_nanos();
    let wall = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
    let path = capture.as_ref().map(|recording| recording.path.clone()).unwrap_or_else(|| format!("{DIRECTORY}/microphone-{wall}-{now}.wav"));
    let capture_seconds = capture.as_ref().map(|c| c.seconds());
    let token = record_task(RecordRequest {
        capture,
        minutes,
        disk,
        path: path.clone(),
        target: target.clone(),
        origin,
    })
    .map_err(|_| "microphone recording task is already running")?;
    control.busy = true;
    control.stop = false;
    let duration = capture_seconds.map(|s| format!("for {s} sec")).or_else(|| minutes.map(|m| format!("for {m} min")))
        .unwrap_or_else(|| String::from("until stopped"));
    let stop = if capture_seconds.is_some() {"stop and save from the capture menu"} else {"stop and save with rec stop or §rec§"};
    super::print_line(
        &target,
        &format!("rec: armed {duration}; trueosfs:/{path}; {stop}"),
    );
    spawner.spawn(token);
    Ok(())
}

fn stopped(target: &MatrixTarget) -> bool {
    CONTROL.lock().stop || super::interrupted(target)
}

fn wav_header(channels: u8, bytes: u32) -> [u8; 44] {
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(bytes + 36).to_le_bytes());
    header[8..16].copy_from_slice(b"WAVEfmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&u16::from(channels).to_le_bytes());
    header[24..28].copy_from_slice(&48_000u32.to_le_bytes());
    header[28..32].copy_from_slice(&(48_000u32 * u32::from(channels) * 2).to_le_bytes());
    header[32..34].copy_from_slice(&(u16::from(channels) * 2).to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&bytes.to_le_bytes());
    header
}

struct AudioSpool {
    disk: crate::disc::block::DeviceHandle,
    path: String,
    pending: Vec<u8>,
    parts: Vec<(String, fs::FileReadHandle)>,
    saved_bytes: u64,
    channels: u8,
}

impl AudioSpool {
    fn new(request: &RecordRequest, channels: u8) -> Self {
        Self {
            disk: request.disk,
            path: request.path.clone(),
            pending: Vec::new(),
            parts: Vec::new(),
            saved_bytes: 0,
            channels,
        }
    }

    async fn flush(&mut self) -> Result<(), &'static str> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let path = format!("{}.part{:06}", self.path, self.parts.len());
        if !fs::file_in_async(self.disk, &path, &self.pending)
            .await
            .map_err(|_| "chunk write failed")?
        {
            return Err("no space for recording chunk");
        }
        // Pin this exact committed record, not a later path lookup that might
        // accidentally substitute an older or replaced file during the join.
        let handle = fs::file_read_open_async(self.disk, &path)
            .await
            .map_err(|_| "cannot verify recording chunk")?
            .filter(|handle| handle.data_len() == self.pending.len() as u64)
            .ok_or("recording chunk length verification failed")?;
        self.saved_bytes += handle.data_len();
        self.parts.push((path, handle));
        self.pending.clear();
        Ok(())
    }

    async fn finish(&mut self, target: &MatrixTarget) -> Result<(), &'static str> {
        self.flush().await?;
        if self.saved_bytes == 0 {
            return Err("no microphone samples were recorded");
        }
        let write = fs::file_write_begin_async(self.disk, &self.path, self.saved_bytes + 44)
            .await
            .map_err(|_| "cannot open final recording")?
            .ok_or("no space for final recording; recovery chunks retained")?;
        let result = self.copy_parts(write, target).await;
        if result.is_err() {
            let _ = fs::file_write_abort_async(write).await;
            return result;
        }
        fs::file_write_finish_async(write)
            .await
            .map_err(|_| "final recording commit failed")?;
        let final_file = fs::file_read_open_async(self.disk, &self.path)
            .await
            .map_err(|_| "cannot verify final recording")?
            .ok_or("final recording is missing")?;
        if final_file.data_len() != self.saved_bytes + 44 {
            return Err("final recording length verification failed");
        }
        // Only the verified final file replaces the temporary parts. Slot
        // retirement never enters a delete/abort path for recorded frames.
        for (path, _) in &self.parts {
            let _ = fs::file_delete_async(self.disk, path).await;
        }
        Ok(())
    }

    async fn copy_parts(&self, write: u32, target: &MatrixTarget) -> Result<(), &'static str> {
        fs::file_write_chunk_async(write, &wav_header(self.channels, self.saved_bytes as u32))
            .await
            .map_err(|_| "WAV header write failed")?;
        let mut buffer = vec![0; COPY_BYTES];
        let mut copied = 0u64;
        let mut next_progress = crate::chronos::monotonic_nanos() + PROGRESS_NS;
        for (_, handle) in &self.parts {
            let mut offset = 0;
            while offset < handle.data_len() {
                let count = (handle.data_len() - offset).min(buffer.len() as u64) as usize;
                let read = fs::file_read_handle_range_async(*handle, offset, &mut buffer[..count])
                    .await
                    .map_err(|_| "recording chunk read failed")?;
                if read != Some(count) {
                    return Err("recording chunk is incomplete");
                }
                fs::file_write_chunk_async(write, &buffer[..count])
                    .await
                    .map_err(|_| "final recording write failed")?;
                offset += count as u64;
                copied += count as u64;
                let now = crate::chronos::monotonic_nanos();
                if now >= next_progress {
                    super::print_line(
                        target,
                        &format!("rec: saving {copied}/{} bytes", self.saved_bytes,),
                    );
                    next_progress = now + PROGRESS_NS;
                }
            }
        }
        Ok(())
    }
}

#[trueos_executor::task]
async fn record_task(request: RecordRequest) {
    let (result, saved) = match run_recording(&request).await {Ok(message) => (message, true), Err(message) => (message, false)};
    super::print_line(&request.target, &result);
    if crate::shell3::matrix_target_slot_lease(&request.target) != crate::shell3::matrix_target_slot_lease(&request.origin) {
        super::print_line(&request.origin, &result);
    }
    crate::log_info!(target: "hda/recording"; "{}\n", result);
    CONTROL.lock().busy = false;
    if let Some(capture) = &request.capture {capture.finish(&result, saved);}
}

async fn run_recording(request: &RecordRequest) -> Result<String, String> {
    if !capture::ensure_started_on_current_worker() {
        return Err(String::from("rec: cannot start microphone capture on the background worker"));
    }
    if !matches!(fs::dir_create_all_async(request.disk, DIRECTORY).await, Ok(true)) {
        return Err(String::from("rec: cannot create recordings directory"));
    }
    let waiting = crate::chronos::monotonic_nanos();
    let mut cursor = loop {
        if stopped(&request.target) || request.capture.as_ref().is_some_and(|c| c.stopped()) {
            return Err(String::from("rec: stopped before microphone became ready; no file saved"));
        }
        if let Some(cursor) = capture::recording_cursor() {
            break cursor;
        }
        if crate::chronos::monotonic_nanos().saturating_sub(waiting) >= 10_000_000_000 {
            return Err(format!(
                "rec: microphone did not become ready within 10 seconds; state={:?}; no file saved",
                capture::status().state
            ));
        }
        Timer::after(Duration::from_millis(20)).await;
    };
    let epoch = if let Some(recording) = &request.capture {
        let Some(epoch) = recording.ready().await else {return Err(String::from("rec: capture startup cancelled or timed out"));};
        // Discard pre-roll so both tracks begin on the shared clock.
        cursor = capture::recording_cursor().ok_or_else(|| String::from("rec: microphone stopped before recording began"))?;
        recording.started(crate::chronos::monotonic_nanos(), cursor.channels);
        Some(epoch)
    } else {None};
    let channels = cursor.channels;
    let mut spool = AudioSpool::new(request, channels);
    // One full DMA ring lets stop drain the completed tail in a single copy.
    let mut pcm = vec![0i16; 128 * 1024];
    let started = crate::chronos::monotonic_nanos();
    let deadline = epoch.map(|epoch| request.capture.as_ref().unwrap().deadline(epoch)).or_else(|| request.minutes.map(|m| started + u64::from(m) * 60_000_000_000));
    let mut next_progress = started + PROGRESS_NS;
    let mut reason = "duration complete";
    let mut frames = 0u64;
    let mut peak = 0u16;
    let mut nonzero = 0u64;
    let mut samples = 0u64;
    super::print_line(
        &request.target,
        &format!("rec: recording microphone; 48000 Hz, {channels} channels, signed 16-bit PCM"),
    );
    loop {
        let now = crate::chronos::monotonic_nanos();
        let stop = stopped(&request.target) || request.capture.as_ref().is_some_and(|c| c.stopped());
        let done = deadline.is_some_and(|d| now >= d);
        // Drain completed audio once on stop, preserving the tail.
        let read = match capture::copy_recording_i16(&mut cursor, &mut pcm) {
            Ok(read) => read,
            Err(error) => {
                reason = error;
                break;
            }
        };
        if spool.saved_bytes + spool.pending.len() as u64 + read.samples as u64 * 2 > MAX_PCM_BYTES
        {
            reason = "WAV size limit reached";
            break;
        }
        for sample in &pcm[..read.samples] {
            peak = peak.max(sample.unsigned_abs());
            nonzero += u64::from(*sample != 0);
            spool.pending.extend_from_slice(&sample.to_le_bytes());
        }
        frames += (read.samples / usize::from(channels)) as u64;
        samples += read.samples as u64;
        if stop {
            reason = "stopped";
        }
        if stop || done {
            break;
        }
        if spool.pending.len() >= CHUNK_BYTES {
            if let Err(error) = spool.flush().await {
                reason = error;
                break;
            }
        }
        if now >= next_progress {
            super::print_line(
                &request.target,
                &format!(
                    "rec: {}s audio, {} frames; peak={} nonzero={} permille; saved={} buffered={} bytes",
                    frames / 48_000,
                    frames,
                    peak,
                    nonzero * 1000 / samples.max(1),
                    spool.saved_bytes,
                    spool.pending.len()
                ),
            );
            next_progress = now + PROGRESS_NS;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
    if let Some(capture) = &request.capture {capture.stop();}
    super::print_line(&request.target, &format!("rec: {reason}; saving WAV"));
    match spool.finish(&request.target).await {
        Ok(()) => Ok(format!(
            "rec: saved trueosfs:/{}; {} frames ({} ms), {} channels, peak={} nonzero={} permille; {reason}",
            request.path,
            frames,
            frames * 1000 / 48_000,
            channels,
            peak,
            nonzero * 1000 / samples.max(1)
        )),
        Err(error) => Err(format!(
            "rec: {reason}; {error}; retained {} PCM chunks ({} bytes) at trueosfs:/{}.part*; format=s16le-48000Hz-{}ch",
            spool.parts.len(),
            spool.saved_bytes,
            request.path,
            channels
        )),
    }
}

}
