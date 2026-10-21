// Bounded SVG-outline probes for the shell-visible UI4 bring-up path.
//
// This is intentionally not a general XML/SVG renderer.  The probe accepts
// byte-embedded, trusted SVG documents containing one `viewBox` and a small
// number of solid-color `<path>` elements.  Absolute M/L/Q/C/Z path commands
// lower directly into the same eight-dword outline ABI supplied by callers.
// That lets the existing SIMD16 non-zero-winding coverage kernel prove the
// vector-to-UI4 lifecycle before a host-side usvg normalizer is introduced.




// The inner cubic contour runs opposite to the outer contour.  A correct
// non-zero-winding fill therefore leaves the center transparent.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SvgOutlineProbeDemo {
    Basic,
}

impl SvgOutlineProbeDemo {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Basic => "basic",
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





/// The legacy outline shader was retired with the font backend. Keep the SVG
/// probe API explicit about its unavailable renderer without admitting GPU work.
pub(crate) fn submit_svg_outline_probe(
    _dst: GpgpuRgba8Surface,
    _demo: SvgOutlineProbeDemo,
) -> GpgpuSvgOutlineProbeResult {
    GpgpuSvgOutlineProbeResult::failed("svg-outline-backend-removed")
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
