//! Frame-local hit regions; hover/press associations are derived per live cursor.
//! No page pixels, script execution or application repaint is involved.
use super::button_outline_geometry::{self as geometry, Rect};
use super::{WindowId, WindowOwner, WindowSnapshot, WindowState};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Instant};
use v::bp_abi::TrueosUi4ButtonRegionV1 as Region;

const MAX_REGISTRATIONS: usize = 32;
static REGIONS: Mutex<Vec<Registration>> = Mutex::new(Vec::new());
static ANIMATED: AtomicBool = AtomicBool::new(false);
static LAST_TICK: AtomicU64 = AtomicU64::new(0);
struct Registration {
    owner: WindowOwner,
    window: WindowId,
    session: super::WindowSessionId,
    regions: Vec<Region>,
    hover_regions: Vec<Region>,
}
pub(super) struct Cursor {
    pub source: super::Ui4CursorSource,
    pub x: u32,
    pub y: u32,
    pub pressed: bool,
}
struct Effect {
    window: WindowSnapshot,
    index: usize,
    region: Region,
    pressed: bool,
    hover_only: bool,
}

pub(super) fn register(window: WindowSnapshot, regions: &[Region]) -> Result<(), i32> {
    register_kind(window, regions, false)
}
pub(super) fn register_hover(window: WindowSnapshot, regions: &[Region]) -> Result<(), i32> {
    register_kind(window, regions, true)
}
fn register_kind(window: WindowSnapshot, regions: &[Region], hover_only: bool) -> Result<(), i32> {
    if regions.len() > 256
        || regions.iter().any(|r| {
            r.width == 0
                || r.height == 0
                || r.width > 8192
                || r.height > 8192
                || r.x.abs_diff(0) > 1_000_000
                || r.y.abs_diff(0) > 1_000_000
                || r.clip_width > 8192
                || r.clip_height > 8192
                || r.clip_x.abs_diff(0) > 1_000_000
                || r.clip_y.abs_diff(0) > 1_000_000
        })
    {
        return Err(-1);
    }
    let mut all = REGIONS.lock();
    if all.iter().any(|old| {
        let previous = if hover_only {
            &old.hover_regions
        } else {
            &old.regions
        };
        old.owner == window.owner
            && old.window == window.id
            && old.session == window.session
            && previous.len() == regions.len()
            && previous.iter().zip(regions).all(|(a, b)| {
                a.x == b.x
                    && a.y == b.y
                    && a.width == b.width
                    && a.height == b.height
                    && a.clip_x == b.clip_x
                    && a.clip_y == b.clip_y
                    && a.clip_width == b.clip_width
                    && a.clip_height == b.clip_height
                    && a.border_rgba == b.border_rgba
            })
    }) {
        return Ok(());
    }
    if let Some(index) = all
        .iter()
        .position(|r| r.owner == window.owner && r.window == window.id)
    {
        let registration = &mut all[index];
        if registration.session != window.session {
            registration.regions.clear();
            registration.hover_regions.clear();
            registration.session = window.session;
        }
        if hover_only {
            registration.hover_regions = regions.to_vec();
        } else {
            registration.regions = regions.to_vec();
        }
        if registration.regions.is_empty() && registration.hover_regions.is_empty() {
            all.remove(index);
        }
    } else if !regions.is_empty() {
        if all.len() >= MAX_REGISTRATIONS {
            return Err(-7);
        }
        all.push(Registration {
            owner: window.owner,
            window: window.id,
            session: window.session,
            regions: if hover_only {
                Vec::new()
            } else {
                regions.to_vec()
            },
            hover_regions: if hover_only {
                regions.to_vec()
            } else {
                Vec::new()
            },
        });
    }
    drop(all);
    super::notify_cursor_visual_change();
    Ok(())
}
pub(super) fn remove_window(owner: WindowOwner, window: WindowId) {
    REGIONS
        .lock()
        .retain(|r| r.owner != owner || r.window != window);
}
pub(super) fn remove_owner(owner: WindowOwner) {
    REGIONS.lock().retain(|r| r.owner != owner);
}
fn rectangle(r: &Region) -> Rect {
    Rect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    }
}
fn clip(r: &Region) -> Rect {
    Rect {
        x: r.clip_x,
        y: r.clip_y,
        width: r.clip_width,
        height: r.clip_height,
    }
}
fn screen_rect(w: &WindowSnapshot) -> Rect {
    let p = w.presentation_placement;
    Rect {
        x: p.x,
        y: p.y,
        width: p.width,
        height: p.height,
    }
}
fn rank(w: &WindowSnapshot) -> (usize, i32, WindowId) {
    (w.plane.slot(), w.placement.z, w.id)
}
fn tick(now: Instant) -> u64 {
    now.as_millis().saturating_mul(15) / 1000
}
pub(super) fn due(now: Instant) -> bool {
    ANIMATED.load(Ordering::Relaxed) && tick(now) != LAST_TICK.load(Ordering::Relaxed)
}
pub(super) fn wait(now: Instant) -> Option<Duration> {
    ANIMATED.load(Ordering::Relaxed).then(|| {
        Duration::from_millis(
            ((tick(now) + 1) * 1000)
                .div_ceil(15)
                .saturating_sub(now.as_millis())
                .max(1),
        )
    })
}

