#!/usr/bin/env python3
"""Production character-area API: ring pixels, resizing, guards and budget.

Run with --nocapture to retain the JSON scenario protocol. These are host
planning/descriptor expectations; they do not claim GPU admission/retirement.
"""
from pathlib import Path
import subprocess
import tempfile
import sys

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = r'''#![allow(dead_code)]
extern crate alloc;
#[path = "__ROOT__/src/ui4/text_area.rs"] mod area;
#[path = "__ROOT__/src/allcaps.rs"] mod allcaps;
use area::*;
const CAP: usize = allcaps::shell3::SH3_PANBUFFER_CAP_BYTES;
fn rect(x:i64,y:i64,c:u32,r:u32)->CellRect {CellRect{x,y,columns:c,rows:r}}
fn layout(v:CellRect)->RasterLayout {RasterLayout::fit(v,(6,11),4,CAP).unwrap()}
fn value(x:i64,y:i64)->i128 {i128::from(y)*1000000+i128::from(x)}
fn paint(pixels:&mut [i128], l:RasterLayout, u:AreaUpdate<i128>) {
    for w in u.writes { pixels[w.y as usize*l.columns as usize+w.x as usize]=w.cell; }
}
fn check_view(a:&TextArea<i128>,v:CellRect,pixels:&[i128]) {
    let l=a.layout(); let copies=a.view_copies(v).unwrap(); assert!(copies.len()<=4);
    let mut seen=vec![false;v.columns as usize*v.rows as usize];
    for copy in copies {
        assert!(copy.source_x+copy.columns<=l.columns);
        assert!(copy.source_y+copy.rows<=l.rows);
        for y in 0..copy.rows {for x in 0..copy.columns {
            let dx=copy.destination_x+x;let dy=copy.destination_y+y;
            let out=dy as usize*v.columns as usize+dx as usize;
            assert!(!seen[out]);seen[out]=true;
            assert_eq!(pixels[(copy.source_y+y) as usize*l.columns as usize+(copy.source_x+x) as usize],
                value(v.x+i64::from(dx),v.y+i64::from(dy)));
        }}
    }
    assert!(seen.into_iter().all(|v|v));
}

#[test] fn static_pan_produces_only_exposed_strips() {
    let v=rect(0,0,80,22); let l=layout(v);let mut a=TextArea::new(l);
    let first=a.update(v,7,value).unwrap();assert_eq!(first.produced,88*30);
    let u=a.update(rect(0,1,80,22),7,value).unwrap();assert_eq!(u.produced,88);
    assert_eq!(u.writes.len(),88);assert_eq!(u.reused,88*29);
    let u=a.update(rect(0,0,80,22),7,value).unwrap();assert_eq!(u.produced,88);
    let u=a.update(rect(1,0,80,22),7,value).unwrap();assert_eq!(u.produced,30);
    let u=a.update(rect(2,1,80,22),7,value).unwrap();assert_eq!(u.produced,88+30-1);
    let u=a.update(rect(2,1,80,22),7,|_,_|panic!("retained producer call")).unwrap();
    assert_eq!(u.produced,0);assert!(u.writes.is_empty());
}

#[test] fn revision_compares_content_and_invalidation_rebuilds() {
    let v=rect(0,0,80,22);let mut a=TextArea::new(layout(v));a.update(v,0,value).unwrap();
    let u=a.update(v,1,|x,y|value(x,y)+i128::from(x==4&&y==5)).unwrap();
    assert_eq!(u.produced,88*30);assert_eq!(u.writes.len(),1);
    a.invalidate();assert!(a.view_copies(v).is_none());
    assert_eq!(a.update(v,1,value).unwrap().writes.len(),88*30);
}

#[test] fn ring_wrap_and_bidirectional_diagonal_pans_preserve_every_cell() {
    let mut v=rect(-8,-12,11,7);let l=layout(v);let mut a=TextArea::new(l);
    let mut pixels=vec![i128::MIN;l.columns as usize*l.rows as usize];
    for (x,y) in [(-8,-12),(-7,-11),(0,0),(12,22),(40,44),(42,43),(-33,-87),(999,1010)] {
        v.x=x;v.y=y;paint(&mut pixels,l,a.update(v,0,value).unwrap());check_view(&a,v,&pixels);
    }
}

#[test] fn resize_copies_overlap_and_paints_only_new_cells() {
    let v=rect(9,17,80,22);let old_l=layout(v);let mut a=TextArea::new(old_l);
    let mut old=vec![i128::MIN;old_l.columns as usize*old_l.rows as usize];
    paint(&mut old,old_l,a.update(v,1,value).unwrap());
    for v in [rect(9,17,120,42),rect(4,8,20,3),rect(-6,-9,90,23)] {
        let new_l=layout(v);let (mut next,copies)=a.resized(new_l,v).unwrap();
        let mut new=vec![i128::MIN;new_l.columns as usize*new_l.rows as usize];
        for c in copies {for y in 0..c.rows {for x in 0..c.columns {
            new[(c.destination_y+y) as usize*new_l.columns as usize+(c.destination_x+x) as usize]=
                old[(c.source_y+y) as usize*a.layout().columns as usize+(c.source_x+x) as usize];
        }}}
        paint(&mut new,new_l,next.update(v,1,value).unwrap());check_view(&next,v,&new);
        a=next;old=new;
    }
}

#[test] fn font_change_cannot_reuse_old_pixels() {
    let v=rect(0,0,20,4);let mut a=TextArea::new(layout(v));a.update(v,0,value).unwrap();
    let l=RasterLayout::fit(v,(12,22),4,CAP).unwrap();let (mut a,copies)=a.resized(l,v).unwrap();
    assert!(copies.is_empty());assert_eq!(a.update(v,0,value).unwrap().reused,0);
}

#[test] fn budgets_include_padding_reduce_guards_and_fallback_without_truncation() {
    let v=rect(0,0,80,22);let l=layout(v);assert_eq!(l.bytes,700416);
    assert_eq!((l.columns,l.rows),(88,30));assert_eq!(l.pitch%64,0);assert_eq!(l.bytes%4096,0);
    let l=RasterLayout::fit(v,(12,22),4,CAP).unwrap();
    assert_eq!((l.columns,l.rows),(82,24));assert_eq!(l.bytes,CAP);
    assert!(RasterLayout::fit(rect(0,0,640,193),(6,11),4,CAP).is_none());
    assert!(RasterLayout::fit(v,(6,11),4,4095).is_none());
    for v in [rect(0,0,0,1),rect(0,0,1,0),rect(i64::MAX,0,2,1),rect(0,i64::MAX,1,2),rect(0,0,u32::MAX,u32::MAX)] {
        assert!(RasterLayout::fit(v,(6,11),4,CAP).is_none());
    }
    assert!(RasterLayout::fit(rect(0,0,1,1),(0,11),4,CAP).is_none());
}

#[test] fn six_regions_share_a_single_cap_and_resize_peak_is_charged() {
    let budget=RasterBudget::new(CAP);let mut regions=Vec::new();
    for _ in 0..6 {regions.push(budget.clone().reserve(327680).unwrap());}
    assert_eq!(budget.used(),1966080);assert!(budget.reserve(131073).is_none());
    let exact=budget.reserve(131072).unwrap();assert_eq!(budget.used(),CAP);
    assert!(budget.reserve(usize::MAX).is_none());drop(exact);drop(regions.pop());
    assert_eq!(budget.used(),1638400);drop(regions);assert_eq!(budget.used(),0);
    let old=budget.reserve(700416).unwrap();
    assert!(budget.reserve(1691648).is_none());drop(old);
    assert!(budget.reserve(1691648).is_some());
}

#[test] fn matrix_scenario_protocol() {
    let mut a=TextArea::new(layout(rect(0,0,80,22)));
    for offset in [0,1,2,1,234,234,233,0,0] {
        let v=rect(0,offset,80,22);let l=a.layout();
        let u=a.update(v,0,|x,y|if (0..256).contains(&y)&&(0..2048).contains(&x) {value(x,y)} else {0}).unwrap();
        let paint=u.writes.len();let copies=a.view_copies(v).unwrap();
        println!("{{\"offset\":{},\"viewport\":[80,22],\"tile\":[{},{}],\"rgba_bytes\":{},\"retained_history_chars\":524288,\"produced\":{},\"paint_cells\":{},\"expected_mono_batches\":{},\"expected_copy_batches\":1,\"copy_rectangles\":{}}}",
            offset,l.columns,l.rows,l.bytes,u.produced,paint,paint.div_ceil(64),copies.len());
    }
}
'''.replace('__ROOT__', str(ROOT))
    with tempfile.TemporaryDirectory(prefix='trueos-text-area-') as temp:
        path = Path(temp)/'test.rs'
        path.write_text(source)
        binary = Path(temp)/'test'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1', *sys.argv[1:]], check=True)


if __name__ == '__main__':
    main()
