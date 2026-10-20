#!/usr/bin/env python3
"""Compile-only ADL-S native line-end PS; ordinary make checks its source hash."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,re,struct,subprocess,sys
ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
FRAG=ROOT/'vendor/microfont/shaders/line_ends.frag'
VERT=ROOT/'crates/trueos-shader/xe_lp_shader_bake/simple_triangle.vert'
DUMPER=ROOT/'crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c'
OUT=ROOT/'crates/trueos-shader/microfont_line_ends'
GENERATED=ROOT/'crates/trueos-shader/generated_microfont_line_ends.rs'
def module(name,path):
    spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
def fingerprint():
    return hashlib.sha256(b''.join(p.read_bytes() for p in [Path(__file__),FRAG,VERT,DUMPER,ROOT/'tools/helio-intel-bake/bake.py',ROOT/'tools/wc3-fixed-bake/bake.py'])).hexdigest()
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--ensure-current',action='store_true');parser.add_argument('--mesa-build',type=Path,default=ROOT/'bld/toolchains/microfont-dista-mesa');args=parser.parse_args()
    sha=fingerprint();manifest=OUT/'manifest.json'
    if args.ensure_current and manifest.exists() and GENERATED.exists():
        old=json.loads(manifest.read_text())
        if old.get('source_sha256')==sha and old.get('rust_sha256')==hashlib.sha256(GENERATED.read_bytes()).hexdigest():
            print('microfont-line-ends: native shader current');return
    work=ROOT/'bld/microfont-line-end-bake';work.mkdir(parents=True,exist_ok=True);OUT.mkdir(parents=True,exist_ok=True)
    baker=module('helio_baker',ROOT/'tools/helio-intel-bake/bake.py')
    emitter=module('fixed_baker',ROOT/'tools/wc3-fixed-bake/bake.py')
    stages=[]
    for stage,path in [('vert',VERT),('frag',FRAG)]:
        target=work/f'line_ends.{stage}.spv';subprocess.run(['glslc','--target-env=vulkan1.1',str(path),'-o',str(target)],check=True);stages.append(target)
    src=DUMPER.read_text().replace('.size = sizeof(push_color),','.size = 64,').replace('.pushConstantRangeCount = push_color_enabled ? 1u : 0u,','.pushConstantRangeCount = 1,').replace('.pPushConstantRanges = push_color_enabled ? &push_constant_range : NULL,','.pPushConstantRanges = &push_constant_range,')
    marker='    dump_pipeline_executables(device, pipeline);'
    assert src.count(marker)==1
    src=src.replace(marker,marker+'\n    printf("line_end_dump: compiled_only=1\\n");\n    return 0;')
    (work/'dump.c').write_text(src)
    subprocess.run(['cc',str(work/'dump.c'),'-o',str(work/'dump'),*baker.vulkan_compile_flags()],check=True)
    mesa=args.mesa_build.resolve();driver=mesa/'src/intel/vulkan/libvulkan_intel.so';shim=mesa/'src/intel/tools/libintel_noop_drm_shim.so'
    if not driver.exists() or not shim.exists():raise SystemExit('Compatible instrumented Mesa toolchain missing; use --mesa-build')
    icd=work/'icd.json';icd.write_text(json.dumps({'file_format_version':'1.0.1','ICD':{'api_version':'1.4.346','library_path':str(driver)}}))
    executable=work/'intel';executable.mkdir(exist_ok=True)
    # SIMD variants may change between compiles; stale assembly must not be
    # counted as another stage in the new pipeline cache.
    for path in executable.iterdir():
        if path.is_file():
            path.unlink()
    env=os.environ.copy();env.update({'LD_PRELOAD':str(shim),'VK_DRIVER_FILES':str(icd),'VK_ICD_FILENAMES':str(icd),'INTEL_STUB_GPU_DEVICE_ID':'4680','TRUEOS_VK_DEVICE_ID':'0x4680','MESA_SHADER_CACHE_DISABLE':'true','TRUEOS_EXECUTABLE_DUMP_DIR':str(executable)})
    baker.run([str(work/'dump'),*map(str,stages)],env=env,log=work/'compile.log')
    log=(work/'compile.log').read_text();device,_=baker.parse_compile_log(log)
    assert device['device_id']==0x4680 and 'line_end_dump: compiled_only=1' in log
    vs,ps8,ps=baker.extract_native(executable,work/'native')
    def state(stage):
        return {k:int(v,0) for k,v in re.findall(r'(\w+)=(0x[0-9a-fA-F]+|\d+)',(executable/f'{stage}_TRUEOS_{"VS" if stage=="vertex" else "PS"}_state_v1.txt').read_text())}
    v=state('vertex');p=state('fragment')
    if not (v['binding_table_entries']==0 and p['dispatch16']==1 and p['push_bytes']==64 and p['scratch_bytes']==0 and p['num_varying_inputs']==0 and p['sampler_count']==0 and p['binding_table_entries']==1 and p['computed_depth_mode']==0 and p['uses_src_depth']==1 and p['uses_src_w']==1):raise SystemExit(f'Unadmitted shader ABI: {v}, {p}')
    for stage,data in [('vs',vs),('ps',ps)]:
        path=OUT/f'line_ends.{stage}.bin';path.write_bytes(data)
        result=subprocess.run(['iga64','-d','-p=12p1','-Xprint-pc',str(path)],text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,check=True)
        if 'illegal' in result.stdout.lower():raise SystemExit('Invalid native shader')
        (OUT/f'line_ends.{stage}.iga.txt').write_text('\n'.join(l.rstrip() for l in result.stdout.splitlines())+'\n')
    emitter.GENERATED=GENERATED;emitter.emit_rust(vs,ps,v,p)
    generated=GENERATED.read_text().replace('tools/wc3-fixed-bake/bake.py','tools/microfont-line-end-bake/bake.py').replace('WC3_FIXED','MICROFONT_LINE_END').replace('wc3_fixed_pipeline','microfont_line_end_pipeline')
    # The emitter's reusable VS/PS declarations default to zero pushed bytes.
    at=generated.index('    ps: TrianglePixelShader')
    generated=generated[:at]+generated[at:].replace('push_constant_bytes: 0,','push_constant_bytes: 64,',1)
    GENERATED.write_text(generated)
    for stage in ['vertex','fragment']:
        path=executable/f'{stage}_TRUEOS_{"VS" if stage=="vertex" else "PS"}_state_v1.txt';(OUT/path.name).write_bytes(path.read_bytes())
    manifest.write_text(json.dumps({'source_sha256':sha,'rust_sha256':hashlib.sha256(GENERATED.read_bytes()).hexdigest(),'device_id':'0x4680','vs_bytes':len(vs),'ps_bytes':len(ps),'vs_state':v,'ps_state':p},indent=2)+'\n')
    print(json.dumps({'vs_bytes':len(vs),'ps_bytes':len(ps),'ps_state':p}))
if __name__=='__main__':main()
