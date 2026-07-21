// Bounded SVG-outline probes for the shell-visible UI4 bring-up path.
//
// This is intentionally not a general XML/SVG renderer.  The probe accepts
// byte-embedded, trusted SVG documents containing one `viewBox` and a small
// number of solid-color `<path>` elements.  Absolute M/L/Q/C/Z path commands
// lower directly into the same eight-dword outline ABI produced by Skrifa.
// That lets the existing SIMD16 non-zero-winding coverage kernel prove the
// vector-to-UI4 lifecycle before a host-side usvg normalizer is introduced.

const SVG_OUTLINE_PROBE_MAX_SOURCE_BYTES: usize = 16 * 1024;
const SVG_OUTLINE_PROBE_MAX_LAYERS: usize = 8;
const SVG_OUTLINE_PROBE_MAX_OPS_PER_LAYER: usize = 1_024;
const SVG_OUTLINE_PROBE_CANVAS_MAX: u32 = 384;
const SVG_OUTLINE_PROBE_CANVAS_MARGIN: u32 = 48;
const SVG_OUTLINE_PROBE_CURVE_SUBDIVISIONS: u32 = 8;

const SVG_BASIC: &[u8] = br##"<svg viewBox="0 0 128 128">
  <path fill="#23B5D3" d="M 64 8 L 120 64 L 64 120 L 8 64 Z"/>
  <path fill="#FFF5D6" d="M 64 32 L 96 64 L 64 96 L 32 64 Z"/>
</svg>"##;

const SVG_CURVES: &[u8] = br##"<svg viewBox="0 0 128 128">
  <path fill="#FFD166" d="M 88 14 C 102 14 114 26 114 40 C 114 54 102 66 88 66 C 74 66 62 54 62 40 C 62 26 74 14 88 14 Z"/>
  <path fill="#2D6CDF" d="M 24 92 C 12 92 8 80 14 69 C 18 60 27 56 38 58 C 43 37 64 28 82 40 C 93 47 97 58 96 66 C 111 64 121 74 118 86 C 116 94 108 98 96 98 L 27 98 Q 24 98 24 92 Z"/>
</svg>"##;

// The inner cubic contour runs opposite to the outer contour.  A correct
// non-zero-winding fill therefore leaves the center transparent.
const SVG_HOLES: &[u8] = br##"<svg viewBox="0 0 128 128">
  <path fill="#EF476F" d="M 64 8 C 95 8 120 33 120 64 C 120 95 95 120 64 120 C 33 120 8 95 8 64 C 8 33 33 8 64 8 Z M 64 42 C 52 42 42 52 42 64 C 42 76 52 86 64 86 C 76 86 86 76 86 64 C 86 52 76 42 64 42 Z"/>
</svg>"##;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SvgOutlineProbeDemo {
    Basic,
    Curves,
    Holes,
}

