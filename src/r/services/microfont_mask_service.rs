//! One boot-lifetime geometry upload and one reusable, color-free R8 atlas.
//! Consumers execute while the atlas is locked; no asynchronous mask reader may
//! survive the callback. Width/size invalidate pixels, never resident geometry.
use crate::{
    intel::{
        gpgpu::GpgpuMask8Surface,
        render::{self, ResidentScenePrimitiveTopology as Topology},
    },
    ui4,
};
use alloc::vec::Vec;

pub(crate) const PREVIEW_HEADER_HEIGHT: u32 = 30;
pub(crate) const ATLAS_COLUMNS: u32 = 16;
pub(crate) const LATIN_SLOTS: usize = 95;
pub(crate) const BOX_START: usize = LATIN_SLOTS;
pub(crate) const BLOCK_START: usize = BOX_START + 128;
const EXTRA_START: usize = BLOCK_START + 32;
const fn is_extra_line(ch: char) -> bool {
    !(' ' <= ch && ch <= '~') && !('\u{2500}' <= ch && ch <= '\u{259f}')
}
const fn extra_count() -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < microfont::raster_line::GLYPH_CHARACTERS.len() {
        if is_extra_line(microfont::raster_line::GLYPH_CHARACTERS[i]) { count += 1; }
        i += 1;
    }
    count
}
pub(crate) const SLOT_COUNT: usize = EXTRA_START + extra_count();
pub(crate) const ATLAS_ROWS: u32 = SLOT_COUNT.div_ceil(ATLAS_COLUMNS as usize) as u32;
// Join recipes use seven bits of X and eight bits of Y. Reject overflow
// explicitly rather than silently changing the shape of a future glyph.
const _: () = assert!(ATLAS_COLUMNS * 6 <= 128 && ATLAS_ROWS * 11 <= 256);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AtlasConfig {
    /// Uniform source-cell scale: 16 gives a 96x176 glyph.
    pub(crate) glyph_scale: u32,
    /// Optional native-pixel height of the eleven-row em cell.
    pub(crate) glyph_height_px: Option<u32>,
    /// Native line width in pixels, rounded once to the engine's U11.7 step.
    pub(crate) line_width_px: f32,
}
impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            glyph_scale: 16,
            glyph_height_px: None,
            line_width_px: 8.0,
        }
    }
}
impl AtlasConfig {
    fn normalized(self) -> Result<Self, &'static str> {
        if !(1..=23).contains(&self.glyph_scale)
            || !self.line_width_px.is_finite()
            || self.line_width_px < 0.25
            || self.line_width_px > 32.0
            || (self.glyph_height_px.is_none() && self.line_width_px > self.glyph_scale as f32)
            || self.glyph_height_px.is_some_and(|h| !(11..=253).contains(&h))
            || self.extent().0 > 4096 || self.extent().1 > 4096
        {
            return Err("microfont-atlas-config");
        }
        Ok(Self {
            line_width_px: libm::roundf(self.line_width_px * 128.0) / 128.0,
            ..self
        })
    }
    fn scale(self) -> f32 {
        self.glyph_height_px.map_or(self.glyph_scale as f32, |h| h as f32 / 11.0)
    }
    fn gutter(self) -> u32 {
        if self.glyph_height_px.is_some() { (libm::ceilf(self.line_width_px * 0.5) as u32 + 1).max(6) }
        else { 0 }
    }
    pub(crate) fn cell_extent(self) -> (u32, u32) {
        let height = self.glyph_height_px.unwrap_or(11 * self.glyph_scale);
        let gutter = 2 * self.gutter();
        (((height * 6 + 10) / 11).max(1) + gutter, height + gutter)
    }
    pub(crate) fn extent(self) -> (u32, u32) {
        let (width, height) = self.cell_extent();
        (ATLAS_COLUMNS * width, ATLAS_ROWS * height)
    }
}

