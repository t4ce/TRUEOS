mod format;

pub use format::{bold, styled};

pub const OPERATOR: char = '§';
pub const MODESTEP: char = '\t';
pub const GROUP_OPEN: char = '[';
pub const GROUP_CLOSE: char = ']';

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

pub struct TitleTime;

static TITLE_TIME: std::sync::OnceLock<std::sync::RwLock<String>> =
    std::sync::OnceLock::new();

fn title_time() -> &'static std::sync::RwLock<String> {
    TITLE_TIME.get_or_init(|| std::sync::RwLock::new(TitleTime::DEFAULT.to_string()))
}

fn title_left_text(time: &str) -> String {
    format!("TrueOS {} {}", OPERATOR, time)
}

impl TitleTime {
    pub const DEFAULT: &'static str = "01:22";

    /// Sets the trusted external time text shown in the left TitleRow strip.
    /// The caller supplies the fixed HH:MM-style value; time sourcing is out of scope.
    pub fn set(time: &str) {
        let mut current = title_time().write().unwrap();
        current.clear();
        current.push_str(time);

        let text = title_left_text(&current);
        drop(current);
        set_strip(SpecialRows::TitleRow, StripSide::Left, &text);
    }

    pub fn get() -> String {
        title_time().read().unwrap().clone()
    }
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
    /// Character offset inside the strip's visible text.
    pub offset: usize,
    /// Number of previously-visible characters replaced/removed at `offset`.
    pub remove: usize,
    /// Replacement characters. Empty means the segment only became hidden.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateBatch {
    /// `set()` was called since the last batch, even when the clamped size stayed equal.
    pub layout_changed: bool,
    pub old_size: (usize, usize),
    pub new_size: (usize, usize),
    /// Smallest per-strip visible edits needed to reach the current state.
    pub segments: Vec<SegmentUpdate>,
}

pub struct MatrixSlots;

#[derive(Clone, Debug)]
struct MatrixSlotsState {
    // Raw unique slot IDs only. The default slot is implicit at index 0.
    ids: Vec<String>,
    active: usize,
}

impl MatrixSlotsState {
    fn new() -> Self {
        Self {
            ids: vec!["id".to_string(), "123".to_string()],
            active: 0,
        }
    }
}

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

impl MatrixSlots {
    pub const DEFAULT: &'static str = "\x1b[1m§\x1b[0m \x1b[1m§\x1b[0mid \x1b[1m§\x1b[0m123";

    /// Replaces the named slots. Names are raw IDs without the `§` prefix.
    /// Duplicate IDs are kept only once, in first-seen order.
    pub fn set<T: AsRef<str>>(names: &[T]) {
        let mut slots = matrix_slots().write().unwrap();

        let previously_active = if slots.active == 0 {
            None
        } else {
            slots.ids.get(slots.active - 1).cloned()
        };

        let mut ids = Vec::with_capacity(names.len());
        for name in names {
            let name = name.as_ref();
            if !ids.iter().any(|existing: &String| existing == name) {
                ids.push(name.to_string());
            }
        }

        slots.active = previously_active
            .as_deref()
            .and_then(|active| ids.iter().position(|id| id == active))
            .map(|index| index + 1)
            .unwrap_or(0);
        slots.ids = ids;

        let text = matrix_slots_text(&slots.ids);
        drop(slots);
        set_strip(SpecialRows::StatusRow, StripSide::Left, &text);
    }

    /// Index 0 is the default bare `§` slot. Named slots begin at index 1.
    pub fn select_index(index: usize) -> bool {
        let mut slots = matrix_slots().write().unwrap();
        if index > slots.ids.len() {
            return false;
        }

        slots.active = index;
        true
    }

    /// Selects a named slot using its raw ID, without the `§` prefix.
    pub fn select_name(name: &str) -> bool {
        let mut slots = matrix_slots().write().unwrap();
        let Some(index) = slots.ids.iter().position(|id| id == name) else {
            return false;
        };

        slots.active = index + 1;
        true
    }

