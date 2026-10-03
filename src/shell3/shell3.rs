mod names;
mod metafmtstr;
mod update;
mod tty;
pub mod net;

pub mod service;
#[path = "show/show.rs"]
pub mod show;

pub use metafmtstr::MetaFmtStr;
pub use show::{Backend as ShowBackend, Show};
pub use update::{SegmentUpdate, UpdateBatch, UpdateCallback};

use names::{ADM_NAMES, CMD_GROUPS, HV_GROUPS};
use alloc::{string::{String, ToString}, collections::VecDeque, vec, vec::Vec};
use spin::Once;

pub const MAX_SHELL3_INSTANCES: usize = 256;
pub const OPERATOR: char = '§';
pub const MODESTEP: char = '\t';
pub const PROMPT_CURSOR: char = '#';
pub const Default_COLUMNS: usize = 100;
pub const Default_ROWS: usize = 25;
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
    Gray = 0x808080FF,
    White = 0xFFFFFFFF,
    Pink = 0xFF69B4FF,
    Blue = 0x4285F4FF,
    Green = 0x34A853FF,
    Orange = 0xFB8C00FF,
}

impl RgbaColor {
    pub const fn rgba(self) -> [u8; 4] {
        let value = self as u32;
        [
            ((value >> 24) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        ]
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
pub const SpecialSeperator: char = '│';

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripSide {
    Left = 1,
    Right = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell3Error {
    InstanceLimit,
    NoExecutor,
    WrongExecutor { expected: u32, actual: u32 },
}

pub fn live_shell3_instances() -> usize {
    service::live_shell_count()
}

pub struct TitleTime;

impl TitleTime {
    /// Capture the current civil time for a shell's startup title.
    pub fn current() -> String {
        let utc_seconds = crate::chronos::best_effort_unix_time_seconds()
            .unwrap_or_else(crate::time::uptime_seconds);
        let local_seconds = crate::locale::local_unix_time_seconds(utc_seconds);
        let minutes_of_day = (local_seconds / 60) % (24 * 60);
        alloc::format!("{:02}:{:02}", minutes_of_day / 60, minutes_of_day % 60)
    }

    pub fn set(shell: &mut Shell3, time: &str) {
        shell.set_time(time);
    }

    pub fn get(shell: &Shell3) -> &str {
        shell.time()
    }
}

fn title_left_text(time: &str) -> String {
    format!("TrueOS {} {}", OPERATOR, time)
}

pub struct MatrixSlots;

#[derive(Clone, Debug)]
struct MatrixSlotsState {
    // Raw unique slot IDs only. The default bare § slot is implicit at index 0.
    ids: Vec<String>,
    lifetimes: Vec<(String, u64)>,
    next_lifetime: u64,
    echoes: Vec<(Option<String>, VecDeque<String>)>,
    generation: u64,
}

impl MatrixSlotsState {
    fn new() -> Self {
        Self {
            ids: vec!["id".to_string(), "123".to_string()],
            lifetimes: vec![("id".to_string(), 1), ("123".to_string(), 2)],
            next_lifetime: 3,
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
    let default_color = if active.is_none() { RgbaColor::Pink } else { RgbaColor::White };
    runs.push(MetaFmtStr::new(OPERATOR.to_string()).color(default_color).bold());
    for id in ids {
        let color = if active == Some(id.as_str()) { RgbaColor::Pink } else { RgbaColor::White };
        runs.push(MetaFmtStr::new(" ").color(RgbaColor::White));
        runs.push(MetaFmtStr::new(OPERATOR.to_string()).color(color).bold());
        runs.push(MetaFmtStr::new(id.clone()).color(color));
    }
    runs
}

fn matrix_slots_text(ids: &[String]) -> String {
    let mut text = String::new();
    text.push(OPERATOR);
    for id in ids {
        text.push(' ');
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
    pub const DEFAULT: &'static str = "§ §id §123";

    /// Shared across every Shell3. Names are supplied without the § prefix.
    pub fn set<T: AsRef<str>>(names: &[T]) {
        let mut slots = matrix_slots().lock();
        let mut ids = Vec::with_capacity(names.len());

        for name in names {
            let name = name.as_ref();
            if !ids.iter().any(|existing: &String| existing == name) {
                ids.push(name.to_string());
            }
        }

        slots.lifetimes.retain(|(id, _)| ids.contains(id));
        for id in &ids {
            if !slots.lifetimes.iter().any(|(name, _)| name == id) {
                let lifetime = slots.next_lifetime;
                slots.next_lifetime = slots.next_lifetime.wrapping_add(1);
                slots.lifetimes.push((id.clone(), lifetime));
            }
        }
        slots.echoes.retain(|(id, _)| id.as_ref().is_none_or(|id| ids.contains(id)));
        slots.ids = ids;
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
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

    /// None resets the implicit default slot; named slots disappear entirely.
    fn drop_slot(name: Option<&str>) -> bool {
        let mut slots = matrix_slots().lock();
        if let Some(name) = name {
            if !slots.ids.iter().any(|id| id == name) { return false; }
            slots.ids.retain(|id| id != name);
            slots.lifetimes.retain(|(id, _)| id != name);
        }
        slots.echoes.retain(|(id, _)| id.as_deref() != name);
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
        true
    }

    /// Echo text into a named Matrix slot (None is the bare § slot).
    /// Slot selection remains per instance; transcripts are shared Matrix data.
    fn echo(active: Option<&str>, lifetime: Option<u64>, text: String) {
        let mut slots = matrix_slots().lock();
        let active = active.filter(|id| slots.lifetimes.iter().any(|(name, current)| name == id && Some(*current) == lifetime)).map(str::to_string);
        let index = slots.echoes.iter().position(|(id, _)| *id == active).unwrap_or_else(|| {
            slots.echoes.push((active, VecDeque::new()));
            slots.echoes.len() - 1
        });
        let lines = &mut slots.echoes[index].1;
        if lines.len() == 256 { lines.pop_front(); }
        lines.push_back(text);
        slots.generation = slots.generation.wrapping_add(1);
        drop(slots);
        service::notify_work();
    }

    /// Transcript of one Matrix slot, independent of any shell's selection.
    pub fn echo_lines(active: Option<&str>) -> Vec<String> {
        Self::echo_snapshot(active).1
    }

    fn echo_snapshot(active: Option<&str>) -> (u64, Vec<String>) {
        let slots = matrix_slots().lock();
        let active = active.filter(|id| slots.ids.iter().any(|name| name == id));
        let lines = slots.echoes.iter().find(|(id, _)| id.as_deref() == active)
            .map(|(_, lines)| lines.iter().cloned().collect()).unwrap_or_default();
        (slots.generation, lines)
    }

    fn view_echo_snapshot(active: Option<&str>, lifetime: Option<u64>) -> (u64, Vec<String>) {
        let slots = matrix_slots().lock();
        let active = active.filter(|id| slots.lifetimes.iter().any(|(name, current)| name == id && Some(*current) == lifetime));
        let lines = slots.echoes.iter().find(|(id, _)| id.as_deref() == active)
            .map(|(_, lines)| lines.iter().cloned().collect()).unwrap_or_default();
        (slots.generation, lines)
    }

    pub fn slot_ids() -> Vec<String> {
        matrix_slots().lock().ids.clone()
    }

    pub fn get() -> String {
        current_matrix_slots_text()
    }

    /// Returns the slot strip as text runs carrying style metadata.
    pub fn formatted() -> Vec<MetaFmtStr> {
        let slots = matrix_slots().lock();
        matrix_slots_meta(&slots.ids, None)
    }

    // Active selection is per shell, not shared.
    pub fn select_index(shell: &mut Shell3, index: usize) -> bool {
        shell.select_matrix_slot_index(index)
    }

    pub fn select_name(shell: &mut Shell3, name: &str) -> bool {
        shell.select_matrix_slot_name(name)
    }

    pub fn active_index(shell: &Shell3) -> usize {
        shell.active_matrix_slot_index()
    }

    pub fn active_name(shell: &Shell3) -> Option<String> {
        shell.active_matrix_slot_name()
    }
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
            promt: RowStrips::new(prompt_left, ""),
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

    fn row_mut(&mut self, row: SpecialRows) -> &mut RowStrips {
        match row {
            SpecialRows::TitleRow => &mut self.title,
            SpecialRows::StatusRow => &mut self.status,
            SpecialRows::PromtRow => &mut self.promt,
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

pub struct Shell3 {
    executor_slot: u32,
    columns: usize,
    rows_count: usize,
    layout_generation: usize,
    time: String,
    mode: Mode,
    active_matrix_slot: Option<String>,
    active_matrix_lifetime: Option<u64>,
    matrix_selection_dirty: bool,
    prompt: PromptState,
    rows: SpecialRowsState,
    aka_names: Vec<String>,
    appdb_names: Vec<String>,
    update_callbacks: Vec<UpdateCallback>,
    update_baseline: update::Snapshot,
    pending_presentation: Option<(update::Snapshot, UpdateBatch, Vec<update::RenderedLine>)>,
    show: Show,
}

impl Drop for Shell3 {
    fn drop(&mut self) {
        service::release_shell_on_executor(self.executor_slot);
    }
}

impl Shell3 {
    /// Construct a previously admitted terminal on its permanent AP owner.
    pub(super) fn new_terminal_reserved(slot: u32) -> Self {
        debug_assert_eq!(crate::percpu::current_slot() as u32, slot);
        let mut shell = Self::new_inner(
            &TitleTime::current(), crate::r::restart::startup_alias_names(),
            service::appdb_names_snapshot().1, Vec::new(), Default_COLUMNS, Default_ROWS, slot,
        );
        shell.set_show_backend(ShowBackend::Network);
        shell
    }

    pub fn new(
        time: &str,
        aka_names: Vec<String>,
        appdb_names: Vec<String>,
        update_callbacks: Vec<UpdateCallback>,
        columns: usize,
        rows: usize,
    ) -> Result<Self, Shell3Error> {
        let executor_slot = crate::percpu::current_slot() as u32;
        service::reserve_shell_on_executor(executor_slot)?;

        Ok(Self::new_inner(
            time, aka_names, appdb_names, update_callbacks, columns, rows, executor_slot,
        ))
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
            return Err(Shell3Error::WrongExecutor { expected: executor_slot, actual });
        }
        Ok(Self::new_inner(
            time, aka_names, appdb_names, update_callbacks, columns, rows, executor_slot,
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
        rows_state.title.right = mode_title_meta(Mode::HV, &aka_names, &appdb_names);

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
        ).with_matrix(&matrix_lines, matrix_generation);

        Self {
            executor_slot,
            columns,
            rows_count,
            layout_generation: 0,
            time,
            mode: Mode::HV,
            active_matrix_slot: None,
            active_matrix_lifetime: None,
            matrix_selection_dirty: false,
            prompt,
            rows: rows_state,
            aka_names,
            appdb_names,
            update_callbacks,
            update_baseline: initial,
            pending_presentation: None,
            show: Show::default(),
        }
    }

    pub const fn show_backend(&self) -> ShowBackend {
        self.show.backend()
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
        let actual_slot = crate::percpu::current_slot() as u32;
        if actual_slot != self.executor_slot {
            return Err("shell3-show-wrong-executor");
        }
        if self.font_scale_needed() {
            let scale = service::microfont_scale();
            let (width, height) = self.show.set_font_scale(scale, self.columns, self.rows_count)?;
            self.set((width / (microfont::FWIDTH as u32 * scale)) as usize,
                (height / (microfont::FHEIGHT as u32 * scale)) as usize);
            self.pending_presentation = None;
            if self.show.resize_needed() { self.resize_ui4_to_current().await?; }
            crate::log_info!(target: "service";
                "sh3srv: microfont scale={}x slot={} extent={}x{} grid={}x{}\n",
                scale, self.executor_slot, width, height, self.columns, self.rows_count,
            );
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
            self.show.present(&line_refs, columns, rows, &batch).await?;
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
        let (width, height) = self.show.resize_target_extent()
            .ok_or("shell3-show-resize-state")?;
        self.set(
            (width / (microfont::FWIDTH as u32 * self.show.font_scale())) as usize,
            (height / (microfont::FHEIGHT as u32 * self.show.font_scale())) as usize,
        );
        let lines = self.capture_update_snapshot().rendered_lines();
        let line_refs: Vec<_> = lines.iter().map(|line| line.as_slice()).collect();
        self.show.resize_to_current(&line_refs).await
    }

    pub(super) fn ui4_resize_needed(&self) -> bool {
        self.show.resize_needed()
    }

    pub fn set(&mut self, columns: usize, rows: usize) {
        self.columns = columns.max(MIN_COLUMNS);
        self.rows_count = rows.max(MIN_ROWS);
        self.layout_generation = self.layout_generation.wrapping_add(1);
    }

    pub fn get_size(&self) -> (usize, usize) {
        (self.columns, self.rows_count)
    }

    /// CPU slot of the executor that created this shell.
    pub const fn executor_slot(&self) -> u32 {
        self.executor_slot
    }

    pub fn set_time(&mut self, time: &str) {
        self.time.clear();
        self.time.push_str(time);
        self.rows.title.left = vec![MetaFmtStr::new(title_left_text(&self.time))];
    }

    pub fn time(&self) -> &str {
        &self.time
    }

    pub fn set_mode(&mut self, mode: u8) -> bool {
        self.mode = match mode {
            1 => Mode::HV,
            2 => Mode::CMD,
            3 => Mode::ADM,
            _ => return false,
        };
        self.refresh_mode_title();
        true
    }

    fn refresh_mode_title(&mut self) {
        self.rows.title.right = mode_title_meta(self.mode, &self.aka_names, &self.appdb_names);
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn get_mode(&self) -> u8 {
        self.mode as u8
    }

    pub fn select_matrix_slot_index(&mut self, index: usize) -> bool {
        if index == 0 {
            self.active_matrix_slot = None;
            self.active_matrix_lifetime = None;
            self.matrix_selection_dirty = true;
            service::notify_work();
            return true;
        }

        let slots = matrix_slots().lock();
        let Some(name) = slots.ids.get(index - 1) else {
            return false;
        };

        self.active_matrix_slot = Some(name.clone());
        self.active_matrix_lifetime = slots.lifetimes.iter().find(|(id, _)| id == name).map(|(_, lifetime)| *lifetime);
        self.matrix_selection_dirty = true;
        drop(slots);
        service::notify_work();
        true
    }

    pub fn select_matrix_slot_name(&mut self, name: &str) -> bool {
        let slots = matrix_slots().lock();
        if !slots.ids.iter().any(|id| id == name) {
            return false;
        }

        self.active_matrix_slot = Some(name.to_string());
        self.active_matrix_lifetime = slots.lifetimes.iter().find(|(id, _)| id == name).map(|(_, lifetime)| *lifetime);
        self.matrix_selection_dirty = true;
        drop(slots);
        service::notify_work();
        true
    }

    pub fn active_matrix_slot_index(&self) -> usize {
        let Some(active) = self.active_matrix_slot_name() else { return 0; };
        matrix_slots().lock().ids.iter().position(|id| *id == active)
            .map(|index| index + 1).unwrap_or(0)
    }

    pub fn active_matrix_slot_name(&self) -> Option<String> {
        let active = self.active_matrix_slot.as_deref()?;
        matrix_slots().lock().lifetimes.iter()
            .find(|(id, lifetime)| id == active && Some(*lifetime) == self.active_matrix_lifetime)
            .map(|(id, _)| id.clone())
    }

    /// Every owner reconciles deletion locally; no cross-AP model mutation.
    pub(super) fn reconcile_matrix_selection(&mut self) {
        if self.active_matrix_slot.is_some() && self.active_matrix_slot_name().is_none() {
            self.select_matrix_slot_index(0);
        }
    }

    /// Enter is reserved for operator submission; other prompt text is inert.
    fn submit_operator_prompt(&mut self) -> bool {
        let input = self.prompt.text.clone();
        if !self.parse_operator(&input) { return false; }
        self.set_prompt("");
        true
    }

    /// UI editing with immediate exact-name echo and AppDB/AKA launch.
    pub(super) fn handle_keyboard(&mut self, event: &crate::r::keyboard::TrueosKeyboardOutputEvent) -> bool {
        use crate::r::keyboard::*;
        if event.kind == KEYBOARD_OUTPUT_KIND_KEY {
            match event.key_code {
                KEYBOARD_KEY_ENTER => return self.submit_operator_prompt(),
                KEYBOARD_KEY_TAB => return self.set_mode(self.get_mode() % 3 + 1),
                KEYBOARD_KEY_BACKSPACE => {
                    if self.prompt.cursor == 0 { return false; }
                    let index = self.prompt.cursor - 1;
                    let byte = self.prompt.text.char_indices().nth(index).unwrap().0;
                    self.prompt.text.remove(byte);
                    if index < self.prompt.colors.len() { self.prompt.colors.remove(index); }
                    self.prompt.cursor = index;
                    self.refresh_prompt_strip();
                    return true;
                }
                _ => return false,
            }
        }
        if event.kind != KEYBOARD_OUTPUT_KIND_TEXT { return false; }
        let Some(ch) = char::from_u32(event.codepoint).filter(|ch| !ch.is_control()) else { return false; };
        let right_len = self.rows.promt.right.iter().map(|run| run.text.chars().count()).sum::<usize>();
        // Keep all typed glyphs and the cursor visible beside the right strip.
        if self.prompt.char_len() + 1 >= self.columns.saturating_sub(right_len) { return false; }
        let byte = self.prompt.text.char_indices().nth(self.prompt.cursor)
            .map(|(byte, _)| byte).unwrap_or(self.prompt.text.len());
        self.prompt.text.insert(byte, ch);
        self.prompt.colors.resize(self.prompt.char_len() - 1, None);
        self.prompt.colors.insert(self.prompt.cursor, None);
        self.prompt.cursor += 1;
        self.echo_recognized_prompt();
        self.refresh_prompt_strip();
        true
    }

    fn echo_recognized_prompt(&mut self) {
        if self.prompt.text.starts_with(OPERATOR) || !self.parse_name(&self.prompt.text) { return; }
        let text = core::mem::take(&mut self.prompt.text);
        MatrixSlots::echo(self.active_matrix_slot.as_deref(), self.active_matrix_lifetime, text.clone());
        if self.mode == Mode::CMD {
            let slot = self.active_matrix_slot.as_deref().unwrap_or("");
            let result = if self.appdb_names.iter().any(|name| name == &text) {
                service::launch_appdb(&text, slot)
            } else if self.aka_names.iter().any(|name| name == &text) {
                service::launch_alias(&text, slot)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                MatrixSlots::echo(self.active_matrix_slot.as_deref(), self.active_matrix_lifetime, error);
            }
        }
        self.prompt.colors.clear();
        self.prompt.cursor = 0;
    }

    pub(super) fn matrix_output_needed(&self) -> bool {
        self.matrix_selection_dirty || matrix_slots().lock().generation != self.update_baseline.matrix_generation()
    }

    pub fn set_prompt(&mut self, text: &str) {
        self.prompt.text.clear();
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

    pub fn cursor(&self) -> usize {
        self.prompt.cursor
    }

    fn refresh_prompt_strip(&mut self) {
        let mut colors = self.prompt.colors.clone();
        colors.resize(self.prompt.char_len(), None);
        colors.insert(self.prompt.cursor, None);
        self.rows.promt.left = self.prompt.render().chars().zip(colors)
            .map(|(ch, color)| MetaFmtStr { text: ch.to_string(), color, bold: false }).collect();
    }

    pub fn set_strip(&mut self, row: SpecialRows, side: StripSide, text: &str) -> bool {
        if matches!(row, SpecialRows::MatrixRow(_)) { return false; }
        if side == StripSide::Left {
            match row {
                SpecialRows::TitleRow => {
                    self.rows.title.left.clear();
                    self.rows.title.left.push(MetaFmtStr::new(text));
                    return true;
                }
                SpecialRows::StatusRow | SpecialRows::MatrixRow(_) => return false,
                SpecialRows::PromtRow => {
                    self.set_prompt(text);
                    return true;
                }
            }
        }

        let strip = self.rows.row_mut(row);
        strip.right.clear();
        strip.right.push(MetaFmtStr::new(text));
        true
    }

    /// Set styled text without flattening its MetaFmt colors. Bold is retained
    /// in the runs but deliberately has no raster effect yet.
    pub fn set_strip_formatted(&mut self, row: SpecialRows, side: StripSide, runs: Vec<MetaFmtStr>) -> bool {
        if matches!(row, SpecialRows::MatrixRow(_)) { return false; }
        if side == StripSide::Left && row == SpecialRows::StatusRow {
            return false;
        }
        if side == StripSide::Left && row == SpecialRows::PromtRow {
            let text: String = runs.iter().map(|run| run.text.as_str()).collect();
            self.set_prompt(&text);
            self.prompt.colors = runs.iter().flat_map(|run| run.text.chars().map(|_| run.color)).collect();
            self.refresh_prompt_strip();
            return true;
        }
        let strip = self.rows.row_mut(row);
        match side {
            StripSide::Left => strip.left = runs,
            StripSide::Right => strip.right = runs,
        }
        true
    }

    pub fn get_strip(&self, row: SpecialRows, side: StripSide) -> String {
        if let SpecialRows::MatrixRow(index) = row {
            return if side == StripSide::Left { MatrixSlots::view_echo_snapshot(self.active_matrix_slot.as_deref(), self.active_matrix_lifetime).1.get(index).cloned().unwrap_or_default() } else { String::new() };
        }
        if row == SpecialRows::StatusRow && side == StripSide::Left {
            return current_matrix_slots_text();
        }

        let strip = self.rows.row(row);
        match side {
            StripSide::Left => strip.left.iter().map(|run| run.text.as_str()).collect(),
            StripSide::Right => strip.right.iter().map(|run| run.text.as_str()).collect(),
        }
    }

    fn row_for_render(&self, row: SpecialRows) -> RowStrips {
        if matches!(row, SpecialRows::MatrixRow(_)) {
            return RowStrips::new(&self.get_strip(row, StripSide::Left), "");
        }
        let mut strips = self.rows.row(row).clone();
        if row == SpecialRows::StatusRow {
            let active = self.active_matrix_slot_name();
            let slots = matrix_slots().lock();
            strips.left = matrix_slots_meta(&slots.ids, active.as_deref());
        }
        strips
    }

    pub fn render_strips(&self, row: SpecialRows) -> String {
        let strips = self.row_for_render(row);
        update::fit_meta_strips(&strips.left, &strips.right, self.columns).iter().map(|cell| cell.0).collect()
    }

    fn capture_update_snapshot(&self) -> update::Snapshot {
        let title = self.row_for_render(SpecialRows::TitleRow);
        let status = self.row_for_render(SpecialRows::StatusRow);
        let promt = self.row_for_render(SpecialRows::PromtRow);
        let (matrix_generation, matrix_lines) = MatrixSlots::view_echo_snapshot(self.active_matrix_slot.as_deref(), self.active_matrix_lifetime);

        update::Snapshot::new(
            (self.columns, self.rows_count),
            self.layout_generation,
            [
                (&title.left, &title.right),
                (&status.left, &status.right),
                (&promt.left, &promt.right),
            ],
            self.columns,
        ).with_matrix(&matrix_lines, matrix_generation)
    }

    pub fn take_updates(&mut self) -> UpdateBatch {
        if let Some((_, batch, _)) = &self.pending_presentation {
            return batch.clone();
        }
        let current = self.capture_update_snapshot();
        let batch = update::take_updates(
            &mut self.update_baseline,
            current.clone(),
            &self.update_callbacks,
        );
        let lines = current.rendered_lines();
        self.pending_presentation = Some((current, batch.clone(), lines));
        batch
    }

    pub fn add_update_callback(&mut self, callback: UpdateCallback) {
        self.update_callbacks.push(callback);
    }

    pub fn set_appdb_names(&mut self, names: &[String]) {
        self.appdb_names.clear();
        self.appdb_names.extend(names.iter().cloned());
        self.refresh_mode_title();
    }

    pub fn parse(&self, input: &str) -> bool {
        if input.starts_with(OPERATOR) {
            return false;
        }
        self.parse_name(input)
    }

    /// Operator contract: §, §id, §id§, and §§, submitted explicitly.
    pub fn parse_operator(&mut self, input: &str) -> bool {
        let Some(rest) = input.strip_prefix(OPERATOR) else { return false; };
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
        if name.is_empty() || name.chars().any(|ch| ch == OPERATOR || ch.is_whitespace() || ch.is_control()) {
            return false;
        }
        if dropping {
            if !MatrixSlots::drop_slot(Some(name)) { return false; }
            self.reconcile_matrix_selection();
            return true;
        }
        let lifetime = MatrixSlots::ensure_named(name);
        self.active_matrix_slot = Some(name.to_string());
        self.active_matrix_lifetime = Some(lifetime);
        self.matrix_selection_dirty = true;
        service::notify_work();
        true
    }

    pub fn parse_name(&self, name: &str) -> bool {
        match self.mode {
            Mode::HV => HV_GROUPS
                .iter()
                .any(|group| group.names.iter().any(|entry| entry.name == name)),
            Mode::CMD => {
                CMD_GROUPS
                    .iter()
                    .any(|group| group.names.iter().any(|entry| entry.name == name))
                    || self.aka_names.iter().any(|alias| alias == name)
                    || self.appdb_names.iter().any(|entry| entry == name)
            }
            Mode::ADM => ADM_NAMES.iter().any(|entry| entry.name == name),
        }
    }
}

#[allow(non_snake_case)]
pub fn newShell3(
    time: &str,
    aka_names: Vec<String>,
    appdb_names: Vec<String>,
    updateCallbacks: Vec<UpdateCallback>,
    col: usize,
    row: usize,
) -> Result<Shell3, Shell3Error> {
    Shell3::new(time, aka_names, appdb_names, updateCallbacks, col, row)
}

/// The mode legend and exact-name recognizer use the same three registries.
fn mode_title_meta(mode: Mode, aka_names: &[String], appdb_names: &[String]) -> Vec<MetaFmtStr> {
    let mut runs = Vec::new();
    let mut append_group = |group: &names::NameGroup, dynamic: Option<&[String]>| {
        if !runs.is_empty() { runs.push(MetaFmtStr::new(" ")); }
        runs.push(MetaFmtStr::new(names::GROUP_OPEN.to_string()));
        if !group.name.is_empty() { runs.push(MetaFmtStr::new(group.name)); }
        for (index, entry) in group.names.iter().enumerate() {
            if !group.name.is_empty() || index != 0 {
                runs.push(MetaFmtStr::new(" "));
            }
            runs.push(MetaFmtStr::new(entry.name).color(entry.color));
        }
        if let Some(entries) = dynamic {
            for entry in entries { runs.push(MetaFmtStr::new(" ")); runs.push(MetaFmtStr::new(entry)); }
        }
        runs.push(MetaFmtStr::new(names::GROUP_CLOSE.to_string()));
    };
    match mode {
        Mode::HV => { for group in &HV_GROUPS { append_group(group, None); } }
        Mode::CMD => {
            for group in &CMD_GROUPS {
                let dynamic = match group.name { "Aka" => Some(aka_names), "AppDB" => Some(appdb_names), _ => None };
                append_group(group, dynamic);
            }
        }
        Mode::ADM => {
            for entry in &ADM_NAMES {
                if !runs.is_empty() { runs.push(MetaFmtStr::new(" ")); }
                runs.push(MetaFmtStr::new(entry.name).color(entry.color));
            }
        }
    }
    runs
}
