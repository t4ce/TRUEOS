use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use parry2d::math::{Pose, Vector};
use parry2d::query;
use parry2d::shape::{Ball, Cuboid};
use spin::{Mutex, Once};

use super::*;

const UI2_HIT_SCENE_RECT_CAP: usize = 500;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Ui2HitKind {
    WindowBody,
    WindowDecoration,
    WindowResizeButton,
    WindowVerticalScrollbar,
    WindowHorizontalScrollbar,
    BrowserInteractive,
}

#[derive(Copy, Clone, Debug)]
struct Ui2HitEntry {
    owner_window_id: u32,
    item_id: u32,
    kind: Ui2HitKind,
    rect: Ui2Rect,
    z: i16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct Ui2HitTarget {
    pub owner_window_id: u32,
    pub item_id: u32,
    pub kind: Ui2HitKind,
}

#[derive(Default)]
struct Ui2HitScene {
    seq: u32,
    entries: Vec<Ui2HitEntry>,
    dropped_entries: usize,
}

struct Ui2HitRuntime {
    queued_ui2_scene: bool,
    last_browser_interactive_seq: u32,
    published_seq: u32,
    dropped_rectangles: u32,
    scene: Ui2HitScene,
}

impl Default for Ui2HitRuntime {
    fn default() -> Self {
        Self {
            queued_ui2_scene: true,
            last_browser_interactive_seq: 0,
            published_seq: 0,
            dropped_rectangles: 0,
            scene: Ui2HitScene::default(),
        }
    }
}

struct Ui2HitBuildContext<'a> {
    state: &'a Ui2State,
}

trait Ui2WindowHitSource {
    fn append_hit_entries(&self, ctx: &Ui2HitBuildContext<'_>, scene: &mut Ui2HitScene);
}

static UI2_HIT_STARTED: AtomicBool = AtomicBool::new(false);
static UI2_HIT_RUNTIME: Once<Mutex<Ui2HitRuntime>> = Once::new();

fn hit_runtime() -> &'static Mutex<Ui2HitRuntime> {
    UI2_HIT_RUNTIME.call_once(|| Mutex::new(Ui2HitRuntime::default()))
}

impl Ui2WindowHitSource for Ui2Window {
    fn append_hit_entries(&self, ctx: &Ui2HitBuildContext<'_>, scene: &mut Ui2HitScene) {
        if !window_is_renderable(self) || !self.hit_test_visible {
            return;
        }

        let rect = effective_window_rect(ctx.state, self);

        scene.append(Ui2HitEntry {
            owner_window_id: self.id,
            item_id: 0,
            kind: Ui2HitKind::WindowBody,
            rect,
            z: self.z,
        });
        if let Some(rect) = window_decoration_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 1,
                kind: Ui2HitKind::WindowDecoration,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_bottom_bar_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 5,
                kind: Ui2HitKind::WindowDecoration,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_bottom_resize_button_hit_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 2,
                kind: Ui2HitKind::WindowResizeButton,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_vertical_scrollbar_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 3,
                kind: Ui2HitKind::WindowVerticalScrollbar,
                rect,
                z: self.z,
            });
        }
        if let Some(rect) = window_horizontal_scrollbar_rect(ctx.state, self) {
            scene.append(Ui2HitEntry {
                owner_window_id: self.id,
                item_id: 4,
                kind: Ui2HitKind::WindowHorizontalScrollbar,
                rect,
                z: self.z,
            });
        }

        match self.kind {
            Ui2WindowKind::HostedBrowser => {
                if !window_content_participates_in_composition(self) {
                    return;
                }
                let Some(content) = window_content_rect(ctx.state, self) else {
                    return;
                };
                let browser_interactives = browser_interactive_state_for_window(self);
                for interactive in &browser_interactives.interactives {
                    if interactive.width == 0 || interactive.height == 0 {
                        continue;
                    }
                    let rect = Ui2Rect::new(
                        content.x + interactive.x as f32,
                        content.y + interactive.y as f32,
                        interactive.width as f32,
                        interactive.height as f32,
                    );
                    scene.append(Ui2HitEntry {
                        owner_window_id: self.id,
                        item_id: interactive.item_id,
                        kind: Ui2HitKind::BrowserInteractive,
                        rect,
                        z: self.z,
                    });
                }
            }
            Ui2WindowKind::HostedSurface => {
                if !window_content_participates_in_composition(self) {
                    return;
                }
                let Some(content) = window_content_rect(ctx.state, self) else {
                    return;
                };
                let surface_state = hosted_surface_state_for_window(self);
                let scroll_x = surface_state.scroll_x as f32;
                let scroll_y = surface_state.scroll_y as f32;
                for interactive in &self.hosted_surface_interactives {
                    if interactive.item_id == 0 || interactive.width == 0 || interactive.height == 0
                    {
                        continue;
                    }
                    let rect = Ui2Rect::new(
                        content.x + interactive.x as f32 - scroll_x,
                        content.y + interactive.y as f32 - scroll_y,
                        interactive.width as f32,
                        interactive.height as f32,
                    );
                    scene.append(Ui2HitEntry {
                        owner_window_id: self.id,
                        item_id: interactive.item_id,
                        kind: Ui2HitKind::BrowserInteractive,
                        rect,
                        z: self.z,
                    });
                }
            }
            Ui2WindowKind::Hosted3d => {}
        }
    }
}

