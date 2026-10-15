#!/usr/bin/env python3
"""Exercise production indexed move orchestration against bounded mock storage."""
from pathlib import Path
import subprocess
import tempfile
from test_indexed_fs_remove import HARNESS, extract, SOURCE

ENGINE = r'''
mod trueos_fs {
use super::*;
#[derive(Clone,Copy,Debug,PartialEq)]pub enum LogKind{Put,Directory,Other}
pub struct FsParams{pub super_lba:u64,pub data_lba:u64,pub data_end_lba_exclusive:u64}
#[derive(Clone,Copy,Debug,PartialEq)]pub struct NodeRecordRef{pub kind:NodeKind,pub file_id:u64,pub entry_lba:u64,pub data_lba:u64,pub data_len:u64,pub content_type:u32,pub record_key:u64}
#[derive(Clone,Copy,Debug,PartialEq)]pub struct FileRecordRef{pub file_id:u64,pub entry_lba:u64,pub data_lba:u64,pub data_len:u64,pub content_type:u32,pub record_key:u64}
impl From<FileRecordRef> for NodeRecordRef{fn from(r:FileRecordRef)->Self{Self{kind:NodeKind::File,file_id:r.file_id,entry_lba:r.entry_lba,data_lba:r.data_lba,data_len:r.data_len,content_type:r.content_type,record_key:r.record_key}}}
pub async fn get_node_record_by_lba(_:&KernelBlockIo,_:&FsParams,lba:u64)->Result<Option<NodeRecordRef>,block::Error>{let mut s=STATE.lock();assert!(s.held);s.reads.push(lba);if s.missing{return Ok(None)}Ok(Some(NodeRecordRef{kind:if lba==1||lba==77{NodeKind::Directory}else{NodeKind::File},file_id:901,entry_lba:lba,data_lba:200,data_len:12,content_type:if s.corrupt{999}else{3},record_key:808}))}
pub async fn rename_file_at_record(_:&KernelBlockIo,_:&FsParams,src:&str,dst:&str,r:&FileRecordRef)->Result<bool,block::Error>{let mut s=STATE.lock();assert!(s.held);assert_eq!(ROOTS.lock()[0].index.as_ref().unwrap()[src.as_bytes()].entry_lba,r.entry_lba);assert_eq!((r.file_id,r.record_key,r.content_type),(901,808,3));s.copies+=1;if s.engine_error{return Err(block::Error::Corrupted)}if s.fail{return Ok(false)}Ok(true)}
pub async fn delete_file_at_record(_:&KernelBlockIo,_:&FsParams,src:&str,r:&FileRecordRef)->Result<bool,block::Error>{let mut s=STATE.lock();assert!(s.held);assert_eq!(s.leases,1);assert_eq!(s.copied,Some(*r));assert_eq!(ROOTS.lock()[0].index.as_ref().unwrap()[src.as_bytes()].entry_lba,r.entry_lba);s.deletes+=1;if s.engine_error{return Err(block::Error::Corrupted)}Ok(!s.cleanup_fail)}
pub async fn rename_tree_at_record(_:&KernelBlockIo,_:&FsParams,src:&str,_:&str,r:&NodeRecordRef)->Result<bool,block::Error>{let mut s=STATE.lock();assert!(s.held);assert_eq!(ROOTS.lock()[0].index.as_ref().unwrap()[src.as_bytes()].entry_lba,r.entry_lba);assert_eq!((r.file_id,r.record_key,r.content_type),(901,808,3));s.copies+=1;Ok(!s.fail)}
}
fn record_successful_content_commit(t:u32,legacy:bool){assert_eq!(t,3);assert!(!legacy);}
fn file_record_cache_insert(_:u8,p:&str,r:trueos_fs::FileRecordRef){assert_eq!((r.file_id,r.record_key,r.content_type),(901,808,3));STATE.lock().cache.push(format!("insert:{p}"));}
'''
TESTS = r'''

fn run_replace(src:&str,dst:&str)->Result<bool,block::Error>{let mut f=Box::pin(file_rename_replace_async(block::DeviceHandle,src,dst));match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(r)=>r,_=>panic!("mock suspended")}}
#[test]fn replacing_file_publishes_one_metadata_record_and_invalidates_both_paths(){reset();let source=entry("tree/a");assert_eq!(run_replace("tree/a","treehouse"),Ok(true));assert_eq!(entry("treehouse"),source);assert!(entry("tree/a").is_none());let s=STATE.lock();assert_eq!(s.reads,vec![2]);assert_eq!((s.leases,s.copies,s.deletes,s.invalidations,s.bumps,s.watches),(1,1,0,0,1,2));assert_eq!(s.cache,vec!["tree/a","treehouse","insert:treehouse"]);assert_eq!(ROOTS.lock()[0].writes_since_checkpoint,1);}
#[test]fn replace_failure_preserves_both_namespace_entries(){for mode in 0..4{reset();{let mut s=STATE.lock();match mode{0=>s.fail=true,1=>s.engine_error=true,2=>s.corrupt=true,_=>s.missing=true}}let before=ROOTS.lock()[0].index.clone();assert_eq!(run_replace("tree/a","treehouse"),if mode==0{Ok(false)}else{Err(block::Error::Corrupted)});assert_eq!(ROOTS.lock()[0].index,before);let s=STATE.lock();assert_eq!((s.deletes,s.bumps,s.watches,s.invalidations),(0,0,0,0));assert!(s.cache.is_empty());assert!(!s.held);}}
#[test]fn replace_rejects_directory_and_descendants_and_missing_parent(){for dst in ["tree","tree/nested","missing/target","treehouse/target"]{reset();let before=ROOTS.lock()[0].index.clone();assert_eq!(run_replace("tree/a",dst),Ok(false));assert_eq!(ROOTS.lock()[0].index,before);assert_eq!(STATE.lock().copies,0);}}
#[test]fn replace_same_path_requires_live_file(){for (src,expected) in [("tree/a",true),("missing",false),("tree",false)]{reset();assert_eq!(run_replace(src,src),Ok(expected));let s=STATE.lock();assert_eq!((s.copies,s.deletes,s.bumps,s.watches),(0,0,0,0));}}
#[test]fn replace_rechecks_destination_after_namespace_wait(){reset();STATE.lock().mutation=3;assert_eq!(run_replace("tree/a","moved"),Ok(true));assert_eq!(entry("moved").unwrap().entry_lba,2);assert_eq!(STATE.lock().copies,1);}
#[test]fn replace_to_vacant_destination_and_invalidated_index(){reset();STATE.lock().mutation=2;assert_eq!(run_replace("tree/a","moved"),Ok(true));let s=STATE.lock();assert_eq!((s.ensures,s.leases,s.copies,s.deletes),(2,2,1,0));assert!(!s.held);}
fn run_move(tree:bool,src:&str,dst:&str)->Result<bool,block::Error>{let f:std::pin::Pin<Box<dyn Future<Output=Result<bool,block::Error>>>>=if tree{Box::pin(dir_rename_async(block::DeviceHandle,src,dst))}else{Box::pin(file_rename_async(block::DeviceHandle,src,dst))};let mut f=f;match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(r)=>r,_=>panic!("mock suspended")}}
fn entry(path:&str)->Option<IndexRef>{ROOTS.lock()[0].index.as_ref().unwrap().get(path.as_bytes()).copied()}
#[test]fn file_success_preserves_warm_index_and_physical_record(){reset();assert_eq!(run_move(false,"tree/a","moved"),Ok(true));assert!(entry("tree/a").is_none());let dst=entry("moved").unwrap();assert_eq!((dst.entry_lba,dst.content_type),(2,3));assert!(entry("treehouse").is_some());let s=STATE.lock();assert_eq!((s.leases,s.deletes,s.copies,s.invalidations),(1,0,1,0));assert_eq!((s.bumps,s.watches),(1,2));assert!(!s.held);assert_eq!(ROOTS.lock()[0].writes_since_checkpoint,1);}
#[test]fn tree_remap_is_exact_and_metadata_preserving(){reset();let child=entry("tree/nested/b");assert_eq!(run_move(true,"tree/","moved/"),Ok(true));assert!(entry("tree").is_none());assert_eq!(entry("moved/nested/b"),child);assert!(entry("treehouse").is_some());assert!(entry("tree-other/a").is_some());let s=STATE.lock();assert_eq!((s.copies,s.deletes,s.invalidations),(1,0,0));assert_eq!(s.cache,vec!["tree/","moved/","tree","moved"]);assert_eq!(ROOTS.lock()[0].writes_since_checkpoint,1);}
#[test]fn overwrite_and_missing_parent_rejected(){for tree in [false,true]{for dst in ["treehouse","tree/nested","missing/target","treehouse/target"]{reset();let before=ROOTS.lock()[0].index.clone();assert_eq!(run_move(tree,if tree{"tree"}else{"tree/a"},dst),Ok(false));assert_eq!(ROOTS.lock()[0].index,before);let s=STATE.lock();assert_eq!((s.copies,s.deletes,s.bumps,s.watches),(0,0,0,0));}}}
#[test]fn parent_directory_allowed(){reset();assert_eq!(run_move(false,"tree/a","tree/new"),Ok(true));assert!(entry("tree/new").is_some());}
#[test]fn failed_rename_or_corrupt_record_does_not_publish(){for tree in [false,true]{for mode in 0..3{reset();{let mut s=STATE.lock();if mode==0{s.fail=true}else if mode==1{s.corrupt=true}else{s.missing=true}}let before=ROOTS.lock()[0].index.clone();assert_eq!(run_move(tree,if tree{"tree"}else{"tree/a"},"moved"),if mode==0{Ok(false)}else{Err(block::Error::Corrupted)});assert_eq!(ROOTS.lock()[0].index,before);let s=STATE.lock();assert_eq!((s.bumps,s.watches,s.invalidations,s.deletes),(0,0,0,0));assert!(s.cache.is_empty());assert!(!s.held);}}}
#[test]fn source_and_destination_rechecked_after_wait(){for tree in [false,true]{for mutation in [1,3,4]{reset();STATE.lock().mutation=mutation;let result=run_move(tree,if tree{"tree"}else{"tree/a"},"moved");if mutation==1{assert_eq!(result,Ok(true));assert_eq!(STATE.lock().reads,vec![if tree{77}else{78}]);}else{assert_eq!(result,Ok(false));assert_eq!(STATE.lock().copies,0);}}}}
#[test]fn invalidated_index_rebuilt_outside_lease(){for tree in [false,true]{reset();STATE.lock().mutation=2;assert_eq!(run_move(tree,if tree{"tree"}else{"tree/a"},"moved"),Ok(true));let s=STATE.lock();assert_eq!((s.ensures,s.leases,s.invalidations),(2,2,0));assert!(!s.held);}}
#[test]fn tree_cannot_move_inside_itself(){reset();assert_eq!(run_move(true,"tree","tree/nested/new"),Ok(false));assert_eq!(STATE.lock().copies,0);}
'''

