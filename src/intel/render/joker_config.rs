//! Named offscreen render experiments and their immutable configuration.
//! This module does not acquire submission guards or touch hardware.

use super::{
    BackendProbeMode, PostDrawSyncVariant, StreamoutProofExperiment, TriangleBlendProbeMode, TriangleFrontEndContract, VfPrimitiveGeometry, TRIANGLE_DEFAULT_FRONT_END_CONTRACT, VS_DRAW_FRONTIER_CONTRACTS, VS_DRAW_SBE_READ0_CONTRACT,
};

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const RENDER_JOKER_VARIANTS: &[&str] = &[
    "canonical",
    "mesa",
    "mesa-retire",
    "bt0",
    "bt0-primary",
    "scratch",
    "oa",
    "point",
    "point-scratch",
    "point-oa",
    "point-oa-pos0",
    "point-oa-header",
    "point-oa-killoff",
    "point-oa-smooth",
    "point-oa-msrast",
    "point-oa-msrast-force",
    "point-oa-deref0",
    "point-oa-hz0",
    "point-oa-wm-normal",
    "point-oa-wm-reemit",
    "point-oa-hz-omit",
    "point-oa-ps-off",
    "point-oa-bt1",
    "point-oa-early",
    "point-oa-early-killoff",
    "point-oa-clip-normal",
    "point-oa-clip-persp",
    "point-oa-clip-disable",
    "point-oa-clip-disable-arm",
    "point-oa-clip-force",
    "point-oa-clip-d3d",
    "point-oa-clip-xy",
    "point-oa-sbe0",
    "point-oa-sbe-pre-clip",
    "point-oa-sbe-pre-sf",
    "point-oa-no-pr",
    "point-oa-vfg",
    "point-oa-w64",
    "point-oa-w64-early",
    "point-oa-w64-early-scissor",
    "point-oa-screen-w64",
    "point-oa-w64-arm",
    "point-oa-w64-wm-normal",
    "point-oa-w64-wm-reemit",
    "point-oa-w64-hz-omit",
    "point-oa-w64-ps-off",
    "point-oa-w64-payload-attr",
    "point-oa-w64-payload-depthw",
    "point-oa-w64-payload-bary",
    "point-oa-w64-sbe-pre-clip",
    "point-oa-w64-sbe-pre-sf",
    "point-oa-w1023",
    "point-oa-w1023-nowmpoint",
    "point-oa-w1023-scissor",
    "point-oa-vtxw",
    "point-oa-early-w1023",
    "point-oa-early-msrast-force",
    "point-bt1",
    "point-slot0",
    "screen-vs-scratch",
    "screen-vs-oa",
    "screen-vs-ndc-oa",
    "screen-vs-ndc-oa-hz0",
    "screen-vs-sbe0",
    "screen-vs-slot0-oa",
    "screen-vs-urb2-oa",
    "screen-vs-urb2-slot0-oa",
    "vf-rect-oa",
    "vf-rect-oa-pos0",
    "vf-rect-oa-header",
    "vf-rect-oa-deref0",
    "vf-rect-ndc-oa",
    "vf-rect-ndc-oa-sbe-pre-clip",
    "vf-rect-ndc-oa-sbe-pre-sf",
    "vf-rect-ndc-oa-drawrect-early",
    "vf-rect-ndc-oa-sample-early",
    "vf-rect-ndc-oa-pc-clip-sf",
    "vf-rect-ndc-oa-hz-pre-wm",
    "vf-rect-ndc-oa-hz-post-extra",
    "vf-rect-ndc-oa-payload-attr",
    "vf-rect-ndc-oa-payload-depthw",
    "vf-rect-ndc-oa-payload-bary",
    "vf-rect-ndc-oa-persp",
    "vf-rect-ndc-oa-clipxy",
    "vf-rect-ndc-oa-clip-disable",
    "vf-rect-ndc-oa-clip-force",
    "vf-rect-ndc-oa-clip-d3d",
    "vf-rect-ndc-oa-early-clipxy",
    "vf-rect-ndc-oa-frontccw",
    "vf-rect-ndc-oa-hz0",
    "vf-rect-ndc-oa-early",
    "vf-rect-ndc-oa-bt1",
    "vf-rect-ndc-order-b-oa",
    "vf-rect-ndc-order-c-oa",
    "vf-rect-ndc-order-c-early-oa",
    "vf-rect-ndc-order-c-clip-disable-oa",
    "vf-rect-ndc-mesa-simple-oa",
    "vf-rect-ndc-mesa-nosrc-header-oa",
    "vf-rect-ndc-small-oa",
    "vf-rect-ndc-cw-oa",
    "vf-rect-ndc-alt-oa",
    "vf-rect-order-b-oa",
    "vf-rect-order-b-early-oa",
    "vf-rect-order-b-scissor-oa",
    "vf-rect-mesa-simple-oa",
    "vf-rect-mesa-simple-oa-early",
    "vf-tri-mesa-simple-oa-early",
    "vf-rect-mesa-simple-oa-arm",
    "vf-rect-mesa-nosrc-header-oa",
    "vf-rect-order-c-oa",
    "vf-tri-ndc-oa",
    "vf-tri-ndc-oa-early",
    "vf-tri-ndc-oa-early-clipxy",
    "vf-tri-ndc-cw-oa-early",
    "screen-rect-scratch",
    "screen-rect-oa-early",
    "so-vf",
    "so-vf-header",
    "so-vs",
    "so-vs-header",
    "bt1",
    "wm-normal",
    "slot0",
    "slot1",
    "slot2",
    "all",
    "simd16",
    "simd16-retire",
    "eot",
    "eot-retire",
    "cps",
    "cps-retire",
    "hz",
    "hz-retire",
    "reemit",
    "reemit-retire",
    "reemit-vs-retire",
    "reemit-vs-slot0-retire",
    "reemit-vs-urb2-retire",
    "reemit-vs-urb2-slot0-retire",
    "payload-push",
    "payload-attr",
    "payload-simple",
    "payload-depthw",
    "payload-bary",
    "grf1",
    "grf2",
    "grf4",
    "mt31",
    "mt15",
    "sync-light",
    "sync-post-no-cs",
    "sync-cs-no-post",
];

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn render_joker_variant_names() -> &'static [&'static str] {
    RENDER_JOKER_VARIANTS
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn retired_render_joker_variant_reason(name: &str) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("point-oa-w8")
        || name.eq_ignore_ascii_case("point-oa-w8-clipmax")
        || name.eq_ignore_ascii_case("point-oa-w64-clipmax")
    {
        Some("retired-invalid-point-width-hw-contract")
    } else {
        None
    }
}

