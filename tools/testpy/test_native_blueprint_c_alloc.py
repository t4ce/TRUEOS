#!/usr/bin/env python3
"""Exercise app-local Rust C allocation and pinned native C source providers.

Host checks run actual codecs/regex/Bash parser. Their build scripts receive an
explicit native-OS fixture environment, while Rust uses the ordinary host std.
A separate no-Unix core+alloc target links the SDK allocator and these same C
archives, proving native heap ownership rather than a renamed retired provider.
"""
from pathlib import Path
import argparse
import json
import os
import re
import subprocess
import tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
TARGET=ROOT.parent/'TRUEOS-Buildins/tools/trueos-object-native.json'
BEHAVIOR_GROUPS=5
PROVIDERS=[('zstd-sys','2.0.16+zstd.1.5.7','build.rs'),('onig_sys','69.9.1','build.rs'),('tree-sitter','0.25.10','binding_rust/build.rs'),('tree-sitter-bash','0.25.1','bindings/rust/build.rs'),('bzip2-sys','0.1.13+1.0.8','build.rs')]
BEHAVIOR=r'''
extern crate alloc;
#[path="__ALLOC__"] mod native_alloc;
#[path="__CLOCK__"] mod native_clock;
mod vclock {
 pub fn monotonic_nanos()->u64 {
  static START:std::sync::OnceLock<std::time::Instant>=std::sync::OnceLock::new();
  START.get_or_init(std::time::Instant::now).elapsed().as_nanos().try_into().unwrap()
 }
}
use std::io::{Read,Write};
#[unsafe(no_mangle)]unsafe extern "C" fn trueos_cabi_write(stream:u32,bytes:*const u8,len:usize){
 let bytes=unsafe{std::slice::from_raw_parts(bytes,len)};
 match stream {1=>std::io::stdout().write_all(bytes).unwrap(),2=>std::io::stderr().write_all(bytes).unwrap(),_=>panic!("unexpected native console stream")}
}
fn main(){
 let bytes=b"Native Blueprint allocation: compress, parse and match real data.\n".repeat(1000);
 let packed=zstd::stream::encode_all(&bytes[..],3).unwrap();assert_eq!(zstd::stream::decode_all(&packed[..]).unwrap(),bytes);println!("ok Zstd compression and decompression with native allocator");
 let mut encoder=bzip2::write::BzEncoder::new(Vec::new(),bzip2::Compression::best());encoder.write_all(&bytes).unwrap();let packed=encoder.finish().unwrap();let mut decoded=Vec::new();bzip2::read::BzDecoder::new(&packed[..]).read_to_end(&mut decoded).unwrap();assert_eq!(decoded,bytes);println!("ok bzip2 compression and decompression with native allocator");
 let regex=onig::Regex::new(r"(?<item>native)-(?<number>[0-9]+)").unwrap();let caps=regex.captures("prefix native-123 suffix").unwrap();assert_eq!(caps.at(1),Some("native"));assert_eq!(caps.at(2),Some("123"));assert!(!regex.is_match("native-zero"));println!("ok Onig captures and rejection with native allocator");
 let mut parser=tree_sitter::Parser::new();parser.set_language(&tree_sitter_bash::LANGUAGE.into()).unwrap();
 let program=b"cat <<'EOF'\nnative heredoc\nEOF\nfor x in a b; do echo \"$x\"; done\n";let tree=parser.parse(program,None).unwrap();assert!(!tree.root_node().has_error());let again=parser.parse(program,Some(&tree)).unwrap();assert!(!again.root_node().has_error());println!("ok actual Bash external scanner and incremental parsing");
 #[allow(deprecated)]parser.set_timeout_micros(1);
 let long="echo native_parser_timeout\n".repeat(20000);assert!(parser.parse(&long,None).is_none());
 parser.reset();#[allow(deprecated)]parser.set_timeout_micros(0);
 assert!(!parser.parse(program,None).unwrap().root_node().has_error());println!("ok native monotonic parser deadline cancels and reset parses again");
}
'''
NATIVE_ENTRY=r'''
#![no_std]
#![no_main]
use core::ffi::{c_int,c_uint,c_void,c_char};
unsafe extern "C" {
 fn ZSTD_compress(dst:*mut c_void,cap:usize,src:*const c_void,len:usize,level:c_int)->usize;
 fn onig_region_new()->*mut c_void;fn onig_region_free(p:*mut c_void,free_self:c_int);
 fn ts_parser_new()->*mut c_void;fn ts_parser_delete(p:*mut c_void);fn ts_parser_set_language(p:*mut c_void,l:*const c_void)->bool;
 fn tree_sitter_bash()->*const c_void;fn ts_parser_parse_string(p:*mut c_void,old:*const c_void,s:*const c_char,len:c_uint)->*mut c_void;fn ts_tree_delete(p:*mut c_void);
 fn BZ2_bzBuffToBuffCompress(dst:*mut c_char,len:*mut c_uint,src:*mut c_char,src_len:c_uint,block:c_int,verbosity:c_int,factor:c_int)->c_int;
}
#[unsafe(no_mangle)]pub extern "C" fn _start()->! {
 unsafe {
  core::hint::black_box(trueos::native_c_clock::trueos_blueprint_c_monotonic_nanos());
  let p=trueos::native_c_alloc::trueos_blueprint_c_malloc(32);trueos::native_c_alloc::trueos_blueprint_c_free(p);
  let p=trueos::native_c_alloc::trueos_blueprint_c_calloc(7,9);let p=trueos::native_c_alloc::trueos_blueprint_c_realloc(p,128);trueos::native_c_alloc::trueos_blueprint_c_free(p);
  let mut out=[0u8;128];let mut input=*b"echo native\n";core::hint::black_box(ZSTD_compress(out.as_mut_ptr().cast(),out.len(),input.as_ptr().cast(),input.len(),1));
  let r=onig_region_new();onig_region_free(r,1);
  let p=ts_parser_new();core::hint::black_box(ts_parser_set_language(p,tree_sitter_bash()));let t=ts_parser_parse_string(p,core::ptr::null(),input.as_ptr().cast(),input.len()as c_uint);ts_tree_delete(t);ts_parser_delete(p);
  let mut len=out.len()as c_uint;core::hint::black_box(BZ2_bzBuffToBuffCompress(out.as_mut_ptr().cast(),&mut len,input.as_mut_ptr().cast(),input.len()as c_uint,1,0,30));
 }
 loop{core::hint::spin_loop()}
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);args=p.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[];cargo=['rustup','run',TOOLCHAIN,'cargo'];archives=[]
 with tempfile.TemporaryDirectory(prefix='trueos-blueprint-c-alloc-') as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w') as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=300)
   checks.append({'label':label,'exit':r.returncode,'command':cmd,'log':str(log)})
   if r.returncode:raise RuntimeError(log.read_text()[-16000:])
   return log.read_text()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/codex_native_c_overlay.rs'))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))','overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]),&args[3],&args[4])')
  (work/'stage.rs').write_text(driver);run('compile-production-c-stage',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
  staged={}
  for name,version,relative in PROVIDERS:
   original=list((Path.home()/'.cargo/registry/src').glob(f'*/{name}-{version}'));assert len(original)==1;source=original[0];before=info(source/relative)['sha256']
   path=Path(run('stage-'+name,[str(work/'stage'),str(work/'staged'),str(source),name,version]).strip());staged[name]=path;assert info(source/relative)['sha256']==before
   # Host-only build-script wrapper selects the same explicit TRUEOS branch.
   # The staged production script is otherwise byte-for-byte unchanged.
   build=path/relative;original_script=build.with_name('__native_host_original.rs');original_script.write_bytes(build.read_bytes())
   build.write_text('mod original { include!("__native_host_original.rs"); pub fn invoke(){ main(); } }\nfn main(){ unsafe {std::env::set_var("CARGO_CFG_TARGET_OS","trueos");} original::invoke();}\n')
  patches='\n'.join(f'{name}={{path={json.dumps(str(path))}}}' for name,path in staged.items())
  manifest(work,f'''[package]
