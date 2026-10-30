#!/usr/bin/env python3
"""Run the production source service and lease registry against atomic host storage."""
import json
from pathlib import Path
import subprocess
import tempfile
from test_ide_workspace_transport import HARNESS, ROOT

EXTRA = r'''
mod wait {
    pub struct EmbassySpinRawMutex(crate::Mutex<()>);
    unsafe impl embassy_sync::blocking_mutex::raw::RawMutex for EmbassySpinRawMutex {
        const INIT: Self = Self(crate::Mutex::new(()));
        fn lock<R>(&self, call: impl FnOnce() -> R) -> R { let _guard = self.0.lock(); call() }
    }
}
#[path = @STORE_SOURCE@] mod ide_source_service;
pub mod fake_fs {
    use std::{collections::BTreeMap, sync::{Mutex, atomic::{AtomicBool, Ordering}}};
    pub static FILES: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
    pub static FAIL: AtomicBool = AtomicBool::new(false);
    pub struct Info { pub data_len: u64 }
    pub async fn file_info_async(_: u8, path: &str) -> Result<Option<Info>, ()> {
        Ok(FILES.lock().unwrap().get(path).map(|bytes| Info { data_len: bytes.len() as u64 }))
    }
    pub async fn file_out_async(_: u8, path: &str) -> Result<Option<Vec<u8>>, ()> { Ok(FILES.lock().unwrap().get(path).cloned()) }
    pub async fn dir_create_all_async(_: u8, _: &str) -> Result<bool, ()> { Ok(true) }
    pub async fn file_in_async(_: u8, path: &str, bytes: &[u8]) -> Result<bool, ()> {
        if FAIL.swap(false, Ordering::SeqCst) { return Err(()); }
        FILES.lock().unwrap().insert(path.into(), bytes.to_vec()); Ok(true)
    }
}
fn block_on<T>(future: impl std::future::Future<Output=T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(value) => value,
        _ => panic!("host fixture unexpectedly blocked"),
    }
}
use trueos_ide::{store_protocol::{Command, Request, Reply}, SourceFileInput};
fn register(vm: u32, database: &str, project: &str) {
    ide_workspace_service::write_for_owner(0x8000_0000 | vm, "vFile:ide/register",
        &serde_json::to_vec(&serde_json::json!({"version":1,"database":database,"project":project})).unwrap()).unwrap();
}
fn call(vm: u32, database: &str, command: Command) -> Reply {
    let owner = 0x8000_0000 | vm;
    let bytes = serde_json::to_vec(&Request { database: database.into(), command }).unwrap();
    block_on(ide_source_service::write_for_owner(owner, &bytes)).unwrap();
    serde_json::from_slice(&ide_source_service::read_for_owner(owner).unwrap()).unwrap()
}
fn create(vm: u32, db: &str, name: &str, text: &str) -> i64 {
    match call(vm, db, Command::Create { project:name.into(), files:vec![SourceFileInput::new("main.odin",text)] }) {
        Reply::Saved { revision } => revision.id, _ => panic!("create failed"),
    }
}
fn source(vm: u32, db: &str, name: &str) -> trueos_ide::LoadedSource {
    match call(vm, db, Command::Get { project:name.into(), revision:None }) {
        Reply::Source { source:Some(source) } => source, _ => panic!("read failed"),
    }
}

#[test]
fn two_editors_save_independent_projects_in_one_durable_image() {
    let db = "/apps/common/ide/source-service-multi.redb";
    register(4, db, "A"); register(5, db, "B");
    let a = create(4, db, "A", "A1"); let b = create(5, db, "B", "B1");
    assert_ne!(a,b);
    let saved = call(4, db, Command::Save { project:"A".into(), expected_revision_id:a, files:vec![SourceFileInput::new("main.odin","A2")] });
    assert!(matches!(saved, Reply::Saved { .. }));
    assert_eq!(source(5,db,"B").revision.id,b);
    assert_eq!(source(5,db,"B").files[0].bytes,b"B1");
    let image = fake_fs::FILES.lock().unwrap()[&db[1..]].clone();
    let image = trueos_ide::source_image::SourceImage::open(&image).unwrap();
    image.database.verify().unwrap();
    assert_eq!(image.database.list_projects().unwrap().len(),2);
    assert_eq!(image.database.get("A",None).unwrap().unwrap().files[0].bytes,b"A2");
    assert_eq!(image.database.get("B",None).unwrap().unwrap().files[0].bytes,b"B1");
    ide_workspace_service::write_for_owner(0x8000_0004,"vFile:ide/unregister",&[]).unwrap();
    assert!(matches!(call(5, db, Command::Save { project:"B".into(), expected_revision_id:b, files:vec![SourceFileInput::new("main.odin","B2")] }),Reply::Saved {..}));
    register(6,db,"A"); assert_eq!(source(6,db,"A").files[0].bytes,b"A2");
}

#[test]
fn foreign_project_writes_and_failed_persistence_do_not_advance_sources() {
    let db = "/apps/common/ide/source-service-guards.redb";
    register(7,db,"owned"); let revision=create(7,db,"owned","original");
    assert!(matches!(call(8,db,Command::Save { project:"owned".into(),expected_revision_id:revision,files:vec![SourceFileInput::new("main.odin","foreign")] }),Reply::Error {code,..} if code=="not_owner"));
    fake_fs::FAIL.store(true,std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(call(7,db,Command::Save { project:"owned".into(),expected_revision_id:revision,files:vec![SourceFileInput::new("main.odin","not durable")] }),Reply::Error {..}));
    let current=source(7,db,"owned"); assert_eq!(current.revision.id,revision); assert_eq!(current.files[0].bytes,b"original");
    assert!(matches!(call(7,db,Command::Save { project:"owned".into(),expected_revision_id:revision,files:vec![SourceFileInput::new("main.odin","durable")] }),Reply::Saved {..}));
    assert!(matches!(call(7,db,Command::Save { project:"owned".into(),expected_revision_id:revision,files:vec![SourceFileInput::new("main.odin","stale")] }),Reply::Error {..}));
    assert_eq!(source(7,db,"owned").files[0].bytes,b"durable");
}

#[test]
fn source_service_coordinates_with_legacy_image_locks_and_cleans_old_replies() {
    let db="/apps/common/ide/source-service-legacy.redb";
    let owner=crate::r::fs::advisory_lock::Owner {realm:99,description:1};
    assert!(crate::r::fs::advisory_lock::LOCKS.lock().try_set(1,&db[1..],owner,Some(true)));
    assert!(matches!(call(9,db,Command::List {}),Reply::Error {code,..} if code=="store_busy"));
    crate::r::fs::advisory_lock::LOCKS.lock().release(owner);
    assert!(matches!(call(9,db,Command::List {}),Reply::Projects {projects} if projects.is_empty()));
    assert!(!fake_fs::FILES.lock().unwrap().contains_key(&db[1..]));
    let request=serde_json::to_vec(&Request {database:db.into(),command:Command::List {}}).unwrap();
    block_on(ide_source_service::write_for_owner(0x8000_0009,&request)).unwrap();
    assert_eq!(block_on(ide_source_service::write_for_owner(0x8000_0009,&request)),Err(crate::r::cabi_codes::FS_ERR_ALREADY_EXISTS));
    ide_source_service::release_vm(9,0);
    assert!(ide_source_service::read_for_owner(0x8000_0009).is_ok());
    block_on(ide_source_service::write_for_owner(0x8000_0009,&request)).unwrap();
    ide_source_service::release_vm(9,1);
    assert_eq!(ide_source_service::read_for_owner(0x8000_0009).err(),Some(crate::r::cabi_codes::FS_ERR_NOT_FOUND));
}

#[test]
fn existing_node_image_keeps_all_projects_sources_and_revisions() {
    let Ok(path)=std::env::var("TRUEOS_IDE_SOURCE_IMAGE") else { return; };
    let bytes=std::fs::read(path).unwrap();
    let image=trueos_ide::source_image::SourceImage::open(&bytes).unwrap();
    let projects=image.database.list_projects().unwrap();
    assert!(!projects.is_empty());
    let sources:Vec<_>=projects.iter().map(|project| image.database.get(&project.name,None).unwrap().unwrap()).collect();
    let bytes=image.into_bytes().unwrap();
    let image=trueos_ide::source_image::SourceImage::open(&bytes).unwrap();
    assert_eq!(image.database.list_projects().unwrap(),projects);
    for source in sources {
        assert_eq!(image.database.get(&source.revision.project,None).unwrap(),Some(source));
    }
}
'''

