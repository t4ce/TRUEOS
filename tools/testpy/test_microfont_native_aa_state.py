#!/usr/bin/env python3
"""Check scoped Gen12 native-AA state and the ordinary render-cache message."""
from pathlib import Path
import subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
pipeline=(ROOT/'src/intel/render/pipeline.rs').read_text()
a=pipeline.index('const fn microfont_line_aa_raster(')
b=pipeline.index('const fn resident_line_width_sf_dw1(',a)
code=pipeline[a:b]
tests=r'''
#[test] fn aa_selects_legacy_single_sample_and_preserves_unrelated_bits() {
    let base=0x04A11C03;
    let enabled=microfont_line_aa_raster(base,true);
    assert_eq!(enabled&(1<<2),1<<2);
    let ms=(3<<22)|(1<<14)|(1<<12)|(3<<10);
    assert_eq!(enabled&ms,0);
    assert_eq!(enabled&!(ms|(1<<2)),base&!(ms|(1<<2)));
    assert_eq!(microfont_line_aa_raster(base,false),base);
}
#[test] fn wm_uses_one_pixel_side_fringe_and_half_pixel_caps() {
    let base=0x80001C00;
    let enabled=microfont_line_aa_wm(base,true);
    assert_eq!((enabled>>6)&3,2);assert_eq!((enabled>>8)&3,0);
    assert_eq!(enabled&!0x3c0,base&!0x3c0);
    assert_eq!(microfont_line_aa_wm(base,false),base);
}
'''
with tempfile.TemporaryDirectory(prefix='native-aa-state-') as tmp:
    tmp=Path(tmp);(tmp/'tests.rs').write_text(code+tests)
    subprocess.run(['rustc','--edition=2024','--test',str(tmp/'tests.rs'),'-o',str(tmp/'tests')],check=True)
    subprocess.run([str(tmp/'tests')],check=True)
isa=(ROOT/'crates/trueos-shader/microfont_line_ends/line_ends.ps.iga.txt').read_text()
assert 'render target write SIMD16' in isa and 'replicated' not in isa
assert '0x10031000' in isa  # Regular RTW, slot group LO: bypassed AA/coverage data.
primary=(ROOT/'src/intel/render/primary.rs').read_text()
assert 'ResidentSceneFragmentContract::MicrofontAa(_) | ResidentSceneFragmentContract::MicrofontMask(_) => TriangleBlendProbeMode::StraightAlpha' in primary
assert 'if draw.microfont_aa && depth_config.is_some()' in primary

assert 'microfont_line_aa_wm(0x8000_0040 | wm_barycentric_mode, native_line_aa)' in pipeline
