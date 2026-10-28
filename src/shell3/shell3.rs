mod admin;
#[path = "system/gpu.rs"]
mod gpu;
#[path = "system/font.rs"]
mod font;
mod helper;
mod lifecycle;
mod os;
#[path = "system/output.rs"]
pub(crate) mod output;
#[path = "system/picker.rs"]
mod picker;
#[path = "system/tlb.rs"]
pub(crate) mod tlb;
#[path = "system/vcpy.rs"]
mod vcpy;
pub(crate) use os::matrix_target_interrupted;
pub(crate) mod capture;
#[path = "system/cry.rs"]
mod cry;
#[path = "system/disc.rs"]
mod disc;
mod log_tap;
pub(crate) mod matrix_target;
mod monitor;
mod os_update;
#[path = "system/ram.rs"]
mod ram;
#[path = "system/smp.rs"]
mod smp;
#[path = "system/log.rs"]
mod system_log;
#[path = "system/net.rs"]
mod system_net;
pub(crate) mod table;
pub(crate) use matrix_target::{
    MatrixSlotLease, MatrixTarget, matrix_target_print_line, matrix_target_slot_lease,
};
mod command_history;
mod command_latch;
mod metafmtstr;
mod names;
pub mod net;
pub(crate) mod sessions;
mod ssh;
mod ssh_boot;
mod status;
mod transition;
mod tty;
pub(crate) mod tui;
mod update;

pub mod service;
#[path = "show/show.rs"]
pub mod show;
pub(crate) mod startup;

pub use metafmtstr::MetaFmtStr;
pub use show::{Backend as ShowBackend, Show};
pub use update::{SegmentUpdate, UpdateBatch, UpdateCallback};

