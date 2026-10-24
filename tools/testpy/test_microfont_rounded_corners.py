#!/usr/bin/env python3
"""Check real rounded-corner recipes, adaptive budgets, seams and coverage."""
from pathlib import Path
import subprocess
import tempfile
from _microfont_build import build_microfont

ROOT = Path(__file__).resolve().parents[2]
TESTS = r'''
use microfont::raster_quad::*;
const CORNERS:[char;4]=['╭','╮','╯','╰'];
fn area(q:[[f32;2];4])->f64 {
    (0..4).map(|i| {let a=q[i];let b=q[(i+1)%4];
        a[0] as f64*b[1] as f64-a[1] as f64*b[0] as f64}).sum::<f64>()*0.5
}
fn interiors_disjoint(a:[[f32;2];4],b:[[f32;2];4])->bool {
    for q in [a,b] { for i in 0..4 {
        let p=q[i];let n=q[(i+1)%4];let axis=[n[1]-p[1],p[0]-n[0]];
        let range=|q:[[f32;2];4]| q.into_iter().map(|p|p[0]*axis[0]+p[1]*axis[1])
            .fold((f32::INFINITY,f32::NEG_INFINITY),|(lo,hi),v|(lo.min(v),hi.max(v)));
        let (lo,hi)=range(a);let (bl,bh)=range(b);
        if hi<=bl+0.00001 || bh<=lo+0.00001 {return true;}
    }} false
}
#[test] fn physical_anchors_pixel_error_and_caps_are_respected() {
    for ch in CORNERS {
        assert_eq!(corner_curve_quads(ch,38,Some(38)),Some(4));
        assert_eq!(corner_curve_quads(ch,76,Some(38)),Some(8));
        assert_eq!(corner_curve_quads(ch,152,Some(38)),Some(16));
        assert_eq!(corner_curve_quads(ch,1,Some(38)),Some(4));
        assert_eq!(corner_curve_quads(ch,4000,Some(38)),Some(16));
        assert!(corner_curve_quads(ch,300,Some(300)).unwrap()>4);
        let mut previous=4;
        for h in 1..=4000 {
            let n=corner_curve_quads(ch,h,None).unwrap();
            assert!((4..=16).contains(&n));assert!(n>=previous);previous=n;
            if n<16 {
                let radius=2.5*h as f64/11.0;
                assert!(radius*(1.0-(std::f64::consts::FRAC_PI_2/(2.0*n as f64)).cos())<=0.2501);
            }
        }
    }
}
#[test] fn every_variant_is_convex_bounded_and_has_no_overlapping_quad_interiors() {
    for ch in CORNERS {for n in 4..=16 {
        let quads=glyph_quads_with_curve_quads(ch,n).unwrap();
        assert_eq!(quads.len(),2+n as usize);
        assert!(quads.get(quads.len()).is_none());
        let all:Vec<_>=quads.iter().map(|q|q.vertices).collect();
        for q in &all {
            assert!(area(*q)>0.0);
            for p in q {assert!(p[0].is_finite() && p[1].is_finite());
                assert!((0.0..=6.0).contains(&p[0])&&(0.0..=11.0).contains(&p[1]));}
            for i in 0..4 {
                let a=q[i];let b=q[(i+1)%4];let c=q[(i+2)%4];
                assert!((b[0]-a[0])*(c[1]-b[1])-(b[1]-a[1])*(c[0]-b[0])>0.0);
            }
        }
        for (i,a) in all.iter().enumerate() {for b in &all[i+1..] {
            assert!(interiors_disjoint(*a,*b),"overlap {ch} budget {n}");
        }}
        // Polygonal annulus area plus two straight arms: any hole or overlap
        // changes this independently computed area, rather than just the count.
        let arm_area=area(all[0])+area(all[1]);
        let expected=arm_area+2.0*n as f64*(std::f64::consts::FRAC_PI_2/n as f64).sin();
        let actual:f64=all.iter().map(|q|area(*q)).sum();
        assert!((actual-expected).abs()<0.00001);
    }}
}
#[test] fn curve_edges_join_exactly_and_arms_match_box_tile_edges() {
    for ch in CORNERS {for n in 4..=16 {
        let q=glyph_quads_with_curve_quads(ch,n).unwrap();
        let mut previous=q.get(0).unwrap().vertices;
        for index in 2..q.len() {
            let current=q.get(index).unwrap().vertices;
            assert_eq!(current.iter().filter(|p|previous.contains(p)).count(),2);
            previous=current;
        }
        assert_eq!(previous.iter().filter(|p|q.get(1).unwrap().vertices.contains(p)).count(),2);
        let vertical=q.get(0).unwrap().vertices;
        let horizontal=q.get(1).unwrap().vertices;
        assert_eq!(vertical.iter().map(|p|p[0]).fold(f32::INFINITY,f32::min),2.0);
        assert_eq!(vertical.iter().map(|p|p[0]).fold(f32::NEG_INFINITY,f32::max),3.0);
        assert_eq!(horizontal.iter().map(|p|p[1]).fold(f32::INFINITY,f32::min),5.0);
        assert_eq!(horizontal.iter().map(|p|p[1]).fold(f32::NEG_INFINITY,f32::max),6.0);
        let vx=if matches!(ch,'╭'|'╮') {11.0} else {0.0};
        let hx=if matches!(ch,'╭'|'╰') {6.0} else {0.0};
        assert_eq!(vertical.iter().filter(|p|p[1]==vx).count(),2);
        assert_eq!(horizontal.iter().filter(|p|p[0]==hx).count(),2);
    }}
}
#[test] fn source_recipes_preserve_the_existing_bitmaps_and_regular_quads() {
    for (ch,mask) in [('╭',0xc4208208),('╮',0x810208208),('╯',0x8208210800000000),('╰',0x82082040c0000000)] {
        assert_eq!(microfont::bundle::bitmap(ch),Some(mask));
        assert!(GLYPH_CHARACTERS.contains(&ch));
        assert_eq!(glyph_quads(ch).unwrap().len(),6);
        assert!(microfont::raster_line::glyph_strips(ch).is_none());
        assert_eq!(glyph_quads_at_size(ch,152,Some(38)).unwrap().len(),18);
    }
    let ordinary=glyph_quads('─').unwrap().get(0).unwrap();
    assert_eq!(glyph_quads_at_size('─',4000,Some(38)).unwrap().get(0).unwrap(),ordinary);
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="microfont-rounded-corners-") as directory:
        work = Path(directory)
        library = build_microfont(work / "libmicrofont.rlib", features=("hex", "raster"))
        source = work / "tests.rs"
        source.write_text(TESTS)
        binary = work / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "--extern",
                        f"microfont={library}", "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