def main():
    source = HARNESS.split("mod async_fs_adapter {")[0]
    source = source.replace('mod trueos_ide { pub mod store_protocol { pub const REPLY: &str = "vFile:ide/store-reply"; } }', '')
    source = source.replace('mod r {', 'mod r { pub mod fs { pub use crate::fake_fs as trueosfs; #[path = '+json.dumps(str(ROOT/'src/r/fs/advisory_lock.rs'))+'] pub(crate) mod advisory_lock; }')
    start = source.index('        pub mod ide_source_service {')
    stop = source.index('\n        }',start)+len('\n        }')
    source = source[:start]+'        pub(crate) use crate::ide_source_service;'+source[stop:]
    source = source.replace('pub mod async_fs_cabi {','pub mod async_fs_cabi { pub fn selected_disk(path:&str)->Result<(u8,&str),i32> { Ok((1,path)) }')
    source += EXTRA
    source = source.replace('@SOURCE@',json.dumps(str(ROOT/'src/r/services/ide_workspace_service.rs'))).replace('@CABI_CODES@',json.dumps(str(ROOT/'src/r/cabi_codes.rs'))).replace('@STORE_SOURCE@',json.dumps(str(ROOT/'src/r/services/ide_source_service.rs')))
    with tempfile.TemporaryDirectory(prefix='trueos-source-service-') as temp:
        temp=Path(temp)
        (temp/'harness.rs').write_text(source)
        (temp/'Cargo.toml').write_text('''[package]
name="source-service-tests"
version="0.1.0"
edition="2024"
[workspace]
[lib]
path="harness.rs"
[dependencies]
serde={version="1.0",features=["derive"]}
serde_json="1.0"
embassy-sync="0.8.0"
trueos-ide={path='''+json.dumps(str(ROOT.parent/'TRUEOS-Blueprints/crates/trueos-ide'))+'}\n')
        subprocess.run(['cargo','+nightly-2026-07-10','test','--offline','--target','x86_64-unknown-linux-gnu','--','--test-threads=1'],cwd=temp,check=True)

if __name__=='__main__': main()