use alloc::{
    collections::VecDeque,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use names::{ADM_GROUPS, CMD_GROUPS, HV_GROUPS};
use spin::Once;
use zeroize::Zeroize;

pub const MAX_SHELL3_INSTANCES: usize = 256;
pub const OPERATOR: char = '§';
pub const PROMPT_CURSOR: char = ' ';
pub const DEFAULT_COLUMNS: usize = 100;
pub const DEFAULT_ROWS: usize = 25;
pub const MIN_COLUMNS: usize = 20;
pub const MIN_ROWS: usize = 5;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    HV = 1,
    CMD = 2,
    ADM = 3,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RgbaColor {
    White = 0xFFFFFFFF,
    Pink = 0xFF69B4FF,
    Cyan = 0x30E0E2FF,
    BlackTransparent = 0x00000080,
    Underlined {
        foreground: [u8; 4],
    },
    Blinking {
        foreground: [u8; 4],
        background: Option<[u8; 4]>,
        underline: bool,
    },
    Terminal {
        foreground: [u8; 4],
        background: [u8; 4],
        underline: bool,
    },
}

impl RgbaColor {
    pub const fn rgba(self) -> [u8; 4] {
        let value: u32 = match self {
            Self::Terminal { foreground, .. }
            | Self::Underlined { foreground }
            | Self::Blinking { foreground, .. } => return foreground,
            Self::White => 0xFFFFFFFF,
            Self::Pink => 0xFF69B4FF,
            Self::Cyan => 0x30E0E2FF,
            Self::BlackTransparent => 0x00000080,
        };
        [
            ((value >> 24) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        ]
    }
    pub const fn background(self) -> Option<[u8; 4]> {
        match self {
            Self::Terminal { background, .. } => Some(background),
            Self::Blinking { background, .. } => background,
            _ => None,
        }
    }
    pub const fn blink(self) -> bool {
        matches!(self, Self::Blinking { .. })
    }

    pub const fn underline(self) -> bool {
        matches!(
            self,
            Self::Terminal {
                underline: true,
                ..
            } | Self::Underlined { .. }
                | Self::Blinking {
                    underline: true,
                    ..
                }
        )
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialRows {
    TitleRow = 1,
    StatusRow = 2,
    PromtRow = 3,
    /// Zero-based line within the visible Matrix transcript.
    MatrixRow(usize) = 4,
}
pub const SPECIAL_SEPARATOR: char = '│';

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripSide {
    Left = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell3Error {
    InstanceLimit,
    NoExecutor,
    WrongExecutor { expected: u32, actual: u32 },
}

pub struct TitleTime;

impl TitleTime {
    /// Capture the current civil time; live titles then follow minute signals.
    pub fn current() -> String {
        let utc_seconds = crate::chronos::best_effort_unix_time_seconds()
            .unwrap_or_else(crate::time::uptime_seconds);
        let local_seconds = crate::locale::local_unix_time_seconds(utc_seconds);
        let minutes_of_day = (local_seconds / 60) % (24 * 60);
        alloc::format!("{:02}:{:02}", minutes_of_day / 60, minutes_of_day % 60)
    }
}

fn title_left_text(time: &str) -> String {
    format!("TrueOS {} {}", OPERATOR, time)
}

pub struct MatrixSlots;

#[derive(Clone)]
struct MatrixSlotsState {
    // Raw unique slot IDs only. The default bare § slot is implicit at index 0.
    ids: Vec<String>,
    lifetimes: Vec<(String, u64)>,
    next_lifetime: u64,
    attachments: Vec<(MatrixSlotLease, alloc::sync::Arc<dyn matrix_target::MatrixSlotAttachment>)>,
    background_active: Vec<MatrixSlotLease>,
    echoes: Vec<(Option<String>, VecDeque<String>)>,
    generation: u64,
}

impl MatrixSlotsState {
    fn new() -> Self {
        Self {
            ids: Vec::new(),
            lifetimes: Vec::new(),
            next_lifetime: 1,
            attachments: Vec::new(),
            background_active: Vec::new(),
            echoes: Vec::new(),
            generation: 0,
        }
    }
}

// MatrixSlots is the one shared shell subsystem.
static MATRIX_SLOTS: Once<spin::Mutex<MatrixSlotsState>> = Once::new();

fn matrix_slots() -> &'static spin::Mutex<MatrixSlotsState> {
    MATRIX_SLOTS.call_once(|| spin::Mutex::new(MatrixSlotsState::new()))
}

fn matrix_slots_meta(ids: &[String], active: Option<&str>) -> Vec<MetaFmtStr> {
    let mut runs = Vec::with_capacity(ids.len().saturating_mul(3).saturating_add(1));
    let active = active.filter(|active| ids.iter().any(|id| id == active));
    for (index, id) in ids.iter().enumerate() {
        let color = if active == Some(id.as_str()) {
            RgbaColor::Pink
        } else {
            RgbaColor::White
        };
        runs.push(MetaFmtStr::new(if index == 0 { "" } else { " " }).color(RgbaColor::White));
        runs.push(MetaFmtStr::new(OPERATOR.to_string()).color(color).bold());
        runs.push(MetaFmtStr::new(id.clone()).color(color));
    }
    runs
}

fn matrix_slots_text(ids: &[String]) -> String {
    let mut text = String::new();
    for (index, id) in ids.iter().enumerate() {
        if index != 0 {
            text.push(' ');
        }
        text.push(OPERATOR);
        text.push_str(id);
    }
    text
}

fn current_matrix_slots_text() -> String {
    let slots = matrix_slots().lock();
    matrix_slots_text(&slots.ids)
}

impl MatrixSlots {
    /// Background host activity is independent of selection and VMX affinity.
    pub(super) fn set_background_active(lease: &MatrixSlotLease, active: bool) -> bool {
        let mut slots = matrix_slots().lock();
        if !slots.lifetimes.iter().any(|(name, lifetime)| {
            name == lease.name() && *lifetime == lease.lifetime_generation()
        }) {
            return false;
        }
        if active && !slots.background_active.contains(lease) {
            slots.background_active.push(lease.clone());
        } else if !active {
            slots.background_active.retain(|entry| entry != lease);
        } else {
            return true;
        }
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
        true
    }

    pub(super) fn background_active_slots() -> Vec<String> {
        matrix_slots()
            .lock()
            .background_active
            .iter()
            .map(|lease| lease.name().to_string())
            .collect()
    }

    /// Demand a slot and return its lifetime, so a freed/recreated id cannot
    /// silently reattach an old Shell3 view to the new slot.
    fn ensure_named(name: &str) -> u64 {
        let mut slots = matrix_slots().lock();
        if let Some((_, lifetime)) = slots.lifetimes.iter().find(|(id, _)| id == name) {
            return *lifetime;
        }
        let lifetime = slots.next_lifetime;
        slots.next_lifetime = slots.next_lifetime.wrapping_add(1);
        slots.ids.push(name.to_string());
        slots.lifetimes.push((name.to_string(), lifetime));
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
        lifetime
    }

    /// Allocate under one lock so simultaneous connections cannot share a name.
    pub(crate) fn fresh_slot(peer_port: Option<u16>) -> (String, u64) {
        let preferred = peer_port
            .filter(|port| *port != 0)
            .map(|port| port.to_string());
        Self::allocate_slot(preferred, "sh")
    }

    /// Blueprint identities use the archive basename without its package suffix.
    pub(crate) fn fresh_app_slot(archive: &str) -> (String, u64) {
        let basename = archive.rsplit('/').next().unwrap_or(archive);
        let stem = basename.strip_suffix(".bp").unwrap_or(basename);
        let prefix: String = stem.chars().take(4).collect();
        let prefix = if prefix.is_empty() { "app" } else { &prefix };
        Self::allocate_slot(Some(prefix.to_string()), prefix)
    }

    fn allocate_slot(preferred: Option<String>, prefix: &str) -> (String, u64) {
        let mut slots = matrix_slots().lock();
        let mut number = 1u64;
        let name = loop {
            if let Some(candidate) = preferred.as_ref().filter(|name| !slots.ids.contains(name)) {
                break candidate.clone();
            }
            let candidate = format!("{prefix}{number}");
            if !slots.ids.contains(&candidate) {
                break candidate;
            }
            number += 1;
        };
        let lifetime = slots.next_lifetime;
        slots.next_lifetime = slots.next_lifetime.wrapping_add(1);
        slots.ids.push(name.clone());
        slots.lifetimes.push((name.clone(), lifetime));
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
        (name, lifetime)
    }

    /// None resets the implicit default slot; named slots disappear entirely.
    pub(crate) fn drop_lease(lease: &MatrixSlotLease) -> bool {
        Self::drop_slot_inner(Some(lease.name()), Some(lease.lifetime_generation()))
    }

    pub(crate) fn drop_slot(name: Option<&str>) -> bool {
        Self::drop_slot_inner(name, None)
    }

    fn drop_slot_inner(name: Option<&str>, expected: Option<u64>) -> bool {
        let mut slots = matrix_slots().lock();
        if expected.is_some_and(|generation| {
            !slots
                .lifetimes
                .iter()
                .any(|(id, lifetime)| Some(id.as_str()) == name && *lifetime == generation)
        }) {
            return false;
        }
        if let Some(name) = name {
            if !slots.ids.iter().any(|id| id == name) {
                return false;
            }
            slots.ids.retain(|id| id != name);
            slots.lifetimes.retain(|(id, _)| id != name);
        }
        slots.echoes.retain(|(id, _)| id.as_deref() != name);
        slots.generation = slots.generation.wrapping_add(1);
        let retired = matrix_target::retire_expired_attachments(&mut slots);
        drop(slots);
        for (lease, resource) in retired {
            resource.on_matrix_slot_freed(&lease);
        }
        service::notify_work();
        true
    }

    /// Prepend text into a named Matrix slot (None is the bare § slot).
    /// Slot selection remains per instance; transcripts are shared Matrix data.
    fn echo(active: Option<&str>, lifetime: Option<u64>, text: String) {
        let mut slots = matrix_slots().lock();
        let active = active
            .filter(|id| {
                slots
                    .lifetimes
                    .iter()
                    .any(|(name, current)| name == id && Some(*current) == lifetime)
            })
            .map(str::to_string);
        let index = slots
            .echoes
            .iter()
            .position(|(id, _)| *id == active)
            .unwrap_or_else(|| {
                slots.echoes.push((active, VecDeque::new()));
                slots.echoes.len() - 1
            });
        let lines = &mut slots.echoes[index].1;
        if lines.len() == 256 {
            lines.pop_back();
        }
        lines.push_front(text);
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
    }

    /// Append guest output only to the exact live slot, never the bare slot.
    pub(crate) fn echo_output(lease: &MatrixSlotLease, text: String) {
        let mut slots = matrix_slots().lock();
        if !slots.lifetimes.iter().any(|(name, generation)| {
            name == lease.name() && *generation == lease.lifetime_generation()
        }) {
            return;
        }
        let id = Some(lease.name().to_string());
        let index = slots
            .echoes
            .iter()
            .position(|(name, _)| *name == id)
            .unwrap_or_else(|| {
                slots.echoes.push((id, VecDeque::new()));
                slots.echoes.len() - 1
            });
        let lines = &mut slots.echoes[index].1;
        if lines.len() == 256 {
            lines.pop_back();
        }
        lines.push_front(text);
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
    }

    fn echo_snapshot(active: Option<&str>) -> (u64, Vec<String>) {
        let slots = matrix_slots().lock();
        let active = active.filter(|id| slots.ids.iter().any(|name| name == id));
        let lines = slots
            .echoes
            .iter()
            .find(|(id, _)| id.as_deref() == active)
            .map(|(_, lines)| lines.iter().cloned().collect())
            .unwrap_or_default();
        (slots.generation, lines)
    }

    fn view_echo_snapshot(active: Option<&str>, lifetime: Option<u64>) -> (u64, Vec<String>) {
        let slots = matrix_slots().lock();
        let active = active.filter(|id| {
            slots
                .lifetimes
                .iter()
                .any(|(name, current)| name == id && Some(*current) == lifetime)
        });
        let lines = slots
            .echoes
            .iter()
            .find(|(id, _)| id.as_deref() == active)
            .map(|(_, lines)| lines.iter().cloned().collect())
            .unwrap_or_default();
        (slots.generation, lines)
    }

    // Active selection is per shell, not shared.
}

#[derive(Clone, Debug)]
struct RowStrips {
    left: Vec<MetaFmtStr>,
    right: Vec<MetaFmtStr>,
}

impl RowStrips {
    fn new(left: &str, right: &str) -> Self {
        Self {
            left: vec![MetaFmtStr::new(left)],
            right: vec![MetaFmtStr::new(right)],
        }
    }
}

#[derive(Clone, Debug)]
struct SpecialRowsState {
    title: RowStrips,
    status: RowStrips,
    promt: RowStrips,
}

impl SpecialRowsState {
    fn new(time: &str, prompt_left: &str) -> Self {
        Self {
            title: RowStrips::new(&title_left_text(time), ""),
            // StatusRow/Left is read from the shared MatrixSlots system.
            status: RowStrips::new("", ""),
            promt: RowStrips {
                left: vec![
                    MetaFmtStr::new(prompt_left)
                        .color(RgbaColor::Terminal {
                            foreground: [0, 0, 0, 255],
                            background: RgbaColor::White.rgba(),
                            underline: false,
                        })
                        .blink(),
                ],
                right: vec![],
            },
        }
    }

    fn row(&self, row: SpecialRows) -> &RowStrips {
        match row {
            SpecialRows::TitleRow => &self.title,
            SpecialRows::StatusRow => &self.status,
            SpecialRows::PromtRow => &self.promt,
            SpecialRows::MatrixRow(_) => panic!("Matrix transcript is not a special strip"),
        }
    }
}

#[derive(Clone, Debug)]
struct PromptState {
    text: String,
    colors: Vec<Option<RgbaColor>>,
    cursor: usize,
}

impl Drop for PromptState {
    fn drop(&mut self) {
        self.text.zeroize();
    }
}

impl PromptState {
    fn new() -> Self {
        Self {
            text: String::new(),
            colors: Vec::new(),
            cursor: 0,
        }
    }

    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    fn render(&self) -> String {
        let mut output = String::with_capacity(self.text.len() + PROMPT_CURSOR.len_utf8());
        let mut inserted = false;

        for (index, ch) in self.text.chars().enumerate() {
            if index == self.cursor {
                output.push(PROMPT_CURSOR);
                inserted = true;
            }
            output.push(ch);
        }

        if !inserted {
            output.push(PROMPT_CURSOR);
        }

        output
    }
}

enum LatchedNavigation {
    Default,
    Slot {
        name: String,
        lifetime: Option<u64>,
    },
    Native {
        name: String,
        lifetime: Option<u64>,
    },
    App {
        app: service::QueuedBlueprint,
        lifetime: u64,
    },
    Resume {
        name: String,
        lifetime: Option<u64>,
    },
}

pub struct Shell3 {
    command_history: command_history::CommandHistory,
    command_feedback: Option<command_latch::Feedback>,
    latched_navigation: Option<LatchedNavigation>,
    tab_from_right: bool,
    title_transition: transition::TokenSteps,
    aka_transition: transition::TokenSteps,
    status_hover: Option<status::Target>,
    tui_frontend: u64,
    executor_slot: u32,
    columns: usize,
    rows_count: usize,
    layout_generation: usize,
    time: String,
    clock: crate::chronos::signals::Subscription,
    mode: Mode,
    active_matrix_slot: Option<String>,
    active_matrix_lifetime: Option<u64>,
    matrix_selection_dirty: bool,
    matrix_scroll: usize,
    matrix_column: usize,
    matrix_drag: Option<(i32, i32)>,
    prompt: PromptState,
    rows: SpecialRowsState,
    aka_names: Vec<String>,
    aka_generation: u64,
    appdb_generation: u64,
    appdb_names: Vec<String>,
    update_callbacks: Vec<UpdateCallback>,
    update_baseline: update::Snapshot,
    pending_presentation: Option<(update::Snapshot, UpdateBatch, Vec<update::RenderedLine>)>,
    show: Show,
}

impl Drop for Shell3 {
    fn drop(&mut self) {
        tui::detach(self.tui_frontend);
        service::release_shell_on_executor(self.executor_slot);
    }
}

impl Shell3 {
    pub(super) fn active_matrix_lease(&self) -> Option<MatrixSlotLease> {
        Some(MatrixSlotLease::from_identity(
            self.active_matrix_slot.clone()?,
            self.active_matrix_lifetime?,
        ))
    }

    /// SSH supplies its accepted PTY size before model construction or drawing.
    pub(super) fn new_terminal_sized_reserved(
        slot: u32,
        peer_port: Option<u16>,
        columns: usize,
        rows: usize,
    ) -> Self {
        debug_assert_eq!(crate::percpu::current_slot() as u32, slot);
        let mut shell = Self::new_inner(
            &TitleTime::current(),
            crate::r::restart::startup_alias_names(),
            service::appdb_names_snapshot().1,
            Vec::new(),
            columns,
            rows,
            slot,
        );
        shell
            .command_history
            .set_scope(matrix_target::TRANSPORT_NET_TCP_SCOPE);
        shell.set_show_backend(ShowBackend::Network);
        let (name, lifetime) = MatrixSlots::fresh_slot(peer_port);
        shell.active_matrix_slot = Some(name);
        shell.active_matrix_lifetime = Some(lifetime);
        shell.matrix_selection_dirty = true;
        tui::select(shell.tui_frontend(), shell.active_matrix_slot.as_deref());
        shell
    }

    pub(super) fn new_reserved(
        time: &str,
        aka_names: Vec<String>,
        appdb_names: Vec<String>,
        update_callbacks: Vec<UpdateCallback>,
        columns: usize,
        rows: usize,
        executor_slot: u32,
    ) -> Result<Self, Shell3Error> {
        let actual = crate::percpu::current_slot() as u32;
        if actual != executor_slot {
            return Err(Shell3Error::WrongExecutor {
                expected: executor_slot,
                actual,
            });
        }
        Ok(Self::new_inner(
            time,
            aka_names,
            appdb_names,
            update_callbacks,
            columns,
            rows,
            executor_slot,
        ))
    }

    fn new_inner(
        time: &str,
        aka_names: Vec<String>,
        appdb_names: Vec<String>,
        update_callbacks: Vec<UpdateCallback>,
        columns: usize,
        rows: usize,
        executor_slot: u32,
    ) -> Self {
        let prompt = PromptState::new();
        let prompt_left = prompt.render();
        let time = time.to_string();
        let columns = columns.max(MIN_COLUMNS);
        let rows_count = rows.max(MIN_ROWS);
        let mut rows_state = SpecialRowsState::new(&time, &prompt_left);
        rows_state.title.right = mode_title_meta(Mode::CMD, &aka_names, &appdb_names);
        rows_state.status.right = status::alias_runs(&aka_names);

        let status_left = {
            let slots = matrix_slots().lock();
            matrix_slots_meta(&slots.ids, None)
        };
        let (matrix_generation, matrix_lines) = MatrixSlots::echo_snapshot(None);
        let initial = update::Snapshot::new(
            (columns, rows_count),
            0,
            [
                (&rows_state.title.left, &rows_state.title.right),
                (&status_left, &rows_state.status.right),
                (&rows_state.promt.left, &rows_state.promt.right),
            ],
            columns,
        )
        .with_matrix(&matrix_lines, matrix_generation);

        Self {
            command_history: command_history::CommandHistory::default(),
            command_feedback: None,
            latched_navigation: None,
            tab_from_right: false,
            title_transition: transition::TokenSteps::default(),
            aka_transition: transition::TokenSteps::default(),
            status_hover: None,
            tui_frontend: tui::new_frontend(),
            executor_slot,
            columns,
            rows_count,
            layout_generation: 0,
            time,
            clock: crate::chronos::signals::subscribe(crate::chronos::signals::Every::MINUTE),
            mode: Mode::CMD,
            active_matrix_slot: None,
            active_matrix_lifetime: None,
            matrix_selection_dirty: false,
            matrix_scroll: 0,
            matrix_column: 0,
            matrix_drag: None,
            prompt,
            rows: rows_state,
            aka_names,
            aka_generation: crate::r::startup_config::generation(),
            appdb_generation: service::appdb_names_snapshot().0,
            appdb_names,
            update_callbacks,
            update_baseline: initial,
            pending_presentation: None,
            show: Show::default(),
        }
    }

    pub(super) fn configure_window(&mut self, mut geometry: service::WindowGeometry) {
        let scale = service::microfont_scale();
        let Some((cell_width, cell_height)) = crate::r::services::shell3_font_service::cell_extent(scale) else {
            self.show.configure_window(geometry);
            return;
        };
        geometry.width = geometry
            .width
            .max(MIN_COLUMNS as u32 * cell_width);
        geometry.height = geometry
            .height
            .max(MIN_ROWS as u32 * cell_height);
        if let Some((width, height)) = crate::ui4::output_dimensions() {
            geometry.width = geometry.width.min(width);
            geometry.height = geometry.height.min(height);
        }
        self.show.configure_window(geometry);
        self.set(
            (geometry.width / (cell_width)) as usize,
            (geometry.height / (cell_height)) as usize,
        );
    }

    pub fn set_show_backend(&mut self, backend: ShowBackend) {
        self.show.set_backend(backend);
    }

    pub(super) fn show_is_closed(&self) -> bool {
        self.show.is_closed()
    }

    pub(super) fn show_handles_window(&self, window: crate::ui4::WindowId) -> bool {
        self.show.handles_window(window)
    }

    /// Publish the current title, status, and prompt strips through UI4.
    pub async fn present(&mut self) -> Result<(), &'static str> {
        self.reconcile_matrix_selection();
        let actual_slot = crate::percpu::current_slot() as u32;
        if actual_slot != self.executor_slot {
            return Err("shell3-show-wrong-executor");
        }
        if self.font_scale_needed() {
            let scale = service::microfont_scale();
            let (cell_width, cell_height) = crate::r::services::shell3_font_service::cell_extent(scale)
                .ok_or("shell3-font-physical-size-unavailable")?;
            let (width, height) = self
                .show
                .set_font_scale(scale, self.columns, self.rows_count)?;
            self.set(
                (width / (cell_width)) as usize,
                (height / (cell_height)) as usize,
            );
            self.pending_presentation = None;
            if self.show.resize_needed() {
                self.resize_ui4_to_current().await?;
            }
            crate::log_info!(target: "service";
                "sh3srv: microfont scale={}x slot={} extent={}x{} grid={}x{}\n",
                scale, self.executor_slot, width, height, self.columns, self.rows_count,
            );
        }
        if self.show.window().is_none() {
            let (columns, rows) = self.show.initial_grid(self.columns, self.rows_count)?;
            if (columns, rows) != (self.columns, self.rows_count) { self.set(columns, rows); }
        }
        loop {
            if self.pending_presentation.is_none() {
                let snapshot = self.capture_update_snapshot();
                let batch = update::take_updates(
                    &mut self.update_baseline,
                    snapshot.clone(),
                    &self.update_callbacks,
                );
                let lines = snapshot.rendered_lines();
                self.pending_presentation = Some((snapshot, batch, lines));
            }
            let Some((snapshot, batch, lines)) = self.pending_presentation.as_ref() else {
                return Err("shell3-show-pending-update-missing");
            };
            let snapshot = snapshot.clone();
            let batch = batch.clone();
            let lines = lines.clone();
            let line_refs: Vec<_> = lines.iter().map(|line| line.as_slice()).collect();
            let (columns, rows) = snapshot.size();
            self.show
                .present(
                    &line_refs,
                    columns,
                    rows,
                    &batch,
                    snapshot.reveals(),
                    snapshot.matrix_area(),
                )
                .await?;
            if let Some(window) = self.show.window() {
                tui::bind_ui4_window(self.tui_frontend, window);
            }
            self.pending_presentation = None;
            if self.capture_update_snapshot() == snapshot {
                break;
            }
        }
        self.matrix_selection_dirty = false;
        Ok(())
    }

    pub(super) fn font_scale_needed(&self) -> bool {
        self.show.font_scale() != service::microfont_scale()
    }

    pub(super) fn presentation_pending(&self) -> bool {
        self.pending_presentation.is_some()
    }

    /// Re-render at UI4's current dock or maximize target on this shell's AP.
    pub(super) async fn resize_ui4_to_current(&mut self) -> Result<(), &'static str> {
        let actual_slot = crate::percpu::current_slot() as u32;
        if actual_slot != self.executor_slot {
            return Err("shell3-show-wrong-executor");
        }
        let (width, height) = self
            .show
            .resize_target_extent()
            .ok_or("shell3-show-resize-state")?;
        let (cell_width, cell_height) = crate::r::services::shell3_font_service::cell_extent(self.show.font_scale())
            .ok_or("shell3-font-physical-size-unavailable")?;
        self.set(
            (width / (cell_width)) as usize,
            (height / (cell_height)) as usize,
        );
        let snapshot = self.capture_update_snapshot();
        let lines = snapshot.rendered_lines();
        let line_refs: Vec<_> = lines.iter().map(|line| line.as_slice()).collect();
        self.show
            .resize_to_current(&line_refs, snapshot.matrix_area())
            .await
    }

    pub(super) fn ui4_resize_needed(&self) -> bool {
        self.show.resize_needed()
    }

    pub fn set(&mut self, columns: usize, rows: usize) {
        self.title_transition.finish();
        self.aka_transition.finish();
        self.columns = columns.max(MIN_COLUMNS);
        self.rows_count = rows.max(MIN_ROWS);
        self.layout_generation = self.layout_generation.wrapping_add(1);
        tui::select(self.tui_frontend(), self.active_matrix_slot_name().as_deref());
    }

    fn tui_frontend(&self) -> tui::Frontend {
        tui::Frontend {
            id: self.tui_frontend,
            cols: self.columns,
            rows: self.rows_count,
        }
    }

    pub fn get_size(&self) -> (usize, usize) {
        (self.columns, self.rows_count)
    }

    pub(super) fn refresh_clock(&mut self) -> bool {
        let Some(tick) = self.clock.take() else {
            return false;
        };
        let minutes = (tick.local_seconds / 60) % (24 * 60);
        let time = alloc::format!("{:02}:{:02}", minutes / 60, minutes % 60);
        if self.time == time {
            return false;
        }
        self.set_time(&time);
        true
    }

    pub fn set_time(&mut self, time: &str) {
        self.time.clear();
        self.time.push_str(time);
        self.rows.title.left = vec![MetaFmtStr::new(title_left_text(&self.time))];
    }

    pub fn set_mode(&mut self, mode: u8) -> bool {
        if self.active_vmx_app().is_some() {
            return false;
        }
        let mode = match mode {
            1 => Mode::HV,
            2 => Mode::CMD,
            3 => Mode::ADM,
            _ => return false,
        };
        if mode != self.mode {
            self.tab_from_right = !self.tab_from_right;
        }
        self.mode = mode;
        self.refresh_mode_title();
        true
    }

    fn refresh_mode_title(&mut self) {
        let target = mode_title_meta(self.mode, &self.aka_names, &self.appdb_names);
        self.title_transition.start(
            &self.rows.title.right,
            &target,
            crate::chronos::monotonic_nanos(),
            self.tab_from_right,
        );
        self.rows.title.right = target;
    }

    pub fn get_mode(&self) -> u8 {
        self.mode as u8
    }

    pub fn select_matrix_slot_index(&mut self, index: usize) -> bool {
        if index == 0 {
            if self.command_feedback.is_some() {
                self.latched_navigation = Some(LatchedNavigation::Default);
                return true;
            }
            if !tui::park(self.tui_frontend) || !tui::select(self.tui_frontend(), None) {
                return false;
            }
            self.active_matrix_slot = None;
            self.active_matrix_lifetime = None;
            self.matrix_selection_dirty = true;
            self.matrix_scroll = 0;
            self.matrix_column = 0;
            self.matrix_drag = None;
            service::notify_work();
            return true;
        }

        let slots = matrix_slots().lock();
        let Some(name) = slots.ids.get(index - 1) else {
            return false;
        };

        let name = name.clone();
        let lifetime = slots
            .lifetimes
            .iter()
            .find(|(id, _)| id == &name)
            .map(|(_, lifetime)| *lifetime);
        drop(slots);
        if self.command_feedback.is_some() {
            self.latched_navigation = Some(LatchedNavigation::Slot {
                name: name.to_string(),
                lifetime,
            });
            return true;
        }
        if !tui::select_for_navigation(self.tui_frontend(), &name) {
            return false;
        }
        self.active_matrix_slot = Some(name);
        self.active_matrix_lifetime = lifetime;
        self.matrix_selection_dirty = true;
        self.matrix_scroll = 0;
        self.matrix_column = 0;
        self.matrix_drag = None;
        service::notify_work();
        true
    }

    pub fn select_matrix_slot_name(&mut self, name: &str) -> bool {
        let slots = matrix_slots().lock();
        if !slots.ids.iter().any(|id| id == name) {
            return false;
        }

        let lifetime = slots
            .lifetimes
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, lifetime)| *lifetime);
        drop(slots);
        if self.command_feedback.is_some() {
            self.latched_navigation = Some(LatchedNavigation::Slot {
                name: name.to_string(),
                lifetime,
            });
            return true;
        }
        if !tui::select_for_navigation(self.tui_frontend(), name) {
            return false;
        }
        self.active_matrix_slot = Some(name.to_string());
        self.active_matrix_lifetime = lifetime;
        self.matrix_selection_dirty = true;
        self.matrix_scroll = 0;
        self.matrix_column = 0;
        self.matrix_drag = None;
        service::notify_work();
        true
    }

    pub fn active_matrix_slot_name(&self) -> Option<String> {
        let active = self.active_matrix_slot.as_deref()?;
        matrix_slots()
            .lock()
            .lifetimes
            .iter()
            .find(|(id, lifetime)| id == active && Some(*lifetime) == self.active_matrix_lifetime)
            .map(|(id, _)| id.clone())
    }

    /// Every owner reconciles deletion locally; no cross-AP model mutation.
    pub(super) fn reconcile_matrix_selection(&mut self) {
        self.finish_command_latch(false);
        let recalled = self.command_history.cursor.is_some();
        if self.command_history.sync() && recalled {
            self.set_prompt("");
        }
        self.sync_aka_names();
        if tui::active(self.tui_frontend, self.active_matrix_slot_name().as_deref()) {
            self.title_transition.finish();
            self.aka_transition.finish();
        }
        if let Some(app) = tui::take_native_launch(self.tui_frontend) {
            self.select_queued_app(app);
        }
        if let Some(destination) = tlb::take_return(self.tui_frontend)
            .or_else(|| font::take_return(self.tui_frontend))
        {
            if let Some(lease) = destination.0
                && matrix_target::matrix_slot_is_live(&lease)
            {
                self.select_matrix_slot_name(lease.name());
            } else {
                self.select_matrix_slot_index(0);
            }
        } else if tui::take_native_return(self.tui_frontend)
            || (self.active_matrix_slot.is_some() && self.active_matrix_slot_name().is_none())
        {
            self.select_matrix_slot_index(0);
        }
    }

    /// Prompt-row input is addressed by the selected app lifetime, not a view.
    fn submit_app_prompt(&mut self) -> bool {
        if self.active_vmx_app().is_none() || self.prompt.text.starts_with(OPERATOR) {
            return false;
        }
        let (Some(name), Some(lifetime)) =
            (self.active_matrix_slot.as_deref(), self.active_matrix_lifetime)
        else {
            return false;
        };
        let Some(vm) = crate::hv::matrix::vm_for_slot(name, lifetime) else {
            return false;
        };
        let Some(run) = crate::hv::vm_run_generation(vm) else {
            return false;
        };
        let lease = MatrixSlotLease::from_identity(name.into(), lifetime);
        let input = format!("{}\n", self.prompt.text);
        if crate::hv::blueprint_console_submit_stdin_for_lease(vm, &lease, run, input.as_bytes())
            != input.len()
        {
            return false;
        }
        MatrixSlots::echo(Some(name), Some(lifetime), self.prompt.text.clone());
        self.command_history.remember(&self.prompt.text);
        self.command_history.end_recall();
        self.set_prompt("");
        true
    }

    /// Outside VMX, Enter is reserved for operator submission.
    fn submit_operator_prompt(&mut self) -> bool {
        let mut input = self.prompt.text.clone();
        if !self.parse_operator(&input) {
            input.zeroize();
            return false;
        }
        self.command_history.remember(&input);
        input.zeroize();
        self.command_history.end_recall();
        self.set_prompt("");
        true
    }

    /// Line transports defer input until Enter, then replay the same events as
    /// UI typing. The first latch consumes the submission; its remaining input
    /// is discarded. Stop early when the prompt cannot become a valid name.
    pub(super) fn replay_terminal_line(&mut self, line: &str) {
        use crate::r::keyboard::*;
        if self.active_vmx_app().is_some() && !line.starts_with(OPERATOR) && !self.parse_name(line)
        {
            self.set_prompt(line);
            self.submit_app_prompt();
            return;
        }
        self.set_prompt("");
        for ch in line.chars() {
            let mut utf8 = [0; 4];
            let utf8_len = ch.encode_utf8(&mut utf8).len() as u8;
            let (changed, latched) = self.handle_keyboard_with_latch(&TrueosKeyboardOutputEvent {
                kind: KEYBOARD_OUTPUT_KIND_TEXT,
                codepoint: ch as u32,
                utf8,
                utf8_len,
                flags: KEYBOARD_OUTPUT_FLAG_PRESS,
                ..Default::default()
            });
            if latched || !changed {
                return;
            }
            if !self.prompt.text.starts_with(OPERATOR)
                && !self.any_name_matching(|name| name.starts_with(&self.prompt.text))
            {
                return;
            }
        }
        self.handle_keyboard(&TrueosKeyboardOutputEvent {
            kind: KEYBOARD_OUTPUT_KIND_KEY,
            key_code: KEYBOARD_KEY_ENTER,
            flags: KEYBOARD_OUTPUT_FLAG_PRESS,
            ..Default::default()
        });
    }

    /// UI editing with immediate exact-name recognition and AppDB/AKA launch.
    pub(super) fn handle_keyboard(
        &mut self,
        event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
    ) -> bool {
        self.handle_keyboard_with_latch(event).0
    }

    /// (Input changed the view or reached a TUI, exact name latched.)
    fn handle_keyboard_with_latch(
        &mut self,
        event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
    ) -> (bool, bool) {
        use crate::r::keyboard::*;
        if event.flags & KEYBOARD_OUTPUT_FLAG_PRESS == 0 {
            return (false, false);
        }
        self.finish_command_latch(true);
        // Tab leaves a VMX view before terminal input routing can consume it.
        // Keep the guest alive and the shell's mode; the next Tab cycles normally.
        if event.kind == KEYBOARD_OUTPUT_KIND_KEY
            && event.key_code == KEYBOARD_KEY_TAB
            && self.active_vmx_app().is_some()
        {
            return (self.select_matrix_slot_index(0), false);
        }
        // A terminal owner receives ordinary text, including the Matrix
        // operator. Shell operators are parsed only while Shell3 owns input.
        if tui::keyboard(self.tui_frontend, self.active_matrix_slot_name().as_deref(), event) {
            return (true, false);
        }
        if event.kind == KEYBOARD_OUTPUT_KIND_KEY {
            match event.key_code {
                KEYBOARD_KEY_ARROW_UP | KEYBOARD_KEY_ARROW_DOWN => {
                    let Some(text) = self
                        .command_history
                        .recall(event.key_code == KEYBOARD_KEY_ARROW_UP, &self.prompt.text)
                    else {
                        return (false, false);
                    };
                    self.set_prompt(&text);
                    self.set_cursor(text.chars().count());
                    return (true, false);
                }
                KEYBOARD_KEY_ENTER => {
                    if self.command_history.cursor.is_some() {
                        let input = self.prompt.text.clone();
                        self.command_history.end_recall();
                        self.replay_terminal_line(&input);
                        return (true, true);
                    }
                    let submitted = self.submit_app_prompt() || self.submit_operator_prompt();
                    return (submitted, false);
                }
                KEYBOARD_KEY_TAB => {
                    let changed = self.set_mode(self.get_mode() % 3 + 1);
                    let latched = changed && self.latch_prompt_prefix();
                    return (changed, latched);
                }
                KEYBOARD_KEY_BACKSPACE => {
                    self.command_history.end_recall();
                    if self.prompt.cursor == 0 {
                        return (false, false);
                    }
                    let index = self.prompt.cursor - 1;
                    let byte = self.prompt.text.char_indices().nth(index).unwrap().0;
                    self.prompt.text.remove(byte);
                    if index < self.prompt.colors.len() {
                        self.prompt.colors.remove(index);
                    }
                    self.prompt.cursor = index;
                    self.refresh_prompt_strip();
                    return (true, false);
                }
                _ => return (false, false),
            }
        }
        if event.kind != KEYBOARD_OUTPUT_KIND_TEXT {
            return (false, false);
        }
        let Some(ch) = char::from_u32(event.codepoint).filter(|ch| !ch.is_control()) else {
            return (false, false);
        };
        self.command_history.end_recall();
        let right_len = self
            .rows
            .promt
            .right
            .iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>();
        // Keep all typed glyphs and the cursor visible beside the right strip.
        if self.prompt.char_len() + 1 >= self.columns.saturating_sub(right_len) {
            return (false, false);
        }
        let byte = self
            .prompt
            .text
            .char_indices()
            .nth(self.prompt.cursor)
            .map(|(byte, _)| byte)
            .unwrap_or(self.prompt.text.len());
        self.prompt.text.insert(byte, ch);
        self.prompt.colors.resize(self.prompt.char_len() - 1, None);
        self.prompt.colors.insert(self.prompt.cursor, None);
        self.prompt.cursor += 1;
        let latched = self.echo_recognized_prompt();
        self.refresh_prompt_strip();
        (true, latched)
    }

    fn launch_named_app(&mut self, text: &str, is_alias: bool) {
        let result = if !is_alias {
            service::launch_appdb(
                if text == "AppDB" { "apps" } else { text },
                self.tui_frontend(),
            )
        } else {
            service::launch_alias(text, self.tui_frontend())
        };
        match result {
            Ok(app) => self.select_queued_app(app),
            Err(error) => MatrixSlots::echo(
                self.active_matrix_slot.as_deref(),
                self.active_matrix_lifetime,
                error,
            ),
        }
    }

    fn finish_command_latch(&mut self, force: bool) {
        let now = crate::chronos::monotonic_nanos();
        if !self
            .command_feedback
            .as_ref()
            .is_some_and(|feedback| force || feedback.finished(now))
        {
            return;
        }
        self.command_feedback = None;
        tui::hold_navigation(self.tui_frontend, false);
        match self.latched_navigation.take() {
            Some(LatchedNavigation::Default) => {
                self.select_matrix_slot_index(0);
            }
            Some(LatchedNavigation::Slot { name, lifetime }) => {
                // Do not navigate into a different slot that reused this name.
                let live = matrix_slots()
                    .lock()
                    .lifetimes
                    .iter()
                    .any(|(id, generation)| id == &name && Some(*generation) == lifetime);
                if live {
                    self.select_matrix_slot_name(&name);
                }
            }
            Some(LatchedNavigation::Native { name, lifetime }) => {
                let live = matrix_slots()
                    .lock()
                    .lifetimes
                    .iter()
                    .any(|(id, generation)| id == &name && Some(*generation) == lifetime);
                if live {
                    self.enter_native_terminal(&name);
                }
            }
            Some(LatchedNavigation::App { app, lifetime }) => {
                let live = matrix_slots()
                    .lock()
                    .lifetimes
                    .iter()
                    .any(|(name, generation)| name == &app.slot && *generation == lifetime);
                if live {
                    self.select_queued_app(app);
                }
            }
            Some(LatchedNavigation::Resume { name, lifetime }) => {
                if self.active_matrix_slot.as_deref() == Some(&name)
                    && self.active_matrix_lifetime == lifetime
                {
                    if let Err(error) = tui::request_takeover(self.tui_frontend(), &name) {
                        MatrixSlots::echo(Some(&name), lifetime, error.into());
                    }
                }
            }
            None => {}
        }
        self.matrix_selection_dirty = true;
        service::notify_work();
    }

    // Starting a helper is an explicit entry action; selecting its retained
    // slot later uses the command pane until the user requests `tui`.
    fn enter_native_terminal(&mut self, name: &str) {
        let lifetime = matrix_slots()
            .lock()
            .lifetimes
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, lifetime)| *lifetime);
        if self.command_feedback.is_some() {
            self.latched_navigation = Some(LatchedNavigation::Native {
                name: name.into(),
                lifetime,
            });
        } else if self.select_matrix_slot_name(name) {
            if let Err(error) = tui::request(self.tui_frontend(), name) {
                MatrixSlots::echo(Some(name), lifetime, error.into());
            }
        }
    }

    fn select_queued_app(&mut self, app: service::QueuedBlueprint) {
        if crate::hv::matrix::app(&app.slot, Some(app.lifetime)).is_none() {
            return;
        }
        if self.command_feedback.is_some() {
            let lifetime = app.lifetime;
            self.latched_navigation = Some(LatchedNavigation::App { app, lifetime });
            service::notify_work();
            return;
        }
        if !tui::select(self.tui_frontend(), Some(&app.slot)) {
            tui::queue_launch(self.tui_frontend, app);
            return;
        }
        let lifetime = app.lifetime;
        self.active_matrix_slot = Some(app.slot.clone());
        self.active_matrix_lifetime = Some(lifetime);
        self.matrix_selection_dirty = true;
        self.matrix_scroll = 0;
        self.matrix_column = 0;
        self.matrix_drag = None;
        service::notify_work();
    }

    /// A mode switch can make an existing prefix executable. Consume only
    /// the first recognized name; trailing input never reaches echo or history.
    fn latch_prompt_prefix(&mut self) -> bool {
        self.sync_aka_names();
        if self.prompt().starts_with(OPERATOR) {
            return false;
        }
        let matched_end = self
            .prompt()
            .char_indices()
            .map(|(index, ch)| index + ch.len_utf8())
            .find(|&end| self.parse_name(&self.prompt()[..end]));
        let Some(end) = matched_end else {
            return false;
        };
        let name = self.prompt()[..end].to_string();
        self.set_prompt(&name);
        let latched = self.echo_recognized_prompt();
        self.refresh_prompt_strip();
        latched
    }

    fn echo_recognized_prompt(&mut self) -> bool {
        self.sync_aka_names();
        if self.prompt.text.starts_with(OPERATOR) || !self.parse_name(&self.prompt.text) {
            return false;
        }
        let mut text = core::mem::take(&mut self.prompt.text);
        self.command_history.remember(&text);
        self.command_history.end_recall();
        self.command_feedback = Some(command_latch::Feedback::new(
            &text,
            crate::chronos::monotonic_nanos(),
            self.tab_from_right,
        ));
        self.latched_navigation = None;
        tui::hold_navigation(self.tui_frontend, true);
        service::notify_work();
        let can_launch = self.mode == Mode::CMD && self.active_vmx_app().is_none();
        let is_app = can_launch
            && (text == "AppDB" || self.appdb_names.iter().any(|name| name == &text));
        let is_alias = self.aka_names.iter().any(|name| name == &text);
        if self.mode == Mode::ADM && text == "log" {
            match system_log::start(self.tui_frontend()) {
                Ok(()) => {
                    self.enter_native_terminal(&text);
                }
                Err(error) => MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    error,
                ),
            }
        } else if text == "env" && (self.mode == Mode::ADM || self.active_vmx_app().is_some()) {
            for line in service::environment_lines(
                self.active_matrix_slot.as_deref(),
                self.active_matrix_lifetime,
            ) {
                MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    line,
                );
            }
        } else if self.mode == Mode::ADM
            && matches!(
                text.as_str(),
                "cry" | "disc" | "ram" | "smp" | "net" | "gpu" | "vcpy" | "tlb" | "font"
            )
        {
            let frontend = self.tui_frontend();
            let result = match text.as_str() {
                "cry" => cry::start(frontend),
                "disc" => disc::start(frontend),
                "ram" => ram::start(frontend),
                "smp" => smp::start(frontend),
                "net" => system_net::start(frontend),
                "gpu" => gpu::start(frontend),
                "vcpy" => vcpy::start(frontend),
                "font" => font::start(frontend),
                _ => {
                    let origin = output::DumpTarget(
                        self.active_matrix_slot
                            .clone()
                            .zip(self.active_matrix_lifetime)
                            .map(|(name, lifetime)| MatrixSlotLease::from_identity(name, lifetime)),
                    );
                    tlb::start(frontend, origin)
                }
            };
            match result {
                Ok(()) => {
                    self.enter_native_terminal(&text);
                }
                Err(error) => MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    error,
                ),
            }
        } else if self.mode == Mode::ADM && text == "sh3" {
            if let Err(error) = service::request_shell3() {
                let message = match error {
                    Shell3Error::NoExecutor => "sh3: no Shell3 AP executor is available".into(),
                    Shell3Error::InstanceLimit => "sh3: Shell3 instance limit reached".into(),
                    other => format!("sh3: {other:?}"),
                };
                MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    message,
                );
            }
        } else if self.mode == Mode::HV && text == "status" {
            for line in service::hv_status_lines() {
                MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    line,
                );
            }
        } else if self.mode == Mode::HV && matches!(text.as_str(), "peer" | "probe") {
            let lines = if text == "peer" {
                service::peer_lines()
            } else {
                service::probe_lines()
            };
            for line in lines {
                MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    line,
                );
            }
        } else if self.mode == Mode::HV
            && self.active_vmx_app().is_none()
            && lifecycle::recognizes(&text)
        {
            match lifecycle::start(&text, self.tui_frontend()) {
                Ok(()) => {
                    self.enter_native_terminal(&text);
                }
                Err(error) => MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    error,
                ),
            }
        } else if text == "Aka" {
            if let Err(error) = service::edit_aka(self.tui_frontend()) {
                MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    error,
                );
            }
        } else if self.mode == Mode::ADM && capture::recognizes(&text) {
            match capture::start(&text, self.tui_frontend()) {
                Ok(()) => {
                    self.enter_native_terminal(&text);
                }
                Err(error) => MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    error,
                ),
            }
        } else if is_alias {
            self.launch_named_app(&text, true);
        } else if text == "stop" && self.active_vmx_app().is_some() {
            self.stop_active_vmx();
        } else if lifecycle::recognizes(&text) && self.active_vmx_app().is_some() {
            if let Some(app) = self.active_vmx_app() {
                let lease = MatrixSlotLease::from_identity(app.slot.clone(), app.lifetime);
                let message = lifecycle::control(&text, &lease);
                MatrixSlots::echo(Some(&app.slot), Some(app.lifetime), message);
            }
        } else if text == "tui"
            && (self.active_vmx_app().is_some()
                || self
                    .active_matrix_slot_name()
                    .is_some_and(|name| tui::native_slot(&name)))
        {
            self.latched_navigation = Some(LatchedNavigation::Resume {
                name: self.active_matrix_slot.clone().unwrap_or_default(),
                lifetime: self.active_matrix_lifetime,
            });
        } else if text == "esc" && self.active_vmx_app().is_some() {
            self.select_matrix_slot_index(0);
        } else if is_app {
            self.launch_named_app(&text, false);
        } else {
            MatrixSlots::echo(
                self.active_matrix_slot.as_deref(),
                self.active_matrix_lifetime,
                text.clone(),
            );
        }
        text.zeroize();
        self.prompt.colors.clear();
        self.prompt.cursor = 0;
        true
    }

    pub(super) fn matrix_output_needed(&self) -> bool {
        self.aka_generation != crate::r::startup_config::generation()
            || self.appdb_generation != service::appdb_names_snapshot().0
            || self.matrix_selection_dirty
            || self
                .update_baseline
                .blink_phase()
                .is_some_and(|phase| phase != Self::cursor_blink_phase())
            || tui::revision(self.tui_frontend) != self.update_baseline.tui_revision()
            || (!self.update_baseline.terminal_active()
                && matrix_slots().lock().generation != self.update_baseline.matrix_generation())
            || (!self.update_baseline.terminal_active()
                && !self
                    .update_baseline
                    .controls_match(&self.capture_controls_snapshot()))
    }

    pub fn set_prompt(&mut self, text: &str) {
        self.prompt.text.zeroize();
        self.prompt.text.push_str(text);
        self.prompt.colors.clear();
        let text_len = self.prompt.char_len();
        self.prompt.cursor = self.prompt.cursor.min(text_len);
        self.refresh_prompt_strip();
    }

    pub fn prompt(&self) -> &str {
        &self.prompt.text
    }

    pub fn set_cursor(&mut self, index: usize) -> bool {
        if index > self.prompt.char_len() {
            return false;
        }

        self.prompt.cursor = index;
        self.refresh_prompt_strip();
        true
    }

    fn refresh_prompt_strip(&mut self) {
        let mut colors = self.prompt.colors.clone();
        colors.resize(self.prompt.char_len(), None);
        colors.insert(self.prompt.cursor, None);
        self.rows.promt.left = self
            .prompt
            .render()
            .chars()
            .zip(colors)
            .enumerate()
            .map(|(index, (ch, color))| MetaFmtStr {
                text: ch.to_string(),
                color: if index == self.prompt.cursor {
                    Some(RgbaColor::Terminal {
                        foreground: [0, 0, 0, 255],
                        background: RgbaColor::White.rgba(),
                        underline: false,
                    })
                } else {
                    color
                },
                bold: false,
                underline: false,
                blink: index == self.prompt.cursor,
            })
            .collect();
    }

    pub fn get_strip(&self, row: SpecialRows, side: StripSide) -> String {
        if let SpecialRows::MatrixRow(index) = row {
            if side != StripSide::Left {
                return String::new();
            }
            let (_, lines) = MatrixSlots::view_echo_snapshot(
                self.active_matrix_slot.as_deref(),
                self.active_matrix_lifetime,
            );
            let offset = self.matrix_scroll.min(
                lines
                    .len()
                    .saturating_sub(self.rows_count.saturating_sub(3)),
            );
            return lines
                .get(offset.saturating_add(index))
                .map(|line| line.chars().skip(self.matrix_column).collect())
                .unwrap_or_default();
        }
        if row == SpecialRows::StatusRow && side == StripSide::Left {
            return current_matrix_slots_text();
        }

        let strip = self.row_for_render(row);
        strip.left.iter().map(|run| run.text.as_str()).collect()
    }

    fn row_for_render(&self, row: SpecialRows) -> RowStrips {
        if matches!(row, SpecialRows::MatrixRow(_)) {
            return RowStrips::new(&self.get_strip(row, StripSide::Left), "");
        }
        let mut strips = self.rows.row(row).clone();
        if row == SpecialRows::TitleRow {
            // The title operator is the permanent default-slot control.
            let mut column = 0;
            strips.left = strips
                .left
                .into_iter()
                .flat_map(|run| {
                    let mut parts = Vec::new();
                    for ch in run.text.chars() {
                        let mut part = run.clone();
                        part.text = ch.to_string();
                        if column == 7 && ch == OPERATOR {
                            part.color = Some(if self.active_matrix_slot_name().is_none() {
                                RgbaColor::Pink
                            } else {
                                RgbaColor::White
                            });
                            part.bold = true;
                            part.underline = self.status_hover == Some(status::Target::Default);
                        }
                        column += 1;
                        parts.push(part);
                    }
                    parts
                })
                .collect();
            if let Some(app) = self.active_vmx_app() {
                strips
                    .left
                    .push(MetaFmtStr::new(format!(" {}", app.app)).bold());
                strips
                    .left
                    .push(MetaFmtStr::new(format!(" {}", vmx_hash_text(&app.sha256))));
                strips.right = vmx_title_meta();
            }
        }
        if row == SpecialRows::PromtRow {
            if let Some(feedback) = &self.command_feedback {
                strips.left = feedback.runs(self.columns, crate::chronos::monotonic_nanos());
                strips.right.clear();
            }
        }
        if row == SpecialRows::StatusRow {
            let active = self.active_matrix_slot_name();
            let working = service::working_vmx_slots();
            let background_active = MatrixSlots::background_active_slots();
            let slots = matrix_slots().lock();
            strips = status::runs_with_activity(
                &slots.ids,
                active.as_deref(),
                &self.aka_names,
                self.status_hover.as_ref(),
                &working,
                &background_active,
            );
        }
        strips
    }

    /// Move this view through the shared newest-first transcript. Positive
    /// rows move down toward older entries; each Shell3 owns its own offset.
    pub(super) fn scroll_matrix(&mut self, rows: i32) -> bool {
        self.pan_matrix(0, rows)
    }

    pub(super) fn pan_matrix(&mut self, columns: i32, rows: i32) -> bool {
        let (_, lines) = MatrixSlots::view_echo_snapshot(
            self.active_matrix_slot.as_deref(),
            self.active_matrix_lifetime,
        );
        let maximum = lines
            .len()
            .saturating_sub(self.rows_count.saturating_sub(3));
        let previous = self.matrix_scroll.min(maximum);
        self.matrix_scroll = if rows >= 0 {
            previous.saturating_add(rows as usize).min(maximum)
        } else {
            previous.saturating_sub(rows.unsigned_abs() as usize)
        };
        let maximum_column = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0)
            .saturating_sub(self.columns);
        let previous_column = self.matrix_column.min(maximum_column);
        self.matrix_column = if columns >= 0 {
            previous_column
                .saturating_add(columns as usize)
                .min(maximum_column)
        } else {
            previous_column.saturating_sub(columns.unsigned_abs() as usize)
        };
        self.matrix_scroll != previous || self.matrix_column != previous_column
    }

    /// Grab the transcript in cell-sized steps, retaining sub-cell movement.
    pub(super) fn drag_matrix(
        &mut self,
        position: (i32, i32),
        pressed: bool,
        held: bool,
        inside: bool,
        cell: (i32, i32),
    ) -> bool {
        if !held || cell.0 <= 0 || cell.1 <= 0 {
            self.matrix_drag = None;
            return false;
        }
        if pressed && inside {
            self.matrix_drag = Some(position);
        }
        let Some(anchor) = self.matrix_drag else {
            return false;
        };
        let columns = (anchor.0 as i64 - position.0 as i64) / cell.0 as i64;
        let rows = (anchor.1 as i64 - position.1 as i64) / cell.1 as i64;
        if columns == 0 && rows == 0 {
            return false;
        }
        self.matrix_drag = Some((
            (anchor.0 as i64 - columns * cell.0 as i64) as i32,
            (anchor.1 as i64 - rows * cell.1 as i64) as i32,
        ));
        self.pan_matrix(
            columns.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            rows.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        )
    }

    fn capture_matrix_snapshot(&self) -> update::Snapshot {
        let (generation, lines) = MatrixSlots::view_echo_snapshot(
            self.active_matrix_slot.as_deref(),
            self.active_matrix_lifetime,
        );
        self.capture_controls_snapshot()
            .with_matrix_pan(
                &lines,
                generation,
                self.matrix_column,
                self.matrix_scroll,
                crate::allcaps::shell3::PANBUFFER_GUARD_CELLS as usize,
            )
            .with_matrix_identity(self.active_matrix_slot.clone(), self.active_matrix_lifetime)
    }

    fn record_terminal_notice(&mut self, text: &str) {
        for line in text.lines().rev() {
            MatrixSlots::echo(
                self.active_matrix_slot.as_deref(),
                self.active_matrix_lifetime,
                line.into(),
            );
        }
    }

    // Shared character composition boundary, before either pixel or ANSI drawing.
    fn capture_controls_snapshot(&self) -> update::Snapshot {
        let title = self.row_for_render(SpecialRows::TitleRow);
        let status = self.row_for_render(SpecialRows::StatusRow);
        let promt = self.row_for_render(SpecialRows::PromtRow);
        let snapshot = update::Snapshot::new(
            (self.columns, self.rows_count),
            self.layout_generation,
            [
                (&title.left, &title.right),
                (&status.left, &status.right),
                (&promt.left, &promt.right),
            ],
            self.columns,
        );
        let now = crate::chronos::monotonic_nanos();
        let snapshot = if self.active_vmx_app().is_none() {
            snapshot.with_reveal(0, &title.left, self.title_transition.frame(now))
        } else {
            snapshot
        };
        snapshot.with_reveal(1, &status.left, self.aka_transition.frame(now))
    }

    fn cursor_blink_phase() -> bool {
        trueos_time::Instant::now().as_millis() / 500 % 2 == 0
    }

    fn capture_update_snapshot(&self) -> update::Snapshot {
        let revision = tui::revision(self.tui_frontend);
        // Admission owns input immediately; pixels change only once the helper
        // has a complete first paint. Keep this window's current frame meanwhile.
        if tui::native_pending_frame(self.tui_frontend, self.active_matrix_slot_name().as_deref())
            && self.update_baseline.size() == (self.columns, self.rows_count)
        {
            return self.update_baseline.clone().with_tui_revision(revision);
        }
        if let Some(lines) =
            tui::snapshot(self.tui_frontend, self.active_matrix_slot_name().as_deref())
        {
            return update::Snapshot::terminal(
                (self.columns, self.rows_count),
                self.layout_generation,
                lines,
                revision,
            );
        }
        self.capture_matrix_snapshot()
            .with_tui_revision(revision)
            .with_blink_phase(Self::cursor_blink_phase())
    }

    fn sync_aka_names(&mut self) {
        let (appdb_generation, appdb_names) = service::appdb_names_snapshot();
        if self.appdb_generation != appdb_generation {
            self.set_appdb_names(&appdb_names);
            self.appdb_generation = appdb_generation;
        }
        let generation = crate::r::startup_config::generation();
        if self.aka_generation != generation {
            self.aka_names = crate::r::restart::startup_alias_names();
            self.aka_generation = generation;
            let target = status::alias_runs(&self.aka_names);
            self.aka_transition.start(
                &self.rows.status.right,
                &target,
                crate::chronos::monotonic_nanos(),
                self.tab_from_right,
            );
            self.rows.status.right = target;
            self.refresh_mode_title();
        }
    }

    pub fn set_appdb_names(&mut self, names: &[String]) {
        self.appdb_names.clear();
        self.appdb_names.extend(names.iter().cloned());
        self.refresh_mode_title();
    }

    /// Operator contract: §, §id, §id§, §§, and §§Appname, submitted by Enter.
    pub fn parse_operator(&mut self, input: &str) -> bool {
        if let Some(name) = names::fastspawn_selector(input) {
            match service::launch_appstore(name, self.tui_frontend()) {
                Ok(app) => self.select_queued_app(app),
                Err(error) => MatrixSlots::echo(
                    self.active_matrix_slot.as_deref(),
                    self.active_matrix_lifetime,
                    error,
                ),
            }
            return true;
        }
        let Some(rest) = input.strip_prefix(OPERATOR) else {
            return false;
        };
        if rest.is_empty() {
            return self.select_matrix_slot_index(0);
        }
        if rest == "§" {
            MatrixSlots::drop_slot(None);
            return true;
        }
        let (name, dropping) = match rest.strip_suffix(OPERATOR) {
            Some(name) => (name, true),
            None => (rest, false),
        };
        // Slot ids are a single token; embedded operators are not patterns.
        if name.is_empty()
            || name
                .chars()
                .any(|ch| ch == OPERATOR || ch.is_whitespace() || ch.is_control())
        {
            return false;
        }
        if dropping {
            if !MatrixSlots::drop_slot(Some(name)) {
                return false;
            }
            self.reconcile_matrix_selection();
            return true;
        }
        if !tui::park(self.tui_frontend) {
            return false;
        }
        let lifetime = MatrixSlots::ensure_named(name);
        if self.command_feedback.is_some() {
            self.latched_navigation = Some(LatchedNavigation::Slot {
                name: name.to_string(),
                lifetime: Some(lifetime),
            });
            return true;
        }
        self.active_matrix_slot = Some(name.to_string());
        self.active_matrix_lifetime = Some(lifetime);
        self.matrix_selection_dirty = true;
        self.matrix_scroll = 0;
        self.matrix_column = 0;
        self.matrix_drag = None;
        tui::select_for_navigation(self.tui_frontend(), name);
        service::notify_work();
        true
    }

    pub(super) fn stop_active_vmx(&mut self) -> bool {
        let Some(app) = self.active_vmx_app() else {
            return false;
        };
        let lease = MatrixSlotLease::from_identity(app.slot.clone(), app.lifetime);
        MatrixSlots::echo(Some(&app.slot), Some(app.lifetime), lifecycle::control("stop", &lease));
        true
    }

    fn active_vmx_app(&self) -> Option<service::QueuedBlueprint> {
        let active = self.active_matrix_slot_name()?;
        crate::hv::matrix::app(&active, self.active_matrix_lifetime)
    }

    pub fn parse_name(&self, name: &str) -> bool {
        self.any_name_matching(|candidate| candidate == name)
    }

    /// Exact recognition and prefix viability consult the same live registry.
    fn any_name_matching(&self, matches: impl Fn(&str) -> bool) -> bool {
        // Aka remains visible and available independently of Tab/VM context.
        if matches("Aka") || self.aka_names.iter().any(|alias| matches(alias)) {
            return true;
        }
        if matches("tui")
            && self
                .active_matrix_slot_name()
                .is_some_and(|name| tui::native_slot(&name))
        {
            return true;
        }
        if self.active_vmx_app().is_some() {
            return names::VME_GROUP
                .names
                .iter()
                .any(|entry| matches(entry.name));
        }
        match self.mode {
            Mode::HV => HV_GROUPS
                .iter()
                .any(|group| group.names.iter().any(|entry| matches(entry.name))),
            Mode::CMD => {
                CMD_GROUPS
                    .iter()
                    .any(|group| {
                        matches(group.name)
                            || group.names.iter().any(|entry| matches(entry.name))
                    })
                    || self.appdb_names.iter().any(|entry| matches(entry))
            }
            Mode::ADM => ADM_GROUPS
                .iter()
                .any(|group| group.names.iter().any(|entry| matches(entry.name))),
        }
    }
}

