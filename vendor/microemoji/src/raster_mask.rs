//! Nonzero-winding scan conversion and antialiased R8 coverage.

use crate::bounds::Bounds;
use crate::raster_line::{Edge, LayerEdges};
use crate::{Glyph, Layer, Metrics};

/// Samples on each axis of an output pixel.
pub const SUBPIXELS: usize = 4;
const SAMPLES: usize = SUBPIXELS * SUBPIXELS;

/// A horizontal span on the subpixel grid, with an exclusive end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub y: usize,
    pub start: usize,
    pub end: usize,
}

/// Caller-owned workspace. Reuse it between rasterizations to retain capacity.
pub struct Scratch<'a> {
    edges: &'a mut [Edge],
    winding: &'a mut [i32],
}

impl<'a> Scratch<'a> {
    pub fn new(edges: &'a mut [Edge], winding: &'a mut [i32]) -> Self {
        Self { edges, winding }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The image dimensions, stride, or byte extent are invalid.
    Target,
    /// The edge or scanline workspace is too small.
    Scratch,
}

impl core::fmt::Display for Error {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str(match self {
            Self::Target => "invalid microemoji R8 target",
            Self::Scratch => "insufficient microemoji raster scratch",
        })
    }
}

impl core::error::Error for Error {}

/// Return the required number of edge slots and winding entries.
///
/// The edge capacity is the maximum number of edges in any painted layer. The
/// winding capacity is shared by span generation and R8 compositing.
pub fn scratch_requirements(
    glyph: Glyph<'_>,
    width: usize,
    height: usize,
) -> Result<(usize, usize), Error> {
    let viewport = Viewport::new(glyph, width, height)?;
    let edges = glyph
        .layers()
        .filter(|layer| layer.color()[3] != 0)
        .map(|layer| layer_edges(layer, glyph.metrics(), viewport.curve_height).count())
        .max()
        .unwrap_or(0);
    if edges > i32::MAX as usize {
        return Err(Error::Scratch);
    }
    Ok((edges, viewport.sample_width))
}

/// Return the number of coverage bytes needed by [rasterize].
pub fn coverage_scratch_size(width: usize, height: usize) -> Result<usize, Error> {
    let [sample_width, sample_height] = sample_extent(width, height)?;
    sample_width.checked_mul(sample_height).ok_or(Error::Target)
}

/// Emit spans in layer order with the layer's alpha. Each layer is filled
/// with the nonzero-winding rule and visited top to bottom. Width and height
/// are output pixel dimensions. Span coordinates are in units of 1/4 pixel.
///
/// The font cell and contour bounds fit proportionally and are centered in the
/// target. The origin is the upper left corner, as in the standalone SVG.
/// Use [scratch_requirements] to size the buffers before emitting any spans.
pub fn scan_layers(
    glyph: Glyph<'_>,
    width: usize,
    height: usize,
    scratch: &mut Scratch<'_>,
    mut visit: impl FnMut(u8, Span),
) -> Result<(), Error> {
    let viewport = Viewport::new(glyph, width, height)?;
    let winding = scratch
        .winding
        .get_mut(..viewport.sample_width)
        .ok_or(Error::Scratch)?;
    for layer in glyph.layers() {
        let alpha = layer.color()[3];
        if alpha == 0 {
            continue;
        }
        let edges = flatten_layer(layer, glyph.metrics(), viewport, scratch.edges)?;
        let [top, bottom] = edges
            .iter()
            .flat_map(|edge| [edge.from[1], edge.to[1]])
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[top, bottom], y| {
                [top.min(y), bottom.max(y)]
            });
        let start = sample_index(top, viewport.sample_height);
        let end = sample_index(bottom, viewport.sample_height);
        for y in start..end {
            sample_row(edges, y, winding);
            visit_spans(winding, |start, end| visit(alpha, Span { y, start, end }));
        }
    }
    Ok(())
}

