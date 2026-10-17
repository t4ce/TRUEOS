#!/usr/bin/env python3
"""Exercise the production checked writer across deterministic lease boundaries.

The production precondition, hash, metadata-selection and publication ordering
run unchanged. Storage is a fault-injecting boundary; SHA-256 is the real crate.
"""
from pathlib import Path
import subprocess
import tempfile

from test_indexed_fs_remove import extract

ROOT = Path(__file__).resolve().parents[2]


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::{collections::BTreeMap, sync::{Mutex, MutexGuard}, future::Future, task::{Context, Poll, Waker}, pin::pin};
use infer::ContentTypeId;
struct Lock<T>(Mutex<T>);
impl<T> Lock<T> { fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()} }
mod block {
 pub type DiscId=u8;
 #[derive(Clone,Copy)] pub struct DeviceHandle(pub bool);
 impl DeviceHandle { pub fn id(self)->u8{1} pub fn parent(self)->Option<()>{self.0.then_some(())} }
 #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Error{InvalidParam,NotReady,Corrupted,Io}
}
#[derive(Clone,Copy,PartialEq,Debug)] enum NodeKind{File,Directory}
#[derive(Clone,Copy,PartialEq,Debug)] enum RecordKey{Ffa,Key(u8)}
#[derive(Clone,Copy,PartialEq,Debug)] struct IndexRef{entry_lba:u64,kind:trueos_fs::LogKind,content_type:ContentTypeId}
struct Mount{disk_id:u8,index:Option<BTreeMap<Vec<u8>,IndexRef>>}
static ROOTS:Lock<Vec<Mount>>=Lock(Mutex::new(Vec::new()));
#[derive(Default)] struct State{
 held:bool,ensures:usize,leases:usize,mutation:u8,begins:usize,finishes:usize,
 chunks:Vec<usize>,reads:Vec<(u64,usize)>,records:BTreeMap<u64,(trueos_fs::NodeRecordRef,Vec<u8>)>,
 durable:BTreeMap<String,u64>,fail_begin:bool,fail_chunk:bool,fail_finish:bool,
 read_limit:usize,zero_read:bool,missing_record:bool,corrupt_type:bool,
 bumps:usize,watches:usize,invalidations:usize,commits:usize,
}
static STATE:Lock<State>=Lock(Mutex::new(State{held:false,ensures:0,leases:0,mutation:0,begins:0,finishes:0,chunks:Vec::new(),reads:Vec::new(),records:BTreeMap::new(),durable:BTreeMap::new(),fail_begin:false,fail_chunk:false,fail_finish:false,read_limit:0,zero_read:false,missing_record:false,corrupt_type:false,bumps:0,watches:0,invalidations:0,commits:0}));
mod disc{pub mod access{pub struct Activity; impl Activity{pub fn begin(_:crate::block::DeviceHandle)->Result<Self,crate::block::Error>{Ok(Self)}}}}
struct Placement{super_lba:u64,data_lba:u64,data_end_lba_exclusive:Option<u64>}
async fn placement_for_io_async(_:block::DeviceHandle)->Result<Option<Placement>,block::Error>{Ok(Some(Placement{super_lba:0,data_lba:1,data_end_lba_exclusive:Some(100000)}))}
struct KernelBlockIo; impl KernelBlockIo{fn new(_:block::DeviceHandle)->Self{Self}}
fn map_engine_err(e:block::Error)->block::Error{e}
async fn ensure_index_async(_:block::DeviceHandle,_:&Placement)->Result<(),block::Error>{
 let mut s=STATE.lock(); assert!(!s.held,"index rebuild while namespace lease held");s.ensures+=1;
 let records=s.durable.iter().map(|(p,l)|{let r=s.records[l].0;(p.as_bytes().to_vec(),IndexRef{entry_lba:*l,kind:if r.kind==NodeKind::File{trueos_fs::LogKind::Put}else{trueos_fs::LogKind::Directory},content_type:r.content_type})}).collect();
 drop(s);let mut roots=ROOTS.lock();if roots[0].index.is_none(){roots[0].index=Some(records);}Ok(())
}
mod write_gate{
 pub struct RootWriteLease;
 impl RootWriteLease{pub async fn acquire(_:u8)->Self{
  let mutation={let mut s=crate::STATE.lock();assert!(!s.held);s.leases+=1;s.held=true;std::mem::take(&mut s.mutation)};
  match mutation{
   1=>crate::install("file",77,b"racing writer",crate::ContentTypeId::HTML,crate::RecordKey::Key(9)),
   2=>crate::ROOTS.lock()[0].index=None,
   3=>crate::install("file",78,b"old",crate::ContentTypeId::HTML,crate::RecordKey::Key(9)),
   4=>{crate::ROOTS.lock()[0].index.as_mut().unwrap().remove(b"parent".as_slice());},
   _=>{}
  }
  Self
 }}
 impl Drop for RootWriteLease{fn drop(&mut self){crate::STATE.lock().held=false;}}
}
#[derive(Clone,Copy)]enum ContentIdentityRejectReason{LegacyDowngrade}
fn record_type_reject(_:ContentIdentityRejectReason){}
mod trueos_fs{
 use super::*;
 #[derive(Clone,Copy,Debug,PartialEq)]pub enum LogKind{Put,Directory,Other}
 pub struct FsParams{pub super_lba:u64,pub data_lba:u64,pub data_end_lba_exclusive:Option<u64>}
 #[derive(Clone,Copy,Debug,PartialEq)]pub struct NodeRecordRef{pub file_id:u64,pub entry_lba:u64,pub data_lba:u64,pub data_len:u64,pub kind:NodeKind,pub content_type:ContentTypeId,pub record_key:RecordKey}
 #[derive(Clone,Copy,Debug,PartialEq)]pub struct FileRecordRef{pub file_id:u64,pub entry_lba:u64,pub data_lba:u64,pub data_len:u64,pub content_type:ContentTypeId,pub record_key:RecordKey}
 #[derive(Clone,Copy)]pub struct FileWriteMetadata{pub file_id:u64,pub content_type:ContentTypeId,pub record_key:RecordKey}
 pub struct Stream{path:String,metadata:FileWriteMetadata,total:u64,bytes:Vec<u8>}
 pub async fn get_node_record_by_lba(_:&KernelBlockIo,_:&FsParams,lba:u64)->Result<Option<NodeRecordRef>,block::Error>{
  let s=STATE.lock();assert!(s.held);if s.missing_record{return Ok(None)}
  let mut record=s.records.get(&lba).map(|v|v.0);if s.corrupt_type{record.as_mut().unwrap().content_type=ContentTypeId::XML;}Ok(record)
 }
 pub async fn read_file_range_at(_:&KernelBlockIo,_:&FsParams,r:&FileRecordRef,offset:u64,buf:&mut[u8])->Result<Option<usize>,block::Error>{
  let mut s=STATE.lock();assert!(s.held);s.reads.push((offset,buf.len()));if s.zero_read{return Ok(Some(0))}
  let Some((_,bytes))=s.records.get(&r.entry_lba)else{return Ok(None)};
  let cap=if s.read_limit==0{buf.len()}else{s.read_limit.min(buf.len())};
  let len=cap.min(bytes.len().saturating_sub(offset as usize));buf[..len].copy_from_slice(&bytes[offset as usize..offset as usize+len]);Ok(Some(len))
 }
 pub async fn begin_write_file_stream_prevalidated(_:&KernelBlockIo,_:&FsParams,p:&str,total:u64,metadata:FileWriteMetadata)->Result<Option<Stream>,block::Error>{
  let mut s=STATE.lock();assert!(s.held);s.begins+=1;if s.fail_begin{return Ok(None)}
  Ok(Some(Stream{path:p.into(),metadata,total,bytes:Vec::new()}))
 }
 pub async fn write_file_stream_chunk(_:&KernelBlockIo,s:&mut Stream,b:&[u8])->Result<(),block::Error>{
  let mut state=STATE.lock();assert!(state.held);state.chunks.push(b.len());if state.fail_chunk{return Err(block::Error::Io)}s.bytes.extend_from_slice(b);Ok(())
 }
 pub fn write_stream_record_ref(s:&Stream)->FileRecordRef{FileRecordRef{file_id:if s.metadata.file_id==0{200}else{s.metadata.file_id},entry_lba:200,data_lba:201,data_len:s.total,content_type:s.metadata.content_type,record_key:s.metadata.record_key}}
 pub async fn finish_write_file_stream(_:&KernelBlockIo,_:&FsParams,s:Stream)->Result<(),block::Error>{
  let mut state=STATE.lock();assert!(state.held);state.finishes+=1;if state.fail_finish{return Err(block::Error::Io)}assert_eq!(s.bytes.len()as u64,s.total);
  let r=write_stream_record_ref(&s);state.records.insert(200,(NodeRecordRef{file_id:r.file_id,entry_lba:r.entry_lba,data_lba:r.data_lba,data_len:r.data_len,kind:NodeKind::File,content_type:r.content_type,record_key:r.record_key},s.bytes));state.durable.insert(s.path,200);Ok(())
 }
}
fn record_successful_content_commit(_:ContentTypeId,legacy:bool){let mut s=STATE.lock();assert!(s.held);assert!(!legacy);s.commits+=1;}
fn bump_root_cache_gen(_:u8){STATE.lock().bumps+=1;}
fn file_record_cache_invalidate_path(_:u8,_:&str){assert!(STATE.lock().held);}
fn file_record_cache_insert(_:u8,_:&str,_:trueos_fs::FileRecordRef){assert!(STATE.lock().held);}
fn update_root_index_put(_:u8,p:&str,r:trueos_fs::FileRecordRef)->bool{assert!(STATE.lock().held);ROOTS.lock()[0].index.as_mut().unwrap().insert(p.as_bytes().to_vec(),IndexRef{entry_lba:r.entry_lba,kind:trueos_fs::LogKind::Put,content_type:r.content_type});true}
fn invalidate_root_index(_:u8){STATE.lock().invalidations+=1;}
mod file_watch{pub fn changed(_:u8,_:&str){let mut s=crate::STATE.lock();assert!(s.held);s.watches+=1;}}
fn install(p:&str,lba:u64,bytes:&[u8],content_type:ContentTypeId,record_key:RecordKey){
 let r=trueos_fs::NodeRecordRef{file_id:41,entry_lba:lba,data_lba:lba+1,data_len:bytes.len()as u64,kind:NodeKind::File,content_type,record_key};
 {let mut s=STATE.lock();s.records.insert(lba,(r,bytes.to_vec()));s.durable.insert(p.into(),lba);}
 if let Some(index)=ROOTS.lock()[0].index.as_mut(){index.insert(p.as_bytes().to_vec(),IndexRef{entry_lba:lba,kind:trueos_fs::LogKind::Put,content_type});}
}
fn reset(){*STATE.lock()=State::default();*ROOTS.lock()=vec![Mount{disk_id:1,index:Some(BTreeMap::new())}];}
fn digest(b:&[u8])->[u8;32]{use sha2::Digest;sha2::Sha256::digest(b).into()}
fn run_path(path:&str,bytes:&[u8],create:bool,hash:Option<[u8;32]>)->Result<bool,CheckedWriteError>{
 let mut f=pin!(file_in_typed_checked_async(block::DeviceHandle(false),path,bytes,ContentTypeId::UTF8_TEXT,create,hash));
 match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(r)=>r,Poll::Pending=>panic!("unexpected pending mock")}
}
fn run(bytes:&[u8],create:bool,hash:Option<[u8;32]>)->Result<bool,CheckedWriteError>{run_path("file",bytes,create,hash)}
fn current()->(trueos_fs::NodeRecordRef,Vec<u8>){let s=STATE.lock();s.records[&s.durable["file"]].clone()}
fn assert_no_publication(){let s=STATE.lock();assert_eq!((s.begins,s.finishes,s.bumps,s.watches,s.commits),(0,0,0,0,0));assert!(!s.held);}
'''

TESTS = r'''
#[test]fn create_exact_bytes_and_explicit_empty_text(){for bytes in [b"hello codex".as_slice(),b"\r\n\t hello \xc3\xb6\n",b""]{reset();assert_eq!(run(bytes,true,None),Ok(true));let(r,saved)=current();assert_eq!(saved,bytes);assert_eq!(r.content_type,ContentTypeId::UTF8_TEXT);assert_eq!(r.record_key,RecordKey::Ffa);assert!(!STATE.lock().held);}}
#[test]fn create_only_checks_after_namespace_lease_wait(){for race in [false,true]{reset();if race{STATE.lock().mutation=1;}else{install("file",10,b"old",ContentTypeId::HTML,RecordKey::Key(3));}assert_eq!(run(b"new",true,None),Err(CheckedWriteError::AlreadyExists));assert_eq!(current().1,if race{b"racing writer".as_slice()}else{b"old"});assert_no_publication();}}
#[test]fn expected_hash_replace_preserves_record_metadata(){reset();install("file",10,b"old",ContentTypeId::HTML,RecordKey::Key(3));assert_eq!(run(b"new",false,Some(digest(b"old"))),Ok(true));let(r,bytes)=current();assert_eq!(bytes,b"new");assert_eq!((r.file_id,r.content_type,r.record_key),(41,ContentTypeId::HTML,RecordKey::Key(3)));}
#[test]fn unchanged_hash_uses_metadata_current_after_wait(){reset();install("file",10,b"old",ContentTypeId::UTF8_TEXT,RecordKey::Key(3));STATE.lock().mutation=3;assert_eq!(run(b"new",false,Some(digest(b"old"))),Ok(true));let(r,_)=current();assert_eq!((r.content_type,r.record_key),(ContentTypeId::HTML,RecordKey::Key(9)));}
#[test]fn changed_content_after_preflight_conflicts(){reset();install("file",10,b"old",ContentTypeId::UTF8_TEXT,RecordKey::Key(3));STATE.lock().mutation=1;assert_eq!(run(b"new",false,Some(digest(b"old"))),Err(CheckedWriteError::Conflict));assert_eq!(current().1,b"racing writer");assert_no_publication();}
#[test]fn missing_expected_target_and_bad_hash_have_no_publication(){for present in [false,true]{reset();if present{install("file",10,b"old",ContentTypeId::HTML,RecordKey::Key(3));}assert_eq!(run(b"new",false,Some(digest(b"other"))),Err(CheckedWriteError::Conflict));assert_no_publication();}}
#[test]fn empty_hash_and_partial_reads_are_complete_and_bounded(){for bytes in [vec![],vec![b'z';2*65536+17]]{reset();install("file",10,&bytes,ContentTypeId::UTF8_TEXT,RecordKey::Ffa);STATE.lock().read_limit=25001;assert_eq!(run(b"new",false,Some(digest(&bytes))),Ok(true));let s=STATE.lock();assert!(s.reads.iter().all(|(_,len)|*len<=65536));assert_eq!(s.reads.is_empty(),bytes.is_empty());}}
#[test]fn invalidated_index_rebuilds_before_reacquiring_lease(){reset();install("file",10,b"old",ContentTypeId::HTML,RecordKey::Key(3));STATE.lock().mutation=2;assert_eq!(run(b"new",false,Some(digest(b"old"))),Ok(true));let s=STATE.lock();assert_eq!((s.ensures,s.leases),(2,2));assert!(!s.held);}
#[test]fn failed_stream_never_publishes_or_mutates_metadata(){for failure in 0..3{reset();install("file",10,b"old",ContentTypeId::HTML,RecordKey::Key(3));let before=current();{let mut s=STATE.lock();match failure{0=>s.fail_begin=true,1=>s.fail_chunk=true,_=>s.fail_finish=true}}let result=run(b"new",false,Some(digest(b"old")));assert_eq!(result,if failure==0{Ok(false)}else{Err(CheckedWriteError::Device(block::Error::Io))});assert_eq!(current(),before);assert_eq!(ROOTS.lock()[0].index.as_ref().unwrap()[b"file".as_slice()].entry_lba,10);let s=STATE.lock();assert_eq!((s.bumps,s.watches,s.commits),(0,0,0));assert!(!s.held);}}
#[test]fn missing_corrupt_record_and_zero_read_fail_closed(){for failure in 0..3{reset();install("file",10,b"old",ContentTypeId::HTML,RecordKey::Key(3));{let mut s=STATE.lock();match failure{0=>s.missing_record=true,1=>s.corrupt_type=true,_=>s.zero_read=true}}assert_eq!(run(b"new",false,Some(digest(b"old"))),Err(CheckedWriteError::Device(block::Error::Corrupted)));assert_no_publication();}}
#[test]fn large_new_content_is_written_in_bounded_chunks(){reset();let bytes=vec![b'x';2*65536+1];assert_eq!(run(&bytes,true,None),Ok(true));assert_eq!(current().1,bytes);assert_eq!(STATE.lock().chunks,vec![65536,65536,1]);}
#[test]fn contradictory_conditions_fail_before_namespace_access(){reset();assert_eq!(run(b"new",true,Some(digest(b"old"))),Err(CheckedWriteError::Device(block::Error::InvalidParam)));assert_eq!(STATE.lock().leases,0);assert_no_publication();}
#[test]fn missing_parent_rejected_without_implicit_directory_creation(){reset();assert_eq!(run_path("parent/file",b"new",true,None),Err(CheckedWriteError::Device(block::Error::InvalidParam)));assert!(ROOTS.lock()[0].index.as_ref().unwrap().is_empty());assert!(STATE.lock().durable.is_empty());assert_no_publication();}
#[test]fn parent_removed_while_waiting_for_lease_rejected(){reset();ROOTS.lock()[0].index.as_mut().unwrap().insert(b"parent".to_vec(),IndexRef{entry_lba:50,kind:trueos_fs::LogKind::Directory,content_type:ContentTypeId::NONE});STATE.lock().mutation=4;assert_eq!(run_path("parent/file",b"new",true,None),Err(CheckedWriteError::Device(block::Error::InvalidParam)));assert!(!ROOTS.lock()[0].index.as_ref().unwrap().contains_key(b"parent/file".as_slice()));assert!(STATE.lock().durable.is_empty());assert_no_publication();}
'''


def main():
    source = (ROOT / 'src/r/fs/trueosfs.rs').read_text()
    enum_start = source.index('#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum CheckedWriteError')
    enum_end = source.index('/// Conditional complete-file write.', enum_start)
    production = source[enum_start:enum_end]
    production += '\n'.join(extract(source, n) for n in [
        'file_in_typed_checked_async', 'validate_indexed_file_target',
        'indexed_removal_entry',
    ])
    production = production.replace('super::file_watch::changed', 'file_watch::changed')
    with tempfile.TemporaryDirectory(prefix='atomic-typed-fs-write-') as directory:
        project = Path(directory)
        (project / 'src').mkdir()
        (project / 'src/lib.rs').write_text(HARNESS + production + TESTS)
        (project / 'Cargo.toml').write_text(f'''[package]
name="atomic-typed-fs-write-tests"
version="0.0.0"
edition="2024"
[dependencies]
sha2="=0.10.9"
infer={{path="{ROOT / 'vendor/infer'}",default-features=false}}
[workspace]
''')
        subprocess.run(['cargo', 'test', '--offline', '--quiet', '--manifest-path',
                        str(project / 'Cargo.toml'), '--', '--test-threads=1'],
                       cwd=project, check=True)


if __name__ == '__main__':
    main()