def main():
    harness = HARNESS.replace('use std::{', 'extern crate alloc;\nuse std::{', 1)
    start = harness.index('mod trueos_fs{')
    end = harness.index('fn bump_root_cache_gen', start)
    harness = harness[:start] + ENGINE + harness[end:]
    harness = harness.replace('engine_error:bool}', 'engine_error:bool,cleanup_fail:bool,copies:usize,copied:Option<trueos_fs::FileRecordRef>}')
    harness = harness.replace('engine_error:false}', 'engine_error:false,cleanup_fail:false,copies:0,copied:None}')
    harness = harness.replace('assert_eq!(s.leases,1);assert_eq!(s.copied', 'assert!(s.leases<=2);assert_eq!(s.copied')
    harness = harness.replace('entry_lba=77;', 'entry_lba=77;crate::ROOTS.lock()[0].index.as_mut().unwrap().get_mut(b"tree/a".as_slice()).unwrap().entry_lba=78;')
    harness = harness.replace('mod write_gate{', 'mod write_gate{use crate::{IndexRef,trueos_fs};')
    harness = harness.replace('if mutation==2{', 'if mutation==3{crate::ROOTS.lock()[0].index.as_mut().unwrap().insert(b"moved".to_vec(),IndexRef{entry_lba:9,kind:trueos_fs::LogKind::Put,content_type:3});}if mutation==4{let mut roots=crate::ROOTS.lock();let index=roots[0].index.as_mut().unwrap();index.remove(b"tree".as_slice());index.remove(b"tree/a".as_slice());}if mutation==2{')
    # The fixture exercises move futures, replacing the removal-specific runner.
    harness = harness[:harness.index('fn run(p:&str)')]
    source = SOURCE.read_text()
    names = ['indexed_move_entry','indexed_move_entry_with_policy','file_rename_replace_async','file_rename_with_policy_async','file_rename_async','dir_rename_async','collect_index_tree_moves','update_root_index_put','update_root_index_node','update_root_index_delete','update_root_index_rename_tree','normalize_dir_name','normalized_dir_prefix']
    production = '\n'.join(extract(source,n) for n in names).replace('super::file_watch::changed','file_watch::changed')
    # Use the production normalization helper instead of the removal harness mock.
    a=harness.index('fn normalized_dir_prefix('); b=harness.index('\nfn map_engine_err',a)
    harness=harness[:a]+harness[b:]
    with tempfile.TemporaryDirectory(prefix='indexed-fs-move-') as directory:
        rust=Path(directory)/'test.rs'; binary=Path(directory)/'test'
        rust.write_text(harness+production+TESTS)
        subprocess.run(['rustc','--edition=2024','--test','-Awarnings',str(rust),'-o',str(binary)],check=True)
        subprocess.run([str(binary),'--test-threads=1'],check=True)
if __name__=='__main__':
    main()