pub(crate) fn glyph_slot(ch: char) -> Option<usize> {
    match ch {
        ' '..='~' => Some(ch as usize - 0x20),
        '\u{2500}'..='\u{257f}' => Some(BOX_START + ch as usize - 0x2500),
        '\u{2580}'..='\u{259f}' => Some(BLOCK_START + ch as usize - 0x2580),
        _ => microfont::raster_line::GLYPH_CHARACTERS.iter()
            .copied().filter(|&candidate| is_extra_line(candidate))
            .position(|candidate| candidate == ch).map(|index| EXTRA_START + index),
    }
}

struct ResidentPrimitive {
    mesh: render::ResidentTriangleMesh,
    topology: Topology,
    params: [u32; 16],
}
struct ResidentGlyphs {
    batches: Vec<ResidentPrimitive>,
    bytes: usize,
    constants: Option<render::ResidentRenderBuffer>,
}
impl Drop for ResidentGlyphs {
    fn drop(&mut self) {
        if let Some(constants) = self.constants.take() {
            let _ = render::release_resident_render_buffer(&constants);
        }
        for batch in &self.batches {
            let _ = render::release_resident_triangle_mesh(&batch.mesh);
        }
    }
}
fn cell_origin(slot: usize) -> [f32; 2] {
    [
        (slot % ATLAS_COLUMNS as usize) as f32 * 6.0,
        (slot / ATLAS_COLUMNS as usize) as f32 * 11.0,
    ]
}
fn upload(
    batches: &mut ResidentGlyphs,
    vertices: &[[f32; 3]],
    indices: &[u32],
    topology: Topology,
    params: [u32; 16],
) -> Result<(), &'static str> {
    if indices.is_empty() {
        return Ok(());
    }
    let mesh = render::create_resident_indexed_mesh(vertices, indices, topology)?;
    batches.bytes += vertices.len() * 12 + indices.len() * 4;
    batches.batches.push(ResidentPrimitive {
        mesh,
        topology,
        params,
    });
    Ok(())
}
fn append_quad(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    slot: usize,
    corners: [[f32; 2]; 4],
) {
    let at = cell_origin(slot);
    let base = vertices.len() as u32;
    // Quad edges may lie exactly at x=6/y=11. Carry the owning cell explicitly
    // so physical gutters do not turn that boundary into the next cell.
    vertices.extend(corners.map(|p| [at[0] + p[0], at[1] + p[1], -(slot as f32 + 1.0)]));
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}
fn endpoint_params(strip: &microfont::raster_line::Strip, slot: usize) -> [u32; 16] {
    use microfont::raster_line::Boundary::End;
    let mut p = [0.0f32; 16];
    let origin = cell_origin(slot);
    for (offset, index, start) in [(0, 0, true), (4, strip.points.len() - 1, false)] {
        let at = strip.point(index).unwrap();
        let neighbor = if start {
            (1..strip.points.len()).find_map(|i| strip.point(i).filter(|v| *v != at))
        } else {
            (0..index)
                .rev()
                .find_map(|i| strip.point(i).filter(|v| *v != at))
        };
        if let Some(next) = neighbor {
            let d = [
                next.x() as f32 - at.x() as f32,
                next.y() as f32 - at.y() as f32,
            ];
            let len = libm::sqrtf(d[0] * d[0] + d[1] * d[1]);
            p[offset..offset + 4].copy_from_slice(&[
                origin[0] + at.x() as f32 + 0.5,
                origin[1] + at.y() as f32 + 0.5,
                d[0] / len,
                d[1] / len,
            ]);
            p[if start { 10 } else { 11 }] = if (if start { strip.start } else { strip.end }) == End
            {
                1.0
            } else {
                0.0
            };
        }
    }
    p[8] = 1.0; // line mode; replaced by half-width on each dispatch
    p.map(f32::to_bits)
}

