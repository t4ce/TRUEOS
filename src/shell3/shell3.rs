use alloc::{string::{String, ToString}, vec, vec::Vec};
use spin::Once;

mod names;
mod metafmtstr;
pub mod service;
pub mod show;
mod update;

pub use metafmtstr::MetaFmtStr;
pub use names::{GROUP_CLOSE, GROUP_OPEN};
pub use show::{Backend as ShowBackend, Show};
pub use update::{SegmentUpdate, UpdateBatch, UpdateCallback};

use names::{ADM_NAMES, CMD_GROUPS, HV_GROUPS, RuntimeNameEntry};

pub const OPERATOR: char = '§';
pub const MODESTEP: char = '\t';
pub const PROMPT_CURSOR: char = '▏';
pub const MIN_COLUMNS: usize = 20;
pub const MIN_ROWS: usize = 5;
pub const MAX_SHELL3_INSTANCES: usize = 256;

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

pub use RgbaColor as Color;

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
    WrongExecutor { expected: u32, actual: u32 },
}

pub fn live_shell3_instances() -> usize {
    service::live_shell_count()
}

pub struct TitleTime;

impl TitleTime {
    pub const DEFAULT: &'static str = "01:22";

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
}

impl MatrixSlotsState {
    fn new() -> Self {
        Self {
            ids: vec!["id".to_string(), "123".to_string()],
        }
    }
}

// MatrixSlots is the one shared shell subsystem.
static MATRIX_SLOTS: Once<spin::Mutex<MatrixSlotsState>> = Once::new();

fn matrix_slots() -> &'static spin::Mutex<MatrixSlotsState> {
    MATRIX_SLOTS.call_once(|| spin::Mutex::new(MatrixSlotsState::new()))
}

