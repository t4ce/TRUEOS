//! Cached, transparent CHROMATIC controls. Rectangles and glyph copies use BCS0;
//! only the handles and hue curve change while a slider is dragged.
use super::chroma_color::Settings;
use crate::intel::{
    GucBcs0CopyCompletion, GucBcs0CopySubmitError, GucBcs0RgbaCopy, GucBcs0RgbaFill,
    GucBcs0RgbaSurface,
    bcs_plan::{BcsOp, BcsPlan, CopyBackend, SourceCache},
    gpgpu::GpgpuOwnedRgba8Surface,
};
use alloc::vec::Vec;
use trueos_time::{Duration, Instant, Timer};

const GRAY: u32 = u32::from_le_bytes([64, 64, 64, 128]);
const TEXT_COLOR: [u8; 4] = [0, 0, 0, 255];
// The font API takes straight RGBA and premultiplies it into the cached cell.
const TEXT_BACKGROUND: [u8; 4] = [128, 128, 128, 128];
const PIPE_BORDER: u32 = u32::from_le_bytes([0, 0, 0, 128]);
const PIPE_SELECTED: u32 = u32::from_le_bytes([0, 0, 0, 255]);
const LABELS: [&str; 5] = [
    "SOFTEN YELLOWS",
    "SHADOW TINT",
    "HIGHLIGHT TINT",
    "SATURATION",
    "STRENGTH",
];

#[path = "chroma_layout.rs"]
mod layout;
pub(super) use layout::Layout;
use layout::Rect;

impl Layout {
    pub fn curve_point_at(&self, x: i32, y: i32, settings: &Settings) -> Option<usize> {
        self.curve_handle_at(x, y, &settings.sat_curve)
    }
}