/// Join recipe: one exact packed centre and three offsets per corner. The VS
/// expands offsets by the current pixel width and exports the edge distance.
/// A bevel patch uses the shared centre and the exposed endpoint of each
/// perpendicular half-width edge. Its winding is positive after the Y flip.
/// This diagnostic computes join patches; the stroke bodies remain native.
pub(super) fn corner_triangle(
    previous: [f32; 2],
    at: [f32; 2],
    next: [f32; 2],
    width: f32,
) -> Option<[[f32; 2]; 3]> {
    let mut incoming = [at[0] - previous[0], at[1] - previous[1]];
    let mut outgoing = [next[0] - at[0], next[1] - at[1]];
    let a = libm::sqrtf(incoming[0] * incoming[0] + incoming[1] * incoming[1]);
    let b = libm::sqrtf(outgoing[0] * outgoing[0] + outgoing[1] * outgoing[1]);
    if !width.is_finite() || width <= 0.0 || a <= 1e-6 || b <= 1e-6 {
        return None;
    }
    for k in 0..2 {
        incoming[k] /= a;
        outgoing[k] /= b;
    }
    let turn = incoming[0] * outgoing[1] - incoming[1] * outgoing[0];
    if turn.abs() <= 1e-6 {
        return None;
    }
    let side = if turn > 0.0 { 1.0 } else { -1.0 };
    let half = width * 0.5 * side;
    let mut triangle = [
        at,
        [at[0] + incoming[1] * half, at[1] - incoming[0] * half],
        [at[0] + outgoing[1] * half, at[1] - outgoing[0] * half],
    ];
    let cross = (triangle[1][0] - at[0]) * (triangle[2][1] - at[1])
        - (triangle[1][1] - at[1]) * (triangle[2][0] - at[0]);
    if cross > 0.0 {
        triangle.swap(1, 2);
    }
    Some(triangle)
}

fn append_join(out: &mut Vec<[f32; 3]>, slot: usize, triple: &[microfont::raster_line::Point]) {
    let points = core::array::from_fn::<_, 3, _>(|i| {
        [triple[i].x() as f32 + 0.5, triple[i].y() as f32 + 0.5]
    });
    let Some(triangle) = corner_triangle(points[0], points[1], points[2], 1.0) else {
        return;
    };
    let at = cell_origin(slot);
    let x = at[0] as u32 + triple[1].x() as u32;
    let y = at[1] as u32 + triple[1].y() as u32;
    let code = (x | (y << 7)) as f32;
    let a = [
        triangle[1][0] - triangle[0][0],
        triangle[1][1] - triangle[0][1],
    ];
    let b = [
        triangle[2][0] - triangle[0][0],
        triangle[2][1] - triangle[0][1],
    ];
    // Reflex exterior >270 degrees means the two outward normals are
    // more than 90 degrees apart. Only these sharp tips get a circular join.
    if a[0] * b[0] + a[1] * b[1] < -1e-6 {
        let length = libm::sqrtf((a[0]+b[0])*(a[0]+b[0]) + (a[1]+b[1])*(a[1]+b[1]));
        let middle = [(a[0]+b[0])*0.5/length, (a[1]+b[1])*0.5/length];
        let round_code = (code as u32 | 0x8000) as f32;
        // Two bounded tangent envelopes cover the circular sector. Four
        // triangles share one centre; the PS supplies the exact circular edge.
        for (left, right) in [(a, middle), (middle, b)] {
            let divisor = 1.0 + 4.0*(left[0]*right[0]+left[1]*right[1]);
            let tangent = [(left[0]+right[0])/divisor, (left[1]+right[1])/divisor];
            for (one, two) in [(left, tangent), (tangent, right)] {
                out.extend_from_slice(&[
                    [round_code, 0.0, 0.0],
                    [round_code, one[0], one[1]],
                    [round_code, two[0], two[1]],
                ]);
            }
        }
        return;
    }
    let edge = [b[0] - a[0], b[1] - a[1]];
    let height =
        (a[0] * b[1] - a[1] * b[0]).abs() / libm::sqrtf(edge[0] * edge[0] + edge[1] * edge[1]);
    out.extend_from_slice(&[
        [code, -1.0 - height, 0.0],
        [code, a[0], a[1]],
        [code, b[0], b[1]],
    ]);
}
pub(super) fn append_strip_joins(out: &mut Vec<[f32; 3]>, slot: usize, strip: &microfont::raster_line::Strip) {
    use microfont::raster_line::Boundary::Junction;
    let points: Vec<_> = strip.iter_points().collect();
    for triple in points.windows(3) {
        append_join(out, slot, triple);
    }
    // The repeated point already supplies the closing line segment. Only the
    // join spanning the end/start boundary is absent from windows(3).
    if points.len() >= 4 && points.first() == points.last()
        && strip.start == Junction && strip.end == Junction
    {
        append_join(out, slot, &[points[points.len()-2], points[0], points[1]]);
    }
}

