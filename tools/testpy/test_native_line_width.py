#!/usr/bin/env python3
"""Check the production native line-width conversion and SF state field."""
from pathlib import Path
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[2]
TESTS = r'''
#[test] fn adjacency_requires_clip_enabled_even_for_in_bounds_vertices() {
    assert!(native_adjacency_clip_enabled(0xD4000001));
    assert!(!native_adjacency_clip_enabled(0x04000000));
    assert!(!native_adjacency_clip_enabled(0));
}
#[test] fn default_and_explicit_screen_pixel_widths() {
    for (px,raw) in [(0.0,384),(1.0,128),(1.5,192),(3.0,384),(8.0,1024)] {
        assert_eq!(resident_line_width_raw(px),Some(raw));
    }
}
#[test] fn fractional_precision_and_bounds() {
    assert_eq!(resident_line_width_raw(1.0/128.0),Some(1));
    assert_eq!(resident_line_width_raw(1.002),Some(128));
    assert_eq!(resident_line_width_raw(1.006),Some(129));
    assert_eq!(resident_line_width_raw(262143.0/128.0),Some(262143));
    for px in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY,-1.0,2048.0,0.001] {
        assert_eq!(resident_line_width_raw(px),None);
    }
}
#[test] fn replaces_captured_width_instead_of_oring_it_into_eight() {
    const MASK:u32=0x3ffff<<12;
    for base in [0x0008_0402,0x402,0xc008_0402] {
        let state=resident_line_width_sf_dw1(base,true,1024);
        assert_eq!((state&MASK)>>12,1024); // 8px, never captured 1px OR 8px = 9px.
        assert_eq!(state&!MASK,base&!MASK);
        assert_eq!(resident_line_width_sf_dw1(base,false,1024),base);
        assert_eq!((resident_line_width_sf_dw1(state,true,0)&MASK)>>12,384);
    }
}
'''
def main():
    source=(ROOT/'src/intel/render/pipeline.rs').read_text()
    source=source[source.index('const fn native_adjacency_clip_enabled('):source.index('/// Zero is the legacy Maxpix default')]
    with tempfile.TemporaryDirectory(prefix='native-line-width-') as d:
        path=Path(d);(path/'test.rs').write_text(source+TESTS)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests')],check=True)
if __name__=='__main__':main()
