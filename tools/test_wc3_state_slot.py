#!/usr/bin/env python3
"""Check real WC3/adjacency ISA and production descriptor layout fit a draw slot."""
from pathlib import Path
import subprocess, tempfile
from test_clip_position3_uv_texture import ROOT, constant, item
resources=(ROOT/'src/intel/render/resources.rs').read_text()
pipeline=(ROOT/'src/intel/render/pipeline.rs').read_text()
relocate=resources.split('    let mut graphics_end =',1)[1].split('    if pipeline.vs.meta.kernel.grf_used',1)[0]
layout=pipeline.split('    let mut cursor = shader_layout.state_region_offset_bytes as usize;',1)[1].split('    let dwords = unsafe {',1)[0]
source='#![allow(dead_code,unused_variables,unfulfilled_lint_expectations)]\n'
source+='mod intel { pub fn align_up(v:usize,a:usize)->Option<usize>{v.checked_add(a-1).map(|v|v & !(a-1))}\n'
source+=f'#[path="{ROOT}/src/intel/shader.rs"] pub mod shader; }}\n'
source+='\n'.join(constant('src/intel/render/pipeline.rs',n) for n in ['CPS_STATE_DWORDS_PER_VIEWPORT','CPS_STATE_VIEWPORTS','CPS_STATE_DWORDS','SAMPLER_CACHE_LINE_DWORDS'])
source+='\n'.join(constant('src/intel/render/constants.rs',n) for n in ['RESIDENT_SCENE_STATE_SLOT_BYTES','GFX125_SLICE_HASH_TABLE_DWORDS','GFX125_SLICE_HASH_TABLE_BYTES'])
source+='\n'.join(item('src/intel/render/resources.rs',n) for n in ['StageUploadRange','stage_range','stage_error','stage_end'])
source+='''
struct Warm { device_id:u16, draw_state_len:usize }
struct Layout { state_region_offset_bytes:u32 }
fn device_is_gfx125(id:u16)->bool { id==0xa780 }
fn required(device_id:u16, bytes:usize)->Result<usize,&'static str>{
 let pipeline=intel::shader::wc3_fixed_pipeline();
 let vs=stage_range("vs",pipeline.vs.meta.kernel,pipeline.vs.code)?;
 let ps=stage_range("ps",pipeline.ps.meta.kernel,pipeline.ps.code)?;
 let line=intel::shader::line_adjacency_geometry_shader();
 let triangle=intel::shader::triangle_adjacency_geometry_shader();
 let mut line_gs=stage_range("line",line.meta.kernel,line.code)?;
 let mut triangle_gs=stage_range("triangle",triangle.meta.kernel,triangle.code)?;
 let host_simd16:Option<StageUploadRange>=None;
 let mut graphics_end ='''+relocate+'''
 let used_end=graphics_end.max(line_gs.code_offset_bytes+line_gs.code_size_bytes)
     .max(triangle_gs.code_offset_bytes+triangle_gs.code_size_bytes);
 let shader_layout=Layout { state_region_offset_bytes:intel::align_up(used_end,4096).unwrap() as u32 };
 let warm=Warm { device_id, draw_state_len:bytes };
 let fixed_gl=true; let native_sampled=false; let native_pbr=false;
 let binding_table_entries=2usize; let ps_binding_table_entries=4usize;
 let surface_state_count=3usize;
 let mut cursor=shader_layout.state_region_offset_bytes as usize;
'''+layout+'''
 Ok(end_offset)
}
#[test] fn compiled_wc3_and_descriptors_fit_resident_slot(){
 for device in [0x4680,0xa780] {
  let used=required(device,RESIDENT_SCENE_STATE_SLOT_BYTES).unwrap();
  println!("device={device:x} bytes={used} slot={RESIDENT_SCENE_STATE_SLOT_BYTES}");
  assert!(used<=RESIDENT_SCENE_STATE_SLOT_BYTES);
 }
}
#[test] fn old_slot_reproduces_live_failure(){
 assert_eq!(required(0x4680,11*4096),Err("probe-state-exceeds-state-bo"));
}
'''
with tempfile.TemporaryDirectory(prefix='wc3-slot-') as tmp:
 src,exe=Path(tmp)/'test.rs',Path(tmp)/'test'
 src.write_text(source)
 subprocess.run(['rustc','--edition=2024','--test',str(src),'-o',str(exe)],check=True)
 subprocess.run([str(exe),'--nocapture'],check=True)
