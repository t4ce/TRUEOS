use alloc::vec::Vec;

use lyon_tessellation::path::{Builder as LyonPathBuilder, Path as LyonPath};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex as LyonFillPoint,
    VertexBuffers as LyonPointBuffers,
};
use skrifa::{
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
    raw::TableProvider,
    FontRef, MetadataProvider,
};

const BOOT_FONT_BYTES: &[u8] = include_bytes!("../tools/L_10646.TTF");
const FONT_BENCH_REPEAT: usize = 256;
const ATHLAS_BENCH_VIEWPORT_WIDTH: u32 = 1024;
const ATHLAS_BENCH_VIEWPORT_HEIGHT: u32 = 256;
const SVG_VECTOR_BENCH_SAMPLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="192" height="128" viewBox="0 0 192 128"><path fill="#2f7fd0" d="M16 112 L60 16 L104 112 Z"/><path fill="#db5146" d="M88 112 C92 48 152 48 160 112 Z"/><path fill="#33a45f" fill-rule="evenodd" d="M116 24 C152 24 176 48 176 76 C176 104 152 124 116 124 C80 124 56 104 56 76 C56 48 80 24 116 24 Z M116 48 C94 48 80 60 80 76 C80 92 94 104 116 104 C138 104 152 92 152 76 C152 60 138 48 116 48 Z"/></svg>"##;

pub(crate) fn log_boot_font_probe() {
    match boot_font_probe_summary() {
        Ok(summary) => crate::log_info!(
            target: "boot";
            "font-probe: result=ok parser=skrifa font=L_10646.TTF bytes={} tables={} glyphs={} units_per_em={} cmap={} glyph_A={} glyph_space={}\n",
            summary.bytes,
            summary.tables,
            summary.glyphs,
            summary.units_per_em,
            summary.cmap_status,
            summary.glyph_a,
            summary.glyph_space
        ),
        Err(err) => crate::log_warn!(
            target: "boot";
            "font-probe: result=failed parser=skrifa font=L_10646.TTF bytes={} err={:?}\n",
            BOOT_FONT_BYTES.len(),
            err
        ),
    }
}

#[derive(Debug)]
pub(crate) struct FontProbeSummary {
    pub(crate) bytes: usize,
    pub(crate) tables: usize,
    pub(crate) glyphs: u16,
    pub(crate) units_per_em: u16,
    pub(crate) cmap_status: &'static str,
    pub(crate) glyph_a: u32,
    pub(crate) glyph_space: u32,
}

#[derive(Debug)]
pub(crate) struct FontStackFaceSummary {
    pub(crate) family: &'static str,
    pub(crate) tier: &'static str,
    pub(crate) slots: u32,
    pub(crate) line_height_px: u16,
}