fn matrix_slots_meta(ids: &[String]) -> Vec<MetaFmtStr> {
    let mut runs = Vec::with_capacity(ids.len().saturating_mul(3).saturating_add(1));
    runs.push(MetaFmtStr::new(OPERATOR.to_string()).bold());
    for id in ids {
        runs.push(MetaFmtStr::new(" "));
        runs.push(MetaFmtStr::new(OPERATOR.to_string()).bold());
        runs.push(MetaFmtStr::new(id.clone()));
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

        slots.ids = ids;
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
        matrix_slots_meta(&slots.ids)
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
    left: String,
    right: String,
}

impl RowStrips {
    fn new(left: &str, right: &str) -> Self {
        Self {
            left: left.to_string(),
            right: right.to_string(),
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
        }
    }

    fn row_mut(&mut self, row: SpecialRows) -> &mut RowStrips {
        match row {
            SpecialRows::TitleRow => &mut self.title,
            SpecialRows::StatusRow => &mut self.status,
            SpecialRows::PromtRow => &mut self.promt,
        }
    }
}

#[derive(Clone, Debug)]
struct PromptState {
    text: String,
    cursor: usize,
}

impl PromptState {
    fn new() -> Self {
        Self {
            text: String::new(),
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
    prompt: PromptState,
    rows: SpecialRowsState,
    aka_names: Vec<String>,
    appdb_runtime: Vec<RuntimeNameEntry>,
    update_callbacks: Vec<UpdateCallback>,
    update_baseline: update::Snapshot,
}

impl Drop for Shell3 {
    fn drop(&mut self) {
        service::release_shell_on_executor(self.executor_slot);
    }
}

impl Shell3 {
    pub fn new(
        time: &str,
        aka_names: Vec<String>,
        update_callbacks: Vec<UpdateCallback>,
        columns: usize,
        rows: usize,
    ) -> Result<Self, Shell3Error> {
        let executor_slot = crate::percpu::current_slot() as u32;
        service::reserve_shell_on_executor(executor_slot)?;

        let prompt = PromptState::new();
        let prompt_left = prompt.render();
        let time = time.to_string();
        let columns = columns.max(MIN_COLUMNS);
        let rows_count = rows.max(MIN_ROWS);
        let rows_state = SpecialRowsState::new(&time, &prompt_left);

        let status_left = current_matrix_slots_text();
        let initial = update::Snapshot::new(
            (columns, rows_count),
            0,
            [
                (&rows_state.title.left, &rows_state.title.right),
                (&status_left, &rows_state.status.right),
                (&rows_state.promt.left, &rows_state.promt.right),
            ],
            columns,
        );

        Ok(Self {
            executor_slot,
            columns,
            rows_count,
            layout_generation: 0,
            time,
            mode: Mode::HV,
            active_matrix_slot: None,
            prompt,
            rows: rows_state,
            aka_names,
            appdb_runtime: Vec::new(),
            update_callbacks,
            update_baseline: initial,
        })
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
        self.rows.title.left = title_left_text(&self.time);
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
        true
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
            return true;
        }

        let slots = matrix_slots().lock();
        let Some(name) = slots.ids.get(index - 1) else {
            return false;
        };

        self.active_matrix_slot = Some(name.clone());
        true
    }

    pub fn select_matrix_slot_name(&mut self, name: &str) -> bool {
        let slots = matrix_slots().lock();
        if !slots.ids.iter().any(|id| id == name) {
            return false;
        }

        self.active_matrix_slot = Some(name.to_string());
        true
    }

    pub fn active_matrix_slot_index(&self) -> usize {
        let Some(active) = self.active_matrix_slot.as_deref() else {
            return 0;
        };

        matrix_slots()
            .lock()
            .ids
            .iter()
            .position(|id| id == active)
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    pub fn active_matrix_slot_name(&self) -> Option<String> {
        let active = self.active_matrix_slot.as_deref()?;
        let slots = matrix_slots().lock();
        slots.ids.iter().find(|id| id.as_str() == active).cloned()
    }

    pub fn set_prompt(&mut self, text: &str) {
        self.prompt.text.clear();
        self.prompt.text.push_str(text);
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
        self.rows.promt.left = self.prompt.render();
    }

    pub fn set_strip(&mut self, row: SpecialRows, side: StripSide, text: &str) -> bool {
        if side == StripSide::Left {
            match row {
                SpecialRows::TitleRow => {
                    self.rows.title.left.clear();
                    self.rows.title.left.push_str(text);
                    return true;
                }
                SpecialRows::StatusRow => return false,
                SpecialRows::PromtRow => {
                    self.set_prompt(text);
                    return true;
                }
            }
        }

        let strip = self.rows.row_mut(row);
        strip.right.clear();
        strip.right.push_str(text);
        true
    }

    pub fn get_strip(&self, row: SpecialRows, side: StripSide) -> String {
        if row == SpecialRows::StatusRow && side == StripSide::Left {
            return current_matrix_slots_text();
        }

        let strip = self.rows.row(row);
        match side {
            StripSide::Left => strip.left.clone(),
            StripSide::Right => strip.right.clone(),
        }
    }

    fn row_for_render(&self, row: SpecialRows) -> RowStrips {
        let mut strips = self.rows.row(row).clone();
        if row == SpecialRows::StatusRow {
            strips.left = current_matrix_slots_text();
        }
        strips
    }

    pub fn render_strips(&self, row: SpecialRows) -> String {
        let strips = self.row_for_render(row);
        update::fit_lr_strips(&strips.left, &strips.right, self.columns)
    }

    fn capture_update_snapshot(&self) -> update::Snapshot {
        let title = self.row_for_render(SpecialRows::TitleRow);
        let status = self.row_for_render(SpecialRows::StatusRow);
        let promt = self.row_for_render(SpecialRows::PromtRow);

        update::Snapshot::new(
            (self.columns, self.rows_count),
            self.layout_generation,
            [
                (&title.left, &title.right),
                (&status.left, &status.right),
                (&promt.left, &promt.right),
            ],
            self.columns,
        )
    }

    pub fn take_updates(&mut self) -> UpdateBatch {
        let current = self.capture_update_snapshot();
        update::take_updates(&mut self.update_baseline, current, &self.update_callbacks)
    }

    pub fn add_update_callback(&mut self, callback: UpdateCallback) {
        self.update_callbacks.push(callback);
    }

    pub fn set_appdb_names(&mut self, names: &[(&str, RgbaColor)]) {
        self.appdb_runtime.clear();
        self.appdb_runtime
            .extend(names.iter().map(|(name, color)| RuntimeNameEntry {
                name: (*name).to_string(),
                color: *color,
            }));
    }

    pub fn parse(&self, input: &str) -> bool {
        if input.starts_with(OPERATOR) {
            self.parse_operator(input);
            return false;
        }
        self.parse_name(input)
    }

    pub fn parse_operator(&self, _input: &str) {
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
                    || self.appdb_runtime.iter().any(|entry| entry.name == name)
            }
            Mode::ADM => ADM_NAMES.iter().any(|entry| entry.name == name),
        }
    }
}

#[allow(non_snake_case)]
pub fn newShell3(
    time: &str,
    aka_names: Vec<String>,
    updateCallbacks: Vec<UpdateCallback>,
    col: usize,
    row: usize,
) -> Result<Shell3, Shell3Error> {
    Shell3::new(time, aka_names, updateCallbacks, col, row)
}
