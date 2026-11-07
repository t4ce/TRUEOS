#![allow(dead_code, unused_imports)]
extern crate alloc;
use std::future::Future;
use std::task::{Context,Poll,Waker};
use std::sync::atomic::{AtomicBool,Ordering};
fn run<T>(future:impl Future<Output=T>)->T{let mut future=std::pin::pin!(future);let mut cx=Context::from_waker(Waker::noop());loop{match future.as_mut().poll(&mut cx){Poll::Ready(value)=>return value,Poll::Pending=>std::thread::yield_now()}}}
mod disc {
 pub mod block {
  #[derive(Clone,Copy,Debug,Eq,PartialEq)]pub enum Error{NotSupported,NotReady,InvalidParam,OutOfBounds,Io,Corrupted}
  #[derive(Clone,Copy,Debug,Eq,PartialEq)]pub struct DiscId(u32);impl DiscId{pub const fn raw(self)->u32{self.0}}
  #[derive(Clone,Copy,Debug)]pub struct DeviceHandle(pub u32);impl DeviceHandle{pub fn id(self)->DiscId{DiscId(self.0)}pub fn parent(self)->Option<Self>{None}}
 }
 pub mod access {pub struct Activity;impl Activity{pub fn begin(_:super::block::DeviceHandle)->Result<Self,super::block::Error>{Ok(Self)}}}
}
mod r{
 pub mod cabi_codes{include!("@CABI_CODES_SOURCE@");}
 pub mod fs{
  pub mod file_watch{pub fn changed(_:crate::disc::block::DiscId,_:&str){}}
  pub mod trueosfs{
   use alloc::{collections::BTreeMap,string::String,vec::Vec};use crate::disc::block;use spin::Mutex;
   pub use trueos_fs::{ContentTypeId,RecordKey,NodeKind};
   struct RootMount{disk_id:block::DiscId,seq:u32,index:Option<()>}
   static ROOTS:Mutex<Vec<RootMount>>=Mutex::new(Vec::new());
   static INDEX:Mutex<BTreeMap<String,trueos_fs::FileRecordRef>>=Mutex::new(BTreeMap::new());
   static DISK:Mutex<Vec<u8>>=Mutex::new(Vec::new());
   static FAIL_WRITE:core::sync::atomic::AtomicBool=core::sync::atomic::AtomicBool::new(false);
   static WRITER:core::sync::atomic::AtomicBool=core::sync::atomic::AtomicBool::new(false);
   #[derive(Clone,Copy)]struct Placement{super_lba:u64,data_lba:u64,data_end_lba_exclusive:Option<u64>}
   struct KernelBlockIo;impl KernelBlockIo{fn new(_:block::DeviceHandle)->Self{Self}}
   impl trueos_fs::BlockIo for KernelBlockIo{
    type Error=block::Error;fn block_size(&self)->usize{512}fn block_count(&self)->u64{32768}
    async fn read_blocks(&self,lba:u64,blocks:usize)->Result<Vec<u8>,Self::Error>{let bytes=DISK.lock();let start=lba as usize*512;Ok(bytes.get(start..start+blocks*512).ok_or(block::Error::OutOfBounds)?.to_vec())}
    async fn write_blocks(&self,lba:u64,bytes:&[u8])->Result<(),Self::Error>{assert!(WRITER.load(core::sync::atomic::Ordering::SeqCst));if FAIL_WRITE.swap(false,core::sync::atomic::Ordering::SeqCst){return Err(block::Error::Io)}let mut data=DISK.lock();let start=lba as usize*512;data.get_mut(start..start+bytes.len()).ok_or(block::Error::OutOfBounds)?.copy_from_slice(bytes);Ok(())}
    async fn flush(&self)->Result<(),Self::Error>{Ok(())}
   }
   fn map_engine_err(error:trueos_fs::FsError<block::Error>)->block::Error{match error{trueos_fs::FsError::Device(e)=>e,trueos_fs::FsError::InvalidParam=>block::Error::InvalidParam,_=>block::Error::Corrupted}}
   async fn placement_for_io_async(_:block::DeviceHandle)->Result<Option<Placement>,block::Error>{Ok(Some(Placement{super_lba:0,data_lba:1,data_end_lba_exclusive:Some(32768)}))}
   async fn ensure_index_async(_:block::DeviceHandle,_:&Placement)->Result<(),block::Error>{assert!(!WRITER.load(core::sync::atomic::Ordering::SeqCst));Ok(())}
   mod write_gate{use super::*;pub struct RootWriteLease;impl RootWriteLease{pub async fn acquire(_:block::DiscId)->Self{core::future::poll_fn(|cx|if WRITER.compare_exchange(false,true,core::sync::atomic::Ordering::SeqCst,core::sync::atomic::Ordering::SeqCst).is_ok(){core::task::Poll::Ready(Self)}else{cx.waker().wake_by_ref();core::task::Poll::Pending}).await}}impl Drop for RootWriteLease{fn drop(&mut self){WRITER.store(false,core::sync::atomic::Ordering::SeqCst);}}}
   struct IndexRef{entry_lba:u64}
   fn indexed_removal_entry(_:block::DiscId,path:&str)->Result<Option<IndexRef>,block::Error>{assert!(WRITER.load(core::sync::atomic::Ordering::SeqCst));Ok(INDEX.lock().get(path).map(|r|IndexRef{entry_lba:r.entry_lba}))}
   fn validate_indexed_file_target(_:block::DiscId,path:&str,_:bool)->Result<bool,block::Error>{assert!(WRITER.load(core::sync::atomic::Ordering::SeqCst));if path.contains('/') {return Err(block::Error::InvalidParam)}Ok(true)}
   fn record_successful_content_commit(_:ContentTypeId,_:bool){}fn bump_root_cache_gen(_:block::DiscId){}fn file_record_cache_invalidate_path(_:block::DiscId,_:&str){}fn file_record_cache_insert(_:block::DiscId,_:&str,_:trueos_fs::FileRecordRef){}fn update_root_index_put(_:block::DiscId,path:&str,record:trueos_fs::FileRecordRef)->bool{INDEX.lock().insert(path.into(),record);true}fn invalidate_root_index(_:block::DiscId){}
   #[path="@FILE_OBJECT_SOURCE@"]pub mod file_objects;
   #[cfg(test)]mod tests{
    use super::*;use file_objects::{Command,open,operate};const D:block::DeviceHandle=block::DeviceHandle(1);
    fn reset(){for owner in 1..=3{file_objects::release_owner(owner);}let mut bytes=vec![0;32768*512];trueos_fs::write_blank_superblock(&mut bytes[..512]);*DISK.lock()=bytes;INDEX.lock().clear();*ROOTS.lock()=vec![RootMount{disk_id:D.id(),seq:1,index:Some(())}];FAIL_WRITE.store(false,core::sync::atomic::Ordering::SeqCst);assert!(!WRITER.load(core::sync::atomic::Ordering::SeqCst));}
    fn id(bytes:Vec<u8>)->u64{u64::from_le_bytes(bytes.try_into().unwrap())}
    fn call(owner:u32,op:u32,flags:u32,object:u64,arg:u64,payload:&[u8])->Result<Vec<u8>,i32>{crate::run(operate(owner,&Command{op,flags,object,arg,payload:payload.to_vec()}))}
    fn opened(owner:u32,path:&str,flags:u32)->u64{id(crate::run(open(owner,D,path,flags)).unwrap())}
    fn read(owner:u32,h:u64)->Vec<u8>{call(owner,2,0,h,1024,&[]).unwrap()}
    fn seek(owner:u32,h:u64,at:u64){assert_eq!(id(call(owner,4,0,h,at,&[]).unwrap()),at);}
    fn disk(path:&str)->Option<Vec<u8>>{crate::run(trueos_fs::read_file(&KernelBlockIo,&trueos_fs::FsParams{super_lba:0,data_lba:1,data_end_lba_exclusive:Some(32768)},path)).unwrap()}
    fn params()->trueos_fs::FsParams{trueos_fs::FsParams{super_lba:0,data_lba:1,data_end_lba_exclusive:Some(32768)}}
    fn remove(path:&str){crate::run(async{let _lease=write_gate::RootWriteLease::acquire(D.id()).await;let retained=file_objects::prepare_orphans(D,&params(),path,false).await.unwrap();assert!(trueos_fs::delete_file(&KernelBlockIo,&params(),path).await.unwrap());INDEX.lock().remove(path);file_objects::commit_orphans(retained);});}
    fn rename(src:&str,dst:&str){crate::run(async{let _lease=write_gate::RootWriteLease::acquire(D.id()).await;let retained=file_objects::prepare_orphans(D,&params(),dst,false).await.unwrap();let record=INDEX.lock()[src];assert!(trueos_fs::rename_file_at_record(&KernelBlockIo,&params(),src,dst,&record).await.unwrap());INDEX.lock().remove(src);INDEX.lock().insert(dst.into(),record);file_objects::commit_orphans(retained);file_objects::renamed(D,src,dst);});}
    #[test]fn full_file_semantics_and_identity(){reset();let h=opened(1,"file",1|2|16);assert_eq!(id(call(1,3,0,h,0,b"abcd").unwrap()),4);assert_eq!(disk("file").unwrap(),b"abcd");seek(1,h,1);call(1,3,0,h,0,b"XY").unwrap();assert_eq!(disk("file").unwrap(),b"aXYd");let duplicate=id(call(1,7,0,h,0,&[]).unwrap());assert_eq!(read(1,duplicate),b"d");assert!(read(1,h).is_empty());assert_eq!(call(2,2,0,h,1,&[]),Err(-8));let original_id=call(1,5,0,h,0,&[]).unwrap()[12..20].to_vec();rename("file","renamed");assert!(disk("file").is_none());seek(1,h,0);assert_eq!(read(1,h),b"aXYd");assert_eq!(call(1,5,0,h,0,&[]).unwrap()[12..20],original_id);call(1,8,0,h,0,&[]).unwrap();seek(1,duplicate,0);assert_eq!(read(1,duplicate),b"aXYd");}
    #[test]fn append_clone_locks_and_retirement(){reset();let a=opened(1,"log",1|4|16);let b=opened(2,"log",1|4);for n in 0..50{call(1,3,0,a,0,format!("A{n};").as_bytes()).unwrap();call(2,3,0,b,0,format!("B{n};").as_bytes()).unwrap();}let bytes=disk("log").unwrap();for n in 0..50{assert!(String::from_utf8_lossy(&bytes).contains(&format!("A{n};B{n};")));}call(1,9,1,a,0,&[]).unwrap();call(2,9,1,b,0,&[]).unwrap();assert_eq!(call(1,9,2,a,0,&[]),Err(-18));call(2,10,0,b,0,&[]).unwrap();call(1,9,2,a,0,&[]).unwrap();let d=id(call(1,7,0,a,0,&[]).unwrap());call(1,8,0,a,0,&[]).unwrap();assert_eq!(call(2,9,2,b,0,&[]),Err(-18));assert_eq!(file_objects::release_owner(1),1);call(2,9,2,b,0,&[]).unwrap();assert_eq!(call(1,2,0,d,2,&[]),Err(-8));}
    #[test]fn unlink_and_replace_retain_real_objects(){reset();let old=opened(1,"target",1|2|16);call(1,3,0,old,0,b"old").unwrap();let replacement=opened(2,"new",1|2|16);call(2,3,0,replacement,0,b"new").unwrap();rename("new","target");assert_eq!(disk("target").unwrap(),b"new");seek(1,old,0);assert_eq!(read(1,old),b"old");seek(1,old,0);call(1,3,0,old,0,b"OLD").unwrap();seek(1,old,0);assert_eq!(read(1,old),b"OLD");assert_eq!(disk("target").unwrap(),b"new");remove("target");assert!(disk("target").is_none());seek(2,replacement,0);assert_eq!(read(2,replacement),b"new");call(2,3,0,replacement,0,b"!").unwrap();assert!(disk("target").is_none());}
    #[test]fn create_new_truncate_sparse_and_failed_publish(){reset();let h=opened(1,"file",1|2|32);assert_eq!(crate::run(open(2,D,"file",2|32)),Err(-9));seek(1,h,5);call(1,3,0,h,0,b"x").unwrap();assert_eq!(disk("file").unwrap(),b"\0\0\0\0\0x");call(1,6,0,h,2,&[]).unwrap();assert_eq!(disk("file").unwrap(),b"\0\0");assert_eq!(id(call(1,4,1,h,0,&[]).unwrap()),6);FAIL_WRITE.store(true,core::sync::atomic::Ordering::SeqCst);assert!(call(1,3,0,h,0,b"bad").is_err());assert_eq!(disk("file").unwrap(),b"\0\0");assert_eq!(id(call(1,4,1,h,0,&[]).unwrap()),6);let _=opened(2,"file",2|8);assert!(disk("file").unwrap().is_empty());}
    #[test]fn live_external_write_and_positional_io(){reset();let h=opened(1,"file",1|2|16);call(1,3,0,h,0,b"abcdef").unwrap();let other=opened(2,"file",1|2);call(2,3,0,other,0,b"XYZ").unwrap();seek(1,h,0);assert_eq!(read(1,h),b"XYZdef");seek(1,h,5);assert_eq!(call(1,12,3,h,1,&[]).unwrap(),b"YZd");assert_eq!(id(call(1,4,1,h,0,&[]).unwrap()),5);call(1,13,0,h,2,b"@!").unwrap();assert_eq!(disk("file").unwrap(),b"XY@!ef");assert_eq!(id(call(1,4,1,h,0,&[]).unwrap()),5);}
    #[test]fn actual_concurrent_append_jobs_preserve_all_entries(){reset();let a=opened(1,"log",1|4|16);let b=opened(2,"log",1|4);let barrier=std::sync::Arc::new(std::sync::Barrier::new(2));let gate=barrier.clone();let first=std::thread::spawn(move||{gate.wait();for n in 0..100{call(1,3,0,a,0,format!("A{n:03};").as_bytes()).unwrap();}});let second=std::thread::spawn(move||{barrier.wait();for n in 0..100{call(2,3,0,b,0,format!("B{n:03};").as_bytes()).unwrap();}});first.join().unwrap();second.join().unwrap();let bytes=disk("log").unwrap();assert_eq!(bytes.len(),1000);let text=String::from_utf8(bytes).unwrap();for n in 0..100{assert_eq!(text.matches(&format!("A{n:03};")).count(),1);assert_eq!(text.matches(&format!("B{n:03};")).count(),1);}}
    #[test]fn access_modes_negative_seek_and_mount_epoch(){reset();let write=opened(1,"file",2|16);call(1,3,0,write,0,b"123").unwrap();assert_eq!(call(1,2,0,write,1,&[]),Err(-19));let read_only=opened(2,"file",1);assert_eq!(call(2,3,0,read_only,0,b"x"),Err(-19));assert_eq!(call(2,6,0,read_only,0,&[]),Err(-19));assert_eq!(call(2,4,1,read_only,(-1_i64)as u64,&[]),Err(-4));assert_eq!(id(call(2,4,1,read_only,0,&[]).unwrap()),0);ROOTS.lock()[0].seq=2;assert_eq!(call(2,2,0,read_only,1,&[]),Err(-8));call(2,8,0,read_only,0,&[]).unwrap();}
    #[test]fn requests_validate_flags_and_bounds(){let mut request=vec![0;24];request[0..4].copy_from_slice(&2u32.to_le_bytes());request[8..16].copy_from_slice(&1u64.to_le_bytes());request[16..24].copy_from_slice(&u64::MAX.to_le_bytes());assert!(Command::decode(&request).is_err());request[16..24].copy_from_slice(&1u64.to_le_bytes());assert!(Command::decode(&request).is_ok());request.push(0);assert!(Command::decode(&request).is_err());}
   }
  }
 }
}
