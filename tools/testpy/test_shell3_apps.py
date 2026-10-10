#!/usr/bin/env python3
"""Production app views, async handoff, mouse tags and generation-bound VM stops."""
from pathlib import Path
import os
import re
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[2]
def item(path, marker):
    return re.search(r'^'+re.escape(marker)+r'.*?^}', (ROOT/path).read_text(), re.M|re.S).group()
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_executor;
extern crate self as trueos_time;
use alloc::{string::String,vec::Vec};use spin::Mutex;
use std::{future::Future,pin::Pin,task::{Context,Poll,Waker},sync::atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering},cell::RefCell};
#[derive(Clone,Copy)]pub struct Spawner;impl Spawner {pub fn spawn<T>(&self,_:T){}}
pub struct Duration;impl Duration {pub fn from_millis(_:u64)->Self{Self}}
pub struct Timer(bool);impl Timer {pub fn after(_:Duration)->Self{Self(false)}}
impl Future for Timer {type Output=();fn poll(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<()> {if self.0 {Poll::Ready(())}else{self.0=true;Poll::Pending}}}
fn run<T>(f:impl Future<Output=T>)->T {let mut f=std::pin::pin!(f);loop {if let Poll::Ready(out)=f.as_mut().poll(&mut Context::from_waker(Waker::noop())){return out;}}}
static NOW:AtomicU64=AtomicU64::new(0);static VISIBLE:AtomicBool=AtomicBool::new(true);static LIVE:AtomicBool=AtomicBool::new(true);
static WORK:AtomicUsize=AtomicUsize::new(0);static FETCH_FAIL:AtomicBool=AtomicBool::new(false);static SNAPSHOTS:AtomicUsize=AtomicUsize::new(0);
static INPUT:Mutex<Vec<u8>>=Mutex::new(Vec::new());static WRITES:Mutex<Vec<Vec<u8>>>=Mutex::new(Vec::new());static SAVES:Mutex<Vec<String>>=Mutex::new(Vec::new());static LAUNCHES:Mutex<Vec<String>>=Mutex::new(Vec::new());
mod chronos {pub fn monotonic_nanos()->u64{crate::NOW.load(crate::Ordering::SeqCst)}}
mod workers {pub type WorkerSpawner=crate::Spawner;pub fn pick_background_spawner()->Option<WorkerSpawner>{Some(crate::Spawner)}}
mod wait {
    use super::*;thread_local!{static QUEUE:RefCell<Vec<Pin<Box<dyn Future<Output=()>>>>>=RefCell::new(Vec::new());}
    pub fn spawn_local_detached(f:impl Future<Output=()>+'static){QUEUE.with(|q|q.borrow_mut().push(Box::pin(f)));}
    pub fn drain(){loop {let f=QUEUE.with(|q|q.borrow_mut().pop());if let Some(f)=f{crate::run(f);}else{break;}}}
}
mod app_db {pub fn insert_download(name:&str,_:&[u8])->Result<(),String>{crate::SAVES.lock().push(name.into());Ok(())}}
mod shell2 {
    #[derive(Clone)]pub struct MatrixTarget;pub const OUTPUT_SYSTEM_MASK:u16=1;
    pub fn matrix_target_for_slot_name(_:u16,_:&str)->MatrixTarget{MatrixTarget}
    pub fn claim_matrix_target_for_named_app_slot(_:&MatrixTarget,_:&str,_:&str)->Option<MatrixTarget>{Some(MatrixTarget)}
    pub fn set_matrix_target_active(_:&MatrixTarget,active:bool){if active {crate::WORK.fetch_add(1,crate::Ordering::SeqCst);}else{crate::WORK.fetch_sub(1,crate::Ordering::SeqCst);}}
    pub fn matrix_target_slot_lease(_:&MatrixTarget){}
    pub fn matrix_slot_is_live(_: &())->bool{crate::LIVE.load(crate::Ordering::SeqCst)}
    pub mod cmds {pub mod run {#[derive(Clone)]pub struct QueuedBlueprint {pub slot:String}}}
    pub mod shell2_dl {
        #[derive(Clone)]pub struct OnlineApp {pub name:String,pub archive_name:String,pub sha256:String}
        pub async fn catalog()->Result<Vec<OnlineApp>,String>{Ok((0..44).map(|i|OnlineApp{name:format!("app{i}"),archive_name:format!("app{i}.bp"),sha256:"a".repeat(64)}).collect())}
        pub async fn fetch_app(_:&OnlineApp)->Result<Vec<u8>,String>{if crate::FETCH_FAIL.load(crate::Ordering::SeqCst){Err("Blueprint SHA-256 mismatch.\nRejected".into())}else{Ok(vec![1,2,3])}}
    }
    pub mod shell2_apps {@STATE_LABEL@}
}
mod transport {
    use alloc::{string::{String,ToString},vec::Vec};use core::fmt::Write;use sha2::{Digest,Sha256};
    const ONLINE_APP_HASH_SEPARATOR:&str="§§";const SHA256_HEX_LEN:usize=64;const ONLINE_APP_MAX_BYTES:usize=512*1024*1024;
    @TRANSPORT@
    async fn wait_for_online_ready()->bool{true}
    async fn fetch_url_bytes(_:String,max:usize)->Result<Vec<u8>,String>{assert_eq!(max,ONLINE_APP_MAX_BYTES);Ok(vec![1])}
    #[test]fn published_catalog_keeps_ids_names_and_hashes_for_verified_fetch(){
        let hash="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let html=format!("<li><a href=\"a.bp%C2%A7%C2%A7{hash}\">a.bp</a></li><li><a href=\"b.bp\">b.bp</a></li>");
        let apps=parse_online_apps(&html,OnlineCatalog::Apps);assert_eq!(apps.len(),2);assert_eq!(apps[0].name,"a");assert_eq!(apps[0].sha256,hash);assert_eq!(apps[1].archive_name,"b.bp");assert!(apps[0].url.starts_with("https://trueos.eu/apps/"));assert!(online_app_sha256_matches(&apps[0],b""));assert!(!online_app_sha256_matches(&apps[0],b"changed"));
        assert!(crate::run(fetch_app(&apps[0])).unwrap_err().contains("SHA-256 mismatch"));assert_eq!(crate::run(fetch_app(&apps[1])).unwrap(),vec![1]);
    }
}
mod log_os {pub fn blueprint_important_line(_:core::fmt::Arguments<'_>) {}}
mod r {pub mod pat {pub fn find_str(text:&str,pattern:&str)->Option<usize>{text.find(pattern)}}pub mod blocking {pub fn close_guest_jobs(_:u8){}pub fn guest_jobs_in_flight(_:u8)->usize{0}}}
mod hv {
    use super::*;
    @HV_STATE@
    @HV_STATUS@
    #[derive(Debug)]pub enum StopError {UnsupportedVmId}
    struct Cooperative;impl Cooperative {fn request(&self)->bool{true}}
    struct Vm {running:AtomicBool,starting:AtomicBool,stop_req:AtomicBool,run_generation:AtomicU64,lifecycle_generation:AtomicU64,lifecycle_control:Mutex<()>,cooperative_stop:Cooperative}
    static VM:Vm=Vm {running:AtomicBool::new(true),starting:AtomicBool::new(false),stop_req:AtomicBool::new(false),run_generation:AtomicU64::new(1),lifecycle_generation:AtomicU64::new(1),lifecycle_control:Mutex::new(()),cooperative_stop:Cooperative};
    pub const TRUEOS_VM_ID_LIMIT:usize=64;
    fn vm_slot(id:u8)->Option<&'static Vm>{(id==0).then_some(&VM)}
    pub fn vm_lifecycle_generation(id:u8)->Option<u64>{vm_slot(id).map(|vm|vm.lifecycle_generation.load(Ordering::SeqCst))}
    pub fn configure(run:u64,running:bool){VM.lifecycle_generation.store(run,Ordering::SeqCst);VM.running.store(running,Ordering::SeqCst);VM.stop_req.store(false,Ordering::SeqCst);}
    pub fn requested()->bool{VM.stop_req.load(Ordering::SeqCst)}
    pub fn vm_state(id:u8)->HvVmState{HvVmState{id,supported:true,running:id==0&&VM.running.load(Ordering::SeqCst),starting:false,stop_requested:id==0&&requested(),preserve_requested:false,preserve_exit:false,replicatable:false,pause_latched:false,pause_snapshot_ready:false,prepare_pause_pending:false,lifecycle_ready:false,restore_inflight:false}}
    pub fn app_vm_display_label(id:u8)->Option<String>{(id==0).then(||"voxy.bp".into())}
    pub fn status()->HvStatus{crate::SNAPSHOTS.fetch_add(1,Ordering::SeqCst);HvStatus{vendor_intel:true,has_msr:true,has_vmx:true,feature_control_locked:true,feature_control_vmx_outside_smx:true,guest_module_present:true,stored_vm_count:1,vm_id_limit:64,running_count:1,starting_count:0,active_vm_ids:[None;64],vm_shared_heap_total_bytes:3*1024*1048576,vm_shared_heap_free_bytes:2*1024*1048576,vm_shared_stack_bytes:32*1048576,vm_shared_vmx_bytes:2*1048576}}
    pub mod store {pub fn has_committed_vm(id:u8)->bool{id==0}}
    pub mod control_kick {pub enum LifecycleKickAction{Stop}}
    #[derive(Clone,Copy)]struct Wait;impl Wait{fn notify_all(&self){}}static VM_CONTROL_WAITS:[Wait;64]=[Wait;64];
    fn clear_blueprint_lifecycle_capability(_:u8){}fn nudge_vm_control(_:u8,_:control_kick::LifecycleKickAction,_:&str){}fn report_shutdown_diagnostics(_:u8){}fn hvwarnf(_:core::fmt::Arguments<'_>){}
    @STOP@
    @STOP_GENERATION@
    @STOP_MATCH@
}
mod shell3 {
    pub mod tui {
        #[derive(Clone,Copy)]pub struct Frontend {pub id:u64,pub cols:usize,pub rows:usize}
        pub struct Surface {pub cols:u32,pub rows:u32}
        pub fn native_slot(_:&str)->bool{false}pub fn request(_:Frontend,_:&str)->Result<(),&'static str>{Ok(())}
        pub fn attach_native(_:Frontend,_:&crate::shell2::MatrixTarget)->Result<(),String>{Ok(())}pub fn cancel_native_attach(_:&crate::shell2::MatrixTarget){}
        pub fn native_visible(_:&crate::shell2::MatrixTarget)->bool{crate::VISIBLE.load(crate::Ordering::SeqCst)}
        pub fn native_return(_:&crate::shell2::MatrixTarget){crate::VISIBLE.store(false,crate::Ordering::SeqCst);}
        pub fn native_write(_:&crate::shell2::MatrixTarget,bytes:&[u8]){crate::WRITES.lock().push(bytes.to_vec());}
        pub fn native_read(_:&crate::shell2::MatrixTarget)->Option<(Vec<u8>,Vec<String>)>{crate::LIVE.load(crate::Ordering::SeqCst).then(||(std::mem::take(&mut *crate::INPUT.lock()),vec![]))}
        pub fn surface(_:&crate::shell2::MatrixTarget)->Option<Surface>{Some(Surface{cols:100,rows:18})}
        pub fn native_frontend(_:&crate::shell2::MatrixTarget)->Option<Frontend>{Some(Frontend{id:2,cols:100,rows:18})}
        pub fn native_launch(_:&crate::shell2::MatrixTarget,app:crate::shell2::cmds::run::QueuedBlueprint){crate::LAUNCHES.lock().push(app.slot);crate::VISIBLE.store(false,crate::Ordering::SeqCst);}
    }
    pub mod service {
        pub fn notify_work(){}pub fn refresh_appdb_names(){}
        pub fn launch_bytes(archive:String,_:Vec<u8>,_:&str,frontend:super::tui::Frontend)->Result<crate::shell2::cmds::run::QueuedBlueprint,String>{assert_eq!(frontend.id,2);Ok(crate::shell2::cmds::run::QueuedBlueprint{slot:archive})}
    }
    #[path="@ROOT@/src/shell3/helper.rs"]mod helper;
    mod apps {
        @APPS@
        fn apps_task(_:Kind,_:crate::shell2::MatrixTarget)->Result<(),()>{Ok(())}
        #[cfg(test)]mod tests {
            use super::*;use crate::{Ordering,NOW,VISIBLE,LIVE,WORK,SAVES,LAUNCHES,WRITES,FETCH_FAIL,SNAPSHOTS,INPUT};
            fn reset(){NOW.store(0,Ordering::SeqCst);VISIBLE.store(true,Ordering::SeqCst);LIVE.store(true,Ordering::SeqCst);WORK.store(0,Ordering::SeqCst);FETCH_FAIL.store(false,Ordering::SeqCst);SNAPSHOTS.store(0,Ordering::SeqCst);SAVES.lock().clear();LAUNCHES.lock().clear();WRITES.lock().clear();INPUT.lock().clear();crate::hv::configure(1,true);}
            fn catalog()->View{let mut v=View::new(Kind::Online);v.submit(&crate::shell2::MatrixTarget,Request::Catalog,"Fetching…".into());crate::wait::drain();v.complete();v}
            fn poll(f:crate::Pin<&mut impl crate::Future>)->crate::Poll<()>{f.poll(&mut crate::Context::from_waker(crate::Waker::noop())).map(|_|())}
            #[test]fn mouse_columns_survive_fragmentation_and_ignore_releases(){
                for split in 0..=12 {let mut input=Input::default();let report=b"\x1b[<0;5;5M";let split=split.min(report.len());let mut actions=input.feed_cells(&report[..split],0);actions.extend(input.feed_cells(&report[split..],1));assert_eq!(actions,vec![(Action::Click(4),Some(4))]);assert!(input.feed_cells(b"\x1b[<0;5;5m",2).is_empty());}
            }
            #[test]fn download_tag_saves_without_launch_and_row_click_launches(){
                reset();let target=crate::shell2::MatrixTarget;let mut v=catalog();v.action(Action::Click(4),Some(4),&target,18);assert_eq!(WORK.load(Ordering::SeqCst),1);crate::wait::drain();v.complete();assert_eq!(*SAVES.lock(),vec!["app0.bp"]);assert!(v.ready.is_none());assert!(LAUNCHES.lock().is_empty());assert!(v.result.contains("Saved app.db:"));assert_eq!(WORK.load(Ordering::SeqCst),0);
                v.action(Action::Click(5),Some(14),&target,18);crate::wait::drain();v.complete();assert_eq!(v.ready.as_ref().unwrap().0.name,"app1");assert_eq!(SAVES.lock().len(),1);
            }
            #[test]fn keyboard_download_toolbar_and_click_tags_match_painted_columns(){
                reset();let mut v=catalog();let t=crate::shell2::MatrixTarget;for _ in 0..3{v.action(Action::Up,None,&t,18);}assert_eq!(v.selected,1);v.action(Action::Choose,None,&t,18);crate::wait::drain();v.complete();assert_eq!(SAVES.lock().len(),1);
                v.paint(&t,100,18,0);let mut terminal=trueos_terminal::Terminal::new(100,18);for bytes in WRITES.lock().drain(..){terminal.feed(&bytes);}let text=&terminal.render_rows()[2];assert!(text.contains("[dl]"));let col=text.find("[dl]").unwrap();v.action(Action::Click(2),Some(col),&t,18);crate::wait::drain();v.complete();assert_eq!(SAVES.lock().len(),2);
            }
            #[test]fn catalog_errors_and_closed_slots_do_not_save_or_launch(){
                reset();let mut v=catalog();FETCH_FAIL.store(true,Ordering::SeqCst);v.fetch_selected(&crate::shell2::MatrixTarget,false);crate::wait::drain();v.complete();assert!(!v.result.contains('\n'));assert!(v.result.contains("SHA-256"));assert!(SAVES.lock().is_empty());
                FETCH_FAIL.store(false,Ordering::SeqCst);v.fetch_selected(&crate::shell2::MatrixTarget,true);LIVE.store(false,Ordering::SeqCst);crate::wait::drain();v.complete();assert!(v.ready.is_none());assert_eq!(WORK.load(Ordering::SeqCst),0);
            }
            #[test]fn async_launch_waits_for_reentry_and_parking_keeps_the_slot(){
                reset();let mut task=std::pin::pin!(apps_task_run(Kind::Online,crate::shell2::MatrixTarget));assert!(poll(task.as_mut()).is_pending());crate::wait::drain();assert!(poll(task.as_mut()).is_pending());INPUT.lock().extend_from_slice(b"\r");assert!(poll(task.as_mut()).is_pending());assert_eq!(WORK.load(Ordering::SeqCst),1);INPUT.lock().extend_from_slice(b"q");assert!(poll(task.as_mut()).is_pending());assert!(!VISIBLE.load(Ordering::SeqCst));crate::wait::drain();assert!(poll(task.as_mut()).is_pending());assert!(LAUNCHES.lock().is_empty());assert!(LIVE.load(Ordering::SeqCst));VISIBLE.store(true,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(*LAUNCHES.lock(),vec!["app0.bp"]);LIVE.store(false,Ordering::SeqCst);assert!(poll(task.as_mut()).is_ready());
            }
            #[test]fn status_samples_250ms_and_paints_only_changed_cells(){
                reset();let mut v=View::new(Kind::Status);v.refresh(0);v.paint(&crate::shell2::MatrixTarget,100,18,0);assert_eq!(v.vms.len(),5);assert!(v.vms[0].stoppable);assert!(!v.vms[1].stoppable);assert!(v.vms[0].stored);v.refresh(249_999_999);assert_eq!(SNAPSHOTS.load(Ordering::SeqCst),1);v.paint(&crate::shell2::MatrixTarget,100,18,0);assert_eq!(WRITES.lock().len(),1);v.refresh(250_000_000);assert_eq!(SNAPSHOTS.load(Ordering::SeqCst),2);
                crate::hv::configure(1,false);v.refresh(500_000_000);v.paint(&crate::shell2::MatrixTarget,100,18,500_000_000);let diff=WRITES.lock().last().unwrap().clone();assert!(!String::from_utf8(diff).unwrap().contains("2J"));
                for rows in 0..10 {v.paint(&crate::shell2::MatrixTarget,20,rows,0);}
            }
            #[test]fn production_stop_rejects_recycled_vm_generation_under_control_lock(){
                reset();crate::hv::configure(2,true);assert!(!crate::hv::stop_for_generation(0,1).unwrap());assert!(!crate::hv::requested());assert!(crate::hv::stop_for_generation(0,2).unwrap());assert!(crate::hv::requested());assert!(crate::hv::stop_for_generation(1,2).is_err());crate::hv::configure(3,false);assert!(!crate::hv::stop_for_generation(0,3).unwrap());
            }
            #[test]fn status_stop_uses_displayed_generation_and_waits_for_cleanup(){
                reset();let mut v=View::new(Kind::Status);v.refresh(0);crate::hv::configure(2,true);v.action(Action::Choose,None,&crate::shell2::MatrixTarget,18);crate::wait::drain();v.complete();assert!(!crate::hv::requested());assert!(v.result.contains("no longer"));
                v.refresh(250_000_000);let reply=Arc::new(Mutex::new(Job{progress:String::new(),result:None}));let mut stop=std::pin::pin!(Request::Stop(vec![(0,2)]).run(&crate::shell2::MatrixTarget,&reply));assert!(poll(stop.as_mut()).is_pending());assert!(crate::hv::requested());crate::hv::configure(2,false);assert!(poll(stop.as_mut()).is_ready());
            }
            #[test]fn stop_wait_timeout_and_offline_rows_are_clear(){
                reset();let mut v=View::new(Kind::Status);v.refresh(0);v.action(Action::Click(8),Some(4),&crate::shell2::MatrixTarget,18);assert!(v.pending.is_none());assert!(v.result.contains("No running"));
                let reply=Arc::new(Mutex::new(Job{progress:String::new(),result:None}));let mut stop=std::pin::pin!(Request::Stop(vec![(0,1)]).run(&crate::shell2::MatrixTarget,&reply));assert!(poll(stop.as_mut()).is_pending());NOW.store(45_000_000_000,Ordering::SeqCst);match stop.as_mut().poll(&mut crate::Context::from_waker(crate::Waker::noop())){crate::Poll::Ready(Err(err))=>assert!(err.contains("still waiting")),_=>panic!("expected timeout")}
            }
            #[test]fn stop_toolbar_uses_the_last_selected_vm(){
                reset();let mut v=View::new(Kind::Status);v.refresh(0);v.selected=0;v.action(Action::Choose,None,&crate::shell2::MatrixTarget,18);assert!(v.pending.is_some());assert_eq!(WORK.load(Ordering::SeqCst),1);LIVE.store(false,Ordering::SeqCst);crate::wait::drain();v.complete();assert_eq!(WORK.load(Ordering::SeqCst),0);
            }
            #[test]fn status_parks_without_sampling_and_refreshes_on_reentry(){
                reset();let mut task=std::pin::pin!(apps_task_run(Kind::Status,crate::shell2::MatrixTarget));assert!(poll(task.as_mut()).is_pending());assert_eq!(SNAPSHOTS.load(Ordering::SeqCst),1);INPUT.lock().extend_from_slice(b"q");assert!(poll(task.as_mut()).is_pending());NOW.store(1_000_000_000,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(SNAPSHOTS.load(Ordering::SeqCst),1);VISIBLE.store(true,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(SNAPSHOTS.load(Ordering::SeqCst),2);LIVE.store(false,Ordering::SeqCst);assert!(poll(task.as_mut()).is_ready());
            }
            #[test]fn scrolling_and_toolbar_return_work_with_mouse_and_keyboard(){
                reset();let mut v=catalog();let t=crate::shell2::MatrixTarget;for _ in 0..60 {v.action(Action::Down,None,&t,18);}v.paint(&t,100,18,0);assert_eq!(v.app,43);assert_eq!(v.scroll,32);v.selected=3;v.action(Action::Choose,None,&t,18);assert!(!VISIBLE.load(Ordering::SeqCst));
            }
        }
    }
}
'''
def main():
    apps=(ROOT/'src/shell3/apps.rs').read_text()
    apps=re.sub(r'^//!.*\n|^#\[trueos_executor::task[^\n]*\n','',apps,flags=re.M).replace('async fn apps_task(','async fn apps_task_run(')
    code=HARNESS.replace('@ROOT@',str(ROOT)).replace('@APPS@',apps)
    for key,path,marker in [('HV_STATE','src/hv/mod.rs','pub struct HvVmState {'),('HV_STATUS','src/hv/mod.rs','pub struct HvStatus {'),('STATE_LABEL','src/shell2/shell2_apps.rs','pub(crate) fn vm_state_label('),('STOP','src/hv/mod.rs','pub fn stop('),('STOP_GENERATION','src/hv/mod.rs','pub(crate) fn stop_for_generation('),('STOP_MATCH','src/hv/mod.rs','fn stop_matching_generation(')]:
        value=item(path,marker)
        if key=='HV_STATE':value='#[derive(Clone,Copy)]\n'+value
        code=code.replace('@'+key+'@',value)
    transport='#[derive(Clone)]\n'+item('src/shell2/shell2_dl.rs','pub(crate) struct OnlineApp {')+'\n#[derive(Clone,Copy)]\n'+item('src/shell2/shell2_dl.rs','enum OnlineCatalog {')+'\n'+item('src/shell2/shell2_dl.rs','impl OnlineCatalog {')
    for marker in ['fn absolutize_online_url(', 'fn parse_attr_value', 'fn published_app_name_parts(', 'fn hex_nibble(', 'fn percent_decode(', 'fn published_link_parts(', 'fn clean_archive_name(', 'fn parse_online_apps(', 'fn trim_bp_suffix(', 'fn online_app_sha256_matches(', 'pub(crate) async fn fetch_app(']:
        transport+='\n'+item('src/shell2/shell2_dl.rs',marker)
    code=code.replace('@TRANSPORT@',transport)
    with tempfile.TemporaryDirectory(prefix='shell3-apps-') as tmp:
        tmp=Path(tmp);(tmp/'src').mkdir();(tmp/'src/lib.rs').write_text(code)
        (tmp/'Cargo.toml').write_text(f'''[package]
name="shell3-apps-host"
version="0.1.0"
edition="2024"
[dependencies]
spin="0.10"
sha2="0.10"
trueos-terminal={{path="{ROOT}/crates/trueos-terminal"}}
''')
        env=dict(os.environ,CARGO_TARGET_DIR=str(ROOT/'tgt/apps-host-tests'))
        subprocess.run(['cargo','test','--offline','--quiet','--manifest-path',str(tmp/'Cargo.toml'),'--','--test-threads=1'],env=env,cwd=tmp,check=True)
if __name__=='__main__':main()
