#!/usr/bin/env python3
"""Host checks for retained VM selection, execution controls, and status."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    lifecycle = (ROOT / 'src/shell3/lifecycle.rs').read_text()
    lifecycle = re.sub(r'^//!.*\n|^#\[trueos_executor::task[^\n]*\n', '', lifecycle, flags=re.M)
    lifecycle = lifecycle.replace('async fn lifecycle_task(', 'async fn lifecycle_task_run(')
    source = r'''#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::sync::{Mutex,atomic::{AtomicBool,Ordering}};
use std::{future::Future,pin::Pin,task::{Context,Poll,Waker}};
#[derive(Clone,Copy)] pub struct Duration;
impl Duration {pub fn from_millis(_:u64)->Self{Self}}
pub struct Timer(bool);
impl Timer {pub fn after(_:Duration)->Self{Self(false)}}
impl Future for Timer {type Output=();fn poll(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<()> {if self.0 {Poll::Ready(())}else{self.0=true;Poll::Pending}}}
fn poll(f:Pin<&mut impl Future<Output=()>>)->Poll<()> {f.poll(&mut Context::from_waker(Waker::noop()))}
static CALLS:Mutex<Vec<String>>=Mutex::new(Vec::new());
static LIVE:AtomicBool=AtomicBool::new(true);
static RACE:AtomicBool=AtomicBool::new(false);
static INPUT:Mutex<Vec<u8>>=Mutex::new(Vec::new());
static FRAMES:Mutex<Vec<Vec<String>>>=Mutex::new(Vec::new());
mod chronos {pub fn monotonic_nanos()->u64{0}}
mod hv {
    @VM_STATE@
    @HV_STATUS@
    pub fn vm_state(id:u8)->HvVmState {HvVmState{id,supported:true,running:id==0,starting:id==1,stop_requested:false,pause_requested:id==0,paused:id==0,preserve_requested:false,preserve_exit:false,replicatable:false,pause_latched:false,pause_snapshot_ready:false,prepare_pause_pending:false,lifecycle_ready:false,restore_inflight:false}}
    pub fn status()->HvStatus {HvStatus {vendor_intel:true,has_msr:true,has_vmx:true,feature_control_locked:true,feature_control_vmx_outside_smx:true,guest_module_present:true,stored_vm_count:99,vm_id_limit:3,running_count:0,paused_count:1,starting_count:1,active_vm_ids:[Some(0),Some(1),None],vm_shared_heap_total_bytes:1,vm_shared_heap_free_bytes:1,vm_shared_stack_bytes:1,vm_shared_vmx_bytes:1}}
    pub fn app_vm_display_label(id:u8)->Option<String>{(id<2).then(||format!("app{id}.bp"))}
    pub fn blueprint_console_target(id:u8)->Option<crate::shell3::MatrixTarget>{(id<2).then(||crate::shell3::MatrixTarget(crate::shell3::MatrixSlotLease::from_identity(format!("app{id}"),10+id as u64)))}
    pub mod matrix {
        pub fn vm_for_lease(lease:&crate::shell3::MatrixSlotLease)->Option<u8> {for id in 0..2 {if lease.name()==format!("app{id}") && lease.lifetime_generation()==10+id as u64{return Some(id);}}None}
    }
    fn control(action:&str,id:u8,lease:&crate::shell3::MatrixSlotLease)->Result<bool,&'static str> {
        if crate::RACE.load(crate::Ordering::SeqCst){return Ok(false);}
        crate::CALLS.lock().unwrap().push(format!("{action}:{id}:{}:{}",lease.name(),lease.lifetime_generation()));Ok(true)
    }
    pub fn stop_for_matrix_slot(id:u8,lease:&crate::shell3::MatrixSlotLease)->Result<bool,&'static str>{control("stop",id,lease)}
    pub fn pause_for_matrix_slot(id:u8,lease:&crate::shell3::MatrixSlotLease)->Result<bool,&'static str>{control("pause",id,lease)}
    pub fn resume_for_matrix_slot(id:u8,lease:&crate::shell3::MatrixSlotLease)->Result<bool,&'static str>{control("resume",id,lease)}
    pub fn kill_for_matrix_slot(id:u8,lease:&crate::shell3::MatrixSlotLease)->Result<bool,&'static str>{control("kill",id,lease)}
}
mod shell3 {
    #[derive(Clone)] pub struct MatrixSlotLease {name:String,generation:u64}
    impl MatrixSlotLease {pub fn from_identity(name:String,generation:u64)->Self{Self{name,generation}}pub fn name(&self)->&str{&self.name}pub fn lifetime_generation(&self)->u64{self.generation}}
    #[derive(Clone)] pub struct MatrixTarget(pub MatrixSlotLease);
    pub fn matrix_target_slot_lease(target:&MatrixTarget)->MatrixSlotLease{target.0.clone()}
    pub mod tui {
        #[derive(Clone,Copy)] pub struct Frontend;
        pub struct Surface {pub rows:u32,pub cols:u32}
        pub fn cancel_native_attach(_:&super::MatrixTarget){}
        pub fn native_read(_:&super::MatrixTarget)->Option<(Vec<u8>,())>{crate::LIVE.load(crate::Ordering::SeqCst).then(||(std::mem::take(&mut *crate::INPUT.lock().unwrap()),()))}
        pub fn native_visible(_:&super::MatrixTarget)->bool{true}
        pub fn surface(_:&super::MatrixTarget)->Option<Surface>{Some(Surface{cols:80,rows:10})}
        pub fn native_return(_:&super::MatrixTarget){crate::LIVE.store(false,crate::Ordering::SeqCst);}
        pub fn native_frontend(_:&super::MatrixTarget)->Option<Frontend>{Some(Frontend)}
        pub fn native_launch(_:&super::MatrixTarget,app:String){crate::CALLS.lock().unwrap().push(format!("launch:{app}"));crate::LIVE.store(false,crate::Ordering::SeqCst);}
    }
    pub mod helper {
        #[derive(Clone,Copy,PartialEq,Eq)] pub enum Action {Up,Down,Choose,Quit,Click(usize)}
        #[derive(Default)]pub struct Input;
        impl Input {pub fn in_sequence(&self)->bool{false}pub fn timeout(&mut self,_:u64)->Option<Action>{None}pub fn feed(&mut self,bytes:&[u8],_:u64)->Vec<Action>{bytes.iter().filter_map(|byte|match byte{b'\r'=>Some(Action::Choose),b'q'=>Some(Action::Quit),b'j'=>Some(Action::Down),_=>None}).collect()}}
        #[derive(Default)]pub struct Screen;
        impl Screen {pub fn paint(&mut self,_:&super::MatrixTarget,lines:&[String],_:usize,_:usize){crate::FRAMES.lock().unwrap().push(lines.into());}}
        pub struct Spawner;impl Spawner{pub fn spawn<T>(&self,_:T){}}
        pub fn admit(_: &str,_:super::tui::Frontend)->Result<Option<(super::MatrixTarget,Spawner)>,String>{Ok(None)}
    }
    #[path="@ROOT@/src/shell3/system/picker.rs"] mod picker;
    pub mod service {
        pub fn appdb_names_snapshot()->(u64,Vec<String>){(1,vec!["one".into(),"appstore".into()])}
        pub fn launch_appdb(name:&str,_:super::tui::Frontend)->Result<String,String>{crate::CALLS.lock().unwrap().push(format!("start:{name}"));Ok(name.into())}
        @STATUS_LINES@
        @STATE_PHASE@
    }
    mod lifecycle {
        @LIFECYCLE@
        fn lifecycle_task(_:String,_:MatrixTarget)->Result<(),()>{Ok(())}
        fn reset(){crate::CALLS.lock().unwrap().clear();crate::FRAMES.lock().unwrap().clear();crate::INPUT.lock().unwrap().clear();crate::LIVE.store(true,crate::Ordering::SeqCst);crate::RACE.store(false,crate::Ordering::SeqCst);}
        #[test]fn accepted_commands_and_legacy_commands_are_distinct(){for name in ["start","stop","pause","resume","kill"]{assert!(recognizes(name));}for name in ["load","store","snap","preserve","eject","delete","kick"]{assert!(!recognizes(name));}}
        #[test]fn controls_use_the_exact_slot_lifetime_without_deleting_it(){reset();let lease=MatrixSlotLease::from_identity("app0".into(),10);for action in ["pause","resume","stop","kill"]{assert!(control(action,&lease).contains("accepted"));}assert_eq!(*crate::CALLS.lock().unwrap(),vec!["pause:0:app0:10","resume:0:app0:10","stop:0:app0:10","kill:0:app0:10"]);assert!(crate::LIVE.load(crate::Ordering::SeqCst));}
        #[test]fn expired_or_recycled_slots_cannot_control_another_vm(){reset();assert!(control("kill",&MatrixSlotLease::from_identity("app0".into(),9)).contains("no longer"));crate::RACE.store(true,crate::Ordering::SeqCst);assert!(control("pause",&MatrixSlotLease::from_identity("app0".into(),10)).contains("state changed"));assert!(crate::CALLS.lock().unwrap().is_empty());}
        #[test]fn paused_vm_stays_selectable_and_start_lists_appdb(){let entries=choices("resume");assert_eq!(entries.len(),2);assert!(entries[0].0.contains("paused"));assert!(entries[1].0.contains("starting"));assert_eq!(choices("start").iter().map(|(label,_)|label.as_str()).collect::<Vec<_>>(),vec!["one","appstore"]);}
        #[test]fn picker_pause_and_resume_control_selected_retained_vm(){for action in ["pause","resume"]{reset();crate::INPUT.lock().unwrap().push(b'\r');let mut task=std::pin::pin!(lifecycle_task_run(action.into(),MatrixTarget(MatrixSlotLease::from_identity(action.into(),1))));assert!(crate::poll(task.as_mut()).is_pending());assert_eq!(*crate::CALLS.lock().unwrap(),vec![format!("{action}:0:app0:10")]);crate::LIVE.store(false,crate::Ordering::SeqCst);assert!(crate::poll(task.as_mut()).is_ready());}}
        #[test]fn picker_start_launches_selected_app_through_existing_pipeline(){reset();crate::INPUT.lock().unwrap().extend_from_slice(b"j\r");let mut task=std::pin::pin!(lifecycle_task_run("start".into(),MatrixTarget(MatrixSlotLease::from_identity("start".into(),1))));assert!(crate::poll(task.as_mut()).is_pending());assert_eq!(*crate::CALLS.lock().unwrap(),vec!["start:appstore","launch:appstore"]);assert!(crate::poll(task.as_mut()).is_ready());}
        #[test]fn status_counts_paused_ownership_without_snapshot_store_dependency(){let lines=service::hv_status_lines();assert_eq!(lines[0],"VMs: running=0 paused=1 starting=1 limit=3");assert_eq!(lines[3],"vmid blueprint state");assert_eq!(lines[4],"0 app0.bp paused");assert_eq!(lines[5],"1 app1.bp starting");assert_eq!(lines.len(),6);assert!(!lines.iter().any(|line|line.contains("saved")||line.contains("snapshot")));}
        #[test]fn status_distinguishes_pausing_and_paused_before_running(){let mut state=crate::hv::vm_state(0);assert_eq!(service::hv_vm_phase(&state),"paused");state.paused=false;assert_eq!(service::hv_vm_phase(&state),"pausing");state.pause_requested=false;assert_eq!(service::hv_vm_phase(&state),"running");state.stop_requested=true;assert_eq!(service::hv_vm_phase(&state),"stopping");}
    }
}
'''
    fields = [('VM_STATE', 'src/hv/mod.rs', 'HvVmState'), ('HV_STATUS', 'src/hv/mod.rs', 'HvStatus'),
              ('STATUS_LINES', 'src/shell3/service.rs', 'hv_status_lines'), ('STATE_PHASE', 'src/shell3/service.rs', 'hv_vm_phase')]
    for key, path, name in fields:
        value = extract.item(path, name).replace('TRUEOS_VM_ID_LIMIT', '3')
        value = re.sub(r'#\[expect\(dead_code[^\n]*\n', '', value)
        source = source.replace('@'+key+'@', value)
    source = source.replace('@ROOT@', str(ROOT)).replace('@LIFECYCLE@', lifecycle)
    with tempfile.TemporaryDirectory(prefix='shell3-lifecycle-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
