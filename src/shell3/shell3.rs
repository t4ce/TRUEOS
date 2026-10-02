mod format;
pub mod service;
pub mod show;

pub use format::{bold, styled};
pub use show::{Backend as ShowBackend, Show};

pub const OPERATOR: char = '§';
pub const MODESTEP: char = '\t';
pub const GROUP_OPEN: char = '[';
pub const GROUP_CLOSE: char = ']';
pub const SpecialSeperator: char = '│';
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

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripSide {
    Left = 1,
    Right = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentUpdate {
    pub row: SpecialRows,
    pub side: StripSide,
    pub offset: usize,
    pub remove: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateBatch {
    pub layout_changed: bool,
    pub old_size: (usize, usize),
    pub new_size: (usize, usize),
    pub segments: Vec<SegmentUpdate>,
}

pub type UpdateCallback = fn(&UpdateBatch);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell3Error {
    InstanceLimit,
}

static LIVE_SHELL3_INSTANCES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn reserve_shell3_instance() -> bool {
    LIVE_SHELL3_INSTANCES
        .fetch_update(
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
            |current| {
                if current < MAX_SHELL3_INSTANCES {
                    Some(current + 1)
                } else {
                    None
                }
            },
        )
        .is_ok()
}

pub fn live_shell3_instances() -> usize {
    LIVE_SHELL3_INSTANCES.load(std::sync::atomic::Ordering::Acquire)
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
static MATRIX_SLOTS: std::sync::OnceLock<std::sync::RwLock<MatrixSlotsState>> =
    std::sync::OnceLock::new();

fn matrix_slots() -> &'static std::sync::RwLock<MatrixSlotsState> {
    MATRIX_SLOTS.get_or_init(|| std::sync::RwLock::new(MatrixSlotsState::new()))
}

fn push_bold_operator(text: &mut String) {
    text.push_str(&bold(&OPERATOR.to_string()));
}

fn matrix_slots_text(ids: &[String]) -> String {
    let mut text = String::new();
    push_bold_operator(&mut text);

    for id in ids {
        text.push(' ');
        push_bold_operator(&mut text);
        text.push_str(id);
    }

    text
}

fn current_matrix_slots_text() -> String {
    let slots = matrix_slots().read().unwrap();
    matrix_slots_text(&slots.ids)
}

impl MatrixSlots {
    pub const DEFAULT: &'static str =
        "\x1b[1m§\x1b[0m \x1b[1m§\x1b[0mid \x1b[1m§\x1b[0m123";

    /// Shared across every Shell3. Names are supplied without the § prefix.
    pub fn set<T: AsRef<str>>(names: &[T]) {
        let mut slots = matrix_slots().write().unwrap();
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
        matrix_slots().read().unwrap().ids.clone()
    }

    pub fn get() -> String {
        current_matrix_slots_text()
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct VisibleRow {
    left: String,
    right: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UpdateSnapshot {
    size: (usize, usize),
    layout_generation: usize,
    rows: [VisibleRow; 3],
}

pub struct Shell3 {
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
    update_baseline: UpdateSnapshot,
}

impl Drop for Shell3 {
    fn drop(&mut self) {
        LIVE_SHELL3_INSTANCES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
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
        if !reserve_shell3_instance() {
            return Err(Shell3Error::InstanceLimit);
        }

        let prompt = PromptState::new();
        let prompt_left = prompt.render();
        let time = time.to_string();
        let columns = columns.max(MIN_COLUMNS);
        let rows_count = rows.max(MIN_ROWS);
        let rows_state = SpecialRowsState::new(&time, &prompt_left);

        let initial = UpdateSnapshot {
            size: (columns, rows_count),
            layout_generation: 0,
            rows: [
                visible_LR_strips(&rows_state.title.left, &rows_state.title.right, columns),
                visible_LR_strips(&current_matrix_slots_text(), &rows_state.status.right, columns),
                visible_LR_strips(&rows_state.promt.left, &rows_state.promt.right, columns),
            ],
        };

        Ok(Self {
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

        let slots = matrix_slots().read().unwrap();
        let Some(name) = slots.ids.get(index - 1) else {
            return false;
        };

        self.active_matrix_slot = Some(name.clone());
        true
    }

    pub fn select_matrix_slot_name(&mut self, name: &str) -> bool {
        let slots = matrix_slots().read().unwrap();
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
            .read()
            .unwrap()
            .ids
            .iter()
            .position(|id| id == active)
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    pub fn active_matrix_slot_name(&self) -> Option<String> {
        let active = self.active_matrix_slot.as_deref()?;
        let slots = matrix_slots().read().unwrap();
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
        fit_LR_strips(&strips.left, &strips.right, self.columns)
    }

    fn capture_update_snapshot(&self) -> UpdateSnapshot {
        let title = self.row_for_render(SpecialRows::TitleRow);
        let status = self.row_for_render(SpecialRows::StatusRow);
        let promt = self.row_for_render(SpecialRows::PromtRow);

        UpdateSnapshot {
            size: (self.columns, self.rows_count),
            layout_generation: self.layout_generation,
            rows: [
                visible_LR_strips(&title.left, &title.right, self.columns),
                visible_LR_strips(&status.left, &status.right, self.columns),
                visible_LR_strips(&promt.left, &promt.right, self.columns),
            ],
        }
    }

    pub fn take_updates(&mut self) -> UpdateBatch {
        let current = self.capture_update_snapshot();
        let old = std::mem::replace(&mut self.update_baseline, current.clone());
        let mut segments = Vec::new();

        for index in 0..3 {
            let row = row_from_index(index);
            let old_row = &old.rows[row_index(row)];
            let new_row = &current.rows[row_index(row)];

            if let Some(update) =
                diff_visible_segment(row, StripSide::Left, &old_row.left, &new_row.left)
            {
                segments.push(update);
            }
            if let Some(update) =
                diff_visible_segment(row, StripSide::Right, &old_row.right, &new_row.right)
            {
                segments.push(update);
            }
        }

        let batch = UpdateBatch {
            layout_changed: old.layout_generation != current.layout_generation,
            old_size: old.size,
            new_size: current.size,
            segments,
        };

        if batch.layout_changed || !batch.segments.is_empty() {
            for callback in &self.update_callbacks {
                callback(&batch);
            }
        }

        batch
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

fn row_index(row: SpecialRows) -> usize {
    match row {
        SpecialRows::TitleRow => 0,
        SpecialRows::StatusRow => 1,
        SpecialRows::PromtRow => 2,
    }
}

fn row_from_index(index: usize) -> SpecialRows {
    match index {
        0 => SpecialRows::TitleRow,
        1 => SpecialRows::StatusRow,
        _ => SpecialRows::PromtRow,
    }
}

fn styled_glyphs(text: &str) -> Vec<String> {
    let mut glyphs = Vec::new();
    let mut pending = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            let mut sequence = String::from("\x1b");
            sequence.push(chars.next().unwrap());
            while let Some(next) = chars.next() {
                sequence.push(next);
                if next == 'm' {
                    break;
                }
            }

            if sequence == format::RESET && !glyphs.is_empty() && pending.is_empty() {
                glyphs.last_mut().unwrap().push_str(&sequence);
            } else {
                pending.push_str(&sequence);
            }
            continue;
        }

        pending.push(ch);
        glyphs.push(std::mem::take(&mut pending));
    }

    if !pending.is_empty() {
        if let Some(last) = glyphs.last_mut() {
            last.push_str(&pending);
        }
    }

    glyphs
}

fn visible_len(text: &str) -> usize {
    styled_glyphs(text).len()
}

fn take_visible(text: &str, limit: usize) -> String {
    styled_glyphs(text)
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>()
        .concat()
}

fn visible_LR_strips(left: &str, right: &str, columns: usize) -> VisibleRow {
    let left_len = visible_len(left);
    let right_len = visible_len(right);

    if left_len + right_len <= columns {
        return VisibleRow {
            left: left.to_string(),
            right: right.to_string(),
        };
    }

    if left_len == 0 {
        return VisibleRow {
            left: String::new(),
            right: take_visible(right, columns),
        };
    }

    if right_len == 0 {
        return VisibleRow {
            left: take_visible(left, columns),
            right: String::new(),
        };
    }

    let usable = columns.saturating_sub(1);
    let left_half = usable / 2;
    let right_half = usable - left_half;

    let (left_limit, right_limit) = if left_len < left_half {
        (left_len, usable - left_len)
    } else if right_len < right_half {
        (usable - right_len, right_len)
    } else {
        (left_half, right_half)
    };

    VisibleRow {
        left: take_visible(left, left_limit),
        right: take_visible(right, right_limit),
    }
}

fn diff_visible_segment(
    row: SpecialRows,
    side: StripSide,
    old: &str,
    new: &str,
) -> Option<SegmentUpdate> {
    if old == new {
        return None;
    }

    let old_glyphs = styled_glyphs(old);
    let new_glyphs = styled_glyphs(new);

    let mut prefix = 0;
    let prefix_limit = old_glyphs.len().min(new_glyphs.len());
    while prefix < prefix_limit && old_glyphs[prefix] == new_glyphs[prefix] {
        prefix += 1;
    }

    let mut suffix = 0;
    let old_remaining = old_glyphs.len() - prefix;
    let new_remaining = new_glyphs.len() - prefix;
    while suffix < old_remaining.min(new_remaining)
        && old_glyphs[old_glyphs.len() - 1 - suffix]
            == new_glyphs[new_glyphs.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_end = old_glyphs.len() - suffix;
    let new_end = new_glyphs.len() - suffix;

    Some(SegmentUpdate {
        row,
        side,
        offset: prefix,
        remove: old_end - prefix,
        text: new_glyphs[prefix..new_end].concat(),
    })
}

fn fit_LR_strips(left: &str, right: &str, columns: usize) -> String {
    let visible = visible_LR_strips(left, right, columns);
    let left_len = visible_len(&visible.left);
    let right_len = visible_len(&visible.right);
    let original_left_len = visible_len(left);
    let original_right_len = visible_len(right);
    let overflowed = original_left_len + original_right_len > columns
        && original_left_len > 0
        && original_right_len > 0;

    if overflowed {
        let mut output = String::with_capacity(columns);
        output.push_str(&visible.left);
        output.push(SpecialSeperator);
        output.push_str(&visible.right);
        return output;
    }

    let gap = columns.saturating_sub(left_len + right_len);
    let mut output = String::with_capacity(columns);
    output.push_str(&visible.left);
    output.extend(std::iter::repeat(' ').take(gap));
    output.push_str(&visible.right);
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NameEntry {
    name: &'static str,
    color: RgbaColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NameGroup {
    name: &'static str,
    names: &'static [NameEntry],
}

const HV_GROUP_1: [NameEntry; 3] = [
    NameEntry { name: "online", color: RgbaColor::White },
    NameEntry { name: "peer", color: RgbaColor::White },
    NameEntry { name: "dl", color: RgbaColor::White },
];

const HV_GROUP_2: [NameEntry; 3] = [
    NameEntry { name: "status", color: RgbaColor::White },
    NameEntry { name: "pause", color: RgbaColor::White },
    NameEntry { name: "stop", color: RgbaColor::White },
];

const HV_GROUP_3: [NameEntry; 8] = [
    NameEntry { name: "snap", color: RgbaColor::White },
    NameEntry { name: "preserve", color: RgbaColor::White },
    NameEntry { name: "eject", color: RgbaColor::White },
    NameEntry { name: "delete", color: RgbaColor::White },
    NameEntry { name: "kick", color: RgbaColor::White },
    NameEntry { name: "load", color: RgbaColor::White },
    NameEntry { name: "store", color: RgbaColor::White },
    NameEntry { name: "probe", color: RgbaColor::White },
];

const HV_GROUPS: [NameGroup; 3] = [
    NameGroup { name: "", names: &HV_GROUP_1 },
    NameGroup { name: "", names: &HV_GROUP_2 },
    NameGroup { name: "", names: &HV_GROUP_3 },
];

const CMD_AKA_NAMES: [NameEntry; 0] = [];

const CMD_MEDIA_NAMES: [NameEntry; 5] = [
    NameEntry { name: "img", color: RgbaColor::White },
    NameEntry { name: "shot", color: RgbaColor::White },
    NameEntry { name: "vid", color: RgbaColor::White },
    NameEntry { name: "film", color: RgbaColor::White },
    NameEntry { name: "cam", color: RgbaColor::White },
];

const CMD_APPDB_NAMES: [NameEntry; 0] = [];

const CMD_GROUPS: [NameGroup; 3] = [
    NameGroup { name: "Aka", names: &CMD_AKA_NAMES },
    NameGroup { name: "Media", names: &CMD_MEDIA_NAMES },
    NameGroup { name: "AppDB", names: &CMD_APPDB_NAMES },
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeNameEntry {
    name: String,
    color: RgbaColor,
}

const ADM_NAMES: [NameEntry; 10] = [
    NameEntry { name: "cry", color: RgbaColor::Pink },
    NameEntry { name: "disc", color: RgbaColor::Pink },
    NameEntry { name: "tlb", color: RgbaColor::White },
    NameEntry { name: "xhci", color: RgbaColor::White },
    NameEntry { name: "ram", color: RgbaColor::White },
    NameEntry { name: "smp", color: RgbaColor::White },
    NameEntry { name: "net", color: RgbaColor::White },
    NameEntry { name: "bios", color: RgbaColor::White },
    NameEntry { name: "vgpu", color: RgbaColor::White },
    NameEntry { name: "vcpy", color: RgbaColor::White },
];