fn upload_glyphs() -> Result<ResidentGlyphs, &'static str> {
    let mut resident = ResidentGlyphs {
        batches: Vec::new(),
        bytes: 0,
        constants: None,
    };
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (i, &(ch, _)) in microfont::BOX_DRAW_SYMBOLS.iter().enumerate() {
        if let Some(quads) = microfont::raster_quad::boxdraw_quad_slice(ch) {
            for quad in quads {
                append_quad(&mut vertices, &mut indices, BOX_START + i, quad.vertices);
            }
        }
    }
    for (i, &(ch, _)) in microfont::BLOCK_SYMBOLS.iter().enumerate() {
        if let Some(quads) = microfont::raster_quad::block_quad_slice(ch) {
            for quad in quads {
                let [l, t] = quad.top_left.map(f32::from);
                let [r, b] = quad.bottom_right.map(f32::from);
                append_quad(
                    &mut vertices,
                    &mut indices,
                    BLOCK_START + i,
                    [[l, t], [r, t], [r, b], [l, b]],
                );
            }
        }
    }
    upload(&mut resident, &vertices, &indices, Topology::TriangleList, [0; 16])?;
    for ch in (' '..='~').chain(microfont::raster_line::GLYPH_CHARACTERS.iter()
        .copied().filter(|&ch| is_extra_line(ch))) {
        let Some(strips) = microfont::raster_line::glyph_strips(ch) else {
            continue;
        };
        let slot = glyph_slot(ch).ok_or("microfont-glyph-slot")?;
        let at = cell_origin(slot);
        for strip in strips {
            vertices.clear();
            indices.clear();
            for i in 0..strip.points.len() + 2 {
                let [x, y, z] = strip.adjacency_vertex(i).ok_or("microfont-adjacency")?;
                // Z is a resident VS recipe marker, not a seed-data change.
                // Identify real endpoints by index, including coincident points.
                let z = if i == 1 { 0.25 } else if i == strip.points.len() { 0.75 } else { z };
                vertices.push([at[0] + x + 0.5, at[1] + y + 0.5, z]);
            }
            indices.extend(0..vertices.len() as u32);
            upload(
                &mut resident,
                &vertices,
                &indices,
                Topology::LineStripAdj,
                endpoint_params(strip, slot),
            )?;
        }
        vertices.clear();
        indices.clear();
        for strip in strips {
            append_strip_joins(&mut vertices, slot, strip);
        }
        indices.extend(0..vertices.len() as u32);
        let mut params = [0; 16];
        params[8] = (-1.0f32).to_bits();
        upload(&mut resident, &vertices, &indices, Topology::TriangleList, params)?;
    }
    resident.constants =
        Some(render::allocate_resident_render_buffer(resident.batches.len() * 64)?);
    let constants = resident.constants.as_ref().unwrap();
    for (i, batch) in resident.batches.iter().enumerate() {
        for (k, word) in batch.params.iter().enumerate() {
            if !constants.write(i * 64 + k * 4, &word.to_le_bytes()) {
                return Err("microfont-constants-upload");
            }
        }
    }
    constants.flush();
    crate::log_important!(target:"microfont-mask";
        "microfont-mask: geometry-ready uploads=1 draws={} geometry_bytes={} metadata_bytes={} slots={} cpu_geometry_rebuilds=0 storage=none\n",
        resident.batches.len(),resident.bytes,resident.batches.len()*64,SLOT_COUNT);
    Ok(resident)
}

