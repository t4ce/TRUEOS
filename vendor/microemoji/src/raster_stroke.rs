//! Closed outline geometry and antialiased R8 strokes.

use crate::{
    Command, Glyph, Layer,
    bounds::Bounds,
    raster_line::{Edge, LayerEdges},
};

/// Stroke width in final output pixels.
pub const WIDTH_PX: f32 = 1.0;
/// Number of samples along each axis of an output pixel.
pub const SUBPIXELS: usize = 4;
const RADIUS: f64 = WIDTH_PX as f64 * 0.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The image extent or stride is invalid.
    Target,
    /// The supersample buffer is too small.
    Scratch,
}

impl core::fmt::Display for Error {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str(match self {
            Self::Target => "invalid microemoji stroke target",
            Self::Scratch => "insufficient microemoji stroke scratch",
        })
    }
}

impl core::error::Error for Error {}

/// The glyph outline fitted to a pixel rectangle, shared by both renderers.
#[derive(Clone, Copy)]
pub struct Outline<'a> {
    glyph: Glyph<'a>,
    width: usize,
    height: usize,
    scale: f64,
    origin: [f64; 2],
    curve_height: u32,
}

impl<'a> Outline<'a> {
    /// Fit and center the font bounds with room for a half stroke on each side.
    pub fn new(glyph: Glyph<'a>, width: usize, height: usize) -> Result<Self, Error> {
        sample_extent(width, height)?;
        let bounds = Bounds::for_glyph(glyph);
        let [font_width, font_height] = bounds.extent().map(f64::from);
        let (w, h) = (width as f64, height as f64);
        let margin = RADIUS.min(w * 0.25).min(h * 0.25);
        let scale = ((w - 2.0 * margin) / font_width).min((h - 2.0 * margin) / font_height);
        let metrics = glyph.metrics();
        let line_height = i32::from(metrics.ascender) - i32::from(metrics.descender);
        let scaled_height = f64::from(line_height) * scale;
        let whole_px = scaled_height as u32;
        let curve_height = whole_px
            .saturating_add(u32::from(scaled_height > f64::from(whole_px)))
            .max(1);
        Ok(Self {
            glyph,
            width,
            height,
            scale,
            origin: [
                (w - font_width * scale) * 0.5 - f64::from(bounds.left) * scale,
                (h - font_height * scale) * 0.5 - f64::from(bounds.top) * scale,
            ],
            curve_height,
        })
    }

    /// Visit the closed contours in source order. Fully transparent layers are skipped.
    pub fn contours(self) -> impl Iterator<Item = Contour<'a>> + 'a {
        self.glyph
            .layers()
            .filter(|layer| layer.color()[3] != 0)
            .flat_map(move |layer| {
                let mut commands = layer.commands();
                core::iter::from_fn(move || {
                    let remaining = commands.0;
                    for command in commands.by_ref() {
                        if command == Command::Close {
                            break;
                        }
                    }
                    let length = remaining.len() - commands.0.len();
                    (length != 0).then(|| Contour {
                        layer: Layer {
                            color: layer.color(),
                            path: &remaining[..length],
                        },
                        outline: self,
                    })
                })
            })
    }

    fn point(self, [x, y]: [f64; 2]) -> [f64; 2] {
        [
            x * self.scale + self.origin[0],
            self.origin[1] - y * self.scale,
        ]
    }
}

/// A single closed contour, retaining its original layer opacity.
#[derive(Clone, Copy)]
pub struct Contour<'a> {
    layer: Layer<'a>,
    outline: Outline<'a>,
}

impl<'a> Contour<'a> {
    pub fn alpha(self) -> u8 {
        self.layer.color()[3]
    }

    /// Emit directed nonzero edges in final pixels, with Y down. The last
    /// endpoint equals the first start, except for an empty contour.
    pub fn edges(self) -> impl Iterator<Item = Edge> + 'a {
        LayerEdges::new(
            self.layer,
            self.outline.glyph.metrics(),
            self.outline.curve_height,
        )
        .expect("validated glyph metrics and positive height")
        .map(move |edge| Edge {
            from: self.outline.point(edge.from),
            to: self.outline.point(edge.to),
        })
        .filter(|edge| edge.from != edge.to)
    }
}

