use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::channel::Channel;
use heapless::String as HeaplessString;
use spin::Mutex;

use super::*;

pub const UI2_TEXT_SCENE_MAX_ROWS: usize = 16;
const UI2_TEXT_SCENE_CMD_DEPTH: usize = 32;
const UI2_TEXT_SCENE_ROW_TEXT_CAP: usize = 96;
const UI2_TEXT_SCENE_ROW_H: f32 = 20.0;
const UI2_TEXT_SCENE_ROW_PAD_X: f32 = 10.0;
const UI2_TEXT_SCENE_ROW_PAD_Y: f32 = 10.0;

pub type Ui2TextSceneString = HeaplessString<UI2_TEXT_SCENE_ROW_TEXT_CAP>;

#[derive(Clone, Debug)]
pub struct Ui2TextSceneRow {
    pub text: Ui2TextSceneString,
    pub alpha: u8,
}

impl Default for Ui2TextSceneRow {
    fn default() -> Self {
        Self {
            text: Ui2TextSceneString::new(),
            alpha: 255,
        }
    }
}

impl Ui2TextSceneRow {
    pub fn new(text: &str, alpha: u8) -> Self {
        let mut row = Self {
            text: Ui2TextSceneString::new(),
            alpha,
        };
        row.set_text(text);
        row
    }

    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        for ch in text.chars() {
            if self.text.push(ch).is_err() {
                break;
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum Ui2TextSceneCmd {
    Clear,
    SetRow {
        index: u8,
        row: Ui2TextSceneRow,
    },
    ReplaceRows {
        count: u8,
        rows: [Ui2TextSceneRow; UI2_TEXT_SCENE_MAX_ROWS],
    },
}

#[derive(Clone, Debug)]
pub(super) struct Ui2TextSceneState {
    rows: [Ui2TextSceneRow; UI2_TEXT_SCENE_MAX_ROWS],
    row_count: usize,
    seq: u32,
}

impl Default for Ui2TextSceneState {
    fn default() -> Self {
        Self {
            rows: core::array::from_fn(|_| Ui2TextSceneRow::default()),
            row_count: 0,
            seq: 0,
        }
    }
}

struct SpinRawMutex(Mutex<()>);

unsafe impl RawMutex for SpinRawMutex {
    const INIT: Self = Self(Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock();
        f()
    }
}

static UI2_TEXT_SCENE_CMDS: Channel<SpinRawMutex, Ui2TextSceneCmd, UI2_TEXT_SCENE_CMD_DEPTH> =
    Channel::new();

pub fn ui2_text_scene_try_send(cmd: Ui2TextSceneCmd) -> bool {
    UI2_TEXT_SCENE_CMDS.try_send(cmd).is_ok()
}

pub fn ui2_text_scene_replace_rows_cmd(lines: &[&str]) -> Ui2TextSceneCmd {
    let mut rows = core::array::from_fn(|_| Ui2TextSceneRow::default());
    let mut count = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        if idx >= UI2_TEXT_SCENE_MAX_ROWS {
            break;
        }
        rows[idx] = Ui2TextSceneRow::new(line, 255);
        count = idx + 1;
    }
    Ui2TextSceneCmd::ReplaceRows {
        count: count as u8,
        rows,
    }
}

fn apply_text_scene_cmd(scene: &mut Ui2TextSceneState, cmd: Ui2TextSceneCmd) -> bool {
    match cmd {
        Ui2TextSceneCmd::Clear => {
            let was_nonempty = scene.row_count != 0;
            scene.row_count = 0;
            for row in &mut scene.rows {
                *row = Ui2TextSceneRow::default();
            }
            if was_nonempty {
                scene.seq = scene.seq.wrapping_add(1);
            }
            was_nonempty
        }
        Ui2TextSceneCmd::SetRow { index, row } => {
            let index = index as usize;
            if index >= UI2_TEXT_SCENE_MAX_ROWS {
                return false;
            }
            let changed =
                scene.rows[index].text != row.text || scene.rows[index].alpha != row.alpha;
            if !changed {
                return false;
            }
            scene.rows[index] = row;
            scene.row_count = scene.row_count.max(index + 1);
            scene.seq = scene.seq.wrapping_add(1);
            true
        }
        Ui2TextSceneCmd::ReplaceRows { count, rows } => {
            let next_count = (count as usize).min(UI2_TEXT_SCENE_MAX_ROWS);
            let changed = scene.row_count != next_count
                || scene
                    .rows
                    .iter()
                    .zip(rows.iter())
                    .take(UI2_TEXT_SCENE_MAX_ROWS)
                    .any(|(lhs, rhs)| lhs.text != rhs.text || lhs.alpha != rhs.alpha);
            if !changed {
                return false;
            }
            scene.row_count = next_count;
            scene.rows = rows;
            scene.seq = scene.seq.wrapping_add(1);
            true
        }
    }
}

pub(super) fn drain_text_scene_cmds(state: &mut Ui2State) -> usize {
    let mut drained = 0usize;
    let mut changed = false;
    while let Ok(cmd) = UI2_TEXT_SCENE_CMDS.try_receive() {
        drained = drained.saturating_add(1);
        changed |= apply_text_scene_cmd(&mut state.text_scene, cmd);
    }
    if changed {
        state.compose_reason = "text-scene-cmd";
        let window_ids: Vec<u32> = state
            .windows
            .iter()
            .filter(|window| window.kind == Ui2WindowKind::TextScene)
            .map(|window| window.id)
            .collect();
        for window_id in window_ids {
            let _ = note_window_dirty(state, window_id, "text-scene-cmd");
            refresh_window_hit_entries(state, window_id);
        }
    }
    drained
}

pub(super) fn draw_text_scene_window(state: &Ui2State, window: &Ui2Window, content: Ui2Rect) {
    let panel_rgba = modulate_rgba_alpha((0xF7, 0xF8, 0xFB, 0xFF), window.alpha);
    let row_rgba = modulate_rgba_alpha((0xE8, 0xED, 0xF4, 0x70), window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    if state.text_scene.row_count == 0 {
        crate::gfx::text::draw_atlas_text_in_frame_alpha(
            b"Text scene idle",
            content.x + UI2_TEXT_SCENE_ROW_PAD_X,
            content.y + UI2_TEXT_SCENE_ROW_PAD_Y,
            state.view_w,
            state.view_h,
            window.alpha,
        );
        return;
    }

    for idx in 0..state.text_scene.row_count.min(UI2_TEXT_SCENE_MAX_ROWS) {
        let row = &state.text_scene.rows[idx];
        let top = content.y + UI2_TEXT_SCENE_ROW_PAD_Y + (idx as f32 * UI2_TEXT_SCENE_ROW_H);
        if top + UI2_TEXT_SCENE_ROW_H > content.y + content.h {
            break;
        }
        let _ = crate::gfx::lyon::draw_solid_rect_no_present(
            content.x + 6.0,
            top - 1.0,
            (content.w - 12.0).max(1.0),
            (UI2_TEXT_SCENE_ROW_H - 2.0).max(1.0),
            row_rgba,
            state.view_w,
            state.view_h,
        );
        crate::gfx::text::draw_atlas_text_in_frame_alpha(
            row.text.as_bytes(),
            content.x + UI2_TEXT_SCENE_ROW_PAD_X,
            top + 2.0,
            state.view_w,
            state.view_h,
            modulate_alpha(row.alpha, window.alpha),
        );
    }
}