pub(crate) struct CoverageAtlas {
    geometry: ResidentGlyphs,
    mask: Option<render::ResidentCoverageMask>,
    config: Option<AtlasConfig>,
    generation: u64,
    poisoned: bool,
}
impl Drop for CoverageAtlas {
    fn drop(&mut self) {
        if self.poisoned {
            // A future service teardown must not free vertices while an
            // accepted GPU submission has no proof of retirement.
            core::mem::forget(core::mem::take(&mut self.geometry.batches));
            if let Some(constants) = self.geometry.constants.take() {
                core::mem::forget(constants);
            }
            if let Some(mask) = self.mask.take() {
                core::mem::forget(mask);
            }
        }
    }
}
static ATLAS: spin::Mutex<Option<CoverageAtlas>> = spin::Mutex::new(None);

impl CoverageAtlas {
    pub(crate) fn surface(&self) -> GpgpuMask8Surface {
        self.mask.as_ref().unwrap().surface()
    }
    /// Atlas rectangle in pixels; absent source glyphs retain an empty slot.
    pub(crate) fn glyph_rect(&self, ch: char) -> Option<[u32; 4]> {
        let slot = glyph_slot(ch)?;
        let (width, height) = self.config().cell_extent();
        Some([
            (slot % ATLAS_COLUMNS as usize) as u32 * width,
            (slot / ATLAS_COLUMNS as usize) as u32 * height,
            width, height,
        ])
    }
    pub(crate) fn config(&self) -> AtlasConfig {
        self.config.unwrap()
    }
    fn warm(&mut self, config: AtlasConfig) -> Result<(), &'static str> {
        if self.poisoned {
            return Err("microfont-atlas-quarantined");
        }
        if self.config == Some(config) {
            return Ok(());
        }
        let (width, height) = config.extent();
        if self
            .mask
            .as_ref()
            .is_some_and(|m| m.surface().width != width || m.surface().height != height)
        {
            if !render::release_resident_coverage_mask(self.mask.take().unwrap()) {
                self.poisoned = true;
                return Err("microfont-atlas-release");
            }
        }
        // Invalidate before writing: a failed submission cannot leave an older
        // configuration falsely marked warm after a partially changed target.
        self.config = None;
        if self.mask.is_none() {
            self.mask = Some(render::create_resident_coverage_mask(width, height)?);
        }
        let constants = self.geometry.constants.as_ref().unwrap();
        // Only dynamic values are written. Endpoint frames, boundary tags and
        // join kind remain in their original GPU allocation for service life.
        let transform = [
            config.scale(),
            config.line_width_px,
            1.0 / width as f32,
            1.0 / height as f32,
        ]
        .map(f32::to_bits);
        for (i, batch) in self.geometry.batches.iter().enumerate() {
            // Physical-size mode packs independent cells with a stroke-sized
            // gutter. It prevents wide strokes leaking into adjacent masks.
            if !constants.write(i * 64 + 36,
                &(config.glyph_height_px.unwrap_or(0) as f32).to_bits().to_le_bytes()) {
                return Err("microfont-constants-size");
            }
            if batch.topology == Topology::LineStripAdj
                && !constants
                    .write(i * 64 + 32, &(config.line_width_px * 0.5).to_bits().to_le_bytes())
            {
                return Err("microfont-constants-width");
            }
            for (k, word) in transform.iter().enumerate() {
                if !constants.write(i * 64 + 48 + k * 4, &word.to_le_bytes()) {
                    return Err("microfont-constants-transform");
                }
            }
        }
        constants.flush();
        let draws: Vec<_> = self
            .geometry
            .batches
            .iter()
            .enumerate()
            .map(|(i, batch)| render::ResidentSceneDraw {
                mesh: &batch.mesh,
                depth_flags: None,
                rgba: [255; 4],
                sampled_texture: None,
                fragment_contract: render::ResidentSceneFragmentContract::MicrofontMask(
                    constants.gpu_base() + i as u64 * 64,
                ),
                viewport_translation_px: [0.0; 2],
                topology: batch.topology,
                point_width_px: 0,
                line_width_px: if batch.topology == Topology::LineStripAdj {
                    config.line_width_px
                } else {
                    0.0
                },
                clip_adjacency: batch.topology == Topology::LineStripAdj,
            })
            .collect();
        // Err means no unretired GPU work: the encoder rejected the request,
        // or earlier chunks retired before a later chunk was rejected. Keep
        // the mask cold and reusable. Only uncertain completion below pins it.
        let result = render::render_resident_coverage_mask(&draws, self.mask.as_mut().unwrap())?;
        if let Some(reason) = result.completion_error() {
            self.poisoned = true;
            return Err(reason);
        }
        if result.release_fence.is_none() {
            self.poisoned = true;
            return Err("microfont-mask-release");
        }
        self.config = Some(config);
        self.generation += 1;
        crate::log_important!(target:"microfont-mask";
            "microfont-mask: warm generation={} format=r8 size={}x{} bytes={} scale={} width_px={} geometry_uploads=1 geometry_reuploads=0 gpu_body={:?} gpu_target_ready={:?} samples=1\n",
            self.generation,width,height,self.surface().bytes,config.scale(),config.line_width_px,
            result.gpu_timestamp,result.gpu_target_ready_timestamp);
        Ok(())
    }
}

