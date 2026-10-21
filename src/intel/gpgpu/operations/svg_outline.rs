// Bounded SVG-outline probes for the shell-visible UI4 bring-up path.
//
// This is intentionally not a general XML/SVG renderer.  The probe accepts
// byte-embedded, trusted SVG documents containing one `viewBox` and a small
// number of solid-color `<path>` elements.  Absolute M/L/Q/C/Z path commands
// lower directly into the same eight-dword outline ABI supplied by callers.
// That lets the existing SIMD16 non-zero-winding coverage kernel prove the
// vector-to-UI4 lifecycle before a host-side usvg normalizer is introduced.

const SVG_OUTLINE_PROBE_MAX_SOURCE_BYTES: usize = 16 * 1024;
const SVG_OUTLINE_PROBE_MAX_LAYERS: usize = 8;
const SVG_OUTLINE_PROBE_MAX_OPS_PER_LAYER: usize = 1_024;
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
struct SvgNumberCursor<'a> {
    bytes: &'a [u8],
    index: usize,
#[derive(Copy, Clone)]
struct SvgProbeTransform {
    scale: f32,
    x: f32,
    y: f32,
}
}
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
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuSvgOutlineProbeResult {
    pub(crate) ok: bool,
    /// At least one destination-writing batch was accepted for this attempt.
    pub(crate) submitted: bool,
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
            submitted: false,
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

/// The legacy outline shader was retired with the font backend. Keep the SVG
/// probe API explicit about its unavailable renderer without admitting GPU work.
pub(crate) fn submit_svg_outline_probe(
    _dst: GpgpuRgba8Surface,
    _demo: SvgOutlineProbeDemo,
) -> GpgpuSvgOutlineProbeResult {
    GpgpuSvgOutlineProbeResult::failed("svg-outline-backend-removed")
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
        while !self.at_end() && matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n' | b',') {
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
        let text =
            core::str::from_utf8(&self.bytes[start..self.index]).map_err(|_| "svg-number-utf8")?;
        let value = text.parse::<f32>().map_err(|_| "svg-number")?;
        value.is_finite().then_some(value).ok_or("svg-number-range")
    }

    fn finish(&mut self) -> Result<(), &'static str> {
        self.skip_separators();
        self.at_end().then_some(()).ok_or("svg-extra-numbers")
    }
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
            assert!(
                layers
                    .iter()
                    .flat_map(|layer| layer.ops.iter())
                    .all(|op| op[0] <= 4)
            );
        }
    }
}