impl Ui2HitScene {
    fn append(&mut self, entry: Ui2HitEntry) {
        if self.entries.len() >= UI2_HIT_SCENE_RECT_CAP {
            self.dropped_entries = self.dropped_entries.saturating_add(1);
            return;
        }
        self.entries.push(entry);
    }

    fn hit_at(&self, cursor_x: f32, cursor_y: f32) -> Option<Ui2HitTarget> {
        let mut best: Option<(i16, Ui2HitKind, u32, u32)> = None;
        for entry in &self.entries {
            if !hit_entry_intersects_cursor(entry, cursor_x, cursor_y) {
                continue;
            }
            let candidate = (entry.z, entry.kind, entry.owner_window_id, entry.item_id);
            if best
                .as_ref()
                .map(|current| candidate > *current)
                .unwrap_or(true)
            {
                best = Some(candidate);
            }
        }
        best.map(|(_, kind, owner_window_id, item_id)| Ui2HitTarget {
            owner_window_id,
            item_id,
            kind,
        })
    }
}

fn build_ui2_hit_scene(state: &Ui2State, seq: u32) -> Ui2HitScene {
    let ctx = Ui2HitBuildContext { state };
    let mut next_scene = Ui2HitScene {
        seq,
        entries: Vec::with_capacity(UI2_HIT_SCENE_RECT_CAP),
        dropped_entries: 0,
    };
    for idx in sorted_window_indices(state).into_iter().rev() {
        let window = &state.windows[idx];
        if !window_is_renderable(window) {
            continue;
        }
        window.append_hit_entries(&ctx, &mut next_scene);
        if next_scene.entries.len() >= UI2_HIT_SCENE_RECT_CAP {
            break;
        }
    }
    next_scene
}

fn hit_entry_intersects_cursor(entry: &Ui2HitEntry, cursor_x: f32, cursor_y: f32) -> bool {
    if !rect_contains_point(
        Ui2Rect::new(
            entry.rect.x - UI2_CURSOR_HIT_RADIUS_PX,
            entry.rect.y - UI2_CURSOR_HIT_RADIUS_PX,
            entry.rect.w + (UI2_CURSOR_HIT_RADIUS_PX * 2.0),
            entry.rect.h + (UI2_CURSOR_HIT_RADIUS_PX * 2.0),
        ),
        cursor_x,
        cursor_y,
    ) {
        return false;
    }

    let cursor = Ball::new(UI2_CURSOR_HIT_RADIUS_PX.max(0.5));
    let rect =
        Cuboid::new(Vector::new((entry.rect.w * 0.5).max(0.5), (entry.rect.h * 0.5).max(0.5)));
    let cursor_iso = Pose::translation(cursor_x, cursor_y);
    let rect_iso =
        Pose::translation(entry.rect.x + (entry.rect.w * 0.5), entry.rect.y + (entry.rect.h * 0.5));
    matches!(query::intersection_test(&cursor_iso, &cursor, &rect_iso, &rect), Ok(true))
}

