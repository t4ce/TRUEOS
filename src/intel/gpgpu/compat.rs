// These signatures are still named by dead callers outside this cleanup's file scope.
// Keep inert compatibility definitions so pruning this module does not break the crate.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSprite64Placement;

pub(crate) fn present_rgba8_to_primary_xrgb_rect_stats(
    _src: GpgpuRgba8Surface,
    _src_rect: GpgpuRect,
    _dst: GpgpuRgba8Surface,
    _dst_xy: GpgpuPoint,
    _flip_y: bool,
) -> GpgpuSubmitStats {
    let stats = GpgpuSubmitStats::default();
    let _ = (stats.spans, stats.total_ms);
    stats
}

pub(crate) fn present_rgba8_rect_to_primary_xrgb_stats_with_flip(
    _src: GpgpuRgba8Surface,
    _src_rect: GpgpuRect,
    _dst_xy: GpgpuPoint,
    _flip_y: bool,
) -> Option<GpgpuSubmitStats> {
    None
}

pub(crate) fn present_rgba_frame_to_primary(_src: &[u8], _width: u32, _height: u32) -> bool {
    false
}

// The compatibility definitions above are intentionally retained until their dead callers
// can be removed in a broader cleanup.
const _: Option<GpgpuSprite64Placement> = Some(GpgpuSprite64Placement);
const _: fn(GpgpuRgba8Surface, GpgpuRect, GpgpuRgba8Surface, GpgpuPoint, bool) -> GpgpuSubmitStats =
    present_rgba8_to_primary_xrgb_rect_stats;
const _: fn(GpgpuRgba8Surface, GpgpuRect, GpgpuPoint, bool) -> Option<GpgpuSubmitStats> =
    present_rgba8_rect_to_primary_xrgb_stats_with_flip;
const _: fn(&[u8], u32, u32) -> bool = present_rgba_frame_to_primary;