/// Write a row-major R8 alpha mask with one byte per pixel. Layers composite
/// using source-over at each sample, then each pixel averages its samples.
///
/// The viewport and sample grid match [scan_layers]. Stride is in bytes and
/// row padding is preserved. `coverage` is a scratch plane sized with
/// [coverage_scratch_size]. The output is only written after scan conversion
/// succeeds.
pub fn rasterize(
    glyph: Glyph<'_>,
    output: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    scratch: &mut Scratch<'_>,
    coverage: &mut [u8],
) -> Result<(), Error> {
    let coverage_len = coverage_scratch_size(width, height)?;
    if stride < width
        || (height - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(width))
            .is_none_or(|n| n > output.len())
    {
        return Err(Error::Target);
    }
    let samples = coverage.get_mut(..coverage_len).ok_or(Error::Scratch)?;
    samples.fill(0);
    let sample_width = width * SUBPIXELS;
    scan_layers(glyph, width, height, scratch, |alpha, span| {
        let start = span.y * sample_width;
        let pixels = &mut samples[start + span.start..start + span.end];
        if alpha == 255 {
            pixels.fill(255);
        } else {
            let alpha = u16::from(alpha);
            for pixel in pixels {
                *pixel = (alpha + (u16::from(*pixel) * (255 - alpha) + 127) / 255) as u8;
            }
        }
    })?;

    let sample_rows = samples.chunks_exact(sample_width * SUBPIXELS);
    for (row, sample_row) in output.chunks_mut(stride).zip(sample_rows) {
        for (x, pixel) in row[..width].iter_mut().enumerate() {
            let columns = x * SUBPIXELS..(x + 1) * SUBPIXELS;
            let total: u16 = sample_row
                .chunks_exact(sample_width)
                .flat_map(|row| &row[columns.clone()])
                .map(|&sample| u16::from(sample))
                .sum();
            *pixel = ((total + (SAMPLES / 2) as u16) / SAMPLES as u16) as u8;
        }
    }
    Ok(())
}

fn sample_extent(width: usize, height: usize) -> Result<[usize; 2], Error> {
    if width == 0 || height == 0 || u32::try_from(width).is_err() || u32::try_from(height).is_err()
    {
        return Err(Error::Target);
    }
    Ok([
        width.checked_mul(SUBPIXELS).ok_or(Error::Target)?,
        height.checked_mul(SUBPIXELS).ok_or(Error::Target)?,
    ])
}

#[derive(Clone, Copy)]
struct Viewport {
    scale: f64,
    origin_x: f64,
    origin_y: f64,
    curve_height: u32,
    sample_width: usize,
    sample_height: usize,
}

impl Viewport {
    fn new(glyph: Glyph<'_>, width: usize, height: usize) -> Result<Self, Error> {
        let [sample_width, sample_height] = sample_extent(width, height)?;
        let bounds = Bounds::for_glyph(glyph);
        let [font_width, font_height] = bounds.extent();
        let width = width as f64;
        let height = height as f64;
        let scale = (width / f64::from(font_width)).min(height / f64::from(font_height));
        let metrics = glyph.metrics();
        let line_height = i32::from(metrics.ascender) - i32::from(metrics.descender);
        let line_height_px = f64::from(line_height) * scale;
        let whole_px = line_height_px as u32;
        let curve_height = whole_px
            .saturating_add(u32::from(line_height_px > f64::from(whole_px)))
            .max(1);
        Ok(Self {
            scale,
            origin_x: (width - f64::from(font_width) * scale) * 0.5
                - f64::from(bounds.left) * scale,
            origin_y: (height - f64::from(font_height) * scale) * 0.5
                - f64::from(bounds.top) * scale,
            curve_height,
            sample_width,
            sample_height,
        })
    }

    fn transform(self, [x, y]: [f64; 2]) -> [f64; 2] {
        [
            x * self.scale + self.origin_x,
            self.origin_y - y * self.scale,
        ]
    }
}

