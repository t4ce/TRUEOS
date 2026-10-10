#!/usr/bin/env python3
"""Run the production AppDB virtual-write handler with host filesystem fakes."""
from pathlib import Path
import subprocess
import tempfile
root = Path(__file__).resolve().parents[2]
source = (root/'src/r/io/async_fs_cabi.rs').read_text()
start = source.index('    if let RequestKind::Write { path, bytes, .. } = &request.kind', source.index('async fn process'))
end = source.index('    if matches!(request.kind, RequestKind::ListMounts)', start)
handler = source[start:end]
harness = r'''
extern crate alloc;
use std::sync::{Mutex,Arc};
use alloc::string::String;
const FS_ERR_BAD_PATH:i32=-6; const FS_ERR_BAD_PARAM:i32=-4; const FS_ERR_NOT_FOUND:i32=-5; const FS_ERR_TOO_LARGE:i32=-7; const FS_ERR_IO:i32=-2;
struct State {scope:bool,len:u64,valid:bool,missing:bool,db_error:bool,imports:Vec<String>}
static STATE:Mutex<State>=Mutex::new(State{scope:true,len:2,valid:true,missing:false,db_error:false,imports:Vec::new()});
mod hv {
    pub fn with_guest_broker_context<T>(_:u8,f:impl FnOnce()->T)->T{f()}
    pub mod blueprint {pub fn prebind_required_readiness(_: &[u8])->Result<u32,()> {if crate::STATE.lock().unwrap().valid {Ok(0)} else {Err(())}}}
}
mod app_db {pub fn insert_download(name:&str,_:&[u8])->Result<(),()> {let mut s=crate::STATE.lock().unwrap();if s.db_error {Err(())} else {s.imports.push(name.into());Ok(())}}}
mod r {pub mod fs {pub mod trueosfs {
    pub struct Info {pub data_len:u64}
    pub async fn file_info_async(_:u8,_:&str)->Result<Option<Info>,()> {let s=crate::STATE.lock().unwrap();Ok((!s.missing).then_some(Info{data_len:s.len}))}
    pub async fn file_out_async(_:u8,_:&str)->Result<Option<Vec<u8>>,()> {Ok(Some(vec![1,2]))}
}}}
mod env {
    pub fn trueosfs_scope_granted()->bool {crate::STATE.lock().unwrap().scope}
    pub fn resolve_fs_path(path:&str,_:bool)->Option<String> {(!path.is_empty() && !path.contains('\0')).then(||path.to_string())}
}
fn selected_disk(path:&str)->Result<(u8,&str),i32> {Ok((0,path))}
fn map_block_error(_:())->i32 {FS_ERR_IO}
enum RequestKind {Write {path:String,bytes:Vec<u8>},Other}
struct Request {owner:u32,kind:RequestKind}
#[derive(Debug,PartialEq)] enum OperationState {Unit,Failed(i32)}
mod handler {
    use super::*;
    fn is_virtual_write(path:&str)->bool {path=="vFile:appdb-install"}
    pub async fn process(request:&Request)->OperationState {
'''
footer = r'''
        OperationState::Failed(-99)
    }
}
fn run(path:&str,owner:u32)->OperationState {
    struct Wake;impl std::task::Wake for Wake{fn wake(self:Arc<Self>) {}}
    let request=Request{owner,kind:RequestKind::Write{path:"vFile:appdb-install".into(),bytes:path.as_bytes().to_vec()}};
    let waker=std::task::Waker::from(Arc::new(Wake)); let mut cx=std::task::Context::from_waker(&waker);
    match Box::pin(handler::process(&request)).as_mut().poll(&mut cx) {std::task::Poll::Ready(result)=>result,_=>panic!("unexpected pending")}
}
#[test] fn import_acknowledges_appdb_only_and_checks_scope_size_and_archive() {
    assert_eq!(run("common/dl/appstore/osm.bp",0x80000003),OperationState::Unit);
    assert_eq!(STATE.lock().unwrap().imports,vec!["osm.bp"]);
    assert_eq!(run("common/dl/appstore/osm.txt",0x80000003),OperationState::Failed(FS_ERR_BAD_PATH));
    assert_eq!(run("common/dl/appstore/osm.bp",3),OperationState::Failed(FS_ERR_BAD_PATH));
    STATE.lock().unwrap().scope=false;
    assert_eq!(run("common/dl/appstore/osm.bp",0x80000003),OperationState::Failed(FS_ERR_BAD_PATH));
    {let mut s=STATE.lock().unwrap();s.scope=true;s.len=513*1024*1024;}
    assert_eq!(run("common/dl/appstore/osm.bp",0x80000003),OperationState::Failed(FS_ERR_TOO_LARGE));
    {let mut s=STATE.lock().unwrap();s.len=2;s.valid=false;}
    assert_eq!(run("common/dl/appstore/osm.bp",0x80000003),OperationState::Failed(FS_ERR_BAD_PARAM));
    {let mut s=STATE.lock().unwrap();s.valid=true;s.db_error=true;}
    assert_eq!(run("common/dl/appstore/osm.bp",0x80000003),OperationState::Failed(FS_ERR_IO));
    assert_eq!(STATE.lock().unwrap().imports.len(),1);
}
'''
with tempfile.TemporaryDirectory(prefix='appstore-import-') as temp:
    path=Path(temp)
    (path/'test.rs').write_text(harness+handler+footer)
    subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'test')],check=True)
    subprocess.run([str(path/'test')],check=True)