/// Owns geometry for the lifetime of the service, independent of preview windows.
/// Repeated identical requests reuse the ready mask without submitting GPU work.
pub(crate) fn with_warmed_atlas<T>(
    config: AtlasConfig,
    consumer: impl FnOnce(&CoverageAtlas) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let config = config.normalized()?;
    let mut guard = ATLAS.try_lock().ok_or("microfont-atlas-busy")?;
    if guard.is_none() {
        *guard = Some(CoverageAtlas {
            geometry: upload_glyphs()?,
            mask: None,
            config: None,
            generation: 0,
            poisoned: false,
        });
    }
    let atlas = guard.as_mut().unwrap();
    atlas.warm(config)?;
    consumer(atlas)
}

/// Immutable service-owned R8 snapshot. Keeping this data alive does not hold
/// the preview atlas lock; later Tab/width changes cannot replace its pixels.
/// Glyph rows are tightly packed, including the safe cap overhang gutter.
pub(crate) struct OwnedAlphaAtlas {
    pub(crate) em_height_px: u32,
    pub(crate) line_width_px: f32,
    pub(crate) glyph_width: u32,
    pub(crate) glyph_height: u32,
    pub(crate) gutter_px: u32,
    pixels: Vec<u8>,
}
impl OwnedAlphaAtlas {
    pub(crate) fn glyph(&self, ch: char) -> Option<&[u8]> {
        // Unminted line slots stay blank for the demo, but are absent to font
        // consumers so they can retain their existing bitmap fallback.
        if ch != ' ' && microfont::raster_line::glyph_strips(ch).is_none()
            && !('\u{2500}'..='\u{259f}').contains(&ch) {
            return None;
        }
        let slot = glyph_slot(ch)?;
        let bytes = self.glyph_width as usize * self.glyph_height as usize;
        self.pixels.get(slot * bytes..(slot + 1) * bytes)
    }
    pub(crate) fn glyphs(&self) -> impl Iterator<Item = (char, &[u8])> + '_ {
        (' '..='~').chain('\u{2500}'..='\u{259f}')
            .chain(microfont::raster_line::GLYPH_CHARACTERS.iter().copied()
                .filter(|&ch| is_extra_line(ch)))
            .filter_map(|ch| self.glyph(ch).map(|pixels| (ch, pixels)))
    }
}