#[derive(Debug)]
pub(crate) struct FontStackSummary {
    pub(crate) athlas_faces: Vec<FontStackFaceSummary>,
    pub(crate) athlas_slots: u32,
    pub(crate) twemoji_slots: u16,
    pub(crate) sprite64_slots: u32,
    pub(crate) sprite64_kernel: &'static str,
    pub(crate) sprite64_atlas: &'static str,
    pub(crate) glyph_mask_kernel: &'static str,
    pub(crate) skrifa: Option<FontProbeSummary>,
    pub(crate) svg_lyon: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AthlasBenchSummary {
    pub(crate) sample: &'static str,
    pub(crate) repeats: usize,
    pub(crate) face_family: &'static str,
    pub(crate) face_tier: &'static str,
    pub(crate) chars: usize,
    pub(crate) whitespace: usize,
    pub(crate) glyph_hits: usize,
    pub(crate) glyph_misses: usize,
    pub(crate) clipped: usize,
    pub(crate) placements: usize,
    pub(crate) slot_misses: usize,
    pub(crate) glyph_lookup_ms: u64,
    pub(crate) slot_ms: u64,
    pub(crate) placement_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SkrifaBenchSummary {
    pub(crate) repeats: usize,
    pub(crate) bytes: usize,
    pub(crate) tables: usize,
    pub(crate) glyphs: u16,
    pub(crate) units_per_em: u16,
    pub(crate) sample_chars: usize,
    pub(crate) charmap_hits: usize,
    pub(crate) charmap_misses: usize,
    pub(crate) parse_ms: u64,
    pub(crate) charmap_ms: u64,
    pub(crate) outline_ms: u64,
    pub(crate) outline_tessellate_ms: u64,
    pub(crate) outline_attempts: usize,
    pub(crate) outline_success: usize,
    pub(crate) outline_failures: usize,
    pub(crate) outline_commands: usize,
    pub(crate) outline_tessellate_success: usize,
    pub(crate) outline_tessellate_failures: usize,
    pub(crate) outline_vertices: usize,
    pub(crate) outline_indices: usize,
    pub(crate) outline_triangles: usize,
    pub(crate) outline_status: &'static str,
    pub(crate) total_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct VectorBenchSummary {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) primitives: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    pub(crate) triangles: usize,
    pub(crate) pixels: usize,
    pub(crate) rgba_bytes: usize,
    pub(crate) parse_ms: u64,
    pub(crate) tessellate_ms: u64,
    pub(crate) paint_ms: u64,
    pub(crate) upload_ms: u64,
    pub(crate) upload_status: &'static str,
    pub(crate) total_ms: u64,
}

pub(crate) fn boot_font_probe_summary() -> Result<FontProbeSummary, skrifa::raw::ReadError> {
    let font = FontRef::new(BOOT_FONT_BYTES)?;
    let head = font.head()?;
    let maxp = font.maxp()?;
    let charmap = font.charmap();
    let glyph_a = charmap.map('A' as u32).map(|gid| gid.to_u32()).unwrap_or(0);
    let glyph_space = charmap
        .map(' ' as u32)
        .map(|gid| gid.to_u32())
        .unwrap_or(0);
    let cmap_status = if charmap.has_map() { "present" } else { "missing" };

    Ok(FontProbeSummary {
        bytes: BOOT_FONT_BYTES.len(),
        tables: font.table_directory().table_records().len(),
        glyphs: maxp.num_glyphs(),
        units_per_em: head.units_per_em(),
        cmap_status,
        glyph_a,
        glyph_space,
    })
}

pub(crate) fn font_stack_summary() -> FontStackSummary {
    use crate::ui3::althlasfont::bitmapfont::{
        ATHLAS_UI3_SPRITE64_FONT_FACES, athlas_font_face_cell_count,
        athlas_font_family_name, athlas_font_line_height_px, athlas_font_tier_name,
    };

    let mut athlas_faces = Vec::with_capacity(ATHLAS_UI3_SPRITE64_FONT_FACES.len());
    let mut athlas_slots = 0u32;
    for face in ATHLAS_UI3_SPRITE64_FONT_FACES {
        let slots = athlas_font_face_cell_count(face).unwrap_or(0);
        athlas_slots = athlas_slots.saturating_add(slots);
        athlas_faces.push(FontStackFaceSummary {
            family: athlas_font_family_name(face.family),
            tier: athlas_font_tier_name(face.tier),
            slots,
            line_height_px: athlas_font_line_height_px(face).unwrap_or(0),
        });
    }

    let sprite64_slots = crate::intel::gpgpu::sprite64_font_slot_count().unwrap_or(0);
    FontStackSummary {
        athlas_faces,
        athlas_slots,
        twemoji_slots: crate::ui3::althlasfont::twemoji::twemoji_slot_count(),
        sprite64_slots,
        sprite64_kernel: status_word(crate::intel::gpgpu::sprite64_worklist_rgba8_upload_status()
            .is_some()),
        sprite64_atlas: if sprite64_slots != 0 {
            "addressable"
        } else {
            "missing"
        },
        glyph_mask_kernel: status_word(
            crate::intel::gpgpu::glyph_mask_rgba8_upload_status().is_some(),
        ),
        skrifa: boot_font_probe_summary().ok(),
        svg_lyon: "compiled:usvg+tiny-skia-path+lyon_tessellation+cpu-rgba",
    }
}

pub(crate) fn bench_athlas_samples() -> Vec<AthlasBenchSummary> {
    use crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;

    const SAMPLES: [(&str, &str); 4] = [
        ("short", "TRUEOS font"),
        (
            "medium",
            "The quick brown fox visits UI3 textRuns, sprite64 slots, and layout paint.",
        ),
        ("unicode-heavy", "UI3: ΔЖ漢字 אבג देवनागरी 😀 ✅ 🟦"),
        ("missing-glyph", "Missing probe: \u{10FFFF}\u{E000}\u{0378}\u{2B740}"),
    ];

    let mut out = Vec::with_capacity(SAMPLES.len());
    for (name, text) in SAMPLES {
        out.push(bench_athlas_text(name, text, ATHLAS_FONT_FACE_LUCIDA_HALF));
    }
    out
}

fn bench_athlas_text(
    sample: &'static str,
    text: &str,
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
) -> AthlasBenchSummary {
    use crate::ui3::althlasfont::bitmapfont::{
        athlas_font_family_name, athlas_font_line_height_px, athlas_font_tier_name,
        athlas_glyph_advance_px, athlas_lookup_glyph_region,
    };

    let total_start = embassy_time_driver::now();
    let line_height = athlas_font_line_height_px(face).unwrap_or(22) as f32;
    let preserved_space_advance = line_height * 0.35;
    let max_draw_x = ATHLAS_BENCH_VIEWPORT_WIDTH
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let max_draw_y = ATHLAS_BENCH_VIEWPORT_HEIGHT
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let mut chars = 0usize;
    let mut whitespace = 0usize;
    let mut glyph_hits = 0usize;
    let mut glyph_misses = 0usize;
    let mut clipped = 0usize;
    let mut placements = 0usize;
    let mut slot_misses = 0usize;
    let mut glyph_lookup_ms = 0u64;
    let mut slot_ms = 0u64;
    let mut placement_ms = 0u64;

    for repeat in 0..FONT_BENCH_REPEAT {
        let mut pen_x = 16.0f32;
        let dst_y = 16 + ((repeat % 4) as i32 * line_height as i32);
        if dst_y < 0 || dst_y > max_draw_y {
            clipped = clipped.saturating_add(text.chars().count());
            continue;
        }
        for ch in text.chars() {
            chars = chars.saturating_add(1);
            if ch.is_control() {
                continue;
            }
            if ch.is_whitespace() {
                whitespace = whitespace.saturating_add(1);
                pen_x += preserved_space_advance;
                continue;
            }
            let lookup_start = embassy_time_driver::now();
            let region = athlas_lookup_glyph_region(face, ch);
            glyph_lookup_ms = glyph_lookup_ms.saturating_add(elapsed_ms_since(lookup_start));
            let Some(region) = region else {
                glyph_misses = glyph_misses.saturating_add(1);
                pen_x += preserved_space_advance;
                continue;
            };
            glyph_hits = glyph_hits.saturating_add(1);
            let advance = f32::from(athlas_glyph_advance_px(region));
            let dst_x = floor_i32(pen_x);
            if dst_x < 0 || dst_x > max_draw_x {
                clipped = clipped.saturating_add(1);
                pen_x += advance;
                continue;
            }
            let slot_start = embassy_time_driver::now();
            let slot = crate::intel::gpgpu::sprite64_font_slot_for_region(face, region);
            slot_ms = slot_ms.saturating_add(elapsed_ms_since(slot_start));
            if slot.is_none() {
                slot_misses = slot_misses.saturating_add(1);
                pen_x += advance;
                continue;
            }
            let placement_start = embassy_time_driver::now();
            placements = placements.saturating_add(1);
            placement_ms = placement_ms.saturating_add(elapsed_ms_since(placement_start));
            pen_x += advance;
        }
    }

    AthlasBenchSummary {
        sample,
        repeats: FONT_BENCH_REPEAT,
        face_family: athlas_font_family_name(face.family),
        face_tier: athlas_font_tier_name(face.tier),
        chars,
        whitespace,
        glyph_hits,
        glyph_misses,
        clipped,
        placements,
        slot_misses,
        glyph_lookup_ms,
        slot_ms,
        placement_ms,
        total_ms: elapsed_ms_since(total_start),
    }
}

pub(crate) fn bench_skrifa() -> Result<SkrifaBenchSummary, skrifa::raw::ReadError> {
    const SAMPLE: &str = "AaZz 09 ΔЖ漢字 אבג देवनागरी 😀 \u{10FFFF}";

    let total_start = embassy_time_driver::now();
    let parse_start = embassy_time_driver::now();
    for _ in 0..FONT_BENCH_REPEAT {
        let _ = FontRef::new(BOOT_FONT_BYTES)?;
    }
    let parse_ms = elapsed_ms_since(parse_start);

    let font = FontRef::new(BOOT_FONT_BYTES)?;
    let head = font.head()?;
    let maxp = font.maxp()?;
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let mut sample_chars = 0usize;
    let mut charmap_hits = 0usize;
    let mut charmap_misses = 0usize;
    let charmap_start = embassy_time_driver::now();
    for _ in 0..FONT_BENCH_REPEAT {
        for ch in SAMPLE.chars() {
            if ch.is_control() {
                continue;
            }
            sample_chars = sample_chars.saturating_add(1);
            if charmap.map(ch as u32).is_some() {
                charmap_hits = charmap_hits.saturating_add(1);
            } else {
                charmap_misses = charmap_misses.saturating_add(1);
            }
        }
    }
    let charmap_ms = elapsed_ms_since(charmap_start);

    let mut outline_attempts = 0usize;
    let mut outline_success = 0usize;
    let mut outline_failures = 0usize;
    let mut outline_commands = 0usize;
    let mut outline_tessellate_success = 0usize;
    let mut outline_tessellate_failures = 0usize;
    let mut outline_vertices = 0usize;
    let mut outline_indices = 0usize;
    let mut tessellator = FillTessellator::new();
    let outline_start = embassy_time_driver::now();
    let mut outline_tessellate_ms = 0u64;
    for _ in 0..FONT_BENCH_REPEAT {
        for ch in SAMPLE.chars() {
            if ch.is_control() {
                continue;
            }
            let Some(glyph_id) = charmap.map(ch as u32) else {
                continue;
            };
            outline_attempts = outline_attempts.saturating_add(1);
            let Some(glyph) = outlines.get(glyph_id) else {
                outline_failures = outline_failures.saturating_add(1);
                continue;
            };
            let settings = DrawSettings::unhinted(Size::new(16.0), LocationRef::default());
            let mut outline_pen = OutlineLyonPen::default();
            match glyph.draw(settings, &mut outline_pen) {
                Ok(_) => {
                    outline_success = outline_success.saturating_add(1);
                    outline_commands = outline_commands.saturating_add(outline_pen.commands);
                    let path = outline_pen.build();
                    if path.iter().next().is_none() {
                        continue;
                    }
                    let mut buffers: LyonPointBuffers<[f32; 2], u32> = Default::default();
                    let tessellate_start = embassy_time_driver::now();
                    let tessellated = tessellator.tessellate_path(
                        &path,
                        &FillOptions::default(),
                        &mut BuffersBuilder::new(&mut buffers, |point: LyonFillPoint| {
                            [point.position().x, point.position().y]
                        }),
                    );
                    outline_tessellate_ms =
                        outline_tessellate_ms.saturating_add(elapsed_ms_since(tessellate_start));
                    match tessellated {
                        Ok(_) => {
                            outline_tessellate_success =
                                outline_tessellate_success.saturating_add(1);
                            outline_vertices =
                                outline_vertices.saturating_add(buffers.vertices.len());
                            outline_indices =
                                outline_indices.saturating_add(buffers.indices.len());
                        }
                        Err(_) => {
                            outline_tessellate_failures =
                                outline_tessellate_failures.saturating_add(1);
                        }
                    }
                }
                Err(_) => {
                    outline_failures = outline_failures.saturating_add(1);
                }
            }
        }
    }
    let outline_ms = elapsed_ms_since(outline_start);
    let outline_status = if outline_success != 0 {
        "ok:outline-draw-commands"
    } else if outline_attempts != 0 {
        "failed:no-outline-draw-success"
    } else {
        "not-run:no-charmap-hits"
    };

    Ok(SkrifaBenchSummary {
        repeats: FONT_BENCH_REPEAT,
        bytes: BOOT_FONT_BYTES.len(),
        tables: font.table_directory().table_records().len(),
        glyphs: maxp.num_glyphs(),
        units_per_em: head.units_per_em(),
        sample_chars,
        charmap_hits,
        charmap_misses,
        parse_ms,
        charmap_ms,
        outline_ms,
        outline_tessellate_ms,
        outline_attempts,
        outline_success,
        outline_failures,
        outline_commands,
        outline_tessellate_success,
        outline_tessellate_failures,
        outline_vertices,
        outline_indices,
        outline_triangles: outline_indices / 3,
        outline_status,
        total_ms: elapsed_ms_since(total_start),
    })
}

pub(crate) fn bench_vector_svg() -> Result<VectorBenchSummary, i32> {
    let (info, rgba, stats) =
        crate::graphics::svg::render_svg_text_rgba_profile(SVG_VECTOR_BENCH_SAMPLE)?;
    Ok(VectorBenchSummary {
        width: info.width,
        height: info.height,
        primitives: stats.primitives,
        vertices: stats.vertices,
        indices: stats.indices,
        triangles: stats.triangles,
        pixels: stats.pixels,
        rgba_bytes: rgba.len(),
        parse_ms: stats.parse_ms,
        tessellate_ms: stats.tessellate_ms,
        paint_ms: stats.paint_ms,
        upload_ms: stats.upload_ms,
        upload_status: stats.upload_status,
        total_ms: stats.total_ms,
    })
}

fn status_word(ok: bool) -> &'static str {
    if ok { "ready" } else { "missing" }
}

struct OutlineLyonPen {
    builder: LyonPathBuilder,
    commands: usize,
}

impl OutlineLyonPen {
    fn build(self) -> LyonPath {
        self.builder.build()
    }
}

impl Default for OutlineLyonPen {
    fn default() -> Self {
        Self {
            builder: LyonPath::builder(),
            commands: 0,
        }
    }
}

impl OutlinePen for OutlineLyonPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands = self.commands.saturating_add(1);
        self.builder.begin(lyon_tessellation::math::point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.commands = self.commands.saturating_add(1);
        self.builder.line_to(lyon_tessellation::math::point(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.commands = self.commands.saturating_add(1);
        self.builder.quadratic_bezier_to(
            lyon_tessellation::math::point(cx0, cy0),
            lyon_tessellation::math::point(x, y),
        );
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.commands = self.commands.saturating_add(1);
        self.builder.cubic_bezier_to(
            lyon_tessellation::math::point(cx0, cy0),
            lyon_tessellation::math::point(cx1, cy1),
            lyon_tessellation::math::point(x, y),
        );
    }

    fn close(&mut self) {
        self.commands = self.commands.saturating_add(1);
        self.builder.close();
    }
}

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn elapsed_ms_since(start: u64) -> u64 {
    let now = embassy_time_driver::now();
    let ticks = now.saturating_sub(start);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}