pub(super) struct Controls {
    pub layout: Layout,
    cache: GpgpuOwnedRgba8Surface,
    in_flight: bool,
}
impl Drop for Controls {
    fn drop(&mut self) {
        if self.in_flight {
            self.cache.quarantine_backing();
        }
    }
}
impl Controls {
    pub async fn new(width: u32, height: u32) -> Result<Self, &'static str> {
        let mut layout = Layout::new(width, height)?;
        crate::r::services::shell3_font_service::prepare().await?;
        // Fit shared physical font metrics to the controls column, then use
        // half-sized labels.
        if let Some((cell_width, cell_height)) =
            crate::r::services::shell3_font_service::cell_extent(
                crate::r::services::shell3_font_service::DEFAULT_UI4_FONT_SCALE,
            )
        {
            let available = layout.controls_width - 2 * layout.margin;
            let fitted_width = cell_width.min(available / 18).max(1);
            let fitted_height = (cell_height * fitted_width / cell_width)
                .min(layout.row_height / 3)
                .max(1);
            layout.cell_height = fitted_height;
            layout.cell_width = (cell_width * fitted_height / cell_height).max(1);
        }
        layout.cell_width = (layout.cell_width / 2).max(1);
        layout.cell_height = (layout.cell_height / 2).max(1);
        let cache = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(
            layout.controls_width,
            height,
        )
        .ok_or("chroma-controls-cache-allocation")?;
        let mut controls = Self {
            layout,
            cache,
            in_flight: false,
        };
        let mut operations = Vec::new();
        // Static tracks, curve baseline and labels are drawn exactly once per size.
        let l = controls.layout;
        for (index, label) in ["A", "B"].into_iter().enumerate() {
            let mut button = l.pipe_button(index);
            button.x -= l.controls_x;
            fill(&mut operations, button, GRAY);
            border(&mut operations, button, 1, PIPE_BORDER);
            text_sized(
                &mut operations,
                &l,
                label,
                button.x + 4,
                button.y + 4,
                2 * l.cell_width,
                2 * l.cell_height,
            )?;
        }
        for i in 0..5 {
            let mut r = l.slider_track(i);
            r.x -= l.controls_x;
            fill(&mut operations, r, GRAY);
            text(
                &mut operations,
                &l,
                LABELS[i],
                l.margin,
                l.slider_start + i as u32 * l.row_height,
            )?;
        }
        text(
            &mut operations,
            &l,
            "HUE / SATURATION",
            l.margin,
            l.curve.y - l.cell_height - 10,
        )?;
        fill(
            &mut operations,
            Rect {
                x: l.margin,
                y: l.curve.y + l.curve.height / 2,
                width: l.curve.width,
                height: 1,
            },
            GRAY,
        );
        for index in 0..7 {
            let (x, _) = l.curve_point(index, 0.0);
            fill(
                &mut operations,
                Rect {
                    x: x - l.controls_x,
                    y: l.curve.y,
                    width: 1,
                    height: l.curve.height,
                },
                GRAY,
            );
        }
        // The outer guides also form the frame's left and right edges.
        for y in [l.curve.y, l.curve.y + l.curve.height - 1] {
            fill(
                &mut operations,
                Rect {
                    x: l.margin,
                    y,
                    width: l.curve.width,
                    height: 1,
                },
                GRAY,
            );
        }
        let s = controls.cache_surface();
        let plan = BcsPlan::new(s, &operations).map_err(|_| "chroma-controls-cache-plan")?;
        let mut poisoned = false;
        // Pin the cache before the first await: cancellation can otherwise
        // drop its backing while an admitted BCS0 batch still references it.
        controls.in_flight = true;
        let result = execute(&plan, &mut poisoned).await;
        controls.in_flight = poisoned;
        result?;
        Ok(controls)
    }
    fn cache_surface(&self) -> GucBcs0RgbaSurface {
        let s = self.cache.surface();
        GucBcs0RgbaSurface {
            phys: s.phys,
            gpu: s.gpu,
            bytes: s.bytes,
            width: s.width,
            height: s.height,
            pitch_bytes: s.pitch_bytes,
        }
    }
    /// Clears the cube region, seeds cached controls, then draws only the changing
    /// curve and bars. The caller owns and pins the destination through retirement.
    pub async fn paint(
        &mut self,
        destination: GucBcs0RgbaSurface,
        settings: &Settings,
        selected_pipe: usize,
        poisoned: &mut bool,
    ) -> Result<(), &'static str> {
        if *poisoned || self.in_flight {
            return Err("chroma-controls-pinned");
        }
        let l = self.layout;
        if destination.width != l.width || destination.height != l.height {
            return Err("chroma-controls-size");
        }
        let mut ops = Vec::new();
        fill(
            &mut ops,
            Rect {
                x: 0,
                y: 0,
                width: l.cube_width,
                height: l.height,
            },
            0,
        );
        ops.push(BcsOp::Copy {
            copy: GucBcs0RgbaCopy {
                source: self.cache_surface(),
                source_x: 0,
                source_y: 0,
                destination_x: l.controls_x,
                destination_y: 0,
                width: l.controls_width,
                height: l.height,
            },
            backend: CopyBackend::PreferFast,
            source_cache: SourceCache::WriteBack,
        });
        if selected_pipe < 2 {
            border(&mut ops, l.pipe_button(selected_pipe), 1, PIPE_SELECTED);
        }
        let values = [
            settings.yellow,
            settings.shadows,
            settings.highlights,
            settings.saturation,
            settings.strength,
        ];
        for (i, value) in values.into_iter().enumerate() {
            fill(&mut ops, l.slider_bar(i, value), GRAY);
        }
        // Smooth interpolation matches the saturation curve used by the LUT.
        let mut previous_y = l.curve_point(0, settings.sat_curve[0]).1;
        for offset in (0..l.curve.width).step_by(2) {
            let phase = offset as f32 / (l.curve.width - 1) as f32 * 6.0;
            let index = (phase as usize).min(5);
            let t = (phase - index as f32).clamp(0.0, 1.0);
            let smooth = t * t * (3.0 - 2.0 * t);
            let v = settings.sat_curve[index]
                + (settings.sat_curve[index + 1] - settings.sat_curve[index]) * smooth;
            let y = l.curve_point(0, v).1;
            fill(
                &mut ops,
                Rect {
                    x: l.curve.x + offset,
                    y: y.min(previous_y),
                    width: 2.min(l.curve.width - offset),
                    height: y.abs_diff(previous_y) + 2,
                },
                GRAY,
            );
            previous_y = y;
        }
        for i in 0..7 {
            let (x, y) = l.curve_point(i, settings.sat_curve[i]);
            circle(&mut ops, x, y, 4, hue_color(i), l.width, l.height);
        }
        let plan = BcsPlan::new(destination, &ops).map_err(|_| "chroma-controls-paint-plan")?;
        self.in_flight = true;
        let result = execute(&plan, poisoned).await;
        self.in_flight = *poisoned;
        result
    }
}

