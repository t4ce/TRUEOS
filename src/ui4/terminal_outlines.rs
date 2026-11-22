//! Cell outlines follow a published Shell3 surface on the interaction plane.
use super::button_outline_geometry::{Rect, subtract};
use super::slot4_service::rect_border_parts;
use super::{
    FrameHandle, OutputId, Ui4VisualRect, WindowId, WindowOwner, WindowSessionId, WindowSnapshot,
    WindowState,
};
use alloc::vec::Vec;
use spin::Mutex;
use trueos_terminal::cell_outlines::{CellOutlines, CellRect};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Registration {
    window: WindowId,
    session: WindowSessionId,
    frame: FrameHandle,
    cell_size: (u32, u32),
    extent: (u32, u32),
    cells: CellOutlines,
}

static OUTLINES: Mutex<Vec<Registration>> = Mutex::new(Vec::new());

pub(crate) fn publish(
    window: WindowId,
    session: WindowSessionId,
    frame: FrameHandle,
    cell_size: (u32, u32),
    extent: (u32, u32),
    cells: CellOutlines,
) {
    let next = Registration {
        window,
        session,
        frame,
        cell_size,
        extent,
        cells,
    };
    let mut all = OUTLINES.lock();
    match all.iter().position(|old| old.window == window) {
        Some(index) if all[index] == next => return,
        Some(index) => all[index] = next,
        None if all.len() < super::window_broker::MAX_WINDOWS => all.push(next),
        None => return,
    }
    drop(all);
    super::notify_cursor_visual_change();
}

pub(crate) fn clear(window: WindowId) {
    let mut all = OUTLINES.lock();
    let Some(index) = all.iter().position(|old| old.window == window) else {
        return;
    };
    all.remove(index);
    drop(all);
    super::notify_cursor_visual_change();
}

pub(super) fn has_active() -> bool {
    !OUTLINES.lock().is_empty()
}

pub(super) fn append(rects: &mut Vec<crate::intel::LiveOverlayRect>, screen_w: u32, screen_h: u32) {
    if !has_active() {
        return;
    }
    let Some(output) = OutputId::from_slot(0) else {
        return;
    };
    let windows = super::visible_windows_for_output(output);
    let screen = Rect {
        x: 0,
        y: 0,
        width: screen_w,
        height: screen_h,
    };
    let all = OUTLINES.lock();
    for outline in all.iter() {
        let Some(window) = windows.iter().find(|window| outline.applies_to(window)) else {
            continue;
        };
        let frame = window_rect(window);
        let Some(clip) = frame.intersection(screen) else {
            continue;
        };
        let own_rank = rank(window);
        let covers: Vec<_> = windows
            .iter()
            .filter(|other| {
                rank(other) > own_rank
                    && other.presentation_placement.visible
                    && other.presentation_placement.opacity != 0
            })
            .filter_map(|other| clip.intersection(window_rect(other)))
            .collect();
        let color = crate::graphics::primitives::Rgba8::new(255, 194, 110, 128);
        let mut push = |part: Rect| {
            rects.push(crate::intel::LiveOverlayRect::new(
                part.x as u32,
                part.y as u32,
                part.width,
                part.height,
                color,
            ))
        };
        for &cell in outline.cells.as_slice() {
            for edge in cell_edges(cell, outline.cell_size, frame) {
                if let Some(visible) = edge.intersection(clip) {
                    if covers.is_empty() {
                        push(visible);
                    } else {
                        for part in visible_parts(visible, &covers) {
                            push(part);
                        }
                    }
                }
            }
        }
    }
}

impl Registration {
    fn applies_to(self, window: &WindowSnapshot) -> bool {
        let p = window.presentation_placement;
        window.owner == WindowOwner::SHELL3_SERVICE
            && window.id == self.window
            && window.session == self.session
            && window.frame == self.frame
            && window.state == WindowState::Ready
            && p.visible
            && p.opacity != 0
            && (p.width, p.height) == self.extent
            && (window.placement.width, window.placement.height) == self.extent
    }
}

fn rank(window: &WindowSnapshot) -> (usize, i32, WindowId) {
    (window.plane.stack_rank(), window.placement.z, window.id)
}

fn window_rect(window: &WindowSnapshot) -> Rect {
    let p = window.presentation_placement;
    Rect {
        x: p.x,
        y: p.y,
        width: p.width,
        height: p.height,
    }
}

fn cell_edges(cell: CellRect, (cell_width, cell_height): (u32, u32), frame: Rect) -> [Rect; 4] {
    let local = Ui4VisualRect {
        x: u32::from(cell.column).saturating_mul(cell_width),
        y: u32::from(cell.row).saturating_mul(cell_height),
        width: u32::from(cell.width).saturating_mul(cell_width),
        height: u32::from(cell.height).saturating_mul(cell_height),
    };
    rect_border_parts(local, 1).map(|part| Rect {
        x: frame.x.saturating_add(part.x as i32),
        y: frame.y.saturating_add(part.y as i32),
        width: part.width,
        height: part.height,
    })
}

fn visible_parts(edge: Rect, covers: &[Rect]) -> Vec<Rect> {
    let mut parts = alloc::vec![edge];
    for cover in covers {
        parts = parts
            .into_iter()
            .flat_map(|part| subtract(part, *cover))
            .collect();
        if parts.is_empty() || parts.len() > 32 {
            // Avoid a fragmentation explosion behind many overlapping windows.
            return Vec::new();
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_edges_follow_window_origin_and_clip_without_inventing_an_edge() {
        let frame = Rect {
            x: -13,
            y: 30,
            width: 100,
            height: 60,
        };
        let rect = CellRect {
            column: 1,
            row: 2,
            width: 3,
            height: 1,
        };
        let screen = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let edges: Vec<_> = cell_edges(rect, (8, 12), frame)
            .into_iter()
            .filter_map(|edge| edge.intersection(screen))
            .collect();
        assert_eq!(
            edges,
            [
                Rect {
                    x: 0,
                    y: 54,
                    width: 19,
                    height: 1
                },
                Rect {
                    x: 0,
                    y: 65,
                    width: 19,
                    height: 1
                },
                Rect {
                    x: 18,
                    y: 55,
                    width: 1,
                    height: 10
                },
            ]
        );
        assert_eq!(
            visible_parts(
                edges[0],
                &[Rect {
                    x: 5,
                    y: 53,
                    width: 7,
                    height: 3
                }]
            ),
            [
                Rect {
                    x: 0,
                    y: 54,
                    width: 5,
                    height: 1
                },
                Rect {
                    x: 12,
                    y: 54,
                    width: 7,
                    height: 1
                },
            ]
        );
    }
}
