#!/usr/bin/env python3
"""Validate production startup parsing, publication and path subscriptions."""
from pathlib import Path
import re, subprocess, tempfile
root=Path(__file__).resolve().parents[2]
restart=(root/'src/r/restart.rs').read_text()
items=[]
for name in ['enum ColdStartAction','struct ColdStartBlueprint','struct StartupAlias','struct ColdStartConfiguration']:
    items.append(re.search(r'^'+name+r' \{.*?^}',restart,re.M|re.S).group())
validation=re.search(r'^pub\(crate\) fn validate_startup_manifest.*?^}',restart,re.M|re.S).group()
config=(root/'src/r/startup_config.rs').read_text()
config=config[config.index('struct Configuration'):config.index('fn changed(')]
watch=(root/'src/r/fs/file_watch.rs').read_text();watch=watch[watch.index('use alloc::'):]
source='''#![allow(dead_code)]
extern crate alloc;
mod spin {pub struct Mutex<T>(std::sync::Mutex<T>);pub type MutexGuard<'a,T>=std::sync::MutexGuard<'a,T>;impl<T> Mutex<T>{pub const fn new(v:T)->Self{Self(std::sync::Mutex::new(v))}pub fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()}}}
mod disc {pub mod block {pub type DiscId=u32;}}
mod shell3 {pub mod service {pub fn notify_work(){}}}
mod r {
    pub mod restart {
        use alloc::{string::String,vec::Vec};use serde::Deserialize;
'''
source+='#[derive(Deserialize)]\n#[serde(rename_all="snake_case")]\n'+items[0]+'\n'
source+='\n'.join('#[derive(Deserialize)]\n#[serde(deny_unknown_fields)]\n'+x for x in items[1:])+'\n'+validation
source+=f'\npub fn embedded_startup_manifest()->&\'static [u8]{{include_bytes!("{root}/startup.json")}}\n}}\n'
source+='''pub mod startup_config {
use alloc::{string::String,sync::Arc,vec::Vec};use core::sync::atomic::{AtomicBool,AtomicU64,Ordering};use crate::spin;
'''+config+'''
#[test]fn one_publication_refreshes_all_consumers_and_invalid_edits_keep_last_good(){
    let first=manifest();let before=generation();assert!(aliases().contains(&"td".into()));
    let json=br#"{"aliases":[{"name":"map","blueprint":"osm.bp"}],"autostart":[]}"#;
    assert_eq!(publish(json.to_vec()),Ok(true));
    let shell_a=aliases();let shell_b=aliases();assert_eq!(shell_a,shell_b);assert_eq!(shell_a,vec!["map"]);
    assert_eq!(blueprint("map"),Some("osm.bp".into()));assert_eq!(generation(),before+1);
    assert_eq!(manifest().as_ref(),json);assert_ne!(first.as_ref(),json);
    assert_eq!(publish(json.to_vec()),Ok(false));assert_eq!(generation(),before+1);
    for invalid in [br#"{"aliases":[{"name":"Aka","blueprint":"osm"}],"autostart":[]}"#.as_slice(),br#"{"aliases":[{"name":"map","blueprint":"osm"},{"name":"map","blueprint":"img"}],"autostart":[]}"#,b"{broken"] {assert!(publish(invalid.to_vec()).is_err());assert_eq!(generation(),before+1);assert_eq!(aliases(),shell_a);}
}
}
pub mod fs {pub mod file_watch {use crate::spin;
'''+watch+'''
#[test]fn committed_path_callbacks_are_deduplicated_and_do_not_hold_registry_lock(){
    use core::sync::atomic::{AtomicUsize,Ordering};static CALLS:AtomicUsize=AtomicUsize::new(0);
    fn callback(_:u32,_:&str){CALLS.fetch_add(1,Ordering::SeqCst);subscribe("other/path",callback);}
    subscribe("trueos/startup.json",callback);subscribe("trueos/startup.json",callback);
    changed(2,"trueos/uncrypted.blob");assert_eq!(CALLS.load(Ordering::SeqCst),0);
    changed(2,"/trueos/startup.json");assert_eq!(CALLS.load(Ordering::SeqCst),1);
}
}}}
'''
with tempfile.TemporaryDirectory(prefix='trueos-startup-tests-') as temp:
    directory = Path(temp)
    (directory/'test.rs').write_text(source)
    (directory/'Cargo.toml').write_text('[package]\nname="startup-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nserde={version="1",features=["derive"]}\nserde_json="1"\n[lib]\npath="test.rs"\n')
    subprocess.run(['cargo','test','--offline','--target','x86_64-unknown-linux-gnu','--manifest-path',str(directory/'Cargo.toml')],cwd='/tmp',check=True)
