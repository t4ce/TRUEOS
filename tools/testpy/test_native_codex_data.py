#!/usr/bin/env python3
"""Probe pinned native private Git file snapshots and rand's non-fork path.

Actual production staging code and complete upstream libraries are exercised.
File bytes and Git pack/loose/ref parsing are real host filesystem operations;
this is not a TRUEOS rig/scheduler or filesystem permission check.
"""
from pathlib import Path
import argparse
import json
import re
import subprocess
import tempfile
from test_native_mio import ROOT, TOOLCHAIN, environment, info
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
PROBE=r'''
use std::{fs::{self,File,OpenOptions},io::{Seek,SeekFrom},path::PathBuf,sync::Arc};
use memmap2::{MmapOptions,MmapMut};
fn main(){
 let args:Vec<String>=std::env::args().collect();let root=PathBuf::from(&args[1]);
 let path=root.join("snapshot");fs::write(&path,b"0123456789").unwrap();let mut file=File::open(&path).unwrap();file.seek(SeekFrom::Start(3)).unwrap();
 let map=unsafe{MmapOptions::new().offset(2).len(5).map_copy_read_only(&file)}.unwrap();assert_eq!(&map[..],b"23456");assert_eq!(file.stream_position().unwrap(),3);println!("ok private slice and preserved file cursor");
 let map=Arc::new(map);let clone=map.clone();drop(map);assert_eq!(&clone[..],b"23456");println!("ok readonly snapshot stable across shared ownership");
 fs::write(&path,b"xxxxxxxxxx").unwrap();assert_eq!(&clone[..],b"23456");drop(file);fs::remove_file(&path).unwrap();assert_eq!(&clone[..],b"23456");println!("ok private bytes survive file replacement and close");
 fs::write(&path,b"mutable").unwrap();let file=OpenOptions::new().read(true).write(true).open(&path).unwrap();let mut map=unsafe{MmapOptions::new().map_copy(&file)}.unwrap();map[0]=b'M';map.flush().unwrap();assert_eq!(&map[..],b"Mutable");assert_eq!(fs::read(&path).unwrap(),b"mutable");println!("ok writable private bytes never claim file writeback");
 assert_eq!(unsafe{MmapOptions::new().map(&file)}.unwrap_err().kind(),std::io::ErrorKind::Unsupported);assert_eq!(unsafe{MmapOptions::new().map_mut(&file)}.unwrap_err().kind(),std::io::ErrorKind::Unsupported);assert_eq!(unsafe{MmapOptions::new().map_exec(&file)}.unwrap_err().kind(),std::io::ErrorKind::Unsupported);println!("ok shared and executable mappings rejected");
 assert_eq!(unsafe{MmapOptions::new().populate().map_copy_read_only(&file)}.unwrap_err().kind(),std::io::ErrorKind::Unsupported);println!("ok unavailable mapping option rejected");
 assert_eq!(unsafe{MmapOptions::new().offset(100).map_copy_read_only(&file)}.unwrap_err().kind(),std::io::ErrorKind::InvalidData);assert_eq!(unsafe{MmapOptions::new().len(100).map_copy_read_only(&file)}.unwrap_err().kind(),std::io::ErrorKind::UnexpectedEof);println!("ok invalid ranges and short reads propagate");
 fs::write(&path,b"").unwrap();let file=File::open(&path).unwrap();assert!(unsafe{MmapOptions::new().map_copy_read_only(&file)}.unwrap().is_empty());println!("ok empty file snapshot");
 let mut anon=MmapMut::map_anon(8).unwrap();assert_eq!(&anon[..],&[0u8;8]);anon[0]=2;let ro=anon.make_read_only().unwrap();assert_eq!(ro[0],2);assert_eq!(ro.lock().unwrap_err().kind(),std::io::ErrorKind::Unsupported);println!("ok owned anonymous bytes and honest lock rejection");
 let idx=root.join("repo/.git/objects/pack");let idx=fs::read_dir(idx).unwrap().map(|x|x.unwrap().path()).find(|p|p.extension().is_some_and(|s|s=="idx")).unwrap();
 let bundle=gix_pack::Bundle::at(&idx,gix_hash::Kind::Sha1).unwrap();assert!(bundle.index.num_objects()>0);let oid=gix_hash::ObjectId::from_hex(args[2].as_bytes()).unwrap();let mut bytes=Vec::new();let mut inflate=gix_zlib::Inflate::default();let(data,_)=bundle.find(&oid,&mut bytes,&mut inflate,&mut gix_pack::cache::Never).unwrap().unwrap();assert_eq!(data.data,b"packed-native-data\n");println!("ok actual Git pack index lookup and object inflation");
 let odb=gix_odb::loose::Store::at(root.join("repo/.git/objects"),gix_hash::Kind::Sha1);let oid=gix_hash::ObjectId::from_hex(args[3].as_bytes()).unwrap();let data=odb.try_find(&oid,&mut bytes).unwrap().unwrap();assert_eq!(data.data,b"loose-native-data\n");println!("ok actual Git loose-object native snapshot and inflation");
 let refs=gix_ref::packed::Buffer::open(root.join("repo/.git/packed-refs"),0,gix_hash::Kind::Sha1).unwrap();let found=refs.try_find("refs/tags/native-data").unwrap().unwrap();assert_eq!(found.target(),gix_hash::ObjectId::from_hex(args[2].as_bytes()).unwrap());println!("ok actual Git packed-ref mapped branch lookup");
 use rand08::RngCore;let mut rng=rand08::thread_rng();let mut values=[0;64];rng.fill_bytes(&mut values);assert!(values.iter().any(|x|*x!=0));println!("ok rand thread RNG without fork bookkeeping");
}
'''

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[];cargo=['rustup','run',TOOLCHAIN,'cargo']
 with tempfile.TemporaryDirectory(prefix='trueos-native-codex-data-') as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None,input=None):
   log=out/(label+'.log')
   with log.open('w') as stream:r=subprocess.run(cmd,cwd=work,env=env,input=input,text=True,stdout=stream,stderr=subprocess.STDOUT,timeout=240)
   checks.append({'label':label,'exit':r.returncode,'command':cmd,'log':str(log)})
   if r.returncode:raise RuntimeError(log.read_text()[-14000:])
   return log.read_text().strip()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/codex_native_data_overlay.rs'))).replace('overlay::stage(', '(if args[3] == "rand" { overlay::stage_rand } else { overlay::stage_memmap })(')
  (work/'stage.rs').write_text(driver);run('compile-production-data-stage',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')])
  sources={}
  for name,version,kind in [('rand','0.8.6','rand'),('memmap2','0.9.11','memmap')]:
   candidates=list((Path.home()/'.cargo/registry/src').glob(f'*/{name}-{version}'));assert len(candidates)==1
   source=candidates[0];before={str(p.relative_to(source)):info(p)['sha256'] for p in sorted((source/'src').rglob('*.rs'))}
   staged=Path(run('stage-'+name,[str(work/'stage'),str(work/'staged'),str(source),kind]));sources[name]=staged
   assert before=={str(p.relative_to(source)):info(p)['sha256'] for p in sorted((source/'src').rglob('*.rs'))}
  (work/'Cargo.toml').write_text(f'''[package]
name="native-codex-data-check"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
memmap2={{path={json.dumps(str(sources['memmap2']))}}}
rand08={{package="rand",path={json.dumps(str(sources['rand']))}}}
gix-pack={{version="=0.76.0",features=["sha1"]}}
gix-odb={{version="=0.86.0",features=["sha1"]}}
gix-ref={{version="=0.69.0",features=["sha1"]}}
gix-hash={{version="=0.25.0",features=["sha1"]}}
gix-zlib="0.1"
[patch.crates-io]
memmap2={{path={json.dumps(str(sources['memmap2']))}}}
[[bin]]
name="native-codex-data"
path="probe.rs"
''')
  # Match the exact dependency versions selected by these pinned gix packages.
  import tomllib
  gm=tomllib.loads((Path.home()/'.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gix-pack-0.76.0/Cargo.toml').read_text())
  manifest=(work/'Cargo.toml').read_text().replace('=0.25.0',gm['dependencies']['gix-hash']['version']).replace('gix-zlib="0.1"','gix-zlib='+json.dumps(gm['dependencies']['gix-zlib']['version']))
  (work/'Cargo.toml').write_text(manifest);(work/'probe.rs').write_text(PROBE)
  repo=work/'repo';run('git-init',['git','init','-q','-b','main',str(repo)])
  packed=run('git-packed-blob',['git','-C',str(repo),'hash-object','-w','--stdin'],input='packed-native-data\n')
  run('git-update-ref',['git','-C',str(repo),'update-ref','refs/tags/native-data',packed]);run('git-repack',['git','-C',str(repo),'repack','-ad']);run('git-pack-refs',['git','-C',str(repo),'pack-refs','--all'])
  loose=run('git-loose-blob',['git','-C',str(repo),'hash-object','-w','--stdin'],input='loose-native-data\n')
  wrapper=work/'native-provider-rustc.py'
  wrapper.write_text('#!/usr/bin/env python3\nimport os,sys\nargs=sys.argv[1:]\nif "--crate-name" in args and args[args.index("--crate-name")+1] in {"memmap2","rand"}: args += ["--cfg", \'target_os="trueos"\', "-Aexplicit_builtin_cfgs_in_flags"]\nos.execvp(args[0],args)\n')
  wrapper.chmod(0o755)
  env=environment(out/'target')
  env['RUSTC_WRAPPER']=str(wrapper)
  counts={}
  for mode,flags in [('debug',[]),('release',['--release'])]:
   text=run('native-private-data-'+mode,cargo+['run','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--bin','native-codex-data','--',str(work),packed,loose],env);counts[mode]=len(re.findall('^ok ',text,re.M));assert counts[mode]==13
  run('native-no-atfork',['sh','-c','nm -u "$1" | python3 -c \'import sys; text=sys.stdin.read(); assert "pthread_atfork" not in text\'','sh',str(out/'target/x86_64-unknown-linux-gnu/release/native-codex-data')])
 sources=[info(SDK/name) for name in ['src/codex_native_data_overlay.rs','src/memmap2_trueos.rs']]
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'cases':counts,'sources':sources},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