fn queue_ui2_hit_scene_refresh() {
    let mut runtime = hit_runtime().lock();
    runtime.queued_ui2_scene = true;
}

#[inline]
fn cursor_source_snapshot_px(
    view_w: u32,
    view_h: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
) -> Option<(f32, f32)> {
    let (nx, ny) =
        crate::r::cursor::cursor_source_pos(controller_id, slot_id, ep_target, hid_kind)?;
    let max_x = view_w.saturating_sub(1) as f32;
    let max_y = view_h.saturating_sub(1) as f32;
    Some(((nx.clamp(0.0, 1.0) as f32) * max_x, (ny.clamp(0.0, 1.0) as f32) * max_y))
}

fn publish_ui2_hit_scene() {
    let next_seq = {
        let runtime = hit_runtime().lock();
        runtime.published_seq.wrapping_add(1).max(1)
    };
    let (scene, browser_interactive_seq) = {
        let state_lock = init_state();
        let state = state_lock.lock();
        (build_ui2_hit_scene(&state, next_seq), hosted_browser_interactive_seq(&state))
    };
    let dropped_rectangles = scene.dropped_entries;
    let mut runtime = hit_runtime().lock();
    runtime.published_seq = scene.seq;
    runtime.last_browser_interactive_seq = browser_interactive_seq;
    runtime.scene = scene;
    if dropped_rectangles != 0 {
        runtime.dropped_rectangles = runtime.dropped_rectangles.wrapping_add(1);
        let drop_count = runtime.dropped_rectangles;
        if drop_count <= 8 || drop_count.is_multiple_of(32) {
            crate::log!(
                "ui2-hit: rectangle-cap reached cap={} seq={} dropped={} count={}\n",
                UI2_HIT_SCENE_RECT_CAP,
                runtime.published_seq,
                dropped_rectangles,
                drop_count
            );
        }
    }
}

pub(super) fn ui2_hit_for_cursor_source(
    view_w: u32,
    view_h: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
) -> Option<(f32, f32, Ui2HitTarget)> {
    let (cursor_x, cursor_y) =
        cursor_source_snapshot_px(view_w, view_h, controller_id, slot_id, ep_target, hid_kind)?;
    let hit = hit_runtime().lock().scene.hit_at(cursor_x, cursor_y)?;
    Some((cursor_x, cursor_y, hit))
}

pub(super) fn ui2_cursor_px_for_source(
    view_w: u32,
    view_h: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
) -> Option<(f32, f32)> {
    cursor_source_snapshot_px(view_w, view_h, controller_id, slot_id, ep_target, hid_kind)
}

pub(super) fn refresh_all_window_hit_entries(state: &mut Ui2State) {
    let _ = state;
    queue_ui2_hit_scene_refresh();
}

pub(super) fn refresh_window_hit_entries(state: &mut Ui2State, owner_window_id: u32) {
    let _ = (state, owner_window_id);
    queue_ui2_hit_scene_refresh();
}

#[embassy_executor::task]
pub async fn ui2_hit_task() {
    if UI2_HIT_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    crate::log!("boot-probe: ui2-hit task start ms={}\n", boot_probe_ms());

    loop {
        let browser_interactive_seq = {
            let state_lock = init_state();
            let state = state_lock.lock();
            hosted_browser_interactive_seq(&state)
        };
        let should_rebuild = {
            let mut runtime = hit_runtime().lock();
            let should_rebuild = runtime.queued_ui2_scene
                || runtime.published_seq == 0
                || runtime.last_browser_interactive_seq != browser_interactive_seq;
            if should_rebuild {
                runtime.queued_ui2_scene = false;
            }
            should_rebuild
        };

        if should_rebuild {
            publish_ui2_hit_scene();
        }

        Timer::after(EmbassyDuration::from_millis(if should_rebuild { 4 } else { 10 })).await;
    }
}