impl SvgOutlineProbeDemo {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Curves => "curves",
            Self::Holes => "holes",
        }
    }

    const fn source(self) -> &'static [u8] {
        match self {
            Self::Basic => SVG_BASIC,
            Self::Curves => SVG_CURVES,
            Self::Holes => SVG_HOLES,
        }
    }

    const fn background_rgba(self) -> u32 {
        match self {
            Self::Basic => pack_rgba(13, 24, 36, 255),
            Self::Curves => pack_rgba(8, 19, 38, 255),
            Self::Holes => pack_rgba(27, 20, 38, 255),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuSvgOutlineProbeResult {
    pub(crate) ok: bool,
    /// True only when hardware may still own the destination after a timeout.
    /// The UI4 caller must retain rather than cancel that exact write lease.
    pub(crate) destination_submitted: bool,
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
    pub(crate) layers: usize,
    pub(crate) ops: usize,
    pub(crate) nonzero_pixels: usize,
    pub(crate) submit_ms: u64,
    pub(crate) error: &'static str,
}

impl GpgpuSvgOutlineProbeResult {
    const fn failed(error: &'static str) -> Self {
        Self {
            ok: false,
            destination_submitted: false,
            release: None,
            layers: 0,
            ops: 0,
            nonzero_pixels: 0,
            submit_ms: 0,
            error,
        }
    }
}

struct SvgOutlineProbeLayer {
    color_rgba: u32,
    ops: Vec<[u32; 8]>,
}

#[derive(Copy, Clone)]
struct SvgViewBox {
    min_x: f32,
    min_y: f32,
    width: f32,
    height: f32,
}

#[derive(Copy, Clone)]
struct SvgProbeTransform {
    scale: f32,
    x: f32,
    y: f32,
}

impl SvgProbeTransform {
    fn point(self, x: f32, y: f32) -> Result<(f32, f32), &'static str> {
        let x = x * self.scale + self.x;
        let y = y * self.scale + self.y;
        if x.is_finite() && y.is_finite() {
            Ok((x, y))
        } else {
            Err("svg-outline-coordinate")
        }
    }
}

/// Parse and render one trusted byte-embedded SVG probe into an exact UI4
/// frame allocation.  Every GPU stage retires before the next stage starts;
/// the returned release is minted only by the final PAT3/UC scanout handoff.
pub(crate) fn submit_svg_outline_probe(
    dst: GpgpuRgba8Surface,
    demo: SvgOutlineProbeDemo,
) -> GpgpuSvgOutlineProbeResult {
    let started = direct_rcs_now_tick();
    if !dst.is_valid() {
        return GpgpuSvgOutlineProbeResult::failed("svg-destination-invalid");
    }
    let shortest = dst.width.min(dst.height);
    if shortest <= SVG_OUTLINE_PROBE_CANVAS_MARGIN.saturating_mul(2) {
        return GpgpuSvgOutlineProbeResult::failed("svg-destination-small");
    }
    let canvas = shortest
        .saturating_sub(SVG_OUTLINE_PROBE_CANVAS_MARGIN.saturating_mul(2))
        .min(SVG_OUTLINE_PROBE_CANVAS_MAX);
    let layers = match parse_svg_probe(demo.source(), canvas) {
        Ok(layers) => layers,
        Err(error) => return GpgpuSvgOutlineProbeResult::failed(error),
    };
    let mut result = GpgpuSvgOutlineProbeResult::failed("svg-background-submit");
    result.layers = layers.len();
    result.ops = layers.iter().map(|layer| layer.ops.len()).sum();

    let background = fill_rect_rgba8_stats(dst, dst.bounds(), demo.background_rgba());
    if background.submits == 0 {
        result.submit_ms = direct_rcs_elapsed_ms_since(started);
        return result;
    }

    let mut masks = Vec::with_capacity(layers.len());
    let mut blits = Vec::with_capacity(layers.len());
    let dst_x = dst.width.saturating_sub(canvas) / 2;
    let dst_y = dst.height.saturating_sub(canvas) / 2;
    for layer in layers {
        let Some(mask) = allocate_font_coverage_mask(canvas, canvas) else {
            result.error = "svg-mask-alloc";
            result.submit_ms = direct_rcs_elapsed_ms_since(started);
            return result;
        };
        match font_outline_coverage_r8(
            &mask,
            layer.ops.as_slice(),
            GpgpuRect::new(0, 0, canvas, canvas),
            SVG_OUTLINE_PROBE_CURVE_SUBDIVISIONS,
            0.0,
        ) {
            GpgpuDispatchRetirement::Complete => {}
            GpgpuDispatchRetirement::NotSubmitted => {
                result.error = "svg-coverage-not-submitted";
                result.submit_ms = direct_rcs_elapsed_ms_since(started);
                return result;
            }
            GpgpuDispatchRetirement::SubmittedIncomplete => {
                // The coverage dispatch owns this mask and its unique PPGTT
                // mapping, but it never referenced the UI4 destination.
                core::mem::forget(mask);
                result.error = "svg-coverage-retirement-uncertain";
                result.submit_ms = direct_rcs_elapsed_ms_since(started);
                return result;
            }
        }
        let Some(audit) = mask.nonzero_audit() else {
            result.error = "svg-coverage-empty";
            result.submit_ms = direct_rcs_elapsed_ms_since(started);
            return result;
        };
        result.nonzero_pixels = result
            .nonzero_pixels
            .saturating_add(audit.nonzero_pixels);
        blits.push(GpgpuGlyphMaskLayer {
            mask: mask.surface(),
            mask_rect: GpgpuRect::new(0, 0, canvas, canvas),
            dst_xy: GpgpuPoint::new(dst_x as i32, dst_y as i32),
            color_rgba: layer.color_rgba,
        });
        masks.push(mask);
    }

    let composite = glyph_mask_layers_rgba8_2d_mode(blits.as_slice(), dst, false);
    if !composite.ok || composite.requested_layers != blits.len() {
        result.destination_submitted = composite.submitted;
        if composite.submitted {
            quarantine_direct_rcs_context("svg-outline-composite-marker-timeout");
            // The accepted batch can still fetch every mask and write dst.
            core::mem::forget(masks);
        }
        result.error = if composite.submitted {
            "svg-composite-retirement-uncertain"
        } else {
            "svg-composite-not-submitted"
        };
        result.submit_ms = direct_rcs_elapsed_ms_since(started);
        return result;
    }
    drop(masks);

    let finalizer = release_rgba8_surface_for_scanout(dst);
    if !finalizer.ok {
        result.destination_submitted = finalizer.submitted;
        if finalizer.submitted {
            quarantine_direct_rcs_context("svg-outline-release-marker-timeout");
        }
        result.error = if finalizer.submitted {
            "svg-release-retirement-uncertain"
        } else {
            "svg-release-not-submitted"
        };
        result.submit_ms = direct_rcs_elapsed_ms_since(started);
        return result;
    }
    let Some(release) = finalizer.release else {
        result.error = "svg-release-missing";
        result.submit_ms = direct_rcs_elapsed_ms_since(started);
        return result;
    };
    result.ok = true;
    result.release = Some(release);
    result.error = "none";
    result.submit_ms = direct_rcs_elapsed_ms_since(started);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: svg-outline probe={} ok=1 source_bytes={} parser=bounded-viewbox+solid-path commands=M/L/Q/C/Z layers={} ops={} masks={} canvas={}x{} nonzero={} fill=simd16-nonzero-r8 composite=single-batch final=pipe-control+post-marker submit_ms={}\n",
        demo.label(),
        demo.source().len(),
        result.layers,
        result.ops,
        result.layers,
        canvas,
        canvas,
        result.nonzero_pixels,
        result.submit_ms,
    );
    result
}

