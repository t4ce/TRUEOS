#!/usr/bin/env python3
"""Run native GPU/picker logic and TLB file export with mocked hardware services."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_shell3_monitor import HARNESS, ROOT


def function(source, name):
    start = source.index('fn ' + name + '(')
    start = source.rfind('\n', 0, start) + 1
    brace = source.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


def main():
    helper = (ROOT / 'src/shell3/helper.rs').read_text()
    helper = re.sub(r'^//!.*\n', '', helper, flags=re.M)
    helper = re.sub(r'pub\(super\) fn admit\(.*?^}', 'pub(super) fn admit(_: &str,_:Frontend)->Result<Option<(MatrixTarget,crate::workers::WorkerSpawner)>,String>{Ok(None)}', helper, flags=re.M | re.S)
    picker = re.sub(r'^//!.*\n', '', (ROOT / 'src/shell3/system/picker.rs').read_text(), flags=re.M)
    gpu = (ROOT / 'src/shell3/system/gpu.rs').read_text()
    gpu = gpu[gpu.index('use super::{\n    MatrixTarget,'):]
    gpu = re.sub(r'^#\[trueos_executor::task[^\n]*\n', '', gpu, flags=re.M)
    gpu = gpu.replace('async fn menu_task(', 'async fn menu_task_run(')
    vcpy = (ROOT / 'src/shell3/system/vcpy.rs').read_text()
    vcpy = vcpy[vcpy.index('use super::{\n    MatrixTarget,'):]
    vcpy = re.sub(r'^#\[trueos_executor::task[^\n]*\n', '', vcpy, flags=re.M)
    vcpy = vcpy.replace('async fn menu_task(', 'async fn menu_task_run(')
    tlb = (ROOT / 'src/shell3/system/tlb.rs').read_text()
    tables = (ROOT / 'src/efi/acpi/tlb.rs').read_text()
    selectors = function(tables, 'parse_acpi_signature') + '\n' + function(tlb, 'valid_selection')
    writer = function(tables, 'write_dump_bytes_to_default_path')
    source = HARNESS.replace("#![allow(dead_code)]", "#![allow(dead_code, unused_imports)]").split('    mod monitor {', 1)[0].replace('@HELPER@', helper)
    source += 'mod picker {' + picker + '}\nmod gpu {' + r'''
    struct Output {lines:Vec<String>}
    impl Output {fn collect(_:usize)->Self {Self{lines:vec![]}} fn finish(self)->Vec<String>{self.lines}}
    fn execute(_: &Output, command:&str){crate::ACTIONS_RUN.lock().unwrap().push(command.into());}
    fn snapshot(_:usize)->Vec<String>{crate::RAM.fetch_add(1,crate::Ordering::SeqCst);vec![format!("frame {}",crate::VALUE.load(crate::Ordering::SeqCst))]}
    fn menu_task(_:super::MatrixTarget)->Result<(),()>{Ok(())}
''' + gpu + r'''
    #[cfg(test)]mod tests {
      use super::*;use crate::{Ordering,NOW,RAM,VALUE,VISIBLE,LIVE,INPUT,WRITES};
      fn poll(f:std::pin::Pin<&mut impl std::future::Future<Output=()>>)->std::task::Poll<()> {f.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))}
      #[test]fn live_cadence_park_reentry_and_explicit_actions(){
        crate::ACTIONS_RUN.lock().unwrap().clear();WRITES.lock().unwrap().clear();INPUT.lock().unwrap().clear();NOW.store(0,Ordering::SeqCst);RAM.store(0,Ordering::SeqCst);VALUE.store(1,Ordering::SeqCst);VISIBLE.store(true,Ordering::SeqCst);LIVE.store(true,Ordering::SeqCst);
        let mut task=std::pin::pin!(menu_task_run(super::super::MatrixTarget));
        assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),1);
        NOW.store(249_999_999,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),1);
        NOW.store(250_000_000,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),2);
        VISIBLE.store(false,Ordering::SeqCst);NOW.store(9_000_000_000,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),2);
        VISIBLE.store(true,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),3);
        INPUT.lock().unwrap().extend_from_slice(b"\x1b[<0;5;");assert!(poll(task.as_mut()).is_pending());
        INPUT.lock().unwrap().extend_from_slice(b"5m");NOW.store(9_250_000_000,Ordering::SeqCst);assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),4);
        INPUT.lock().unwrap().extend_from_slice(b"m");assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),4);
        INPUT.lock().unwrap().extend_from_slice(b"j\r");assert!(poll(task.as_mut()).is_pending());assert_eq!(*crate::ACTIONS_RUN.lock().unwrap(),vec![String::from("test broker")]);
        INPUT.lock().unwrap().extend_from_slice(b"\r");assert!(poll(task.as_mut()).is_pending());assert_eq!(RAM.load(Ordering::SeqCst),5);
        assert!(WRITES.lock().unwrap().iter().all(|b|!String::from_utf8_lossy(b).contains("48;")));
        LIVE.store(false,Ordering::SeqCst);assert!(poll(task.as_mut()).is_ready());
      }
    }
}
mod tlb_selection {use alloc::string::String;use alloc::vec::Vec;
''' + selectors + r'''
#[test]fn acpi_and_aml_parameters_are_checked(){
 for command in ["acpi","acpi ssdt","acpi SSDT 3","aml","aml ec","aml symbol \\_SB.PCI0","aml prefix \\_SB"] {assert!(valid_selection(command),"{command}");}
 for command in ["acpi toolong","acpi SSDT nope","acpi SSDT 1 extra","aml symbol","aml other","aml ec extra"]{assert!(!valid_selection(command),"{command}");}
}
}
#[cfg(test)]mod picker_tests {
 use super::picker::*;
 #[test]fn literal_form_and_crlf_submission(){let mut p=Picker::default();p.form("path");assert!(p.input(b"jhq",2,10,0).is_none());assert!(matches!(p.input(b"\r",2,10,1),Some(Event::Submit(v)) if v=="jhq"));assert!(p.input(b"\n",2,10,2).is_none());}
 #[test]fn scrolled_mouse_hits_the_visible_option(){let mut p=Picker::default();for _ in 0..8 {p.input(b"j",12,8,0);}let labels=vec!["item";12];let frame=p.frame("Title","Summary",&labels,8);assert!(frame[7].contains('›'));assert!(matches!(p.input(b"\x1b[<0;2;4M",12,8,0),Some(Event::Choose(4))));}
 #[test]fn results_and_controls_stay_at_the_top(){let mut scroll=100;let content=vec![String::from("one"),String::from("two")];let frame=page("Title","Summary",&content,&mut scroll,10);assert!(frame[1].contains("Return"));assert_eq!(frame[3],"one");assert_eq!(scroll,0);assert!(frame[9].is_empty());}
}
}
static ACTIONS_RUN:Mutex<Vec<String>>=Mutex::new(Vec::new());
mod disc {pub mod block {#[derive(Debug,PartialEq,Clone,Copy)]pub enum Error{NotReady,Io}}}
mod infer {pub struct ContentTypeId;impl ContentTypeId{pub const UTF8_TEXT:u32=7;}}
static ROOT_READY:AtomicBool=AtomicBool::new(false);
static FILE_RESULT:AtomicUsize=AtomicUsize::new(0);
static FILE_CALLS:Mutex<Vec<(u64,String,Vec<u8>,u32)>>=Mutex::new(Vec::new());
mod r {pub mod fs {pub mod trueosfs {
 pub fn primary_root_handle()->Option<u64>{crate::ROOT_READY.load(crate::Ordering::SeqCst).then_some(9)}
 pub async fn file_in_typed_async(h:u64,path:&str,bytes:&[u8],kind:u32)->Result<bool,crate::disc::block::Error>{crate::FILE_CALLS.lock().unwrap().push((h,path.into(),bytes.to_vec(),kind));match crate::FILE_RESULT.load(crate::Ordering::SeqCst){0=>Ok(true),1=>Ok(false),_=>Err(crate::disc::block::Error::NotReady)}}
}}}
mod dump_writer {use crate::infer;const DUMP_FILE_PATH:&str="trueos/pci/tlb.txt";
''' + writer + r'''
#[test]fn typed_dump_writer_preserves_bytes_and_propagates_failures(){
 use crate::{Ordering,ROOT_READY,FILE_RESULT,FILE_CALLS,disc::block::Error};
 fn run(f:impl std::future::Future<Output=Result<(),Error>>)->Result<(),Error>{let mut f=std::pin::pin!(f);match f.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())){std::task::Poll::Ready(r)=>r,_=>panic!("pending")}}
 assert_eq!(run(write_dump_bytes_to_default_path(b"dump\n")),Err(Error::NotReady));assert!(FILE_CALLS.lock().unwrap().is_empty());
 ROOT_READY.store(true,Ordering::SeqCst);assert_eq!(run(write_dump_bytes_to_default_path(b"dump\n")),Ok(()));assert_eq!(FILE_CALLS.lock().unwrap()[0],(9,DUMP_FILE_PATH.into(),b"dump\n".to_vec(),crate::infer::ContentTypeId::UTF8_TEXT));
 FILE_RESULT.store(1,Ordering::SeqCst);assert_eq!(run(write_dump_bytes_to_default_path(b"")),Err(Error::Io));FILE_RESULT.store(2,Ordering::SeqCst);assert_eq!(run(write_dump_bytes_to_default_path(b"")),Err(Error::NotReady));
}
}
'''
    vcpy_test = r'''
mod vcpy {
    struct Output {lines:Vec<String>}
    impl Output {fn collect(_:usize)->Self{Self{lines:vec![]}}fn finish(self)->Vec<String>{self.lines}}
    fn execute(_: &Output,command:&str){crate::ACTIONS_RUN.lock().unwrap().push(command.into());}
    fn menu_task(_:super::MatrixTarget)->Result<(),()>{Ok(())}
''' + vcpy + r'''
    #[test]fn selector_runs_checks_explicitly_and_returns_to_menu(){
      use crate::{Ordering,LIVE,VISIBLE,INPUT,ACTIONS_RUN};use std::future::Future;
      fn poll(f:std::pin::Pin<&mut impl Future<Output=()>>){assert!(f.poll(&mut std::task::Context::from_waker(std::task::Waker::noop())).is_pending());}
      ACTIONS_RUN.lock().unwrap().clear();INPUT.lock().unwrap().clear();LIVE.store(true,Ordering::SeqCst);VISIBLE.store(true,Ordering::SeqCst);
      let mut task=std::pin::pin!(menu_task_run(super::MatrixTarget));poll(task.as_mut());assert!(ACTIONS_RUN.lock().unwrap().is_empty());
      INPUT.lock().unwrap().extend_from_slice(b"j\r");poll(task.as_mut());assert_eq!(*ACTIONS_RUN.lock().unwrap(),vec![String::from("fillcheck")]);
      INPUT.lock().unwrap().extend_from_slice(b"\r");poll(task.as_mut());INPUT.lock().unwrap().extend_from_slice(b"j\r");poll(task.as_mut());assert_eq!(*ACTIONS_RUN.lock().unwrap(),vec![String::from("fillcheck"),String::from("monocheck")]);
      INPUT.lock().unwrap().extend_from_slice(b"\r");poll(task.as_mut());INPUT.lock().unwrap().extend_from_slice(b"q");poll(task.as_mut());assert!(!VISIBLE.load(Ordering::SeqCst));
      LIVE.store(false,Ordering::SeqCst);assert!(task.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())).is_ready());
    }
}
'''
    source = source.replace('mod tlb_selection {', vcpy_test + 'mod tlb_selection {', 1)
    output = re.sub(r'^//!.*\n', '', (ROOT / 'src/shell3/system/output.rs').read_text(), flags=re.M)
    origin_state = tlb[tlb.index('static ORIGINS:'):tlb.index('pub(super) fn start(')]
    source += r'''
mod routing {
 use std::sync::Mutex as StdMutex;
 pub struct Mutex<T>(StdMutex<T>);
 impl<T> Mutex<T> {pub const fn new(value:T)->Self{Self(StdMutex::new(value))}pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}pub fn into_inner(self)->T{self.0.into_inner().unwrap()}}
 #[derive(Clone,Debug,PartialEq,Eq)]pub struct MatrixSlotLease {id:String,generation:u64}
 impl MatrixSlotLease {pub fn name(&self)->&str{&self.id}}
 pub type MatrixTarget=MatrixSlotLease;
 pub fn matrix_target_slot_lease(target:&MatrixTarget)->MatrixSlotLease{target.clone()}
 static DROPPED:Mutex<Vec<String>>=Mutex::new(Vec::new());
 static PRINTED:Mutex<Vec<(Option<MatrixSlotLease>,String)>>=Mutex::new(Vec::new());
 pub struct MatrixSlots;
 impl MatrixSlots {
 pub fn drop_slot(name:Option<&str>){DROPPED.lock().push(name.unwrap().into());}
 pub fn echo_output(lease:&MatrixSlotLease,text:String){PRINTED.lock().push((Some(lease.clone()),text));}
 pub fn echo(_:Option<&str>,_:Option<u64>,text:String){PRINTED.lock().push((None,text));}
 }
 mod service {pub fn notify_work(){}}
 mod output {use super as spin;
''' + output + r'''
}
 mod tlb {use super::Mutex;use super::output::DumpTarget as MatrixTarget;
''' + origin_state + '\n' + function(tlb, 'close_selector') + r'''
 #[test]fn destination_follows_slot_lifetime_across_frontends_and_is_consumed_once(){
 let slot=super::MatrixSlotLease{id:"tlb".into(),generation:3};let origin=super::MatrixSlotLease{id:"app1".into(),generation:8};
 ORIGINS.lock().push((slot.clone(),MatrixTarget(Some(origin.clone()))));
 let routed=close_selector(&slot,42,true).unwrap();assert_eq!(routed.0,Some(origin.clone()));assert!(ORIGINS.lock().is_empty());assert!(take_return(1).is_none());assert_eq!(take_return(42).unwrap().0,Some(origin));assert!(take_return(42).is_none());assert_eq!(*super::DROPPED.lock(),vec![String::from("tlb")]);
 }
 }
 #[test]fn diagnostic_sink_uses_exact_lease_or_default_and_collects_separately(){
 let lease=MatrixSlotLease{id:"app1".into(),generation:8};let out=output::Output::slot(output::DumpTarget(Some(lease.clone())),80);out.line("table");assert_eq!(out.target().0,Some(lease.clone()));
 let default=output::Output::slot(output::DumpTarget(None),80);default.line("default");assert_eq!(*PRINTED.lock(),vec![(Some(lease),String::from("table")),(None,String::from("default"))]);
 let collected=output::Output::collect(40);collected.line("result");assert_eq!(collected.width(),40);assert_eq!(collected.finish(),vec![String::from("result")]);
 }
}
'''
    source += r'''
mod spirit {pub mod gpu_logger {
 pub enum GpuLoggerSource {Helio}
 pub struct Sample {pub frame_index:u64,pub cadence_us:u64,pub frame_us:u64,pub geometry_us:u64,pub prepare_us:u64,pub retire_wait_us:u64,pub poll_iters:u64,pub objects:u64,pub draws:u64,pub triangles:u64,pub busy_retries:u64,pub incomplete_retries:u64}
 pub fn latest_sample(_:GpuLoggerSource)->Sample{Sample{frame_index:8,cadence_us:20000,frame_us:3000,geometry_us:400,prepare_us:200,retire_wait_us:100,poll_iters:2,objects:3,draws:4,triangles:50,busy_retries:0,incomplete_retries:1}}
 pub fn fps_from_cadence_us(us:u64)->u64{1000000/us}
}}
mod cadence {
 use alloc::format;
 struct Output(std::cell::RefCell<Vec<String>>);
 impl Output{fn collect(_:usize)->Self{Self(Default::default())}fn line(&self,text:&str){self.0.borrow_mut().push(text.into());}fn finish(self)->Vec<String>{self.0.into_inner()}}
 fn print_status(out:&Output){out.line("broker status");}
''' + function((ROOT / 'src/shell3/system/gpu.rs').read_text(), 'snapshot') + r'''
 #[test]fn live_snapshot_uses_spirit_cadence_before_full_broker_status(){let rows=snapshot(100);assert_eq!(rows[0],"Helio / Spirit: frame=8 cadence=20000 us delivered_fps=50");assert!(rows[1].contains("Render=3000 us"));assert!(rows[2].contains("triangles=50"));assert_eq!(rows[3],"broker status");}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-diagnostics-') as tmp:
        path = Path(tmp)
        source_path = path / 'test.rs'
        source_path.write_text(source)
        binary = path / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(source_path), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
