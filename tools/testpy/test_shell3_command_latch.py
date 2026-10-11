#!/usr/bin/env python3
"""Host regression checks for accepted-word animation and deferred slot hops."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    shell = (ROOT/'src/shell3/shell3.rs').read_text()
    source = '''#![allow(dead_code)]
extern crate alloc;
use alloc::{string::{String,ToString},vec::Vec};
mod chronos {pub static NOW:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);pub fn monotonic_nanos()->u64 {NOW.load(std::sync::atomic::Ordering::Relaxed)}}
'''
    source += extract.item('src/shell3/shell3.rs', 'RgbaColor')
    source += re.search(r'^impl RgbaColor \{.*?^}', shell, re.M | re.S).group()
    for name in ('metafmtstr', 'transition', 'command_latch'):
        source += f'#[path="{ROOT}/src/shell3/{name}.rs"] mod {name};\n'
    source += '''use metafmtstr::MetaFmtStr;
mod service {
#[derive(Clone)] pub struct QueuedBlueprint {pub slot:String,pub sha256:[u8;32]}
pub fn notify_work() {}
}
#[derive(Clone,Copy)] struct Frontend;
mod tui {
use super::*;
pub fn hold_navigation(_:u64,_:bool) {}
pub fn select(_:Frontend,_:Option<&str>)->bool {true}
pub fn select_for_navigation(_:Frontend,_:&str)->bool {true}
pub fn queue_launch(_:u64,_:service::QueuedBlueprint) {panic!("unexpected queue")}
pub fn request(_:Frontend,_:&str)->Result<(),String> {Ok(())}
}
#[derive(Default)] struct Slots {ids:Vec<String>,lifetimes:Vec<(String,u64)>,vmx_apps:Vec<service::QueuedBlueprint>,generation:u64}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}}
fn matrix_slots()->&'static Mutex<Slots> {static S:std::sync::OnceLock<Mutex<Slots>>=std::sync::OnceLock::new();S.get_or_init(||Mutex(std::sync::Mutex::new(Slots::default())))}
struct MatrixSlots;
impl MatrixSlots {
fn ensure_named(name:&str)->u64 {
let mut slots=matrix_slots().lock();if let Some((_,generation))=slots.lifetimes.iter().find(|(id,_)|id==name) {return *generation;}
slots.ids.push(name.into());slots.lifetimes.push((name.into(),1));1
}
fn echo(_:Option<&str>,_:Option<u64>,_:String) {}
}
'''
    source += extract.item('src/shell3/shell3.rs', 'LatchedNavigation')
    source += '''struct Shell3 {
input:String, accepted:Vec<String>,
command_feedback:Option<command_latch::Feedback>,latched_navigation:Option<LatchedNavigation>,
tui_frontend:u64,active_matrix_slot:Option<String>,active_matrix_lifetime:Option<u64>,
matrix_selection_dirty:bool,matrix_scroll:usize,matrix_column:usize,matrix_drag:Option<()>,
}
impl Shell3 {
fn tui_frontend(&self)->Frontend {Frontend}
fn sync_aka_names(&mut self) {}
fn prompt(&self)->&str {&self.input}
fn set_prompt(&mut self,text:&str) {self.input=text.into();}
fn parse_name(&self,text:&str)->bool {matches!(text,"app"|"apple"|"äβ")}
fn echo_recognized_prompt(&mut self)->bool {self.accepted.push(core::mem::take(&mut self.input));true}
fn refresh_prompt_strip(&mut self) {}

fn new()->Self {Self {input:String::new(),accepted:vec![],command_feedback:Some(command_latch::Feedback::new("termdir",0,false)),latched_navigation:None,tui_frontend:1,active_matrix_slot:None,active_matrix_lifetime:None,matrix_selection_dirty:false,matrix_scroll:0,matrix_column:0,matrix_drag:None}}
'''
    for name in ('latch_prompt_prefix', 'select_matrix_slot_index', 'select_matrix_slot_name', 'active_matrix_slot_name', 'finish_command_latch', 'select_queued_app'):
        source += re.search(rf'^    (?:pub(?:\([^)]*\))? )?fn {name}\(.*?^    }}', shell, re.M | re.S).group()
    source += "}\nconst OPERATOR:char='§';"
    source += r'''
fn text(feedback:&command_latch::Feedback,now:u64)->String {feedback.runs(20,now).iter().map(|run|run.text.as_str()).collect()}
#[test] fn mode_switch_latches_first_prefix_and_drops_tail() {
    for (input,expected) in [("apple tail","app"),("äβdiscard","äβ")] {
        let mut shell=Shell3::new();shell.input=input.into();
        assert!(shell.latch_prompt_prefix());assert_eq!(shell.accepted,vec![expected]);assert!(shell.input.is_empty());
    }
    for input in ["unknown","§app","ap"] {
        let mut shell=Shell3::new();shell.input=input.into();
        assert!(!shell.latch_prompt_prefix());assert_eq!(shell.input,input);assert!(shell.accepted.is_empty());
    }
}
#[test] fn word_is_centered_then_dissolves_in_both_tab_directions() {
    let left=command_latch::Feedback::new("termdir",0,false);
    let right=command_latch::Feedback::new("termdir",0,true);
    assert_eq!(text(&left,0),"      termdir");assert_eq!(text(&left,149_999_999),"      termdir");
    assert_eq!(text(&left,200_000_000),"       ermdir");
    assert_eq!(text(&right,200_000_000),"      termdi ");
    assert!(!left.finished(399_999_999));assert!(left.finished(400_000_000));
    assert_eq!(text(&left,400_000_000),"");
    assert_eq!(left.runs(4,0).iter().map(|run|run.text.as_str()).collect::<String>(),"termdir");
}
#[test] fn queued_app_is_registered_immediately_but_selected_after_feedback() {
    *matrix_slots().lock()=Slots::default();chronos::NOW.store(0,std::sync::atomic::Ordering::Relaxed);
    let mut shell=Shell3::new();shell.select_queued_app(service::QueuedBlueprint {slot:"td1".into(),sha256:[1;32]});
    assert_eq!(matrix_slots().lock().vmx_apps.len(),1);assert_eq!(shell.active_matrix_slot,None);
    chronos::NOW.store(399_999_999,std::sync::atomic::Ordering::Relaxed);shell.finish_command_latch(false);assert_eq!(shell.active_matrix_slot,None);
    chronos::NOW.store(400_000_000,std::sync::atomic::Ordering::Relaxed);shell.finish_command_latch(false);
    assert_eq!(shell.active_matrix_slot.as_deref(),Some("td1"));assert!(shell.command_feedback.is_none());assert!(shell.latched_navigation.is_none());
    assert_eq!(matrix_slots().lock().vmx_apps.len(),1);shell.finish_command_latch(false);assert_eq!(matrix_slots().lock().vmx_apps.len(),1);
}
#[test] fn reused_slot_is_not_selected_and_new_input_can_finish_early() {
    *matrix_slots().lock()=Slots::default();chronos::NOW.store(0,std::sync::atomic::Ordering::Relaxed);
    MatrixSlots::ensure_named("log");let mut shell=Shell3::new();assert!(shell.select_matrix_slot_name("log"));assert_eq!(shell.active_matrix_slot,None);
    matrix_slots().lock().lifetimes[0].1=2;shell.finish_command_latch(true);assert_eq!(shell.active_matrix_slot,None);
    let mut shell=Shell3::new();assert!(shell.select_matrix_slot_name("log"));shell.finish_command_latch(true);
    assert_eq!(shell.active_matrix_slot.as_deref(),Some("log"));assert!(shell.command_feedback.is_none());
    shell.command_feedback=Some(command_latch::Feedback::new("esc",0,false));assert!(shell.select_matrix_slot_index(0));
    assert_eq!(shell.active_matrix_slot.as_deref(),Some("log"));shell.finish_command_latch(true);assert_eq!(shell.active_matrix_slot,None);
}
#[test] fn removed_app_slot_cannot_be_revived_by_delayed_navigation() {
    *matrix_slots().lock()=Slots::default();chronos::NOW.store(0,std::sync::atomic::Ordering::Relaxed);
    let mut shell=Shell3::new();shell.select_queued_app(service::QueuedBlueprint {slot:"td1".into(),sha256:[1;32]});
    matrix_slots().lock().lifetimes.clear();shell.finish_command_latch(true);assert_eq!(shell.active_matrix_slot,None);
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-command-latch-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)

if __name__ == '__main__':
    main()