fn parse_svg_probe(
    source: &[u8],
    canvas: u32,
) -> Result<Vec<SvgOutlineProbeLayer>, &'static str> {
    if source.is_empty() || source.len() > SVG_OUTLINE_PROBE_MAX_SOURCE_BYTES || canvas == 0 {
        return Err("svg-source-size");
    }
    let svg_start = find_subslice(source, b"<svg").ok_or("svg-root")?;
    let svg_tail = &source[svg_start..];
    let svg_end = svg_tail
        .iter()
        .position(|byte| *byte == b'>')
        .ok_or("svg-root-close")?;
    let view_box = parse_view_box(attribute_value(&svg_tail[..=svg_end], b"viewBox")?)?;
    let longest = view_box.width.max(view_box.height);
    if !longest.is_finite() || longest <= 0.0 {
        return Err("svg-viewbox-range");
    }
    let scale = canvas as f32 / longest;
    let transform = SvgProbeTransform {
        scale,
        x: (canvas as f32 - view_box.width * scale) * 0.5 - view_box.min_x * scale,
        y: (canvas as f32 - view_box.height * scale) * 0.5 - view_box.min_y * scale,
    };
    let mut layers = Vec::new();
    let mut cursor = svg_end + 1;
    while cursor < svg_tail.len() {
        let Some(relative) = find_subslice(&svg_tail[cursor..], b"<path") else {
            break;
        };
        let tag_start = cursor + relative;
        let tag_tail = &svg_tail[tag_start..];
        let tag_end = tag_tail
            .iter()
            .position(|byte| *byte == b'>')
            .ok_or("svg-path-close")?;
        let tag = &tag_tail[..=tag_end];
        let color_rgba = parse_solid_color(attribute_value(tag, b"fill")?)?;
        let data = attribute_value(tag, b"d")?;
        let ops = parse_absolute_path(data, transform)?;
        if ops.is_empty() {
            return Err("svg-path-empty");
        }
        layers.push(SvgOutlineProbeLayer { color_rgba, ops });
        if layers.len() > SVG_OUTLINE_PROBE_MAX_LAYERS {
            return Err("svg-layer-limit");
        }
        cursor = tag_start + tag_end + 1;
    }
    if layers.is_empty() {
        return Err("svg-path-missing");
    }
    Ok(layers)
}

