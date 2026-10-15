#!/usr/bin/env python3
"""Compile production indexed removal with deterministic storage/lease boundaries."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'src/r/fs/trueosfs.rs'

def extract(text, name):
    start = text.index('fn ' + name + '(')
    start = text.rfind('\n', 0, start) + 1
    opening = text.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    return text[start:end]

HARNESS = r'''
use std::{collections::BTreeMap, sync::{Mutex, MutexGuard}, future::Future, task::{Context, Poll, Waker}, pin::pin};
struct Lock<T>(Mutex<T>);
impl<T> Lock<T> { fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()} }
mod block { pub type DiscId=u8; #[derive(Clone,Copy)] pub struct DeviceHandle; impl DeviceHandle { pub fn id(self)->u8{1} pub fn parent(self)->Option<()>{None} } #[derive(Debug,PartialEq)] pub enum Error{InvalidParam,NotReady,Corrupted} }
#[derive(Clone,Copy,PartialEq,Debug)] enum NodeKind{File,Directory}
#[derive(Clone,Copy,PartialEq,Debug)] struct IndexRef{entry_lba:u64,kind:trueos_fs::LogKind,content_type:u32}
type TrueosFsIndex=BTreeMap<Vec<u8>,IndexRef>;
struct Mount{disk_id:u8,index:Option<TrueosFsIndex>,writes_since_checkpoint:u64}
static ROOTS:Lock<Vec<Mount>>=Lock(Mutex::new(Vec::new()));
#[derive(Default)] struct State{held:bool,ensures:usize,leases:usize,mutation:u8,reads:Vec<u64>,deletes:usize,invalidations:usize,bumps:usize,watches:usize,cache:Vec<String>,missing:bool,corrupt:bool,fail:bool,engine_error:bool}
static STATE:Lock<State>=Lock(Mutex::new(State{held:false,ensures:0,leases:0,mutation:0,reads:Vec::new(),deletes:0,invalidations:0,bumps:0,watches:0,cache:Vec::new(),missing:false,corrupt:false,fail:false,engine_error:false}));
mod disc{pub mod access{pub struct Activity; impl Activity{pub fn begin(_:crate::block::DeviceHandle)->Result<Self,crate::block::Error>{Ok(Self)}}}}
struct Placement{super_lba:u64,data_lba:u64,data_end_lba_exclusive:u64}
async fn placement_for_io_async(_:block::DeviceHandle)->Result<Option<Placement>,block::Error>{Ok(Some(Placement{super_lba:0,data_lba:1,data_end_lba_exclusive:100}))}
struct KernelBlockIo; impl KernelBlockIo{fn new(_:block::DeviceHandle)->Self{Self}}
fn normalized_dir_prefix(p:&str)->String{format!("{}/",p.trim_end_matches('/'))}
fn map_engine_err(e:block::Error)->block::Error{e}
async fn ensure_index_async(_:block::DeviceHandle,_:&Placement)->Result<(),block::Error>{let mut s=STATE.lock(); assert!(!s.held,"index rebuild while root write lease held");s.ensures+=1;drop(s);let mut r=ROOTS.lock();if r[0].index.is_none(){r[0].index=Some(fixture());}Ok(())}
mod write_gate{pub struct RootWriteLease;impl RootWriteLease{pub async fn acquire(_:u8)->Self{let mut s=crate::STATE.lock();assert!(!s.held);s.leases+=1;s.held=true;let mutation=std::mem::take(&mut s.mutation);drop(s);if mutation==1{crate::ROOTS.lock()[0].index.as_mut().unwrap().get_mut(b"tree".as_slice()).unwrap().entry_lba=77;}if mutation==2{crate::ROOTS.lock()[0].index=None;}Self}}impl Drop for RootWriteLease{fn drop(&mut self){crate::STATE.lock().held=false;}}}
mod trueos_fs{use super::*;#[derive(Clone,Copy,Debug,PartialEq)]pub enum LogKind{Put,Directory,Other}pub struct FsParams{pub super_lba:u64,pub data_lba:u64,pub data_end_lba_exclusive:u64}pub struct Record{pub kind:NodeKind,pub content_type:u32}pub async fn get_node_record_by_lba(_:&KernelBlockIo,_:&FsParams,lba:u64)->Result<Option<Record>,block::Error>{let mut s=STATE.lock();assert!(s.held);s.reads.push(lba);if s.missing{return Ok(None)}Ok(Some(Record{kind:if lba==1||lba==77{NodeKind::Directory}else{NodeKind::File},content_type:if s.corrupt{999}else{3}}))}async fn delete(tree:bool,r:&Record)->Result<bool,block::Error>{let mut s=STATE.lock();assert!(s.held);assert_eq!(tree,r.kind==NodeKind::Directory);s.deletes+=1;if s.engine_error{return Err(block::Error::Corrupted)}Ok(!s.fail)}pub async fn delete_node_at_record(_:&KernelBlockIo,_:&FsParams,_:&str,r:&Record)->Result<bool,block::Error>{delete(false,r).await}pub async fn delete_tree_at_record(_:&KernelBlockIo,_:&FsParams,_:&str,r:&Record)->Result<bool,block::Error>{delete(true,r).await}}
fn bump_root_cache_gen(_:u8){STATE.lock().bumps+=1;}
fn file_record_cache_invalidate_prefix(_:u8,p:&str){STATE.lock().cache.push(p.into());}
fn file_record_cache_invalidate_path(_:u8,p:&str){STATE.lock().cache.push(p.into());}
fn invalidate_root_index(_:u8){STATE.lock().invalidations+=1;ROOTS.lock()[0].index=None;}
mod file_watch{pub fn changed(_:u8,_:&str){crate::STATE.lock().watches+=1;}}
fn fixture()->TrueosFsIndex{[("tree",1,trueos_fs::LogKind::Directory),("tree/a",2,trueos_fs::LogKind::Put),("tree/nested/b",3,trueos_fs::LogKind::Put),("treehouse",4,trueos_fs::LogKind::Put),("tree-other/a",5,trueos_fs::LogKind::Put)].into_iter().map(|(p,l,k)|(p.as_bytes().to_vec(),IndexRef{entry_lba:l,kind:k,content_type:3})).collect()}
fn reset(){*STATE.lock()=State::default();*ROOTS.lock()=vec![Mount{disk_id:1,index:Some(fixture()),writes_since_checkpoint:0}];}
fn run(p:&str)->Result<bool,block::Error>{let mut f=pin!(remove_recursive_async(block::DeviceHandle,p));match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(r)=>r,Poll::Pending=>panic!("unexpected suspended mock")}}
fn run_file(p:&str)->Result<bool,block::Error>{let mut f=pin!(file_delete_async(block::DeviceHandle,p));match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(r)=>r,Poll::Pending=>panic!("unexpected suspended mock")}}
'''
TESTS = r'''
#[test]fn successful_file_and_tree(){for path in ["tree/a","tree"]{reset();assert_eq!(run(path),Ok(true));let r=ROOTS.lock();let index=r[0].index.as_ref().unwrap();assert!(!index.contains_key(path.as_bytes()));assert!(index.contains_key(b"treehouse".as_slice()));assert!(index.contains_key(b"tree-other/a".as_slice()));if path=="tree"{assert!(!index.contains_key(b"tree/nested/b".as_slice()));}else{assert!(index.contains_key(b"tree/nested/b".as_slice()));}assert_eq!(r[0].writes_since_checkpoint,1);let s=STATE.lock();assert_eq!((s.invalidations,s.bumps,s.watches,s.deletes),(0,1,1,1));assert_eq!(s.cache,vec![normalized_dir_prefix(path),path.to_string()]);assert!(!s.held);}}
#[test]fn failed_and_corrupt_preserve_index(){for mode in 0..5{reset();{let mut s=STATE.lock();match mode{0=>s.fail=true,1=>s.missing=true,2=>s.corrupt=true,3=>s.engine_error=true,_=>{}}}if mode==4{ROOTS.lock()[0].index.as_mut().unwrap().get_mut(b"tree".as_slice()).unwrap().kind=trueos_fs::LogKind::Other;}let before=ROOTS.lock()[0].index.clone();let result=run("tree");assert_eq!(result,if mode==0{Ok(false)}else{Err(block::Error::Corrupted)});let r=ROOTS.lock();assert_eq!(r[0].index,before);assert_eq!(r[0].writes_since_checkpoint,0);let s=STATE.lock();assert_eq!((s.invalidations,s.bumps,s.watches),(0,0,0));assert!(s.cache.is_empty());assert!(!s.held);}}
#[test]fn record_rechecked_after_wait(){reset();STATE.lock().mutation=1;assert_eq!(run("tree"),Ok(true));assert_eq!(STATE.lock().reads,vec![77]);}
#[test]fn invalidated_index_rebuilt_outside_lease(){reset();STATE.lock().mutation=2;assert_eq!(run("tree"),Ok(true));let s=STATE.lock();assert_eq!((s.ensures,s.leases),(2,2));assert_eq!(s.reads,vec![1]);assert_eq!(s.invalidations,0);assert!(!s.held);}
#[test]fn absent_path_has_no_mutation(){reset();assert_eq!(run("missing"),Ok(false));let s=STATE.lock();assert_eq!((s.deletes,s.bumps,s.watches,s.invalidations),(0,0,0,0));assert!(!s.held);assert_eq!(ROOTS.lock()[0].index,Some(fixture()));}
#[test]fn file_only_delete_keeps_directory_and_warm_index(){reset();assert_eq!(run_file("tree"),Ok(false));assert_eq!(run_file("tree/a"),Ok(true));let r=ROOTS.lock();let index=r[0].index.as_ref().unwrap();assert!(index.contains_key(b"tree".as_slice()));assert!(!index.contains_key(b"tree/a".as_slice()));assert!(index.contains_key(b"tree/nested/b".as_slice()));assert_eq!(r[0].writes_since_checkpoint,1);let s=STATE.lock();assert_eq!((s.deletes,s.invalidations,s.watches),(1,0,1));assert!(!s.held);}
#[test]fn file_only_delete_rebuilds_before_reacquiring_lease(){reset();STATE.lock().mutation=2;assert_eq!(run_file("tree/a"),Ok(true));let s=STATE.lock();assert_eq!((s.ensures,s.leases),(2,2));assert_eq!(s.reads,vec![2]);assert!(!s.held);}
'''

def main():
    source = SOURCE.read_text()
    names = ['update_root_index_delete', 'remove_index_subtree', 'update_root_index_delete_tree', 'indexed_removal_entry', 'remove_recursive_async', 'file_delete_async']
    production = '\n'.join(extract(source, n) for n in names).replace('super::file_watch::changed', 'file_watch::changed')
    with tempfile.TemporaryDirectory(prefix='indexed-fs-remove-') as directory:
        directory = Path(directory)
        rust = directory / 'test.rs'
        binary = directory / 'test'
        rust.write_text(HARNESS + production + TESTS)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Awarnings', str(rust), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)

if __name__ == '__main__':
    main()