/// One native-line/quad render submission and one R8 readback for Shell3's
/// service lifetime. Resident source geometry is shared with the demo; each
/// consumer retains its own immutable result after this function returns.
pub(crate) fn build_shell3_alpha_atlas(em_height_px: u32) -> Result<OwnedAlphaAtlas, &'static str> {
    let config = AtlasConfig {
        glyph_height_px: Some(em_height_px),
        line_width_px: 3.5,
        ..AtlasConfig::default()
    };
    with_warmed_atlas(config, |atlas| {
        let surface = atlas.surface();
        let source = crate::phys::phys_to_virt(surface.phys as usize) as *mut u8;
        if !crate::intel::dma_flush_strided_row_spans(&[
            crate::intel::DmaFlushRows::new(source, surface.width as usize,
                surface.pitch_bytes as usize, surface.height as usize),
        ]) {
            return Err("microfont-shell3-alpha-cache");
        }
        let (glyph_width, glyph_height) = config.cell_extent();
        let glyph_bytes = glyph_width as usize * glyph_height as usize;
        let mut pixels = alloc::vec![0; SLOT_COUNT * glyph_bytes];
        for slot in 0..SLOT_COUNT {
            let left = (slot % ATLAS_COLUMNS as usize) * glyph_width as usize;
            let top = (slot / ATLAS_COLUMNS as usize) * glyph_height as usize;
            for y in 0..glyph_height as usize {
                for x in 0..glyph_width as usize {
                    pixels[slot * glyph_bytes + y * glyph_width as usize + x] = unsafe {
                        core::ptr::read_volatile(source.add(
                            (top + y) * surface.pitch_bytes as usize + left + x))
                    };
                }
            }
        }
        Ok(OwnedAlphaAtlas {
            em_height_px,
            line_width_px: config.line_width_px,
            glyph_width,
            glyph_height,
            gutter_px: config.gutter(),
            pixels,
        })
    })
}

/// Preview sink only. The cached baseline remains color-free; apply tint and
/// opacity here exactly once. Output is premultiplied, with optional background.
pub(crate) fn stamp_preview(
    frame: ui4::FrameHandle,
    config: AtlasConfig,
    glyphs: &[char],
    columns: usize,
    display_scale: u32,
    ink: [u8; 4],
    background: [u8; 4],
) -> Result<usize, &'static str> {
    stamp_preview_colored(frame, config, glyphs, columns, display_scale, &[ink], background)
}

/// One ink broadcasts to every cell; otherwise provide one straight RGBA ink
/// per glyph cell. Coverage and opacity are premultiplied exactly once below.
pub(crate) fn stamp_preview_colored(
    frame: ui4::FrameHandle,
    config: AtlasConfig,
    glyphs: &[char],
    columns: usize,
    display_scale: u32,
    inks: &[[u8; 4]],
    background: [u8; 4],
) -> Result<usize, &'static str> {
    stamp_preview_colored_with_header(frame, config, glyphs, columns, display_scale, inks, background, None)
}