/// Number of bytes for the CPU rasterizer's supersample buffer.
pub fn coverage_scratch_size(width: usize, height: usize) -> Result<usize, Error> {
    let [width, height] = sample_extent(width, height)?;
    width.checked_mul(height).ok_or(Error::Target)
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

/// Rasterize closed contours as round-joined strokes into one byte per pixel.
/// Overlapping strokes keep the greatest sample opacity. Row padding is
/// preserved. Size the reusable buffer with [coverage_scratch_size].
pub fn rasterize(
    outline: Outline<'_>,
    output: &mut [u8],
    stride: usize,
    coverage: &mut [u8],
) -> Result<(), Error> {
    let [sample_width, sample_height] = sample_extent(outline.width, outline.height)?;
    if stride < outline.width
        || (outline.height - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(outline.width))
            .is_none_or(|n| n > output.len())
    {
        return Err(Error::Target);
    }
    let sample_len = sample_width
        .checked_mul(sample_height)
        .ok_or(Error::Target)?;
    let samples = coverage.get_mut(..sample_len).ok_or(Error::Scratch)?;
    samples.fill(0);
    for contour in outline.contours() {
        let alpha = contour.alpha();
        for edge in contour.edges() {
            stroke_edge(edge, alpha, samples, sample_width, sample_height);
        }
    }
    let row_samples = sample_width * SUBPIXELS;
    const PIXEL_SAMPLES: u16 = (SUBPIXELS * SUBPIXELS) as u16;
    for (row, block) in output
        .chunks_mut(stride)
        .zip(samples.chunks_exact(row_samples))
    {
        for (x, pixel) in row[..outline.width].iter_mut().enumerate() {
            let start = x * SUBPIXELS;
            let sum: u16 = block
                .chunks_exact(sample_width)
                .flat_map(|sample_row| &sample_row[start..start + SUBPIXELS])
                .map(|&sample| u16::from(sample))
                .sum();
            *pixel = ((sum + PIXEL_SAMPLES / 2) / PIXEL_SAMPLES) as u8;
        }
    }
    Ok(())
}

fn stroke_edge(edge: Edge, alpha: u8, plane: &mut [u8], width: usize, height: usize) {
    let [x0, y0] = edge.from;
    let [x1, y1] = edge.to;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    let radius_squared = RADIUS * RADIUS;
    for row in sample_range(y0, y1, height) {
        let y = sample_position(row);
        // Limit each row to the part of the segment within one radius in Y.
        // The perpendicular distance test below still determines coverage.
        let (a, b) = if dy == 0.0 {
            (x0, x1)
        } else {
            let at = |y: f64| x0 + dx * ((y - y0) / dy).clamp(0.0, 1.0);
            (at(y - RADIUS), at(y + RADIUS))
        };
        for column in sample_range(a, b, width) {
            let x = sample_position(column) - x0;
            let y = y - y0;
            let position = ((x * dx + y * dy) / length_squared).clamp(0.0, 1.0);
            let x = x - position * dx;
            let y = y - position * dy;
            if x * x + y * y <= radius_squared {
                let sample = &mut plane[row * width + column];
                *sample = (*sample).max(alpha);
            }
        }
    }
}

fn sample_position(index: usize) -> f64 {
    (index as f64 + 0.5) / SUBPIXELS as f64
}

fn sample_range(a: f64, b: f64, extent: usize) -> core::ops::Range<usize> {
    // Saturating conversion rounds the lower bound down. Test the exact
    // distance so the extra sample at each bound does not affect coverage.
    let start = ((a.min(b) - RADIUS) * SUBPIXELS as f64) as usize;
    let end = ((a.max(b) + RADIUS) * SUBPIXELS as f64) as usize;
    start.min(extent)..end.saturating_add(1).min(extent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Metrics;
    use std::{vec, vec::Vec};

    const SQUARE: [[i32; 2]; 4] = [[0, 0], [8, 0], [8, 8], [0, 8]];
    const INNER: [[i32; 2]; 4] = [[2, 2], [6, 2], [6, 6], [2, 6]];

    fn append_layer(payload: &mut Vec<u8>, alpha: u8, contours: &[&[[i32; 2]]]) {
        let len = contours
            .iter()
            .map(|points| points.len() * 9 + 1)
            .sum::<usize>();
        payload.extend([0, 0, 0, alpha]);
        payload.extend((len as u32).to_le_bytes());
        for contour in contours {
            for (index, &[x, y]) in contour.iter().enumerate() {
                payload.push(u8::from(index > 0));
                payload.extend((x * 64).to_le_bytes());
                payload.extend((y * 64).to_le_bytes());
            }
            payload.push(4);
        }
    }

    fn outline(payload: &[u8], width: usize, height: usize) -> Outline<'_> {
        let glyph = Glyph {
            scalar: 'x',
            advance: 8,
            metrics: Metrics {
                units_per_em: 8,
                ascender: 8,
                descender: 0,
            },
            payload,
        };
        Outline::new(glyph, width, height).unwrap()
    }

    #[test]
    fn contours_close_independently_and_keep_layer_alpha() {
        let mut payload = Vec::new();
        append_layer(&mut payload, 64, &[&SQUARE, &INNER]);
        append_layer(&mut payload, 0, &[&INNER]);
        let contours: Vec<_> = outline(&payload, 8, 10).contours().collect();
        assert_eq!(contours.len(), 2);
        for contour in &contours {
            assert_eq!(contour.alpha(), 64);
            let edges: Vec<_> = contour.edges().collect();
            assert_eq!(edges.len(), 4);
            assert!(edges.windows(2).all(|pair| pair[0].to == pair[1].from));
            assert_eq!(edges[3].to, edges[0].from);
        }
        let first = contours[0].edges().next().unwrap();
        assert_eq!(first.from, [0.5, 8.5]);
        assert_eq!(first.to, [7.5, 8.5]);
    }

    #[test]
    fn raster_strokes_keep_empty_interiors_and_antialiased_corners() {
        let (width, height, stride) = (8, 10, 11);
        let mut payload = Vec::new();
        append_layer(&mut payload, 127, &[&SQUARE]);
        append_layer(&mut payload, 191, &[&SQUARE]);
        let mut output = vec![29; stride * height];
        let mut scratch = vec![0; coverage_scratch_size(width, height).unwrap()];
        rasterize(
            outline(&payload, width, height),
            &mut output,
            stride,
            &mut scratch,
        )
        .unwrap();
        assert_eq!(output[5 * stride], 191);
        assert_eq!(output[stride + 4], 191);
        assert_eq!(output[5 * stride + 4], 0);
        assert!((1..191).contains(&output[stride]));
        assert!(
            output
                .chunks_exact(stride)
                .all(|row| row[width..] == [29; 3])
        );
    }
}
