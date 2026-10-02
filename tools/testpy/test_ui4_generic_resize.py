#!/usr/bin/env python3
"""Host regressions for production resize-latch policy and dock masks."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
extract.ROOT = Path(__file__).resolve().parents[2]
item = extract.item

SOURCE = "src/ui4/input_broker.rs"
def main():
    harness = "#![allow(dead_code)]\n"
    harness += "const PRIMARY_BUTTON_MASK:u32=1; const DOCK_REFERENCE_WIDTH_MM:u32=64; const DOCK_REFERENCE_HEIGHT_MM:u32=40; const DOCK_CORNER_MM:u32=24; const DOCK_EDGE_DEPTH_MM:u32=12;\n"
    harness += item("src/ui4/window_broker.rs", "WindowDockTarget")
    harness += item(SOURCE, "Ui4VisualRect") + item(SOURCE, "Ui4DockZone").replace("pub(super)", "pub(crate)").replace("super::WindowDockTarget", "WindowDockTarget")
    harness += "mod broker { use super::*;\n"
    for name in ["scale_reference", "DockZoneMetrics", "dock_zone_metrics", "clamp_zone_metric", "dock_zones_with_reference", "dock_target_at_in_zones", "dock_zone_contains", "dock_zone_local_contains", "normalized_ellipse_contains", "visual_rect_contains", "dock_zone_row_span", "dock_zone_column_span", "first_true_suffix", "last_true_prefix", "resize_latch_cancelled", "selection_rect_between"]:
        harness += item(SOURCE, name)
    harness += """
    #[test] fn latch_deadline_and_every_other_button() {
        assert!(!resize_latch_cancelled(500,10499,0,0));
        assert!(!resize_latch_cancelled(500,10499,1,0));
        assert!(resize_latch_cancelled(500,10500,1,0));
        for button in 1..32 { assert!(resize_latch_cancelled(500,501,1<<button,0)); }
        assert!(resize_latch_cancelled(500,501,0,-1));
    }
    #[test] fn bottom_center_is_curved_and_matches_paint() {
        let zones=dock_zones_with_reference(1920,1080,Some((256,160)));
        assert_eq!(dock_target_at_in_zones(960,1079,&zones),Some(WindowDockTarget::GenericResize));
        assert_eq!(dock_target_at_in_zones(960,1000,&zones),None);
        let zone=zones[7];
        for y in 0..zone.rect.height { for x in 0..zone.rect.width {
            let px=zone.rect.x+x; let py=zone.rect.y+y;
            let hit=dock_zone_contains(zone,px,py);
            assert_eq!(hit,dock_zone_row_span(zone,y).is_some_and(|r|visual_rect_contains(r,px,py)));
            assert_eq!(hit,dock_zone_column_span(zone,x).is_some_and(|r|visual_rect_contains(r,px,py)));
        }}
    }
    #[test] fn rectangle_supports_both_drag_directions() {
        let a=selection_rect_between((600,500),(100,200));
        assert_eq!(a,Ui4VisualRect{x:100,y:200,width:501,height:301});
        assert_eq!(a,selection_rect_between((100,200),(600,500)));
    }
    }
    """
    with tempfile.TemporaryDirectory(prefix="trueos-generic-resize-") as directory:
        root=Path(directory); rust=root/"test.rs"; binary=root/"test"
        rust.write_text(harness)
        subprocess.run(["rustc","--edition=2024","--test",str(rust),"-o",str(binary)],check=True)
        subprocess.run([str(binary)],check=True)
if __name__ == "__main__": main()
