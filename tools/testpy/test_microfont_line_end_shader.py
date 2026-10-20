#!/usr/bin/env python3
"""Verify the native PS constant-buffer packet and the checked shader ABI."""
from pathlib import Path
import json,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
def main():
    source=(ROOT/'src/intel/render/pipeline.rs').read_text()
    a=source.index('fn rounded_line_constant_ps_packet(')
    code=source[a:source.index('/// Gen12 ANV places',a)]
    tests=r'''
#[test] fn ps_uses_two_grfs_in_slot_three_with_full_address() {
    assert_eq!(rounded_line_constant_ps_packet(0x1_0000_0040).unwrap(),
        [0x78170409,0,2<<16,0,0,0,0,0,0,0x40,1]);
}
#[test] fn bad_constant_address_is_rejected() {
    assert!(rounded_line_constant_ps_packet(0).is_err());
    assert!(rounded_line_constant_ps_packet(1).is_err());
}
'''
    with tempfile.TemporaryDirectory(prefix='line-end-ps-') as d:
        d=Path(d);(d/'tests.rs').write_text('const RENDER_MOCS:u32=4;\n'+code+tests)
        subprocess.run(['rustc','--edition=2024','--test',str(d/'tests.rs'),'-o',str(d/'tests')],check=True)
        subprocess.run([str(d/'tests')],check=True)
    manifest=json.loads((ROOT/'crates/trueos-shader/microfont_line_ends/manifest.json').read_text())
    p=manifest['ps_state']
    assert p['push_bytes']==64 and p['grf_start16']==6
    assert p['uses_src_depth']==p['uses_src_w']==p['uses_vmask']==1
    assert p['scratch_bytes']==p['num_varying_inputs']==p['sampler_count']==p['computed_depth_mode']==0
    isa=(ROOT/'crates/trueos-shader/microfont_line_ends/line_ends.ps.iga.txt').read_text()
    assert '(f1.0)  sendc.rc' in isa and 'render target write SIMD16' in isa
    subprocess.run(['python3',str(ROOT/'tools/microfont-line-end-bake/bake.py'),'--ensure-current'],check=True)
if __name__=='__main__':main()