/// The mode legend and exact-name recognizer use the same three registries.
fn mode_title_meta(mode: Mode, _aka_names: &[String], appdb_names: &[String]) -> Vec<MetaFmtStr> {
    let mut runs = Vec::new();
    let mut append_group = |group: &names::NameGroup, dynamic: Option<&[String]>| {
        if !runs.is_empty() {
            runs.push(MetaFmtStr::new(" "));
        }
        if !group.name.is_empty() {
            runs.push(MetaFmtStr::new(group.name).color(RgbaColor::Cyan));
        }
        runs.push(MetaFmtStr::new(names::GROUP_OPEN.to_string()));
        for (index, entry) in group.names.iter().enumerate() {
            if index != 0 {
                runs.push(MetaFmtStr::new(" "));
            }
            runs.push(MetaFmtStr::new(entry.name).color(entry.color));
        }
        if let Some(entries) = dynamic {
            // The AppDB group name launches the store itself.
            for (index, entry) in entries
                .iter()
                .filter(|entry| entry.as_str() != "apps")
                .enumerate()
            {
                if !group.names.is_empty() || index != 0 {
                    runs.push(MetaFmtStr::new(" "));
                }
                runs.push(MetaFmtStr::new(entry));
            }
        }
        runs.push(MetaFmtStr::new(names::GROUP_CLOSE.to_string()));
    };
    match mode {
        Mode::HV => {
            for group in &HV_GROUPS {
                append_group(group, None);
            }
        }
        Mode::CMD => {
            for group in &CMD_GROUPS {
                if group.name == "Aka" {
                    continue;
                }
                let dynamic = match group.name {
                    "AppDB" => Some(appdb_names),
                    _ => None,
                };
                append_group(group, dynamic);
            }
        }
        Mode::ADM => {
            for group in &names::ADM_GROUPS {
                append_group(group, None);
            }
        }
    }
    runs
}

fn vmx_hash_text(hash: &[u8; 32]) -> String {
    use core::fmt::Write;
    let mut text = String::with_capacity(35);
    for byte in &hash[..8] {
        let _ = write!(text, "{byte:02x}");
    }
    text.push('…');
    for byte in &hash[24..] {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn vmx_title_meta() -> Vec<MetaFmtStr> {
    let group = &names::VME_GROUP;
    let mut runs = vec![MetaFmtStr::new(group.name).color(RgbaColor::Cyan).bold()];
    for entry in group.names {
        runs.push(MetaFmtStr::new(" "));
        runs.push(MetaFmtStr::new(entry.name).color(entry.color));
    }
    runs
}