fn parse_view_box(bytes: &[u8]) -> Result<SvgViewBox, &'static str> {
    let mut numbers = SvgNumberCursor::new(bytes);
    let min_x = numbers.number()?;
    let min_y = numbers.number()?;
    let width = numbers.number()?;
    let height = numbers.number()?;
    numbers.finish()?;
    if [min_x, min_y, width, height]
        .iter()
        .any(|value| !value.is_finite())
        || width <= 0.0
        || height <= 0.0
    {
        return Err("svg-viewbox-range");
    }
    Ok(SvgViewBox {
        min_x,
        min_y,
        width,
        height,
    })
}

fn parse_absolute_path(
    bytes: &[u8],
    transform: SvgProbeTransform,
) -> Result<Vec<[u32; 8]>, &'static str> {
    let mut input = SvgNumberCursor::new(bytes);
    let mut ops = Vec::new();
    let mut command = 0u8;
    let mut contour_open = false;
    while !input.at_end() {
        input.skip_separators();
        if input.at_end() {
            break;
        }
        let byte = input.peek();
        if byte.is_ascii_alphabetic() {
            command = input.take();
            if command.is_ascii_lowercase() {
                return Err("svg-relative-command-unsupported");
            }
        } else if command == 0 {
            return Err("svg-path-command");
        }
        let op = match command {
            b'M' => {
                let (x, y) = transform.point(input.number()?, input.number()?)?;
                command = b'L';
                contour_open = true;
                outline_move(x, y)
            }
            b'L' => {
                if !contour_open {
                    return Err("svg-line-before-move");
                }
                let (x, y) = transform.point(input.number()?, input.number()?)?;
                outline_line(x, y)
            }
            b'Q' => {
                if !contour_open {
                    return Err("svg-quad-before-move");
                }
                let (cx, cy) = transform.point(input.number()?, input.number()?)?;
                let (x, y) = transform.point(input.number()?, input.number()?)?;
                outline_quad(cx, cy, x, y)
            }
            b'C' => {
                if !contour_open {
                    return Err("svg-cubic-before-move");
                }
                let (c0x, c0y) = transform.point(input.number()?, input.number()?)?;
                let (c1x, c1y) = transform.point(input.number()?, input.number()?)?;
                let (x, y) = transform.point(input.number()?, input.number()?)?;
                outline_cubic(c0x, c0y, c1x, c1y, x, y)
            }
            b'Z' => {
                if !contour_open {
                    return Err("svg-close-before-move");
                }
                contour_open = false;
                command = 0;
                [4, 0, 0, 0, 0, 0, 0, 0]
            }
            _ => return Err("svg-command-unsupported"),
        };
        ops.push(op);
        if ops.len() > SVG_OUTLINE_PROBE_MAX_OPS_PER_LAYER {
            return Err("svg-op-limit");
        }
    }
    Ok(ops)
}

struct SvgNumberCursor<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> SvgNumberCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, index: 0 }
    }

    fn at_end(&self) -> bool {
        self.index >= self.bytes.len()
    }

    fn peek(&self) -> u8 {
        self.bytes[self.index]
    }

    fn take(&mut self) -> u8 {
        let byte = self.bytes[self.index];
        self.index += 1;
        byte
    }

    fn skip_separators(&mut self) {
        while !self.at_end()
            && matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n' | b',')
        {
            self.index += 1;
        }
    }

    fn number(&mut self) -> Result<f32, &'static str> {
        self.skip_separators();
        let start = self.index;
        if !self.at_end() && matches!(self.peek(), b'+' | b'-') {
            self.index += 1;
        }
        let mut digits = 0usize;
        while !self.at_end() && self.peek().is_ascii_digit() {
            digits += 1;
            self.index += 1;
        }
        if !self.at_end() && self.peek() == b'.' {
            self.index += 1;
            while !self.at_end() && self.peek().is_ascii_digit() {
                digits += 1;
                self.index += 1;
            }
        }
        if digits == 0 {
            return Err("svg-number");
        }
        if !self.at_end() && matches!(self.peek(), b'e' | b'E') {
            self.index += 1;
            if !self.at_end() && matches!(self.peek(), b'+' | b'-') {
                self.index += 1;
            }
            let exponent_start = self.index;
            while !self.at_end() && self.peek().is_ascii_digit() {
                self.index += 1;
            }
            if self.index == exponent_start {
                return Err("svg-number-exponent");
            }
        }
        let text = core::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| "svg-number-utf8")?;
        let value = text.parse::<f32>().map_err(|_| "svg-number")?;
        value.is_finite().then_some(value).ok_or("svg-number-range")
    }

    fn finish(&mut self) -> Result<(), &'static str> {
        self.skip_separators();
        self.at_end().then_some(()).ok_or("svg-extra-numbers")
    }
}