#[derive(Copy, Clone)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) struct RenderJokerSpec {
    pub(super) variant: &'static str,
    pub(super) submit_name: &'static str,
    pub(super) target: RenderJokerTarget,
    pub(super) blend: TriangleBlendProbeMode,
    pub(super) geometry: VfPrimitiveGeometry,
    pub(super) backend: BackendProbeMode,
    pub(super) sync: PostDrawSyncVariant,
}

#[derive(Copy, Clone)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) enum RenderJokerTarget {
    ScratchRt,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn parse_render_joker_spec(name: &str) -> Option<RenderJokerSpec> {
    let scratch = RenderJokerTarget::ScratchRt;
    // Historical variants that once targeted the live primary are retained as
    // offscreen diagnostics. Render probes never acquire a display surface.
    let offscreen = scratch;
    let explicit = TriangleBlendProbeMode::ExplicitRt0;
    let zeroed = TriangleBlendProbeMode::MesaZeroedState;
    let canonical = VfPrimitiveGeometry::Canonical;
    let big = VfPrimitiveGeometry::Oversized;
    let point = VfPrimitiveGeometry::CenterPoint;
    let screen_point = VfPrimitiveGeometry::ScreenSpacePoint8x8;
    let screen_space = VfPrimitiveGeometry::ScreenSpace8x8;
    let screen_rect = VfPrimitiveGeometry::ScreenSpaceRect8x8;
    let screen_tri_order_b = VfPrimitiveGeometry::ScreenSpaceTri8x8OrderB;
    let screen_rect_order_b = VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB;
    let screen_rect_order_c = VfPrimitiveGeometry::ScreenSpaceRect8x8OrderC;
    let ndc_triangle = VfPrimitiveGeometry::NdcTriangleLarge;
    let ndc_triangle_cw = VfPrimitiveGeometry::NdcTriangleLargeCw;
    let ndc_rect = VfPrimitiveGeometry::NdcRect;
    let ndc_rect_cw = VfPrimitiveGeometry::NdcRectCw;
    let ndc_rect_alt = VfPrimitiveGeometry::NdcRectAlt;
    let ndc_rect_order_c = VfPrimitiveGeometry::NdcRectUrLrUl;
    let ndc_rect_small = VfPrimitiveGeometry::NdcRectSmall;
    let heavy = PostDrawSyncVariant::HeavyAll;
    let light_post_no_cs = PostDrawSyncVariant::LightPostSyncNoCs;

    let spec = if name.eq_ignore_ascii_case("canonical") {
        RenderJokerSpec {
            variant: "canonical",
            submit_name: "vf-draw-path",
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa") || name.eq_ignore_ascii_case("big") {
        RenderJokerSpec {
            variant: "mesa",
            submit_name: "ps-launch-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa-retire") {
        RenderJokerSpec {
            variant: "mesa-retire",
            submit_name: "ps-launch-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("bt0") || name.eq_ignore_ascii_case("scratch") {
        RenderJokerSpec {
            variant: if name.eq_ignore_ascii_case("scratch") {
                "scratch"
            } else {
                "bt0"
            },
            submit_name: "ps-bt0-scratch-rt",
            target: scratch,
            blend: zeroed,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("bt0-primary") {
        RenderJokerSpec {
            variant: "bt0-primary",
            submit_name: "ps-bt0-primary-rt",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("oa") {
        RenderJokerSpec {
            variant: "oa",
            submit_name: "raster-wm-oa-probe",
            target: scratch,
            blend: zeroed,
            geometry: big,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point") || name.eq_ignore_ascii_case("giant-point") {
        RenderJokerSpec {
            variant: "point",
            submit_name: "point-vf-giant",
            target: offscreen,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-scratch") {
        RenderJokerSpec {
            variant: "point-scratch",
            submit_name: "point-vf-giant-scratch",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa") {
        RenderJokerSpec {
            variant: "point-oa",
            submit_name: "point-vf-giant-oa",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-pos0") {
        RenderJokerSpec {
            variant: "point-oa-pos0",
            submit_name: "point-vf-giant-oa-pos0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-header") {
        RenderJokerSpec {
            variant: "point-oa-header",
            submit_name: "point-vf-giant-oa-header",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-killoff") {
        RenderJokerSpec {
            variant: "point-oa-killoff",
            submit_name: "point-vf-giant-oa-killoff",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaKillOff,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-smooth") {
        RenderJokerSpec {
            variant: "point-oa-smooth",
            submit_name: "point-vf-giant-oa-smooth",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSmoothPoint,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-msrast") {
        RenderJokerSpec {
            variant: "point-oa-msrast",
            submit_name: "point-vf-giant-oa-msrast",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaMsRaster,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-msrast-force") {
        RenderJokerSpec {
            variant: "point-oa-msrast-force",
            submit_name: "point-vf-giant-oa-msrast-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaMsRasterForced,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-deref0") {
        RenderJokerSpec {
            variant: "point-oa-deref0",
            submit_name: "point-vf-giant-oa-deref0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaDerefBlock0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-hz0") {
        RenderJokerSpec {
            variant: "point-oa-hz0",
            submit_name: "point-vf-giant-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-wm-normal") {
        RenderJokerSpec {
            variant: "point-oa-wm-normal",
            submit_name: "point-vf-giant-oa-wm-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaWmNormalDispatch,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-wm-reemit") {
        RenderJokerSpec {
            variant: "point-oa-wm-reemit",
            submit_name: "point-vf-giant-oa-wm-reemit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaWmReemitAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-hz-omit") {
        RenderJokerSpec {
            variant: "point-oa-hz-omit",
            submit_name: "point-vf-giant-oa-hz-omit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaOmitHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-ps-off") {
        RenderJokerSpec {
            variant: "point-oa-ps-off",
            submit_name: "point-vf-giant-oa-ps-off",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-bt1") {
        RenderJokerSpec {
            variant: "point-oa-bt1",
            submit_name: "point-vf-giant-oa-bt1",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaBtCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early") {
        RenderJokerSpec {
            variant: "point-oa-early",
            submit_name: "point-vf-giant-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-killoff") {
        RenderJokerSpec {
            variant: "point-oa-early-killoff",
            submit_name: "point-vf-giant-oa-early-killoff",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyKillOff,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-normal") {
        RenderJokerSpec {
            variant: "point-oa-clip-normal",
            submit_name: "point-vf-giant-oa-clip-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipNormal,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-persp") {
        RenderJokerSpec {
            variant: "point-oa-clip-persp",
            submit_name: "point-vf-giant-oa-clip-persp",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipPerspective,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-disable") {
        RenderJokerSpec {
            variant: "point-oa-clip-disable",
            submit_name: "point-vf-giant-oa-clip-disable",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-disable-arm") {
        RenderJokerSpec {
            variant: "point-oa-clip-disable-arm",
            submit_name: "point-vf-giant-oa-clip-disable-arm",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipDisabledArtificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-force") {
        RenderJokerSpec {
            variant: "point-oa-clip-force",
            submit_name: "point-vf-giant-oa-clip-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipForceMode,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-d3d") {
        RenderJokerSpec {
            variant: "point-oa-clip-d3d",
            submit_name: "point-vf-giant-oa-clip-d3d",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipApiD3d,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-xy") {
        RenderJokerSpec {
            variant: "point-oa-clip-xy",
            submit_name: "point-vf-giant-oa-clip-xy",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe0") {
        RenderJokerSpec {
            variant: "point-oa-sbe0",
            submit_name: "point-vf-giant-oa-sbe0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeRead0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "point-oa-sbe-pre-clip",
            submit_name: "point-vf-giant-oa-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "point-oa-sbe-pre-sf",
            submit_name: "point-vf-giant-oa-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-no-pr") {
        RenderJokerSpec {
            variant: "point-oa-no-pr",
            submit_name: "point-vf-giant-oa-no-pr",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaNoPrimitiveReplication,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-vfg") {
        RenderJokerSpec {
            variant: "point-oa-vfg",
            submit_name: "point-vf-giant-oa-vfg",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaVfGeometryDistribution,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w8") {
        RenderJokerSpec {
            variant: "point-oa-w8",
            submit_name: "point-vf-giant-oa-w8",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth8,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w8-clipmax") {
        RenderJokerSpec {
            variant: "point-oa-w8-clipmax",
            submit_name: "point-vf-giant-oa-w8-clipmax",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth8ClipMax,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64") {
        RenderJokerSpec {
            variant: "point-oa-w64",
            submit_name: "point-vf-giant-oa-w64",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-halign128") {
        RenderJokerSpec {
            variant: "point-oa-w64-halign128",
            submit_name: "point-vf-giant-oa-w64-halign128",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SurfaceHalign128,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-clipmax") {
        RenderJokerSpec {
            variant: "point-oa-w64-clipmax",
            submit_name: "point-vf-giant-oa-w64-clipmax",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64ClipMax,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-early") {
        RenderJokerSpec {
            variant: "point-oa-w64-early",
            submit_name: "point-vf-giant-oa-w64-early",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Early,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-early-scissor") {
        RenderJokerSpec {
            variant: "point-oa-w64-early-scissor",
            submit_name: "point-vf-giant-oa-w64-early-scissor",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64EarlyScissor,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-screen-w64") {
        RenderJokerSpec {
            variant: "point-oa-screen-w64",
            submit_name: "point-vf-screen-oa-w64",
            target: scratch,
            blend: zeroed,
            geometry: screen_point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Screen,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-arm") {
        RenderJokerSpec {
            variant: "point-oa-w64-arm",
            submit_name: "point-vf-giant-oa-w64-arm",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Artificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-wm-normal") {
        RenderJokerSpec {
            variant: "point-oa-w64-wm-normal",
            submit_name: "point-vf-giant-oa-w64-wm-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64WmNormalDispatch,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-wm-reemit") {
        RenderJokerSpec {
            variant: "point-oa-w64-wm-reemit",
            submit_name: "point-vf-giant-oa-w64-wm-reemit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64WmReemitAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-hz-omit") {
        RenderJokerSpec {
            variant: "point-oa-w64-hz-omit",
            submit_name: "point-vf-giant-oa-w64-hz-omit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64OmitHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-ps-off") {
        RenderJokerSpec {
            variant: "point-oa-w64-ps-off",
            submit_name: "point-vf-giant-oa-w64-ps-off",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-attr") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-attr",
            submit_name: "point-vf-giant-oa-w64-payload-attr",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadAttributeEnable,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-depthw") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-depthw",
            submit_name: "point-vf-giant-oa-w64-payload-depthw",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadSourceDepthW,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-bary") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-bary",
            submit_name: "point-vf-giant-oa-w64-payload-bary",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadBaryPlanes,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "point-oa-w64-sbe-pre-clip",
            submit_name: "point-vf-giant-oa-w64-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "point-oa-w64-sbe-pre-sf",
            submit_name: "point-vf-giant-oa-w64-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023") {
        RenderJokerSpec {
            variant: "point-oa-w1023",
            submit_name: "point-vf-giant-oa-w1023",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023-nowmpoint") {
        RenderJokerSpec {
            variant: "point-oa-w1023-nowmpoint",
            submit_name: "point-vf-giant-oa-w1023-nowmpoint",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023NoWmPoint,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023-scissor") {
        RenderJokerSpec {
            variant: "point-oa-w1023-scissor",
            submit_name: "point-vf-giant-oa-w1023-scissor",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023Scissor,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-vtxw") {
        RenderJokerSpec {
            variant: "point-oa-vtxw",
            submit_name: "point-vf-giant-oa-vtxw",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidthVertex,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-w1023") {
        RenderJokerSpec {
            variant: "point-oa-early-w1023",
            submit_name: "point-vf-giant-oa-early-w1023",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyPointWidth1023,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-msrast-force") {
        RenderJokerSpec {
            variant: "point-oa-early-msrast-force",
            submit_name: "point-vf-giant-oa-early-msrast-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyMsRasterForced,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-bt1") {
        RenderJokerSpec {
            variant: "point-bt1",
            submit_name: "point-vf-giant-bt1",
            target: offscreen,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-slot0") {
        RenderJokerSpec {
            variant: "point-slot0",
            submit_name: "point-vf-giant-slot0",
            target: offscreen,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-scratch") {
        RenderJokerSpec {
            variant: "screen-vs-scratch",
            submit_name: "screen-vs-scratch",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-oa") {
        RenderJokerSpec {
            variant: "screen-vs-oa",
            submit_name: "screen-vs-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-ndc-oa") {
        RenderJokerSpec {
            variant: "screen-vs-ndc-oa",
            submit_name: "screen-vs-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-ndc-oa-hz0") {
        RenderJokerSpec {
            variant: "screen-vs-ndc-oa-hz0",
            submit_name: "screen-vs-ndc-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-sbe0") {
        RenderJokerSpec {
            variant: "screen-vs-sbe0",
            submit_name: "screen-vs-sbe0",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-slot0-oa") {
        RenderJokerSpec {
            variant: "screen-vs-slot0-oa",
            submit_name: "screen-vs-slot0-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-urb2-oa") {
        RenderJokerSpec {
            variant: "screen-vs-urb2-oa",
            submit_name: "screen-vs-urb2-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-urb2-slot0-oa") {
        RenderJokerSpec {
            variant: "screen-vs-urb2-slot0-oa",
            submit_name: "screen-vs-urb2-slot0-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa") {
        RenderJokerSpec {
            variant: "vf-rect-oa",
            submit_name: "vf-rect-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-pos0") {
        RenderJokerSpec {
            variant: "vf-rect-oa-pos0",
            submit_name: "vf-rect-oa-pos0",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-header") {
        RenderJokerSpec {
            variant: "vf-rect-oa-header",
            submit_name: "vf-rect-oa-header",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-deref0") {
        RenderJokerSpec {
            variant: "vf-rect-oa-deref0",
            submit_name: "vf-rect-oa-deref0",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOaDerefBlock0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa",
            submit_name: "vf-rect-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-halign128") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-halign128",
            submit_name: "vf-rect-ndc-oa-halign128",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sbe-pre-clip",
            submit_name: "vf-rect-ndc-oa-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sbe-pre-sf",
            submit_name: "vf-rect-ndc-oa-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-drawrect-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-drawrect-early",
            submit_name: "vf-rect-ndc-oa-drawrect-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaDrawRectEarlyOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sample-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sample-early",
            submit_name: "vf-rect-ndc-oa-sample-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSampleMaskEarlyOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-pc-clip-sf") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-pc-clip-sf",
            submit_name: "vf-rect-ndc-oa-pc-clip-sf",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPipeControlClipSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz-pre-wm") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz-pre-wm",
            submit_name: "vf-rect-ndc-oa-hz-pre-wm",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaWmHzOpBeforeWm,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz-post-extra") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz-post-extra",
            submit_name: "vf-rect-ndc-oa-hz-post-extra",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaWmHzOpAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-attr") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-attr",
            submit_name: "vf-rect-ndc-oa-payload-attr",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadAttributeEnable,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-depthw") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-depthw",
            submit_name: "vf-rect-ndc-oa-payload-depthw",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadSourceDepthW,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-bary") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-bary",
            submit_name: "vf-rect-ndc-oa-payload-bary",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadBaryPlanes,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-persp") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-persp",
            submit_name: "vf-rect-ndc-oa-persp",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipPerspective,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clipxy") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clipxy",
            submit_name: "vf-rect-ndc-oa-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-disable") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-disable",
            submit_name: "vf-rect-ndc-oa-clip-disable",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-force") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-force",
            submit_name: "vf-rect-ndc-oa-clip-force",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipForceMode,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-d3d") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-d3d",
            submit_name: "vf-rect-ndc-oa-clip-d3d",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipApiD3d,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-early-clipxy") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-early-clipxy",
            submit_name: "vf-rect-ndc-oa-early-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlyClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-frontccw") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-frontccw",
            submit_name: "vf-rect-ndc-oa-frontccw",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaFrontCcw,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz0") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz0",
            submit_name: "vf-rect-ndc-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-early",
            submit_name: "vf-rect-ndc-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-bt1") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-bt1",
            submit_name: "vf-rect-ndc-oa-bt1",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaBtCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-b-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-b-oa",
            submit_name: "vf-rect-ndc-order-b-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_cw,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-oa",
            submit_name: "vf-rect-ndc-order-c-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-early-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-early-oa",
            submit_name: "vf-rect-ndc-order-c-early-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-clip-disable-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-clip-disable-oa",
            submit_name: "vf-rect-ndc-order-c-clip-disable-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-mesa-simple-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-mesa-simple-oa",
            submit_name: "vf-rect-ndc-mesa-simple-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRect,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-mesa-nosrc-header-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-mesa-nosrc-header-oa",
            submit_name: "vf-rect-ndc-mesa-nosrc-header-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-small-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-small-oa",
            submit_name: "vf-rect-ndc-small-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_small,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-cw-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-cw-oa",
            submit_name: "vf-rect-ndc-cw-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_cw,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-alt-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-alt-oa",
            submit_name: "vf-rect-ndc-alt-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_alt,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-oa",
            submit_name: "vf-rect-order-b-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-early-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-early-oa",
            submit_name: "vf-rect-order-b-early-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-scissor-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-scissor-oa",
            submit_name: "vf-rect-order-b-scissor-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaScissorOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa",
            submit_name: "vf-rect-mesa-simple-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRect,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa-early") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa-early",
            submit_name: "vf-rect-mesa-simple-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-mesa-simple-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-mesa-simple-oa-early",
            submit_name: "vf-tri-mesa-simple-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_tri_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa-arm") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa-arm",
            submit_name: "vf-rect-mesa-simple-oa-arm",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectArtificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-nosrc-header-oa") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-nosrc-header-oa",
            submit_name: "vf-rect-mesa-nosrc-header-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-c-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-c-oa",
            submit_name: "vf-rect-order-c-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa",
            submit_name: "vf-tri-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa-early",
            submit_name: "vf-tri-ndc-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa-early-clipxy") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa-early-clipxy",
            submit_name: "vf-tri-ndc-oa-early-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaEarlyClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-cw-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-cw-oa-early",
            submit_name: "vf-tri-ndc-cw-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle_cw,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-rect-scratch") {
        RenderJokerSpec {
            variant: "screen-rect-scratch",
            submit_name: "screen-rect-scratch",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-rect-oa-early") {
        RenderJokerSpec {
            variant: "screen-rect-oa-early",
            submit_name: "screen-rect-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("so-vf") {
        RenderJokerSpec {
            variant: "so-vf",
            submit_name: "joker-vf-streamout",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vf-header") {
        RenderJokerSpec {
            variant: "so-vf-header",
            submit_name: "joker-vf-streamout-header",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs") {
        RenderJokerSpec {
            variant: "so-vs",
            submit_name: "joker-vs-streamout",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs-header") {
        RenderJokerSpec {
            variant: "so-vs-header",
            submit_name: "joker-vs-streamout-header",
            target: offscreen,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("bt1") {
        RenderJokerSpec {
            variant: "bt1",
            submit_name: "ps-bt1-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("wm-normal") || name.eq_ignore_ascii_case("wm") {
        RenderJokerSpec {
            variant: "wm-normal",
            submit_name: "ps-wm-normal-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmNormalDispatch,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot0") {
        RenderJokerSpec {
            variant: "slot0",
            submit_name: "ps-dispatch-slot0-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot1") {
        RenderJokerSpec {
            variant: "slot1",
            submit_name: "ps-dispatch-slot1-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot2") {
        RenderJokerSpec {
            variant: "slot2",
            submit_name: "ps-dispatch-slot2-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("all") || name.eq_ignore_ascii_case("slots-all") {
        RenderJokerSpec {
            variant: "all",
            submit_name: "ps-dispatch-all-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchAllKspSlots,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16") {
        RenderJokerSpec {
            variant: "simd16",
            submit_name: "ps-simd16-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16-retire") {
        RenderJokerSpec {
            variant: "simd16-retire",
            submit_name: "ps-simd16-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("eot") {
        RenderJokerSpec {
            variant: "eot",
            submit_name: "ps-eot-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("eot-retire") {
        RenderJokerSpec {
            variant: "eot-retire",
            submit_name: "ps-eot-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("cps") || name.eq_ignore_ascii_case("cps-disabled") {
        RenderJokerSpec {
            variant: "cps",
            submit_name: "ps-cps-disabled-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("cps-retire") {
        RenderJokerSpec {
            variant: "cps-retire",
            submit_name: "ps-cps-disabled-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("hz") || name.eq_ignore_ascii_case("wm-hz") {
        RenderJokerSpec {
            variant: "hz",
            submit_name: "wm-hz-sample-mask-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("hz-retire") || name.eq_ignore_ascii_case("wm-hz-retire") {
        RenderJokerSpec {
            variant: "hz-retire",
            submit_name: "wm-hz-sample-mask-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit") || name.eq_ignore_ascii_case("late-reemit") {
        RenderJokerSpec {
            variant: "reemit",
            submit_name: "wm-late-reemit-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("reemit-retire")
        || name.eq_ignore_ascii_case("late-reemit-retire")
    {
        RenderJokerSpec {
            variant: "reemit-retire",
            submit_name: "wm-late-reemit-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-retire",
            submit_name: "wm-late-reemit-vs-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-slot0-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-slot0-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-slot0-retire",
            submit_name: "wm-late-reemit-vs-slot0-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-urb2-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-urb2-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-urb2-retire",
            submit_name: "wm-late-reemit-vs-urb2-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-urb2-slot0-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-urb2-slot0-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-urb2-slot0-retire",
            submit_name: "wm-late-reemit-vs-urb2-slot0-big-primitive-retire",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("payload-push") {
        RenderJokerSpec {
            variant: "payload-push",
            submit_name: "ps-payload-push-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadPushConstant,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-attr") {
        RenderJokerSpec {
            variant: "payload-attr",
            submit_name: "ps-payload-attr-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadAttributeEnable,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-simple") {
        RenderJokerSpec {
            variant: "payload-simple",
            submit_name: "ps-payload-simple-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSimpleHint,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-depthw") {
        RenderJokerSpec {
            variant: "payload-depthw",
            submit_name: "ps-payload-source-depth-w-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSourceDepthW,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-bary") || name.eq_ignore_ascii_case("bary") {
        RenderJokerSpec {
            variant: "payload-bary",
            submit_name: "ps-payload-bary-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadBaryPlanes,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf1") {
        RenderJokerSpec {
            variant: "grf1",
            submit_name: "ps-grf-start-r1-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf2") {
        RenderJokerSpec {
            variant: "grf2",
            submit_name: "ps-grf-start-r2-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf4") {
        RenderJokerSpec {
            variant: "grf4",
            submit_name: "ps-grf-start-r4-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR4,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt31") {
        RenderJokerSpec {
            variant: "mt31",
            submit_name: "ps-grf-maxthreads-31-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads31,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt15") {
        RenderJokerSpec {
            variant: "mt15",
            submit_name: "ps-grf-maxthreads-15-big-primitive",
            target: offscreen,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads15,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("sync-light") {
        RenderJokerSpec {
            variant: "sync-light",
            submit_name: "postdraw-light-only-retire",
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightOnlyRetire,
        }
    } else if name.eq_ignore_ascii_case("sync-post-no-cs") {
        RenderJokerSpec {
            variant: "sync-post-no-cs",
            submit_name: "postdraw-pc-postsync-no-cs",
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("sync-cs-no-post") {
        RenderJokerSpec {
            variant: "sync-cs-no-post",
            submit_name: "postdraw-pc-cs-no-postsync",
            target: offscreen,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightCsNoPostSync,
        }
    } else {
        return None;
    };
    Some(spec)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn render_joker_real_vs_front_end_contract(variant: &str) -> Option<TriangleFrontEndContract> {
    match variant {
        "reemit-vs-retire" => Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT),
        "reemit-vs-slot0-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[1]),
        "reemit-vs-urb2-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[2]),
        "reemit-vs-urb2-slot0-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[3]),
        "screen-vs-sbe0" => Some(VS_DRAW_SBE_READ0_CONTRACT),
        "screen-vs-ndc-oa" | "screen-vs-ndc-oa-hz0" => Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT),
        "screen-vs-slot0-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[1]),
        "screen-vs-urb2-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[2]),
        "screen-vs-urb2-slot0-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[3]),
        "screen-vs-scratch" | "screen-vs-oa" | "screen-rect-scratch" | "screen-rect-oa-early" => {
            Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT)
        }
        _ => None,
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn render_joker_vf_experiment(variant: &str) -> StreamoutProofExperiment {
    match variant {
        "point-oa-pos0" => StreamoutProofExperiment::PositionSlot0,
        "vf-rect-mesa-simple-oa"
        | "vf-rect-mesa-simple-oa-early"
        | "vf-rect-mesa-simple-oa-arm"
        | "vf-rect-ndc-mesa-simple-oa"
        | "vf-rect-mesa-nosrc-header-oa"
        | "vf-rect-ndc-mesa-nosrc-header-oa" => StreamoutProofExperiment::PositionSlot0,
        "vf-rect-oa-pos0" => StreamoutProofExperiment::PositionSlot0,
        "point-oa-header" | "vf-rect-oa-header" | "so-vf-header" | "so-vs-header" => {
            StreamoutProofExperiment::HeaderAndPositionSlots01
        }
        "point-oa-vtxw" => StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        _ => StreamoutProofExperiment::PositionSlot1,
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn render_joker_streamout_kind(variant: &str) -> Option<&'static str> {
    match variant {
        "so-vf" | "so-vf-header" => Some("vf"),
        "so-vs" | "so-vs-header" => Some("vs"),
        _ => None,
    }
}

