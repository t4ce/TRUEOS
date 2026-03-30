#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

use crate::r::ui2::Ui2Rect;

const FPS_DIGIT_COUNT: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct WidgetFpsConfig {
    pub tile_h: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub slot_gap: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub sample_ms: u64,
    pub fade_out_ms: u64,
    pub fade_in_ms: u64,
}

impl Default for WidgetFpsConfig {
    fn default() -> Self {
        Self {
            tile_h: 14.0,
            pad_x: 4.0,
            pad_y: 3.0,
            slot_gap: 1.0,
            margin_right: 8.0,
            margin_bottom: 6.0,
            sample_ms: 250,
            fade_out_ms: 25,
            fade_in_ms: 25,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WidgetFpsPaintCell {
    pub index: usize,
    pub rect: Ui2Rect,
    pub digit: u8,
    pub alpha: u8,
}

#[derive(Clone, Debug, Default)]
pub struct WidgetFpsFrame {
    pub dirty_rects: Vec<Ui2Rect>,
    pub cells: Vec<WidgetFpsPaintCell>,
    pub displayed_value: u16,
    pub animating: bool,
    pub next_tick_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CellPhase {
    Stable,
    FadeOut,
    FadeIn,
}

#[derive(Clone, Copy, Debug)]
struct WidgetFpsCellState {
    stable_digit: u8,
    visible_digit: u8,
    target_digit: u8,
    phase: CellPhase,
    phase_started_ms: u64,
}

impl Default for WidgetFpsCellState {
    fn default() -> Self {
        Self {
            stable_digit: b'0',
            visible_digit: b'0',
            target_digit: b'0',
            phase: CellPhase::Stable,
            phase_started_ms: 0,
        }
    }
}

pub struct WidgetFps {
    cfg: WidgetFpsConfig,
    last_sample_ms: u64,
    compose_present_history_ms: Vec<u64>,
    displayed_value: u16,
    cells: [WidgetFpsCellState; FPS_DIGIT_COUNT],
}

impl WidgetFps {
    pub fn new(cfg: WidgetFpsConfig) -> Self {
        Self {
            cfg,
            last_sample_ms: 0,
            compose_present_history_ms: Vec::new(),
            displayed_value: 0,
            cells: [WidgetFpsCellState::default(); FPS_DIGIT_COUNT],
        }
    }

    pub fn record_present(&mut self, now_ms: u64) {
        self.compose_present_history_ms.push(now_ms);
        let window_start = now_ms.saturating_sub(1000);
        self.compose_present_history_ms
            .retain(|&sample_ms| sample_ms >= window_start);
    }

    pub fn tick(&mut self, now_ms: u64, view_w: u32, view_h: u32) -> WidgetFpsFrame {
        self.record_present(now_ms);

        if self.last_sample_ms == 0
            || now_ms.saturating_sub(self.last_sample_ms) >= self.cfg.sample_ms
        {
            self.last_sample_ms = now_ms;
            let next_value = self.compose_present_history_ms.len().min(999) as u16;
            self.schedule_value(next_value, now_ms);
        }

        self.advance_animations(now_ms);

        let layout = self.layout(view_w, view_h);
        let mut dirty_rects = Vec::with_capacity(FPS_DIGIT_COUNT);
        let mut cells = Vec::with_capacity(FPS_DIGIT_COUNT);
        let mut animating = false;
        let mut next_tick_ms = None;

        for index in 0..FPS_DIGIT_COUNT {
            let state = self.cells[index];
            let rect = layout.slot_rects[index];
            let (digit, alpha) = match state.phase {
                CellPhase::Stable => (state.stable_digit, 255),
                CellPhase::FadeOut => {
                    animating = true;
                    next_tick_ms =
                        Some(next_tick_ms.map_or(now_ms + 1, |prev: u64| prev.min(now_ms + 1)));
                    dirty_rects.push(rect);
                    let elapsed = now_ms.saturating_sub(state.phase_started_ms);
                    let alpha = fade_alpha_desc(elapsed, self.cfg.fade_out_ms);
                    (state.visible_digit, alpha)
                }
                CellPhase::FadeIn => {
                    animating = true;
                    next_tick_ms =
                        Some(next_tick_ms.map_or(now_ms + 1, |prev: u64| prev.min(now_ms + 1)));
                    dirty_rects.push(rect);
                    let elapsed = now_ms.saturating_sub(state.phase_started_ms);
                    let alpha = fade_alpha_asc(elapsed, self.cfg.fade_in_ms);
                    (state.visible_digit, alpha)
                }
            };

            cells.push(WidgetFpsPaintCell {
                index,
                rect,
                digit,
                alpha,
            });
        }

        WidgetFpsFrame {
            dirty_rects,
            cells,
            displayed_value: self.displayed_value,
            animating,
            next_tick_ms,
        }
    }

    fn schedule_value(&mut self, next_value: u16, now_ms: u64) {
        let next_digits = digits_ascii(next_value);
        self.displayed_value = next_value;

        for (index, next_digit) in next_digits.into_iter().enumerate() {
            let cell = &mut self.cells[index];
            if cell.phase != CellPhase::Stable {
                continue;
            }
            if cell.stable_digit == next_digit {
                continue;
            }
            cell.target_digit = next_digit;
            cell.visible_digit = cell.stable_digit;
            cell.phase = CellPhase::FadeOut;
            cell.phase_started_ms = now_ms;
        }
    }

    fn advance_animations(&mut self, now_ms: u64) {
        for cell in &mut self.cells {
            match cell.phase {
                CellPhase::Stable => {}
                CellPhase::FadeOut => {
                    if now_ms.saturating_sub(cell.phase_started_ms) >= self.cfg.fade_out_ms {
                        cell.visible_digit = cell.target_digit;
                        cell.phase = CellPhase::FadeIn;
                        cell.phase_started_ms = now_ms;
                    }
                }
                CellPhase::FadeIn => {
                    if now_ms.saturating_sub(cell.phase_started_ms) >= self.cfg.fade_in_ms {
                        cell.stable_digit = cell.target_digit;
                        cell.visible_digit = cell.target_digit;
                        cell.phase = CellPhase::Stable;
                    }
                }
            }
        }
    }

    fn layout(&self, view_w: u32, view_h: u32) -> WidgetFpsLayout {
        let slot_w = self.slot_width();
        let slot_h = self.slot_height();
        let total_w = slot_w * FPS_DIGIT_COUNT as f32
            + self.cfg.slot_gap * (FPS_DIGIT_COUNT.saturating_sub(1) as f32);
        let total_h = slot_h;
        let base_x = ((view_w as f32) - self.cfg.margin_right - total_w).max(0.0);
        let base_y = ((view_h as f32) - self.cfg.margin_bottom - total_h).max(0.0);
        let mut slot_rects = [Ui2Rect::default(); FPS_DIGIT_COUNT];

        for (index, rect) in slot_rects.iter_mut().enumerate() {
            *rect = Ui2Rect {
                x: base_x + index as f32 * (slot_w + self.cfg.slot_gap),
                y: base_y,
                w: slot_w,
                h: slot_h,
            };
        }

        WidgetFpsLayout { slot_rects }
    }

    fn slot_width(&self) -> f32 {
        max_digit_advance(self.cfg.tile_h) + self.cfg.pad_x * 2.0
    }

    fn slot_height(&self) -> f32 {
        self.cfg.tile_h + self.cfg.pad_y * 2.0
    }
}

#[derive(Clone, Copy, Debug)]
struct WidgetFpsLayout {
    slot_rects: [Ui2Rect; FPS_DIGIT_COUNT],
}

fn digits_ascii(value: u16) -> [u8; FPS_DIGIT_COUNT] {
    let clamped = value.min(999);
    [
        b'0' + ((clamped / 100) % 10) as u8,
        b'0' + ((clamped / 10) % 10) as u8,
        b'0' + (clamped % 10) as u8,
    ]
}

fn fade_alpha_desc(elapsed_ms: u64, duration_ms: u64) -> u8 {
    if duration_ms == 0 {
        return 0;
    }
    let progress = elapsed_ms.min(duration_ms) as u32;
    let duration = duration_ms as u32;
    let alpha = 255u32.saturating_sub((progress.saturating_mul(255)) / duration.max(1));
    alpha.min(255) as u8
}

fn fade_alpha_asc(elapsed_ms: u64, duration_ms: u64) -> u8 {
    if duration_ms == 0 {
        return 255;
    }
    let progress = elapsed_ms.min(duration_ms) as u32;
    let duration = duration_ms as u32;
    let alpha = (progress.saturating_mul(255)) / duration.max(1);
    alpha.min(255) as u8
}

fn max_digit_advance(tile_h: f32) -> f32 {
    tile_h * 0.75
}