fn layer_edges(layer: Layer<'_>, metrics: Metrics, height_px: u32) -> impl Iterator<Item = Edge> {
    // The glyph's metrics were checked when the bundle was parsed.
    LayerEdges::new(layer, metrics, height_px)
        .expect("validated glyph metrics and positive height")
        .filter(|edge| edge.from[1] != edge.to[1])
}

fn flatten_layer<'a>(
    layer: Layer<'_>,
    metrics: Metrics,
    viewport: Viewport,
    output: &'a mut [Edge],
) -> Result<&'a [Edge], Error> {
    let mut count = 0;
    let available = output.len().min(i32::MAX as usize);
    let output = &mut output[..available];
    for edge in layer_edges(layer, metrics, viewport.curve_height) {
        let slot = output.get_mut(count).ok_or(Error::Scratch)?;
        *slot = Edge {
            from: viewport.transform(edge.from),
            to: viewport.transform(edge.to),
        };
        count += 1;
    }
    Ok(&output[..count])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    const CELL: [[i32; 2]; 4] = [[0, 0], [8, 0], [8, 8], [0, 8]];
    const HOLE: [[i32; 2]; 4] = [[2, 2], [2, 6], [6, 6], [6, 2]];
    const HALF_PIXEL: [[i32; 2]; 4] = [[0, 0], [1, 0], [1, 8], [0, 8]];
    const EMPTY_EDGE: Edge = Edge {
        from: [0.0; 2],
        to: [0.0; 2],
    };

    fn append_layer(payload: &mut Vec<u8>, alpha: u8, contours: &[&[[i32; 2]]]) {
        let len = contours.iter().map(|p| p.len() * 9 + 1).sum::<usize>();
        payload.extend([255, 128, 0, alpha]);
        payload.extend((len as u32).to_le_bytes());
        for contour in contours {
            for (index, &[x, y]) in contour.iter().enumerate() {
                payload.push(u8::from(index > 0));
                payload.extend((x * 32).to_le_bytes());
                payload.extend((y * 32).to_le_bytes());
            }
            payload.push(4);
        }
    }

    fn glyph(payload: &[u8]) -> Glyph<'_> {
        Glyph {
            scalar: 'x',
            advance: 4,
            metrics: Metrics {
                units_per_em: 4,
                ascender: 4,
                descender: 0,
            },
            payload,
        }
    }

    fn scratch_for(
        glyph: Glyph<'_>,
        width: usize,
        height: usize,
    ) -> (Vec<Edge>, Vec<i32>, Vec<u8>) {
        let (edges, winding) = scratch_requirements(glyph, width, height).unwrap();
        let coverage = coverage_scratch_size(width, height).unwrap();
        (vec![EMPTY_EDGE; edges], vec![0; winding], vec![0; coverage])
    }

    #[test]
    fn scanlines_keep_holes_and_r8_preserves_padding() {
        let mut payload = Vec::new();
        append_layer(&mut payload, 255, &[&CELL, &HOLE]);
        let glyph = glyph(&payload);
        let (mut edges, mut winding, mut coverage) = scratch_for(glyph, 4, 4);
        let mut scratch = Scratch::new(&mut edges, &mut winding);
        let mut hole_row = Vec::new();
        scan_layers(glyph, 4, 4, &mut scratch, |alpha, span| {
            assert_eq!(alpha, 255);
            if span.y == 4 {
                hole_row.push((span.start, span.end));
            }
        })
        .unwrap();
        assert_eq!(hole_row, [(0, 4), (12, 16)]);

        let mut pixels = [17; 24];
        rasterize(glyph, &mut pixels, 4, 4, 6, &mut scratch, &mut coverage).unwrap();
        assert_eq!(
            pixels,
            [
                255, 255, 255, 255, 17, 17, 255, 0, 0, 255, 17, 17, 255, 0, 0, 255, 17, 17, 255,
                255, 255, 255, 17, 17,
            ]
        );
    }

    #[test]
    fn overlapping_layers_composite_before_pixel_average() {
        for (alpha, expected) in [(255, 128), (128, 96)] {
            let mut payload = Vec::new();
            append_layer(&mut payload, alpha, &[&HALF_PIXEL]);
            append_layer(&mut payload, alpha, &[&HALF_PIXEL]);
            let glyph = glyph(&payload);
            let (mut edges, mut winding, mut coverage) = scratch_for(glyph, 4, 4);
            let mut scratch = Scratch::new(&mut edges, &mut winding);
            let mut pixels = [0; 16];
            rasterize(glyph, &mut pixels, 4, 4, 4, &mut scratch, &mut coverage).unwrap();
            assert!(pixels.chunks_exact(4).all(|row| row == [expected, 0, 0, 0]));
        }
    }

    #[test]
    fn center_fit_keeps_overhangs_and_y_orientation() {
        let mut payload = Vec::new();
        let overhang = [[-2, 4], [0, 4], [0, 8], [-2, 8]];
        append_layer(&mut payload, 255, &[&overhang]);
        let glyph = glyph(&payload);
        let (mut edges, mut winding, mut coverage) = scratch_for(glyph, 5, 6);
        let mut scratch = Scratch::new(&mut edges, &mut winding);
        let mut pixels = [0; 30];
        rasterize(glyph, &mut pixels, 5, 6, 5, &mut scratch, &mut coverage).unwrap();
        assert_eq!(
            pixels
                .iter()
                .enumerate()
                .filter(|(_, alpha)| **alpha != 0)
                .collect::<Vec<_>>(),
            [(5, &255), (10, &255)]
        );
    }

    #[test]
    fn invalid_target_and_scratch_leave_output_untouched() {
        let mut payload = Vec::new();
        append_layer(&mut payload, 255, &[&CELL]);
        let glyph = glyph(&payload);
        let (_, mut winding, mut coverage) = scratch_for(glyph, 4, 4);
        let mut scratch = Scratch::new(&mut [], &mut winding);
        let mut pixels = [42; 16];
        assert_eq!(
            rasterize(glyph, &mut pixels, 4, 4, 3, &mut scratch, &mut coverage),
            Err(Error::Target)
        );
        assert_eq!(
            rasterize(glyph, &mut pixels, 4, 4, 4, &mut scratch, &mut coverage),
            Err(Error::Scratch)
        );
        assert_eq!(pixels, [42; 16]);
    }
}