    pub fn active_index() -> usize {
        matrix_slots().read().unwrap().active
    }

    /// `None` means the default bare `§` slot is active.
    pub fn active_name() -> Option<String> {
        let slots = matrix_slots().read().unwrap();
        if slots.active == 0 {
            None
        } else {
            slots.ids.get(slots.active - 1).cloned()
        }
    }

    pub fn slot_ids() -> Vec<String> {
        matrix_slots().read().unwrap().ids.clone()
    }

    pub fn get() -> String {
        get_strip(SpecialRows::StatusRow, StripSide::Left)
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

#[derive(Debug)]
struct SpecialRowsState {
    title: RowStrips,
    status: RowStrips,
    promt: RowStrips,
}

impl SpecialRowsState {
    fn new() -> Self {
        Self {
            title: RowStrips::new("TrueOS § 01:22", ""),
            status: RowStrips::new(MatrixSlots::DEFAULT, ""),
            promt: RowStrips::new("", ""),
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


pub const MIN_COLUMNS: usize = 20;
pub const MIN_ROWS: usize = 5;

static CURRENT_COLUMNS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(MIN_COLUMNS);
static CURRENT_ROWS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(MIN_ROWS);
static LAYOUT_GENERATION: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub fn set(columns: usize, rows: usize) {
    ensure_update_baseline();

    CURRENT_COLUMNS.store(columns.max(MIN_COLUMNS), std::sync::atomic::Ordering::Relaxed);
    CURRENT_ROWS.store(rows.max(MIN_ROWS), std::sync::atomic::Ordering::Relaxed);

    // A call to set() is itself a layout event. This intentionally advances even
    // when clamping produces the same final size.
    LAYOUT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn get_size() -> (usize, usize) {
    (
        CURRENT_COLUMNS.load(std::sync::atomic::Ordering::Relaxed),
        CURRENT_ROWS.load(std::sync::atomic::Ordering::Relaxed),
    )
}

static SPECIAL_ROWS: std::sync::OnceLock<std::sync::RwLock<SpecialRowsState>> =
    std::sync::OnceLock::new();

fn special_rows() -> &'static std::sync::RwLock<SpecialRowsState> {
    SPECIAL_ROWS.get_or_init(|| std::sync::RwLock::new(SpecialRowsState::new()))
}

pub fn set_strip(row: SpecialRows, side: StripSide, text: &str) {
    ensure_update_baseline();

    let mut rows = special_rows().write().unwrap();
    let strip = rows.row_mut(row);

    match side {
        StripSide::Left => {
            strip.left.clear();
            strip.left.push_str(text);
        }
        StripSide::Right => {
            strip.right.clear();
            strip.right.push_str(text);
        }
    }
}

pub fn get_strip(row: SpecialRows, side: StripSide) -> String {
    let rows = special_rows().read().unwrap();
    let strip = rows.row(row);

    match side {
        StripSide::Left => strip.left.clone(),
        StripSide::Right => strip.right.clone(),
    }
}

pub fn render_strips(row: SpecialRows) -> String {
    let columns = CURRENT_COLUMNS.load(std::sync::atomic::Ordering::Relaxed);
    let rows = special_rows().read().unwrap();
    let strip = rows.row(row);

    fit_LR_strips(&strip.left, &strip.right, columns)
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

static UPDATE_BASELINE: std::sync::OnceLock<std::sync::Mutex<UpdateSnapshot>> =
    std::sync::OnceLock::new();

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

fn capture_update_snapshot() -> UpdateSnapshot {
    let columns = CURRENT_COLUMNS.load(std::sync::atomic::Ordering::Relaxed);
    let row_count = CURRENT_ROWS.load(std::sync::atomic::Ordering::Relaxed);
    let layout_generation = LAYOUT_GENERATION.load(std::sync::atomic::Ordering::Relaxed);
    let rows = special_rows().read().unwrap();

    UpdateSnapshot {
        size: (columns, row_count),
        layout_generation,
        rows: [
            visible_LR_strips(&rows.title.left, &rows.title.right, columns),
            visible_LR_strips(&rows.status.left, &rows.status.right, columns),
            visible_LR_strips(&rows.promt.left, &rows.promt.right, columns),
        ],
    }
}

fn ensure_update_baseline() {
    UPDATE_BASELINE.get_or_init(|| std::sync::Mutex::new(capture_update_snapshot()));
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

/// Returns all visible changes since the previous call, then commits the current
/// state as the new baseline. Updates stay at segment granularity; callers never
/// need to invalidate a whole strip or row just because several segments changed.
pub fn take_updates() -> UpdateBatch {
    ensure_update_baseline();

    let current = capture_update_snapshot();
    let baseline_lock = UPDATE_BASELINE.get().unwrap();
    let mut baseline = baseline_lock.lock().unwrap();
    let old = baseline.clone();

    let mut segments = Vec::new();

    for index in 0..3 {
        let row = row_from_index(index);
        let old_row = &old.rows[row_index(row)];
        let new_row = &current.rows[row_index(row)];

        if let Some(update) = diff_visible_segment(row, StripSide::Left, &old_row.left, &new_row.left) {
            segments.push(update);
        }
        if let Some(update) = diff_visible_segment(row, StripSide::Right, &old_row.right, &new_row.right) {
            segments.push(update);
        }
    }

    *baseline = current.clone();

    UpdateBatch {
        layout_changed: old.layout_generation != current.layout_generation,
        old_size: old.size,
        new_size: current.size,
        segments,
    }
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

static CURRENT_MODE: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(Mode::HV as u8);

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

const CMD_AKA_NAMES: [NameEntry; 3] = [
    NameEntry { name: "cub", color: RgbaColor::White },
    NameEntry { name: "grid", color: RgbaColor::White },
    NameEntry { name: "td", color: RgbaColor::White },
];

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

static CMD_APPDB_RUNTIME: std::sync::OnceLock<std::sync::RwLock<Vec<RuntimeNameEntry>>> =
    std::sync::OnceLock::new();

fn cmd_appdb_runtime() -> &'static std::sync::RwLock<Vec<RuntimeNameEntry>> {
    CMD_APPDB_RUNTIME.get_or_init(|| std::sync::RwLock::new(Vec::new()))
}

pub fn set_appdb_names(names: &[(&str, RgbaColor)]) {
    let mut appdb = cmd_appdb_runtime().write().unwrap();
    appdb.clear();
    appdb.extend(names.iter().map(|(name, color)| RuntimeNameEntry {
        name: (*name).to_string(),
        color: *color,
    }));
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

#[no_mangle]
pub extern "C" fn set_mode(mode: u8) -> bool {
    match mode {
        1 | 2 | 3 => {
            CURRENT_MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

#[no_mangle]
pub extern "C" fn get_mode() -> u8 {
    CURRENT_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn parse(input: &str) -> bool {
    if input.starts_with(OPERATOR) {
        parse_operator(input);
        return false;
    }

    parse_name(input)
}

pub fn parse_operator(_input: &str) {
}

pub fn parse_name(name: &str) -> bool {
    match get_mode() {
        1 => HV_GROUPS
            .iter()
            .any(|group| group.names.iter().any(|entry| entry.name == name)),
        2 => {
            CMD_GROUPS
                .iter()
                .any(|group| group.names.iter().any(|entry| entry.name == name))
                || cmd_appdb_runtime()
                    .read()
                    .unwrap()
                    .iter()
                    .any(|entry| entry.name == name)
        },
        3 => ADM_NAMES.iter().any(|entry| entry.name == name),
        _ => false,
    }
}
