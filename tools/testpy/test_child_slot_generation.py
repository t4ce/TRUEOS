#!/usr/bin/env python3
"""Exercise real parent cleanup after completed compiler VM slots are reused."""
from pathlib import Path
import subprocess
import tempfile
from test_blueprint_archive_scope import function

ROOT=Path(__file__).resolve().parents[2]
HARNESS=r'''
#![allow(dead_code)]
use std::sync::Mutex as StdMutex;
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self { Self(StdMutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
struct Link { parent_vm_id:u8,parent_generation:u64,child_generation:u64,child_lifecycle_generation:u64,state:u8,parent_to_child:Vec<u8> }
const BLUEPRINT_CHILD_STATE_EXITED:u8=4;
static BLUEPRINT_CHILD_LINKS:[Mutex<Option<Link>>;4]=[const {Mutex::new(None)};4];
static LIVE:Mutex<[u64;4]>=Mutex::new([0;4]);
static STOPPED:Mutex<Vec<u8>>=Mutex::new(Vec::new());
mod r {pub mod services {
    pub mod ide_workspace_service {pub fn release_vm(_:u8,_:u64){}}
    pub mod ide_source_service {pub fn release_vm(_:u8,_:u64){}}
}}
fn clear_blueprint_child_template(_:u8){}
fn stop_matching_generation(vm:u8,expected:Option<u64>)->Result<bool,()> {
    if expected==Some(LIVE.lock()[vm as usize]) { STOPPED.lock().push(vm);Ok(true) } else {Ok(false)}
}
@PARENT@
@CLEANUP@
fn setup(state:u8,life:u64) {
    STOPPED.lock().clear();
    for slot in &BLUEPRINT_CHILD_LINKS {*slot.lock()=None;}
    *BLUEPRINT_CHILD_LINKS[1].lock()=Some(Link {parent_vm_id:0,parent_generation:2,child_generation:3,child_lifecycle_generation:life,state,parent_to_child:vec![]});
    *LIVE.lock()=[2,6,0,0];
}
#[test] fn completed_child_record_cannot_stop_a_reused_editor_slot() {
    setup(4,4);blueprint_child_lifecycle_cleanup(0,2,false);
    assert!(STOPPED.lock().is_empty());assert!(BLUEPRINT_CHILD_LINKS[1].lock().is_none());
}
#[test] fn genuine_active_child_is_stopped_with_its_lifecycle_generation() {
    setup(2,6);blueprint_child_lifecycle_cleanup(0,2,false);assert_eq!(*STOPPED.lock(),vec![1]);
}
#[test] fn a_reused_slot_is_protected_even_if_an_old_link_says_running() {
    setup(2,4);blueprint_child_lifecycle_cleanup(0,2,false);assert!(STOPPED.lock().is_empty());
}
#[test] fn old_parent_cleanup_cannot_remove_a_new_parents_child() {
    setup(2,6);blueprint_child_lifecycle_cleanup(0,1,false);
    assert!(STOPPED.lock().is_empty());assert!(BLUEPRINT_CHILD_LINKS[1].lock().is_some());
}
'''
def main():
    source=(ROOT/'src/hv/mod.rs').read_text()
    source=HARNESS.replace('@PARENT@',function(source,'fn blueprint_child_actor_is_parent(')).replace('@CLEANUP@',function(source,'fn blueprint_child_lifecycle_cleanup('))
    # The helper uses only fields shared with the production link.
    source=source.replace('&BlueprintChildLink','&Link')
    with tempfile.TemporaryDirectory(prefix='trueos-child-generation-') as temp:
        temp=Path(temp);(temp/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(temp/'test.rs'),'-o',str(temp/'tests')],check=True)
        subprocess.run([str(temp/'tests'),'--test-threads=1'],check=True)
if __name__=='__main__':main()
