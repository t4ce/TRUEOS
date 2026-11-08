#!/usr/bin/env python3
"""Check real native-selected TAR/ZIP payloads and honest metadata capabilities.

Archive metadata remains readable format data. Host filesystem bytes exercise
the actual selected libraries; native access-key inheritance remains enforced
by the kernel and is not simulated here.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
TESTS=r'''
#[cfg(test)]mod tests {
 use std::{fs,io::{self,Cursor,Read,Write},path::PathBuf};
 struct Dir(PathBuf);impl Dir{fn new()->Self{let p=std::env::temp_dir().join(format!("native-archives-{}",std::process::id()));let _=fs::remove_dir_all(&p);fs::create_dir(&p).unwrap();Self(p)}}impl Drop for Dir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn tar_bytes(kind:tar::EntryType)->Vec<u8>{let mut b=tar::Builder::new(Vec::new());let mut h=tar::Header::new_gnu();h.set_uid(123);h.set_gid(456);h.set_mode(0o400);h.set_mtime(12345);h.set_entry_type(kind);let data=if kind.is_file(){&b"archive bytes"[..]}else{&b""[..]};h.set_size(data.len()as u64);if kind.is_symlink()||kind.is_hard_link(){h.set_link_name("target").unwrap();}h.set_cksum();b.append_data(&mut h,"payload",data).unwrap();b.into_inner().unwrap()}
 #[test]fn tar_default_file_builder_emits_portable_metadata_and_real_bytes(){let d=Dir::new();let source=d.0.join("source");fs::write(&source,b"native payload").unwrap();let mut b=tar::Builder::new(Vec::new());b.append_path_with_name(&source,"nested/file").unwrap();let bytes=b.into_inner().unwrap();let mut a=tar::Archive::new(Cursor::new(&bytes));let mut e=a.entries().unwrap().next().unwrap().unwrap();assert_eq!(e.header().uid().unwrap(),0);assert_eq!(e.header().gid().unwrap(),0);assert_eq!(e.header().mode().unwrap(),0o644);assert_eq!(e.header().mtime().unwrap(),tar::DETERMINISTIC_TIMESTAMP);let mut s=String::new();e.read_to_string(&mut s).unwrap();assert_eq!(s,"native payload");let dest=d.0.join("out");tar::Archive::new(Cursor::new(bytes)).unpack(&dest).unwrap();assert_eq!(fs::read(dest.join("nested/file")).unwrap(),b"native payload");}
 #[test]fn tar_foreign_metadata_is_preserved_in_header_without_applying_unix_policy(){let d=Dir::new();let bytes=tar_bytes(tar::EntryType::Regular);let mut a=tar::Archive::new(Cursor::new(&bytes));let e=a.entries().unwrap().next().unwrap().unwrap();assert_eq!(e.header().uid().unwrap(),123);assert_eq!(e.header().gid().unwrap(),456);assert_eq!(e.header().mode().unwrap(),0o400);assert_eq!(e.header().mtime().unwrap(),12345);tar::Archive::new(Cursor::new(bytes)).unpack(&d.0).unwrap();assert_eq!(fs::read(d.0.join("payload")).unwrap(),b"archive bytes");assert!(!fs::metadata(d.0.join("payload")).unwrap().permissions().readonly());}
 #[test]fn tar_complete_metadata_builder_fails_before_writing_payload(){let d=Dir::new();let p=d.0.join("source");fs::write(&p,b"data").unwrap();let mut f=fs::File::open(&p).unwrap();let mut b=tar::Builder::new(Vec::new());b.mode(tar::HeaderMode::Complete);assert_eq!(b.append_file("file",&mut f).unwrap_err().kind(),io::ErrorKind::Unsupported);assert!(b.get_ref().is_empty());b.mode(tar::HeaderMode::Deterministic);b.append_file("file",&mut f).unwrap();}
 #[test]fn tar_infallible_complete_header_api_fails_explicitly(){let d=Dir::new();let p=d.0.join("source");fs::write(&p,b"data").unwrap();let result=std::panic::catch_unwind(||{let mut h=tar::Header::new_gnu();h.set_metadata(&fs::metadata(&p).unwrap());});assert!(result.is_err());}
 #[test]fn tar_explicit_unix_metadata_preservation_rejects_before_truncation(){for setting in 0..5{let d=Dir::new();fs::write(d.0.join("payload"),b"original").unwrap();let mut a=tar::Archive::new(Cursor::new(tar_bytes(tar::EntryType::Regular)));match setting{0=>a.set_preserve_permissions(true),1=>a.set_preserve_ownerships(true),2=>a.set_preserve_mtime(true),3=>a.set_unpack_xattrs(true),_=>a.set_mask(0o077)}assert_eq!(a.unpack(&d.0).unwrap_err().kind(),io::ErrorKind::Unsupported);assert_eq!(fs::read(d.0.join("payload")).unwrap(),b"original");}}
 #[test]fn tar_links_remain_unsupported_without_creating_a_destination(){for kind in[tar::EntryType::Symlink,tar::EntryType::Link]{let d=Dir::new();fs::write(d.0.join("target"),b"target").unwrap();assert_eq!(tar::Archive::new(Cursor::new(tar_bytes(kind))).unpack(&d.0).unwrap_err().kind(),io::ErrorKind::Unsupported);assert!(!d.0.join("payload").exists());assert_eq!(fs::read(d.0.join("target")).unwrap(),b"target");}}
 #[test]fn tar_unix_special_objects_are_not_reinterpreted_as_regular_files(){for kind in[tar::EntryType::Char,tar::EntryType::Block,tar::EntryType::Fifo]{let d=Dir::new();assert_eq!(tar::Archive::new(Cursor::new(tar_bytes(kind))).unpack(&d.0).unwrap_err().kind(),io::ErrorKind::Unsupported);assert!(!d.0.join("payload").exists());}}
 #[test]fn filetime_reads_actual_available_times_and_refuses_all_mutations(){let d=Dir::new();let p=d.0.join("file");fs::write(&p,b"time").unwrap();let f=fs::File::open(&p).unwrap();let md=f.metadata().unwrap();let t=filetime::FileTime::from_unix_time(1234,5678);assert_eq!(filetime::FileTime::from_last_modification_time(&md),filetime::FileTime::from_system_time(md.modified().unwrap()));assert_eq!(filetime::FileTime::from_last_access_time(&md),filetime::FileTime::from_system_time(md.accessed().unwrap()));assert_eq!(filetime::FileTime::from_creation_time(&md),md.created().ok().map(filetime::FileTime::from_system_time));for r in[filetime::set_file_times(&p,t,t),filetime::set_symlink_file_times(&p,t,t),filetime::set_file_mtime(&p,t),filetime::set_file_atime(&p,t),filetime::set_file_handle_times(&f,Some(t),None)]{assert_eq!(r.unwrap_err().kind(),io::ErrorKind::Unsupported);}filetime::set_file_handle_times(&f,None,None).unwrap();assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(),md.modified().unwrap());}
 fn zip_bytes(link:bool,name:&str)->Vec<u8>{let mut z=zip::ZipWriter::new(Cursor::new(Vec::new()));let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).unix_permissions(0o400);if link{z.add_symlink(name,"target",options).unwrap();}else{z.start_file(name,options).unwrap();z.write_all(b"zip payload").unwrap();}z.finish().unwrap().into_inner()}
 #[test]fn zip_preserves_foreign_header_and_extracts_native_payload(){let d=Dir::new();let mut z=zip::ZipArchive::new(Cursor::new(zip_bytes(false,"nested/file"))).unwrap();assert_eq!(z.by_index(0).unwrap().unix_mode().unwrap()&0o777,0o400);z.extract(&d.0).unwrap();assert_eq!(fs::read(d.0.join("nested/file")).unwrap(),b"zip payload");assert!(!fs::metadata(d.0.join("nested/file")).unwrap().permissions().readonly());}
 #[test]fn zip_links_and_traversal_cannot_create_external_objects(){let d=Dir::new();let result=zip::ZipArchive::new(Cursor::new(zip_bytes(true,"link"))).unwrap().extract(&d.0);assert!(matches!(result,Err(zip::result::ZipError::Io(e))if e.kind()==io::ErrorKind::Unsupported));assert!(!d.0.join("link").exists());let out=d.0.join("extract");let _=zip::ZipArchive::new(Cursor::new(zip_bytes(false,"../escape"))).unwrap().extract(&out);assert!(!d.0.join("escape").exists());}
}
'''
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-archives-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=240)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-14000:])
   return log.read_text()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/codex_native_archive_overlay.rs'))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))','overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]),&args[3],&args[4])');(work/'stage.rs').write_text(driver);run('compile',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')]);staged={}
  for name,version in [('tar','0.4.45'),('filetime','0.2.27'),('zip','9.0.0')]:
   source=list((Path.home()/'.cargo/registry/src').glob(f'*/{name}-{version}'));assert len(source)==1;staged[name]=Path(run('stage-'+name,[str(work/'stage'),str(work/'staged'),str(source[0]),name,version]).strip())
   cargo_manifest=staged[name]/'Cargo.toml';lines=[];skip=False
   for line in cargo_manifest.read_text().splitlines():
    if line.startswith('['):skip='dev-dependencies'in line
    if not skip:lines.append(line)
   cargo_manifest.write_text('\n'.join(lines)+'\n')
  manifest(work,f'''[package]
name="native-blueprint-archives-proof"
version="0.0.0"
edition="2024"
[workspace]
exclude=["staged/source-overlay-crates/tar-0.4.45-trueos","staged/source-overlay-crates/filetime-0.2.27-trueos","staged/source-overlay-crates/zip-9.0.0-trueos"]
[dependencies]
tar={{path={json.dumps(str(staged['tar']))}}}
filetime={{path={json.dumps(str(staged['filetime']))}}}
zip={{path={json.dumps(str(staged['zip']))},default-features=false}}
[patch.crates-io]
filetime={{path={json.dumps(str(staged['filetime']))}}}
[lib]
path="lib.rs"
''');(work/'lib.rs').write_text(TESTS)
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]in["tar","zip","filetime"]:args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper);result=run(mode,['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'],env);assert'10 passed'in result
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SDK/'src'/p)for p in ['codex_native_archive_overlay.rs','filetime_trueos.rs']]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