name="native-blueprint-c-alloc-behavior"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
zstd="=0.13.3"
zstd-sys={{path={json.dumps(str(staged['zstd-sys']))},default-features=false}}
onig={{version="=6.5.1",default-features=false}}
bzip2={{version="=0.6.1",default-features=false,features=["bzip2-sys","static"]}}
tree-sitter="=0.25.10"
tree-sitter-bash="=0.25.1"
[patch.crates-io]
{patches}
[lib]
path="lib.rs"
[[bin]]
name="native-c-alloc-behavior"
path="behavior.rs"
''')
  allocator=str(SDK/'crates/trueos-v/src/native_c_alloc.rs');clock=str(SDK/'crates/trueos-v/src/native_c_clock.rs');(work/'lib.rs').write_text('extern crate alloc;\n#[path='+json.dumps(allocator)+']pub mod native_alloc;\n');(work/'behavior.rs').write_text(BEHAVIOR.replace('__ALLOC__',allocator).replace('__CLOCK__',clock))
  clock_probe=work/'clock-contract.c';clock_probe.write_text('''#include <stdint.h>
#include <stdbool.h>
#include <assert.h>
typedef uint64_t TSDuration;
#include "'''+str(SDK/'src/tree_sitter_native_clock.h')+'''"
uint64_t trueos_blueprint_c_monotonic_nanos(void) { return 1999999015; }
int main(void) {
 TSClock now=clock_now();assert(now.seconds==1 && now.nanos==999999015);
 TSClock deadline=clock_after(now,duration_from_micros(2));assert(deadline.seconds==2 && deadline.nanos==1015);
 assert(clock_is_gt(deadline,now));assert(!clock_is_gt(now,deadline));assert(!clock_is_gt(now,now));
 assert(clock_is_null(clock_null()));assert(!clock_is_null(now));
 assert(duration_to_micros(duration_from_micros(UINT64_MAX))==UINT64_MAX);
 deadline=clock_after(now,UINT64_MAX);assert(deadline.seconds>now.seconds && deadline.nanos<1000000000);
}
''');run('native-clock-contract-build',['cc','-std=c11','-Wall','-Wextra','-Werror',str(clock_probe),'-o',str(work/'clock-contract')]);run('native-clock-contract',[str(work/'clock-contract')])
  counts={}
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'host-target');text=run('real-c-providers-'+mode,cargo+['run','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--bin','native-c-alloc-behavior'],env);counts[mode]=len(re.findall('^ok ',text,re.M));assert counts[mode]==BEHAVIOR_GROUPS
   run('allocation-contract-'+mode,cargo+['test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib'],env)
  base=out/'host-target/x86_64-unknown-linux-gnu/release/build'
  for pattern in ['zstd-sys-*/out/libzstd.a','onig_sys-*/out/libonig.a','tree-sitter-*/out/libtree-sitter.a','tree-sitter-bash-*/out/libtree-sitter-bash.a','bzip2-sys-*/out/lib/libbz2.a']:
   found=list(base.glob(pattern));assert found,(pattern,found);archive=max(found,key=lambda p:p.stat().st_mtime_ns);text=run('imports-'+archive.stem,['nm','-u',str(archive)])
   imports={line.split()[-1]for line in text.splitlines()if re.search(r'\sU\s',line)};assert not imports.intersection({'malloc','calloc','realloc','free'}),(archive,imports);assert any(s.startswith('trueos_blueprint_c_')for s in imports);archives.append(archive)
  probe=work/'native';manifest(probe,f'''[package]