fn attribute_value<'a>(tag: &'a [u8], name: &[u8]) -> Result<&'a [u8], &'static str> {
    let start = find_subslice(tag, name).ok_or("svg-attribute-missing")?;
    let mut cursor = start + name.len();
    while cursor < tag.len() && tag[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if tag.get(cursor) != Some(&b'=') {
        return Err("svg-attribute-equals");
    }
    cursor += 1;
    while cursor < tag.len() && tag[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let quote = *tag.get(cursor).ok_or("svg-attribute-quote")?;
    if !matches!(quote, b'\'' | b'"') {
        return Err("svg-attribute-quote");
    }
    cursor += 1;
    let end = tag[cursor..]
        .iter()
        .position(|byte| *byte == quote)
        .ok_or("svg-attribute-close")?;
    Ok(&tag[cursor..cursor + end])
}

fn parse_solid_color(bytes: &[u8]) -> Result<u32, &'static str> {
    let hex = bytes.strip_prefix(b"#").ok_or("svg-fill-color")?;
    if !matches!(hex.len(), 6 | 8) {
        return Err("svg-fill-color");
    }
    let red = hex_byte(&hex[0..2])?;
    let green = hex_byte(&hex[2..4])?;
    let blue = hex_byte(&hex[4..6])?;
    let alpha = if hex.len() == 8 {
        hex_byte(&hex[6..8])?
    } else {
        u8::MAX
    };
    Ok(pack_rgba(red, green, blue, alpha))
}

fn hex_byte(bytes: &[u8]) -> Result<u8, &'static str> {
    let high = hex_digit(bytes[0]).ok_or("svg-fill-color")?;
    let low = hex_digit(bytes[1]).ok_or("svg-fill-color")?;
    Ok((high << 4) | low)
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const fn pack_rgba(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    (alpha as u32) << 24 | (blue as u32) << 16 | (green as u32) << 8 | red as u32
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

const fn outline_move(x: f32, y: f32) -> [u32; 8] {
    [0, x.to_bits(), y.to_bits(), 0, 0, 0, 0, 0]
}

const fn outline_line(x: f32, y: f32) -> [u32; 8] {
    [1, x.to_bits(), y.to_bits(), 0, 0, 0, 0, 0]
}

const fn outline_quad(cx: f32, cy: f32, x: f32, y: f32) -> [u32; 8] {
    [1 + 1, cx.to_bits(), cy.to_bits(), x.to_bits(), y.to_bits(), 0, 0, 0]
}

const fn outline_cubic(
    c0x: f32,
    c0y: f32,
    c1x: f32,
    c1y: f32,
    x: f32,
    y: f32,
) -> [u32; 8] {
    [
        3,
        c0x.to_bits(),
        c0y.to_bits(),
        c1x.to_bits(),
        c1y.to_bits(),
        x.to_bits(),
        y.to_bits(),
        0,
    ]
}

#[cfg(test)]
mod svg_outline_tests {
    use super::{SVG_BASIC, SVG_CURVES, SVG_HOLES, parse_svg_probe};

    #[test]
    fn embedded_probe_subset_lowers_to_outline_ops() {
        for source in [SVG_BASIC, SVG_CURVES, SVG_HOLES] {
            let layers = parse_svg_probe(source, 256).expect("embedded SVG must parse");
            assert!(!layers.is_empty());
            assert!(layers.iter().all(|layer| !layer.ops.is_empty()));
            assert!(layers
                .iter()
                .flat_map(|layer| layer.ops.iter())
                .all(|op| op[0] <= 4));
        }
    }
}