pub(super) fn append(rects: &mut Vec<crate::intel::LiveOverlayRect>, screen_w: u32, screen_h: u32) {
    let now = Instant::now();
    let phase = tick(now);
    LAST_TICK.store(phase, Ordering::Relaxed);
    let cursors = super::input_broker::button_outline_cursors();
    let windows = super::visible_windows_for_output(super::OutputId::from_slot(0).unwrap());
    let all = REGIONS.lock();
    let mut effects: Vec<Effect> = Vec::new();
    for cursor in cursors {
        let selected = super::selected_frame_for_source(cursor.source);
        let window = windows
            .iter()
            .filter(|w| {
                w.state == WindowState::Ready
                    && w.interaction.hit_testable
                    && super::input_broker::placement_contains(
                        w.presentation_placement,
                        w.arc,
                        cursor.x,
                        cursor.y,
                    )
            })
            .max_by_key(|w| rank(w));
        let Some(window) = window else {
            continue;
        };
        if selected != Some(super::CursorFrameKey::new(window.owner, window.id)) {
            continue;
        }
        // Presentation-only resize animations have no stable local pixel map.
        if window.presentation_placement.width != window.placement.width
            || window.presentation_placement.height != window.placement.height
        {
            continue;
        }
        let Some(reg) = all.iter().find(|r| {
            r.owner == window.owner && r.window == window.id && r.session == window.session
        }) else {
            continue;
        };
        let x = cursor.x as i32 - window.presentation_placement.x;
        let y = cursor.y as i32 - window.presentation_placement.y;
        let Some((index, region, hover_only)) = reg
            .regions
            .iter()
            .enumerate()
            .rev()
            .find(|(_, r)| rectangle(r).contains(x, y) && clip(r).contains(x, y))
            .map(|(index, region)| (index, region, false))
            .or_else(|| {
                reg.hover_regions
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, r)| rectangle(r).contains(x, y) && clip(r).contains(x, y))
                    .map(|(index, region)| (index, region, true))
            })
        else {
            continue;
        };
        if let Some(existing) = effects.iter_mut().find(|e| {
            e.window.id == window.id
                && e.window.owner == window.owner
                && e.index == index
                && e.hover_only == hover_only
        }) {
            existing.pressed |= cursor.pressed && !hover_only;
        } else {
            effects.push(Effect {
                window: *window,
                index,
                region: *region,
                pressed: cursor.pressed && !hover_only,
                hover_only,
            });
        }
    }
    drop(all);
    ANIMATED.store(effects.iter().any(|e| !e.pressed), Ordering::Relaxed);
    let screen = Rect {
        x: 0,
        y: 0,
        width: screen_w,
        height: screen_h,
    };
    for effect in effects {
        let p = effect.window.presentation_placement;
        let color = if effect.hover_only {
            let mut ink = effect.region.border_rgba.to_le_bytes();
            ink[3] = (u16::from(ink[3]) * 128 / 255) as u8;
            ink
        } else {
            geometry::contrast(effect.region.border_rgba.to_le_bytes())
        };
        let window_clip = screen_rect(&effect.window);
        for segment in geometry::outline(rectangle(&effect.region), effect.pressed, phase as u32) {
            let Some(segment) = segment.intersection(clip(&effect.region)) else {
                continue;
            };
            let segment = Rect {
                x: segment.x + p.x,
                y: segment.y + p.y,
                ..segment
            };
            let Some(segment) = segment
                .intersection(window_clip)
                .and_then(|r| r.intersection(screen))
            else {
                continue;
            };
            let mut pieces = alloc::vec![segment];
            for higher in windows.iter().filter(|w| rank(w) > rank(&effect.window)) {
                let mut next = Vec::new();
                for piece in pieces {
                    next.extend(geometry::subtract(piece, screen_rect(higher)));
                }
                if next.len() > 128 {
                    next.clear();
                }
                pieces = next;
                if pieces.is_empty() {
                    break;
                }
            }
            for piece in pieces {
                rects.push(crate::intel::LiveOverlayRect {
                    x: piece.x as u32,
                    y: piece.y as u32,
                    width: piece.width,
                    height: piece.height,
                    color: crate::graphics::primitives::Rgba8::new(
                        color[0], color[1], color[2], color[3],
                    ),
                });
            }
        }
    }
}