fn sample_row(edges: &[Edge], row: usize, winding: &mut [i32]) {
    winding.fill(0);
    let y = (row as f64 + 0.5) / SUBPIXELS as f64;
    for edge in edges {
        let (top, bottom, delta) = if edge.from[1] <= edge.to[1] {
            (edge.from, edge.to, 1)
        } else {
            (edge.to, edge.from, -1)
        };
        // A half-open vertical range counts shared vertices once.
        if !(top[1]..bottom[1]).contains(&y) {
            continue;
        }
        let fraction = (y - top[1]) / (bottom[1] - top[1]);
        let x = top[0] + (bottom[0] - top[0]) * fraction;
        let index = sample_index(x, winding.len());
        if let Some(wind) = winding.get_mut(index) {
            *wind += delta;
        }
    }
}

fn sample_index(position: f64, extent: usize) -> usize {
    // Round up to the first sample center at or beyond the crossing.
    let sample = position * SUBPIXELS as f64 - 0.5;
    let floor = sample as usize;
    floor
        .saturating_add(usize::from(sample > floor as f64))
        .min(extent)
}

fn visit_spans(winding: &[i32], mut visit: impl FnMut(usize, usize)) {
    let mut start = 0;
    let mut total = 0;
    for (x, delta) in winding.iter().copied().enumerate() {
        let previous = total;
        total += delta;
        if previous == 0 && total != 0 {
            start = x;
        } else if previous != 0 && total == 0 {
            visit(start, x);
        }
    }
    if total != 0 {
        visit(start, winding.len());
    }
}