name="native-blueprint-c-alloc-probe"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
trueos={{path={json.dumps(str(SDK/'api'))},default-features=false,features=["native-c-alloc","native-c-clock","default-global-allocator","default-panic-handler"]}}
[[bin]]
name="native-c-alloc-probe"
path="main.rs"
[profile.release]
panic="abort"
''');(probe/'main.rs').write_text(NATIVE_ENTRY)
  native_env=environment(out/'native-target')
  run('native-core-alloc-object',cargo+['build','--offline','--release','--manifest-path',str(probe/'Cargo.toml'),'--bin','native-c-alloc-probe','--target',str(TARGET),'-Zbuild-std=core,compiler_builtins,alloc','-Zbuild-std-features=compiler-builtins-mem','-Zjson-target-spec'],native_env)
  native=out/'native-target'/TARGET.stem/'release/native-c-alloc-probe.o';linked=out/'native-c-codecs.o'
  sysroot=Path(subprocess.check_output(['rustup','run',TOOLCHAIN,'rustc','--print','sysroot'],text=True).strip());linker=sysroot/'lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld'
  run('native-c-codecs-rel-link',[str(linker),'-flavor','gnu','-r','--gc-sections','--undefined=_start','-o',str(linked),str(native),'--start-group',*[str(p)for p in archives],'--end-group'])
  text=run('native-c-codecs-imports',['nm','-u',str(linked)]);symbols=sorted({line.split()[-1]for line in text.splitlines()if line.strip()})
  assert not set(symbols).intersection({'malloc','calloc','realloc','free','trueos_cabi_alloc','trueos_cabi_realloc','trueos_cabi_free','clock_gettime','clock'}),symbols
  assert not any(s.startswith('trueos_blueprint_c_')for s in symbols),symbols
  assert 'sys_alloc_aligned'in symbols and'sys_dealloc'in symbols and'trueos_time_monotonic_nanos'in symbols,symbols
  assert not list((out/'native-target'/TARGET.stem).rglob('libstd-*.rlib'))
 sources=[info(SDK/p)for p in ['src/codex_native_c_overlay.rs','src/tree_sitter_native_clock.h','crates/trueos-v/src/native_c_alloc.rs','crates/trueos-v/src/native_c_clock.rs']]
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'cases':counts,'sources':sources,'object':info(linked),'undefined':symbols,'c_archives':[info(p)for p in archives]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