/// Optional fixed-size status row, separate from the native-line glyphs.
pub(crate) fn stamp_preview_colored_with_header(
    frame: ui4::FrameHandle,
    config: AtlasConfig,
    glyphs: &[char],
    columns: usize,
    display_scale: u32,
    inks: &[[u8; 4]],
    background: [u8; 4],
    header: Option<&str>,
) -> Result<usize, &'static str> {
    let header_height = if header.is_some() { PREVIEW_HEADER_HEIGHT } else { 0 };
    if inks.len() != 1 && inks.len() != glyphs.len() {
        return Err("microfont-stamp-colors");
    }
    with_warmed_atlas(config, |atlas| {
        let lease = ui4::acquire_frame_buffer(frame).map_err(|_| "frame-busy")?;
        let result = (|| {
            let view = ui4::writable_rgba_view(lease).map_err(|_| "frame-view")?;
            let mask = atlas.surface();
            let scale = atlas.config().glyph_scale;
            let (cell_width, cell_height) = if atlas.config().glyph_height_px.is_some() {
                atlas.config().cell_extent()
            } else { (6 * display_scale, 11 * display_scale) };
            if columns == 0
                || display_scale == 0
                || (atlas.config().glyph_height_px.is_none() && scale % display_scale != 0)
                || view.width != columns as u32 * cell_width
                || view.height != (glyphs.len().div_ceil(columns)) as u32 * cell_height + header_height
            {
                return Err("microfont-stamp-shape");
            }
            let header_width = view.width.saturating_sub(8) as usize / 2;
            let header_mask = header.map(|text| {
                let mut pixels = alloc::vec![0u8; header_width * microfont::FHEIGHT];
                microfont::stamp_text(&mut pixels, header_width, microfont::FHEIGHT,
                    0, 0, text, 255u8).map_err(|_| "microfont-header-text")?;
                Ok::<_, &'static str>(pixels)
            }).transpose()?;
            let source = crate::phys::phys_to_virt(mask.phys as usize) as *mut u8;
            let spans = [
                crate::intel::DmaFlushRows::new(
                    source,
                    mask.width as usize,
                    mask.pitch_bytes as usize,
                    mask.height as usize,
                ),
                crate::intel::DmaFlushRows::new(
                    view.virt,
                    view.width as usize * 4,
                    view.pitch as usize,
                    view.height as usize,
                ),
            ];
            if !crate::intel::dma_flush_strided_row_spans(&spans) {
                return Err("microfont-stamp-cache");
            }
            let mut partial = 0usize;
            let mut nonzero = 0usize;
            for y in 0..view.height {
                for x in 0..view.width {
                    let in_header = y < header_height;
                    let body_y = y.saturating_sub(header_height);
                    let cell = (body_y / cell_height) as usize * columns
                        + (x / cell_width) as usize;
                    let coverage = glyphs
                        .get(cell).filter(|_| !in_header)
                        .and_then(|&ch| atlas.glyph_rect(ch))
                        .map_or(0, |rect| {
                            let sx = rect[0]
                                + ((2 * (x % cell_width) + 1) * rect[2] / (2 * cell_width)).min(rect[2] - 1);
                            let sy = rect[1]
                                + ((2 * (body_y % cell_height) + 1) * rect[3] / (2 * cell_height)).min(rect[3] - 1);
                            unsafe {
                                core::ptr::read_volatile(
                                    source.add((sy * mask.pitch_bytes + sx) as usize),
                                )
                            }
                        });
                    partial += usize::from(coverage != 0 && coverage != 255);
                    nonzero += usize::from(coverage != 0);
                    let ink = if inks.len() == 1 { inks[0] }
                        else { inks.get(cell).copied().unwrap_or([0; 4]) };
                    let pixel = if in_header {
                        let hx = x.saturating_sub(4) as usize / 2;
                        let hy = y.saturating_sub(4) as usize / 2;
                        let a = if x >= 4 && y >= 4 && hx < header_width && hy < microfont::FHEIGHT {
                            header_mask.as_ref().unwrap()[hy * header_width + hx]
                        } else { 0 };
                        tint_coverage(a, [48, 64, 96, 255], background)
                    } else { tint_coverage(coverage, ink, background) };
                    unsafe {
                        core::ptr::write_unaligned(
                            view.virt
                                .add(y as usize * view.pitch as usize + x as usize * 4)
                                .cast::<u32>(),
                            u32::from_le_bytes(pixel),
                        );
                    }
                }
            }
            if !crate::intel::dma_flush_strided_row_spans(&spans[1..]) {
                return Err("microfont-stamp-publish-cache");
            }
            ui4::mark_frame_buffer_cpu_authored(lease).map_err(|_| "frame-cpu-authored")?;
            if background[3] == 255 {
                ui4::mark_frame_buffer_fully_opaque(lease).map_err(|_| "frame-opaque")?;
            }
            ui4::publish_frame_buffer(lease).map_err(|_| "frame-publish")?;
            crate::log_important!(target:"microfont-mask";
                "microfont-mask: stamp generation={} glyphs={} copies=1 nonzero={} partial={} inks={} background={:?} geometry_uploads=1\n",
                atlas.generation,glyphs.len(),nonzero,partial,inks.len(),background);
            Ok(nonzero)
        })();
        if result.is_err() {
            let _ = ui4::cancel_frame_buffer(lease);
        }
        result
    })
}

fn tint_coverage(coverage: u8, ink: [u8; 4], background: [u8; 4]) -> [u8; 4] {
    let a = (u32::from(coverage) * u32::from(ink[3]) + 127) / 255;
    let mut out = [0; 4];
    for k in 0..3 {
        out[k] = ((u32::from(ink[k]) * a + u32::from(background[k]) * (255 - a) + 127) / 255) as u8;
    }
    out[3] = (a + (u32::from(background[3]) * (255 - a) + 127) / 255) as u8;
    out
}
