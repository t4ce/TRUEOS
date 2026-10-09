#!/usr/bin/env python3
"""Exercise production helper painting and monitor timing with the real terminal."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_executor;
extern crate self as trueos_time;
use std::sync::{Mutex,atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering}};
use std::{future::Future,pin::Pin,task::{Context,Poll,Waker}};
#[derive(Clone,Copy)] pub struct Spawner;
impl Spawner {pub fn spawn<T>(&self,_:T){}}
pub struct Duration;
impl Duration {pub fn from_millis(_:u64)->Self{Self}}
pub struct Timer(bool);
impl Timer {pub fn after(_:Duration)->Self{Self(false)}}
impl Future for Timer {type Output=();fn poll(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<()> {if self.0 {Poll::Ready(())}else{self.0=true;Poll::Pending}}}
static NOW:AtomicU64=AtomicU64::new(0);
mod chronos {pub fn monotonic_nanos()->u64{crate::NOW.load(crate::Ordering::SeqCst)}}
mod workers {pub type WorkerSpawner=crate::Spawner;pub fn pick_background_spawner()->Option<WorkerSpawner>{Some(crate::Spawner)}}
static CLAIMS:AtomicUsize=AtomicUsize::new(0);
static MODULES:AtomicUsize=AtomicUsize::new(0);
static RAM:AtomicUsize=AtomicUsize::new(0);
static SMP:AtomicUsize=AtomicUsize::new(0);
static VALUE:AtomicUsize=AtomicUsize::new(1);
static LIVE:AtomicBool=AtomicBool::new(true);
static VISIBLE:AtomicBool=AtomicBool::new(true);
static SLOT:AtomicBool=AtomicBool::new(false);
static REQUESTS:AtomicUsize=AtomicUsize::new(0);
static GEOMETRY:Mutex<(usize,usize)>=Mutex::new((100,12));
static INPUT:Mutex<Vec<u8>>=Mutex::new(Vec::new());
static WRITES:Mutex<Vec<Vec<u8>>>=Mutex::new(Vec::new());
mod shell2 {
    #[derive(Clone)]pub struct MatrixTarget;
    pub const OUTPUT_SYSTEM_MASK:u16=1;
    pub fn matrix_target_for_slot_name(_:u16,_:&str)->MatrixTarget{MatrixTarget}
    pub fn claim_matrix_target_for_named_app_slot(_:&MatrixTarget,_:&str,_:&str)->Option<MatrixTarget>{crate::CLAIMS.fetch_add(1,crate::Ordering::SeqCst);Some(MatrixTarget)}
    pub fn set_matrix_target_active(_:&MatrixTarget,_:bool){}
    pub mod cmds {
        pub mod ram {
            pub fn memory_modules_snapshot(_:usize)->Vec<String>{crate::MODULES.fetch_add(1,crate::Ordering::SeqCst);vec!["DIMM: 16 GiB".into()]}
            pub fn usage_snapshot(_:usize)->Vec<String>{crate::RAM.fetch_add(1,crate::Ordering::SeqCst);vec![format!("Used: {} GiB",crate::VALUE.load(crate::Ordering::SeqCst))]}
        }
        pub mod smp {
            pub fn snapshot(_:usize)->Vec<String>{crate::SMP.fetch_add(1,crate::Ordering::SeqCst);(0..20).map(|i|format!("cpu{i}: {}",crate::VALUE.load(crate::Ordering::SeqCst))).collect()}
        }
    }
}
mod shell3 {
    pub mod tui {
        #[derive(Clone,Copy)]pub struct Frontend {pub id:u64,pub cols:usize,pub rows:usize}
        pub struct Surface {pub cols:u32,pub rows:u32}
        pub fn native_slot(_:&str)->bool{crate::SLOT.load(crate::Ordering::SeqCst)}
        pub fn request(_:Frontend,_:&str)->Result<(),&'static str>{crate::REQUESTS.fetch_add(1,crate::Ordering::SeqCst);crate::VISIBLE.store(true,crate::Ordering::SeqCst);Ok(())}
        pub fn attach_native(_:Frontend,_:&crate::shell2::MatrixTarget)->Result<(),String>{crate::SLOT.store(true,crate::Ordering::SeqCst);Ok(())}
        pub fn cancel_native_attach(_:&crate::shell2::MatrixTarget){}
        pub fn native_visible(_:&crate::shell2::MatrixTarget)->bool{crate::VISIBLE.load(crate::Ordering::SeqCst)}
        pub fn native_return(_:&crate::shell2::MatrixTarget){crate::VISIBLE.store(false,crate::Ordering::SeqCst);}
        pub fn native_write(_:&crate::shell2::MatrixTarget,bytes:&[u8]){crate::WRITES.lock().unwrap().push(bytes.to_vec());}
        pub fn native_read(_:&crate::shell2::MatrixTarget)->Option<(Vec<u8>,Vec<String>)>{crate::LIVE.load(crate::Ordering::SeqCst).then(||(std::mem::take(&mut *crate::INPUT.lock().unwrap()),vec![]))}
        pub fn surface(_:&crate::shell2::MatrixTarget)->Option<Surface>{let (cols,rows)=*crate::GEOMETRY.lock().unwrap();Some(Surface{cols:cols as u32,rows:rows as u32})}
    }
    #[path="@ROOT@/src/shell3/helper.rs"] mod helper;
    mod monitor {
        @MONITOR@
        fn monitor_task(_:Kind,_:crate::shell2::MatrixTarget)->Result<(),()>{Ok(())}
        #[cfg(test)]mod tests {
            use super::*;
            use crate::{Ordering,VALUE,RAM,SMP,MODULES,WRITES,VISIBLE,NOW,LIVE,SLOT,INPUT,REQUESTS,CLAIMS,GEOMETRY};
            fn reset(){for n in [&RAM,&SMP,&MODULES,&REQUESTS,&CLAIMS]{n.store(0,Ordering::SeqCst);}VALUE.store(1,Ordering::SeqCst);NOW.store(0,Ordering::SeqCst);LIVE.store(true,Ordering::SeqCst);VISIBLE.store(true,Ordering::SeqCst);SLOT.store(false,Ordering::SeqCst);INPUT.lock().unwrap().clear();WRITES.lock().unwrap().clear();*GEOMETRY.lock().unwrap()=(100,12);}
            #[test]fn changed_cells_roundtrip_without_repainting_static_text(){
                let mut screen=Screen::default();let mut terminal=trueos_terminal::Terminal::new(20,4);
                let first=vec!["RAM  memory use".into(),"Used: 8 GiB ⣿".into(),"Static DIMM".into()];
                terminal.feed(screen.diff(&first,20,4).as_bytes());assert_eq!(terminal.render_rows()[1].trim_end(),first[1]);assert!(terminal.mouse_tracking_enabled());
                assert!(screen.diff(&first,20,4).is_empty());let mut next=first.clone();next[1]="Used: 9 GiB ▂".into();
                let diff=screen.diff(&next,20,4);assert!(!diff.contains("RAM")&&!diff.contains("Static")&&!diff.contains("2J"));assert!(diff.len()<35);terminal.feed(diff.as_bytes());assert_eq!(terminal.render_rows()[1].trim_end(),next[1]);
                next[1]="Used: 1".into();next.pop();terminal.feed(screen.diff(&next,20,4).as_bytes());assert_eq!(terminal.render_rows()[1].trim_end(),"Used: 1");assert!(terminal.render_rows()[2].trim().is_empty());
                terminal.resize(12,2);terminal.feed(screen.diff(&next,12,2).as_bytes());assert_eq!(terminal.render_rows()[0].trim_end(),"RAM  memory ".trim_end());
            }
            #[test]fn snapshots_follow_250ms_and_firmware_is_cached_until_resize(){
                reset();let mut ram=View::new(Kind::Ram);assert!(ram.refresh(100,0));assert_eq!(MODULES.load(Ordering::SeqCst),1);
                VALUE.store(2,Ordering::SeqCst);assert!(!ram.refresh(100,249_999_999));assert!(ram.content.last().unwrap().contains("1 GiB"));
                assert!(ram.refresh(100,250_000_000));assert_eq!(RAM.load(Ordering::SeqCst),2);assert_eq!(MODULES.load(Ordering::SeqCst),1);assert!(ram.content.last().unwrap().contains("2 GiB"));
                assert!(ram.refresh(80,250_000_001));assert_eq!(MODULES.load(Ordering::SeqCst),2);
                let mut smp=View::new(Kind::Smp);smp.refresh(100,0);assert!(!smp.refresh(100,249_000_000));smp.refresh(100,250_000_000);assert_eq!(SMP.load(Ordering::SeqCst),2);
            }
            #[test]fn scroll_mouse_footer_and_tiny_geometry_are_bounded(){
                reset();let target=crate::shell2::MatrixTarget;let mut v=View::new(Kind::Smp);v.refresh(100,0);
                for _ in 0..30 {v.action(Action::Down,&target,12);}assert_eq!(v.scroll,11);v.paint(&target,100,12);
                let mut terminal=trueos_terminal::Terminal::new(100,12);for bytes in WRITES.lock().unwrap().drain(..){terminal.feed(&bytes);}assert!(terminal.render_rows()[2].starts_with("cpu11"));
                v.action(Action::Up,&target,12);assert_eq!(v.scroll,10);assert!(!v.action(Action::Click(2),&target,12));assert!(v.action(Action::Click(11),&target,12));assert!(!VISIBLE.load(Ordering::SeqCst));
                for rows in 0..4 {v.paint(&target,10,rows);}v.paint(&target,100,30);assert_eq!(v.scroll,0);
            }
            fn poll(future:crate::Pin<&mut impl crate::Future>)->crate::Poll<()> {future.poll(&mut crate::Context::from_waker(crate::Waker::noop())).map(|_|())}
            #[test]fn parked_task_keeps_its_slot_and_refreshes_immediately_on_reentry(){
                reset();SLOT.store(true,Ordering::SeqCst);let mut task=std::pin::pin!(monitor_task_run(Kind::Ram,crate::shell2::MatrixTarget));assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),1);
                NOW.store(20_000_000,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),1);assert_eq!(WRITES.lock().unwrap().len(),1);
                INPUT.lock().unwrap().extend_from_slice(b"q");assert!(poll(task.as_mut()).is_pending());assert!(!VISIBLE.load(Ordering::SeqCst));assert!(SLOT.load(Ordering::SeqCst));
                NOW.store(10_000_000_000,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),1);
                VALUE.store(2,Ordering::SeqCst);VISIBLE.store(true,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),2);assert_eq!(MODULES.load(Ordering::SeqCst),1);
                LIVE.store(false,Ordering::SeqCst);assert!(poll(task.as_mut()).is_ready());
            }
            #[test]fn entering_existing_helper_requests_reentry_without_second_worker(){
                reset();let f=tui::Frontend{id:1,cols:100,rows:12};start("ram",f).unwrap();assert_eq!(CLAIMS.load(Ordering::SeqCst),1);VISIBLE.store(false,Ordering::SeqCst);start("ram",f).unwrap();assert_eq!(CLAIMS.load(Ordering::SeqCst),1);assert_eq!(REQUESTS.load(Ordering::SeqCst),1);assert!(VISIBLE.load(Ordering::SeqCst));
            }
        }
    }
}
'''


def main():
    monitor = (ROOT / 'src/shell3/monitor.rs').read_text()
    monitor = re.sub(r'^//!.*\n|^#\[trueos_executor::task[^\n]*\n', '', monitor, flags=re.M)
    monitor = monitor.replace('async fn monitor_task(', 'async fn monitor_task_run(')
    with tempfile.TemporaryDirectory(prefix='shell3-monitor-') as tmp:
        path = Path(tmp)
        terminal = path / 'libtrueos_terminal.rlib'
        subprocess.run(['rustc', '--edition=2024', '--crate-type=rlib', '--crate-name', 'trueos_terminal', str(ROOT / 'crates/trueos-terminal/src/lib.rs'), '-o', str(terminal)], check=True)
        source = path / 'tests.rs'
        source.write_text(HARNESS.replace('@ROOT@', str(ROOT)).replace('@MONITOR@', monitor))
        binary = path / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '--extern', f'trueos_terminal={terminal}', '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
