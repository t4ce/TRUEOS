use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Once;
use trueos_gfx_core::{RGB_VERTEX_SIZE, RgbVertexPx, Rgba8, ViewTransform, push_indexed_rgb_mesh_px};

use crate::gfx::svg::{SvgMeshDocument, SvgPaintStyle};

const LOADSCREEN_BG_RGB: u32 = 0xF2EEE8;
const LOADSCREEN_WAIT_POLL_MS: u64 = 250;
const LOADSCREEN_MIN_LIFETIME_MS: u64 = 4_000;
const LOADSCREEN_ANIM_FRAME_MS: u64 = 250;
const LOADSCREEN_WORDMARK_HEIGHT_PX: f32 = 300.0;

struct GlyphBounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl GlyphBounds {
    #[inline]
    fn width(&self) -> f32 {
        (self.max_x - self.min_x).max(0.0)
    }

    #[inline]
    fn height(&self) -> f32 {
        (self.max_y - self.min_y).max(0.0)
    }
}

struct LoadscreenGlyph {
    mesh: SvgMeshDocument,
    bounds: GlyphBounds,
}

static LOADSCREEN_GLYPHS: Once<Vec<LoadscreenGlyph>> = Once::new();

fn cached_loadscreen_glyphs() -> &'static [LoadscreenGlyph] {
    LOADSCREEN_GLYPHS.call_once(|| {
        let assets = [
            include_str!("T.svg"),
            include_str!("R.svg"),
            include_str!("U.svg"),
            include_str!("E.svg"),
            include_str!("O.svg"),
            include_str!("S.svg"),
        ];

        let mut glyphs = Vec::with_capacity(assets.len());
        for svg_text in assets {
            match crate::gfx::svg::tessellate_svg_text(svg_text) {
                Ok(mesh) => {
                    let bounds = glyph_bounds(&mesh);
                    glyphs.push(LoadscreenGlyph { mesh, bounds });
                }
                Err(rc) => crate::log!("gfx-loadscreen: tessellate failed rc={}\n", rc),
            }
        }

        glyphs
    })
}

fn glyph_bounds(mesh: &SvgMeshDocument) -> GlyphBounds {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for primitive in &mesh.primitives {
        for vertex in &primitive.vertices {
            min_x = min_x.min(vertex[0]);
            min_y = min_y.min(vertex[1]);
            max_x = max_x.max(vertex[0]);
            max_y = max_y.max(vertex[1]);
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        GlyphBounds {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    } else {
        GlyphBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: mesh.info.width.max(1) as f32,
            max_y: mesh.info.height.max(1) as f32,
        }
    }
}

fn framebuffer_extent() -> Option<(u32, u32)> {
    crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as u32, fb.height() as u32))
}

fn loadscreen_paint_rgba(paint: &SvgPaintStyle) -> Rgba8 {
    match paint {
        SvgPaintStyle::Solid { rgba } => Rgba8::new(
            (rgba[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            (rgba[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            (rgba[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            (rgba[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        ),
        _ => Rgba8::new(0, 0, 0, 255),
    }
}

fn append_glyph_mesh_blob(
    blob: &mut Vec<u8>,
    transform: ViewTransform,
    glyph: &LoadscreenGlyph,
    draw_x: f32,
    draw_y: f32,
    scale: f32,
) {
    for primitive in &glyph.mesh.primitives {
        let Ok(indices) = primitive
            .indices
            .iter()
            .map(|&index| u16::try_from(index))
            .collect::<Result<Vec<_>, _>>()
        else {
            continue;
        };

        let color = loadscreen_paint_rgba(&primitive.paint);
        let mut vertices = Vec::with_capacity(primitive.vertices.len());
        for vertex in &primitive.vertices {
            vertices.push(RgbVertexPx {
                x: draw_x + vertex[0] * scale,
                y: draw_y + vertex[1] * scale,
                color,
            });
        }

        push_indexed_rgb_mesh_px(blob, transform, vertices.as_slice(), indices.as_slice());
    }
}

fn draw_loadscreen_wordmark_no_present(view_w: u32, view_h: u32) -> bool {
    let glyphs = cached_loadscreen_glyphs();
    if glyphs.is_empty() {
        return true;
    }

    let tight_height = glyphs
        .iter()
        .map(|glyph| glyph.bounds.height())
        .fold(0.0f32, f32::max)
        .max(1.0);
    let scale = LOADSCREEN_WORDMARK_HEIGHT_PX / tight_height;
    let total_width = glyphs
        .iter()
        .map(|glyph| glyph.bounds.width() * scale)
        .sum::<f32>();
    let min_y = glyphs
        .iter()
        .map(|glyph| glyph.bounds.min_y)
        .fold(f32::INFINITY, f32::min);

    let start_x = ((view_w as f32) - total_width) * 0.5;
    let base_y = ((view_h as f32) - LOADSCREEN_WORDMARK_HEIGHT_PX) * 0.5 - min_y * scale;
    let transform = ViewTransform::from_extent(view_w, view_h);
    let capacity = glyphs
        .iter()
        .flat_map(|glyph| glyph.mesh.primitives.iter())
        .map(|primitive| primitive.indices.len().saturating_mul(RGB_VERTEX_SIZE))
        .sum();
    let mut blob = Vec::with_capacity(capacity);
    let mut cursor_x = start_x;

    for glyph in glyphs {
        let draw_x = cursor_x - glyph.bounds.min_x * scale;
        append_glyph_mesh_blob(&mut blob, transform, glyph, draw_x, base_y, scale);
        cursor_x += glyph.bounds.width() * scale;
    }

    if blob.is_empty() {
        return true;
    }

    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(blob.as_ptr(), blob.len())
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc == 0
}

fn render_loadscreen_frame(bg_rgb: u32) -> bool {
    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(bg_rgb) };
    if begin_rc != 0 {
        crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
        return false;
    }

    if let Some((view_w, view_h)) = framebuffer_extent() {
        let _ = draw_loadscreen_wordmark_no_present(view_w, view_h);
    }

    unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    true
}

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    crate::r::readiness::set_loadscreen_expire_requested(false);

    let mut first_frame_ms = 0u64;

    loop {
        crate::gfx::with_cabi_frame_lock(|| {
            if render_loadscreen_frame(LOADSCREEN_BG_RGB) && first_frame_ms == 0 {
                first_frame_ms = boot_probe_ms();
                crate::r::readiness::set(crate::r::readiness::LOADSCREEN_FRAME_READY);
            }
        });

        if first_frame_ms != 0 {
            break;
        }

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_WAIT_POLL_MS)).await;
    }

    let min_end_ms = first_frame_ms.saturating_add(LOADSCREEN_MIN_LIFETIME_MS);

    loop {
        let now_ms = boot_probe_ms();
        let min_lifetime_reached = now_ms >= min_end_ms;
        let expire_requested = crate::r::readiness::loadscreen_expire_requested();
        if min_lifetime_reached && expire_requested {
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
            break;
        }

        crate::gfx::with_cabi_frame_lock(|| {
            let _ = render_loadscreen_frame(LOADSCREEN_BG_RGB);
        });

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_ANIM_FRAME_MS)).await;
    }

    crate::log!(
        "boot-probe: loadscreen end ms={} lived_ms={}\n",
        boot_probe_ms(),
        boot_probe_ms().saturating_sub(first_frame_ms)
    );
}