fn fill(ops: &mut Vec<BcsOp>, r: Rect, color: u32) {
    if r.width != 0 && r.height != 0 {
        ops.push(BcsOp::Fill(GucBcs0RgbaFill {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
            color,
        }));
    }
}
fn border(ops: &mut Vec<BcsOp>, r: Rect, thickness: u32, color: u32) {
    let thickness = thickness.min(r.width / 2).min(r.height / 2);
    if thickness == 0 {
        return;
    }
    for edge in [
        Rect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: thickness,
        },
        Rect {
            x: r.x,
            y: r.y + r.height - thickness,
            width: r.width,
            height: thickness,
        },
        Rect {
            x: r.x,
            y: r.y + thickness,
            width: thickness,
            height: r.height - 2 * thickness,
        },
        Rect {
            x: r.x + r.width - thickness,
            y: r.y + thickness,
            width: thickness,
            height: r.height - 2 * thickness,
        },
    ] {
        fill(ops, edge, color);
    }
}
fn text(ops: &mut Vec<BcsOp>, l: &Layout, label: &str, x: u32, y: u32) -> Result<(), &'static str> {
    text_sized(ops, l, label, x, y, l.cell_width, l.cell_height)
}
fn text_sized(
    ops: &mut Vec<BcsOp>,
    l: &Layout,
    label: &str,
    x: u32,
    y: u32,
    cell_width: u32,
    cell_height: u32,
) -> Result<(), &'static str> {
    for (i, ch) in label.chars().enumerate() {
        let left = x + i as u32 * cell_width;
        if left + cell_width > l.controls_width || y + cell_height > l.height {
            break;
        }
        let mut copy = crate::r::services::shell3_font_service::glyph(
            ch,
            cell_width,
            cell_height,
            TEXT_COLOR,
            TEXT_BACKGROUND,
            false,
        )?;
        copy.destination_x = left;
        copy.destination_y = y;
        ops.push(BcsOp::Copy {
            copy,
            backend: CopyBackend::Legacy,
            source_cache: SourceCache::WriteBack,
        });
    }
    Ok(())
}
fn hue_color(i: usize) -> u32 {
    let rgb = match i {
        0 | 6 => [255, 0, 0],
        1 => [255, 255, 0],
        2 => [0, 255, 0],
        3 => [0, 255, 255],
        4 => [0, 0, 255],
        _ => [255, 0, 255],
    };
    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255])
}
fn circle(ops: &mut Vec<BcsOp>, x: u32, y: u32, r: i32, color: u32, w: u32, h: u32) {
    for dy in -r..=r {
        let mut dx = r;
        while dx * dx + dy * dy > r * r {
            dx -= 1;
        }
        let left = (x as i32 - dx).max(0) as u32;
        let top = y as i32 + dy;
        let right = (x as i32 + dx + 1).max(0).min(w as i32) as u32;
        if top >= 0 && top < h as i32 && right > left {
            fill(
                ops,
                Rect {
                    x: left,
                    y: top as u32,
                    width: right - left,
                    height: 1,
                },
                color,
            );
        }
    }
}
async fn execute(plan: &BcsPlan, poisoned: &mut bool) -> Result<(), &'static str> {
    for batch in plan.batches() {
        let start = Instant::now();
        let submission = loop {
            match crate::intel::queue_guc_bcs0_plan_batch(batch) {
                Ok(s) => {
                    *poisoned = true;
                    break s;
                }
                Err(GucBcs0CopySubmitError::Busy)
                    if start.elapsed() < Duration::from_millis(500) =>
                {
                    Timer::after(Duration::from_millis(1)).await
                }
                Err(GucBcs0CopySubmitError::SubmitFailed) => {
                    *poisoned = true;
                    return Err("chroma-controls-submit-uncertain");
                }
                Err(_) => return Err("chroma-controls-submit-unavailable"),
            }
        };
        loop {
            match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
                GucBcs0CopyCompletion::Complete => {
                    *poisoned = false;
                    break;
                }
                GucBcs0CopyCompletion::Pending if start.elapsed() < Duration::from_millis(1000) => {
                    Timer::after(Duration::from_millis(1)).await
                }
                _ => return Err("chroma-controls-retirement-uncertain"),
            }
        }
    }
    Ok(())
}
