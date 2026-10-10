#!/usr/bin/env python3
"""Compile the production Shell3 terminal bridge with host lifecycle adapters."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    shell = (ROOT / 'src/shell3/shell3.rs').read_text()
    source = '#![allow(dead_code)]\nextern crate alloc;\nuse alloc::{string::String,vec::Vec};\n'
    for name in ('RgbaColor','SpecialRows','StripSide'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += re.search(r'^impl RgbaColor \{.*?^}', shell, re.M | re.S).group()
    source += f'#[path="{ROOT}/src/shell3/metafmtstr.rs"] mod metafmtstr;use metafmtstr::MetaFmtStr;\n#[path="{ROOT}/src/shell3/update.rs"] mod update;\n'
    source += '''
const SpecialSeperator:char='│';
mod allocators {pub fn with_host_alloc_domain<T>(f:impl FnOnce()->T)->T {f()}}
mod service {pub use crate::shell2::cmds::run::QueuedBlueprint;pub fn notify_work(){}}
struct MatrixSlots;
static TRANSCRIPTS:std::sync::Mutex<Vec<(shell2::MatrixSlotLease,String)>>=std::sync::Mutex::new(Vec::new());
impl MatrixSlots {
fn echo_output(lease:&shell2::MatrixSlotLease,line:String) {if shell2::matrix_slot_is_live(lease) {TRANSCRIPTS.lock().unwrap().push((lease.clone(),line));}}
fn drop_slot(name:Option<&str>)->bool {if let Some(name)=name {shell2::free_name(name);}true}}
mod r {pub mod keyboard {
'''
    keyboard = (ROOT/'src/r/keyboard.rs').read_text()
    source += '\n'.join(re.findall(r'^pub const KEYBOARD_(?:KEY_\w+|OUTPUT_\w+):[^\n]+', keyboard, re.M))
    source += extract.item('src/r/keyboard.rs', 'TrueosKeyboardOutputEvent')
    source += '''}}
mod ui4 {type Ui4CursorSource=u8;pub type WindowId=u32;
#[derive(Clone,Copy)]pub enum WindowOwner{Vm(u8)}
pub mod drag_drop {pub fn release_terminal(_:super::WindowOwner,_:super::WindowId){}}
'''
    source += extract.item('src/ui4/input_broker.rs', 'Ui4PointerEvent')
    source += '''
}
mod shell3 {pub use crate::shell2::{MatrixTarget,MatrixSlotLease,matrix_target_slot_lease};}
mod matrix_target {pub use crate::shell2::{MatrixSlotAttachment,attach_matrix_slot_resource,matrix_slot_is_live,TRANSPORT_NET_TCP_SCOPE,TRANSPORT_LOCAL_SCOPE};}
mod shell2 {
pub mod cmds {pub mod run {#[derive(Clone,Debug)] pub struct QueuedBlueprint {pub slot:String,pub app:String,pub sha256:[u8;32]}}}
pub const TRANSPORT_NET_TCP_SCOPE:u8=1;pub const TRANSPORT_LOCAL_SCOPE:u8=2;
use super::*;use alloc::sync::Arc;
#[derive(Clone,Debug,PartialEq,Eq)] pub struct MatrixSlotLease {pub name:String,pub lifetime:u64}
impl MatrixSlotLease {pub fn name(&self)->&str {&self.name}}
#[derive(Clone)] pub struct MatrixTarget {pub lease:MatrixSlotLease}
pub trait MatrixSlotAttachment:Send+Sync {fn on_matrix_slot_freed(&self,lease:&MatrixSlotLease);}
static LIVE:std::sync::Mutex<Vec<MatrixSlotLease>>=std::sync::Mutex::new(Vec::new());
static ATTACHMENTS:std::sync::Mutex<Vec<(MatrixSlotLease,Arc<dyn MatrixSlotAttachment>)>>=std::sync::Mutex::new(Vec::new());
pub fn target(name:&str,lifetime:u64)->MatrixTarget {let lease=MatrixSlotLease {name:name.into(),lifetime};LIVE.lock().unwrap().push(lease.clone());MatrixTarget {lease}}
pub fn matrix_target_slot_lease(target:&MatrixTarget)->MatrixSlotLease {target.lease.clone()}
pub fn matrix_slot_is_live(lease:&MatrixSlotLease)->bool {LIVE.lock().unwrap().contains(lease)}
pub fn attach_matrix_slot_resource(lease:&MatrixSlotLease,a:Arc<dyn MatrixSlotAttachment>)->Result<(),()> {if !matrix_slot_is_live(lease) {return Err(());}ATTACHMENTS.lock().unwrap().push((lease.clone(),a));Ok(())}
pub fn free_name(name:&str) {
    LIVE.lock().unwrap().retain(|lease|lease.name!=name);
    let callbacks={let mut a=ATTACHMENTS.lock().unwrap();let mut callbacks=Vec::new();let mut i=0;while i<a.len() {if a[i].0.name==name {callbacks.push(a.remove(i));}else {i+=1;}}callbacks};
    for (lease,a) in callbacks {a.on_matrix_slot_freed(&lease);}
}
}
mod hv {
use super::*;use std::sync::atomic::{AtomicU64,AtomicUsize,Ordering};
#[derive(Clone,Copy)] pub struct BlueprintTerminalSurfaceSnapshot {pub generation:u64,pub cols:u32,pub rows:u32}
pub static RUN:AtomicU64=AtomicU64::new(1);
pub static REQUESTS:AtomicUsize=AtomicUsize::new(0);
pub static INPUT:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
pub static SUBMISSIONS:std::sync::Mutex<Vec<Vec<u8>>>=std::sync::Mutex::new(Vec::new());
static TARGETS:std::sync::Mutex<Vec<(u8,shell2::MatrixTarget)>>=std::sync::Mutex::new(Vec::new());
pub fn bind(vm:u8,target:&shell2::MatrixTarget) {TARGETS.lock().unwrap().retain(|e|e.0!=vm);TARGETS.lock().unwrap().push((vm,target.clone()));crate::tui::bind_vm(target,vm);}
pub fn vm_run_generation(_:u8)->Option<u64> {Some(RUN.load(Ordering::Relaxed))}
pub fn blueprint_terminal_request_reentry(_:u8)->Result<(),&'static str> {REQUESTS.fetch_add(1,Ordering::Relaxed);Ok(())}
pub fn blueprint_console_return_to_cli(vm:u8)->bool {let target=TARGETS.lock().unwrap().iter().find(|e|e.0==vm).map(|e|e.1.clone()).unwrap();crate::tui::release(&target,vm)==Some(true)}
pub fn blueprint_console_submit_stdin_for_target(vm:u8,t:&shell2::MatrixTarget,run:u64,bytes:&[u8])->usize {blueprint_console_submit_stdin_for_lease(vm,&t.lease,run,bytes)}
pub fn blueprint_console_submit_stdin_for_lease(_:u8,lease:&shell2::MatrixSlotLease,run:u64,bytes:&[u8])->usize {
    if !shell2::matrix_slot_is_live(lease) || vm_run_generation(0)!=Some(run) {return 0;}
    SUBMISSIONS.lock().unwrap().push(bytes.to_vec());
    INPUT.lock().unwrap().extend_from_slice(bytes);bytes.len()
}
}
'''
    source += f'#[path="{ROOT}/src/shell3/tui.rs"] mod tui;\n'
    source += '''
fn frontend(cols:usize,rows:usize)->tui::Frontend {tui::Frontend {id:tui::new_frontend(),cols,rows}}
fn session(name:&str,vm:u8,cols:usize,rows:usize)->(tui::Frontend,shell2::MatrixTarget) {
    hv::RUN.store(1,std::sync::atomic::Ordering::Relaxed);
    let f=frontend(cols,rows);let t=shell2::target(name,1);tui::attach(f,&t).unwrap();hv::bind(vm,&t);(f,t)
}
#[test] fn stdout_transcript_keeps_prompt_until_terminal_claim() {
    let (f,t)=session("stdout-transcript",23,20,5);
    assert_eq!(tui::write_stdout(&t,23,b"map: caf\\xc3"),9);
    assert_eq!(tui::write_stdout(&t,23,b"\\xa9\\r\\nnext\\n"),8);
    let lines=TRANSCRIPTS.lock().unwrap().iter().filter(|(lease,_)|lease==&t.lease).map(|(_,text)|text.clone()).collect::<Vec<_>>();
    assert_eq!(lines,vec!["map: café","next"]);
    assert!(tui::snapshot(f.id,Some("stdout-transcript")).is_none());
    assert!(!tui::remote_active(f.id));
    assert_eq!(tui::claim(&t,23),Some(true));
    assert_eq!(tui::write_stdout(&t,23,b"terminal"),8);
    assert!(tui::snapshot(f.id,Some("stdout-transcript")).is_some());
    assert_eq!(TRANSCRIPTS.lock().unwrap().iter().filter(|(lease,_)|lease==&t.lease).count(),2);
    shell2::free_name("stdout-transcript");
    assert_eq!(tui::write_stdout(&t,23,b"stale\\n"),0);
}
#[test] fn startup_claim_raw_frame_park_and_reentry_use_one_live_vm() {
    let (f,t)=session("cycle",1,12,5);assert!(tui::supports(&t));assert!(tui::snapshot(f.id,Some("cycle")).is_none());
    assert_eq!(tui::claim(&t,1),Some(true));
    let bytes="\\x1b[?1049h\\x1b[?25l\\x1b[2J\\x1b[2;3H\\x1b[38;2;1;2;3m\\x1b[48;5;196m\\x1b[4mé⣿".as_bytes();
    for chunk in bytes.chunks(1) {assert_eq!(tui::write(&t,1,chunk),Some(1));}
    let rows=tui::snapshot(f.id,Some("cycle")).unwrap();assert_eq!(rows.len(),5);assert_eq!(rows[1][2].0,'é');assert_eq!(rows[1][3].0,'⣿');
    assert_eq!(rows[1][2].1.unwrap().rgba(),[1,2,3,255]);assert_eq!(rows[1][2].1.unwrap().background(),Some([255,0,0,255]));assert!(rows[1][2].1.unwrap().underline());
    let before=tui::revision(f.id);assert_eq!(tui::release(&t,1),Some(true));assert!(tui::revision(f.id)>before);
    assert!(tui::snapshot(f.id,Some("cycle")).is_none());assert_eq!(tui::write(&t,1,b"late paint"),Some(0));
    assert!(tui::supports(&t));tui::request(f,"cycle").unwrap();assert_eq!(tui::claim(&t,1),Some(true));
    assert_eq!(tui::write(&t,1,b"\\x1b[?25lreentered"),Some(15));assert_eq!(tui::snapshot(f.id,Some("cycle")).unwrap()[0][0].0,'r');
    shell2::free_name("cycle");assert!(!tui::supports(&t));assert!(tui::snapshot(f.id,Some("cycle")).is_none());
}
#[test] fn geometry_generation_changes_on_resize_but_not_on_output() {
    let (f,t)=session("size",2,20,8);tui::claim(&t,2);let initial=tui::surface(&t).unwrap();
    tui::write(&t,2,b"frame");assert_eq!(tui::surface(&t).unwrap().generation,initial.generation);
    assert!(tui::select(tui::Frontend {id:f.id,cols:30,rows:12},Some("size")));
    let resized=tui::surface(&t).unwrap();assert_eq!((resized.cols,resized.rows),(30,12));assert!(resized.generation>initial.generation);
    hv::INPUT.lock().unwrap().clear();tui::write(&t,2,b"\\x1b[6n");assert!(!hv::INPUT.lock().unwrap().is_empty());
    tui::detach(f.id);assert!(!tui::supports(&t));
}
#[test] fn ui4_drag_endpoints_follow_the_exact_live_terminal_owner() {
    let (f,t)=session("drag-window",10,20,8);tui::bind_ui4_window(f.id,123);
    assert_eq!(tui::window_for_vm(10),None);assert_eq!(tui::vm_for_window(123),None);
    assert_eq!(tui::claim(&t,10),Some(true));
    assert_eq!(tui::window_for_vm(10),Some(123));assert_eq!(tui::vm_for_window(123),Some(10));
    assert_eq!(tui::window_for_vm(11),None);
    hv::RUN.store(2,std::sync::atomic::Ordering::Relaxed);
    assert_eq!(tui::window_for_vm(10),None);assert_eq!(tui::vm_for_window(123),None);
    hv::RUN.store(1,std::sync::atomic::Ordering::Relaxed);
    tui::release(&t,10);assert_eq!(tui::window_for_vm(10),None);assert_eq!(tui::vm_for_window(123),None);
    tui::claim(&t,10);tui::detach(f.id);assert_eq!(tui::window_for_vm(10),None);assert_eq!(tui::vm_for_window(123),None);
}
#[test] fn exact_owner_run_and_slot_lifetime_prevent_cross_terminal_paint() {
    let (f,t)=session("own",3,10,4);assert_eq!(tui::claim(&t,3),Some(true));
    assert_eq!(tui::claim(&t,4),Some(false));assert_eq!(tui::write(&t,4,b"bad"),Some(0));assert_eq!(tui::release(&t,4),Some(false));
    let other=frontend(10,4);assert!(tui::request(other,"own").is_err());assert!(tui::snapshot(other.id,Some("own")).is_none());
    hv::RUN.store(2,std::sync::atomic::Ordering::Relaxed);assert_eq!(tui::write(&t,3,b"stale run"),Some(0));hv::RUN.store(1,std::sync::atomic::Ordering::Relaxed);
    assert!(tui::select(f,None));assert!(tui::snapshot(f.id,Some("own")).is_none());
    tui::request(other,"own").unwrap();assert_eq!(tui::claim(&t,3),Some(true));assert!(tui::snapshot(other.id,Some("own")).is_some());
    shell2::free_name("own");let replacement=shell2::target("own",2);assert!(!tui::supports(&replacement));assert_eq!(tui::write(&t,3,b"freed"),None);
}
#[test] fn crossterm_keys_mouse_and_replies_reach_only_the_active_owner() {
    use r::keyboard::*;
    let (f,t)=session("keys",4,20,8);assert_eq!(tui::claim(&t,4),Some(true));hv::INPUT.lock().unwrap().clear();
    let mut e=TrueosKeyboardOutputEvent::default();e.flags=KEYBOARD_OUTPUT_FLAG_PRESS;e.kind=KEYBOARD_OUTPUT_KIND_KEY;e.key_code=KEYBOARD_KEY_SPACE;e.codepoint=32;e.device_seq=7;
    assert!(tui::keyboard(f.id,Some("keys"),&e));e.kind=KEYBOARD_OUTPUT_KIND_TEXT;assert!(tui::keyboard(f.id,Some("keys"),&e));assert_eq!(*hv::INPUT.lock().unwrap(),b" ");
    e.codepoint='q' as u32;e.modifiers=1;tui::keyboard(f.id,Some("keys"),&e);assert_eq!(*hv::INPUT.lock().unwrap(),b" \\x11");
    e.kind=KEYBOARD_OUTPUT_KIND_KEY;e.key_code=KEYBOARD_KEY_ARROW_UP;e.codepoint=0;e.modifiers=0;tui::keyboard(f.id,Some("keys"),&e);
    assert!(hv::INPUT.lock().unwrap().ends_with(b"\\x1b[A"));
    e.key_code=KEYBOARD_KEY_ESCAPE;tui::keyboard(f.id,Some("keys"),&e);
    assert!(hv::INPUT.lock().unwrap().ends_with(b"\\x1b"));
    assert!(tui::active(f.id,Some("keys"))); // The consumer decides whether Esc releases its lease.
    hv::INPUT.lock().unwrap().clear();e.key_code=0;e.codepoint='a' as u32;
    tui::keyboard(f.id,Some("keys"),&e);e.kind=KEYBOARD_OUTPUT_KIND_TEXT;tui::keyboard(f.id,Some("keys"),&e);
    assert_eq!(*hv::INPUT.lock().unwrap(),b"a"); // Unmapped named events must not eat printable text.
    tui::write(&t,4,b"\\x1b[?1000h\\x1b[?1006h");hv::INPUT.lock().unwrap().clear();
    let pointer=ui4::Ui4PointerEvent {source:0,window:1,x:0,y:0,local_x:6,local_y:11,dx:0,dy:0,wheel:0,buttons_down:1,buttons_pressed:1,buttons_released:0,combo_id:0,vcursor:false};
    tui::pointer(f.id,Some("keys"),&pointer,1);assert_eq!(*hv::INPUT.lock().unwrap(),b"\\x1b[<0;2;2M");
    tui::release(&t,4);hv::INPUT.lock().unwrap().clear();assert!(!tui::keyboard(f.id,Some("keys"),&e));tui::pointer(f.id,Some("keys"),&pointer,1);assert!(hv::INPUT.lock().unwrap().is_empty());
}
'''
    source += r'''
struct RowStrips {left:Vec<MetaFmtStr>}
struct Shell3 {tui_frontend:u64,name:String,size:(usize,usize),prompt:String,mode:u8}
impl Shell3 {
    fn row_for_render(&self,_:SpecialRows)->RowStrips {RowStrips {left:vec![MetaFmtStr::new("TrueOS")]}}
    fn active_matrix_slot_name(&self)->Option<String>{Some(self.name.clone())}
    fn set_mode(&mut self,mode:u8){self.mode=mode;}
    fn get_mode(&self)->u8{self.mode}
    fn get_size(&self)->(usize,usize){self.size}
    fn set(&mut self,cols:usize,rows:usize){self.size=(cols,rows);tui::select(tui::Frontend {id:self.tui_frontend,cols,rows},Some(&self.name));}
    fn prompt(&self)->&str{&self.prompt}
    fn set_prompt(&mut self,text:&str){self.prompt=text.into();}
    fn set_cursor(&mut self,_:usize){}
    fn reconcile_matrix_selection(&mut self){}
    fn drag_matrix(&mut self,_:(i32,i32),_:bool,_:bool,_:bool,_:(i32,i32))->bool{false}
    fn record_terminal_notice(&mut self,_:&str){}
    fn replay_terminal_line(&mut self,_:&str){}
    fn capture_matrix_snapshot(&self)->update::Snapshot{
        let title=[MetaFmtStr::new("TrueOS")];let prompt=[MetaFmtStr::new(self.prompt.clone()),MetaFmtStr::new(" ").blink()];
        update::Snapshot::new(self.size,0,[(&title,&[]),(&[],&[]),(&prompt,&[])],self.size.0).with_matrix(&[],0)
    }
    fn handle_keyboard_with_latch(&mut self,e:&r::keyboard::TrueosKeyboardOutputEvent)->(bool,bool){
        let operator=e.kind==r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT && e.codepoint=='§' as u32;
        if operator {tui::park(self.tui_frontend);}
        else if tui::keyboard(self.tui_frontend,Some(&self.name),e){return (true,false);}
        if e.kind==r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT{self.prompt.push(char::from_u32(e.codepoint).unwrap());}
        (true,false)
    }
}
'''
    source += 'mod tty {\n'+(ROOT/'src/shell3/tty.rs').read_text().replace('//!','//').replace('mod input;', f'#[path="{ROOT}/src/shell3/tty/input.rs"] mod input;').replace('mod ansi;', f'#[path="{ROOT}/src/shell3/tty/ansi.rs"] mod ansi;')+'\nimpl Terminal { pub fn test_prompt(&self)->&str {self.shell.prompt()} }\n}\n'
    source += r'''
#[test] fn ssh_global_operator_escapes_raw_input_even_across_utf8_packet_splits(){
    let (f,t)=session("ssh-fastspawn",19,40,8);
    let shell=Shell3 {tui_frontend:f.id,name:"ssh-fastspawn".into(),size:(40,8),prompt:String::new(),mode:3};
    let mut tty=tty::Terminal::new_ssh(shell);assert_eq!(tui::claim(&t,19),Some(true));
    hv::INPUT.lock().unwrap().clear();
    tty.input(b"abc\xc2");assert_eq!(*hv::INPUT.lock().unwrap(),b"abc");assert!(tty.test_prompt().is_empty());
    tty.input(b"\xa7\xc2");assert_eq!(tty.test_prompt(),"§");assert!(!tui::active(f.id,Some("ssh-fastspawn")));
    tty.input(b"\xa7gridpaper");assert_eq!(tty.test_prompt(),"§§gridpaper");assert_eq!(*hv::INPUT.lock().unwrap(),b"abc");
}
#[test] fn ssh_shares_tui_frames_raw_input_resize_park_release_and_reentry(){
    let (f,t)=session("ssh-tui",7,12,5);
    let shell=Shell3 {tui_frontend:f.id,name:"ssh-tui".into(),size:(12,5),prompt:String::new(),mode:3};
    let mut tty=tty::Terminal::new_ssh(shell);tty.output.clear();
    assert_eq!(tui::claim(&t,7),Some(true));
    tui::write(&t,7,b"\x1b[?25l\x1b[2J\x1b[2;3H\x1b[38;2;1;2;3mAPP");
    tty.reconcile_matrix_selection();let out=String::from_utf8_lossy(&tty.output);
    assert!(out.contains("\x1b[r"));assert!(out.contains("APP"));assert!(out.contains("38;2;1;2;3m"));assert!(!out.contains("TrueOS"));
    hv::INPUT.lock().unwrap().clear();let keys=b"\x1b[A\x1b[B\x1b[D\x1b[C\x1b[3~\t\r\x03\x04";
    for byte in keys {tty.input(&[*byte]);}
    tty.input("é".as_bytes());let mut expected=keys.to_vec();expected.extend("é".as_bytes());
    assert_eq!(*hv::INPUT.lock().unwrap(),expected);assert!(!tty.closing);
    tty.output.clear();tty.resize(16,7);assert!(tui::snapshot(f.id,Some("ssh-tui")).is_none());
    assert_eq!(tui::surface(&t).unwrap().rows,7);assert!(tty.output.is_empty());
    tty.output.clear();tty.input("§".as_bytes());assert!(!tui::active(f.id,Some("ssh-tui")));
    tty.reconcile_matrix_selection();
    assert!(!tui::active(f.id,Some("ssh-tui")));let out=String::from_utf8_lossy(&tty.output);
    assert!(out.contains("TrueOS"));assert!(out.contains("\x1b[4;7r"));
    tui::request(tui::Frontend {cols:16,rows:7,..f},"ssh-tui").unwrap();assert_eq!(tui::claim(&t,7),Some(true));
    tty.output.clear();tty.reconcile_matrix_selection();assert!(String::from_utf8_lossy(&tty.output).contains("\x1b[r"));
    assert_eq!(tui::release(&t,7),Some(true));tty.output.clear();tty.reconcile_matrix_selection();
    assert!(String::from_utf8_lossy(&tty.output).contains("TrueOS"));
}
'''
    source += r'''
#[test] fn ssh_app_mouse_preferences_are_scoped_to_the_live_lease(){
    use trueos_terminal::MouseTracking;
    let (f,t)=session("mouse-scope",8,12,5);
    let shell=Shell3 {tui_frontend:f.id,name:"mouse-scope".into(),size:(12,5),prompt:String::new(),mode:3};
    let mut tty=tty::Terminal::new_ssh(shell);
    assert!(!String::from_utf8_lossy(&tty.output).contains("?1003h"));
    assert_eq!(tui::claim(&t,8),Some(true));tty.output.clear();tty.reconcile_matrix_selection();
    assert!(!String::from_utf8_lossy(&tty.output).contains("?1003h"));
    // Exact EnableMouseCapture sequence emitted by crossterm/termdir.
    tui::write(&t,8,b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h");
    // VM output remains opaque on SSH; the peer terminal owns mouse modes.
    assert_eq!(tui::mouse_options(f.id,Some("mouse-scope")).tracking,MouseTracking::Off);
    tty.output.clear();tty.reconcile_matrix_selection();let out=String::from_utf8_lossy(&tty.output);
    assert!(out.contains("\x1b[?1003h"));assert!(out.contains("\x1b[?1006h"));
    hv::INPUT.lock().unwrap().clear();let mouse=b"\x1b[<35;2;2M\x1b[<0;2;2M\x1b[<32;3;2M\x1b[<0;3;2m\x1b[<64;3;2M\x1b[<65;3;2M";
    for byte in mouse {tty.input(&[*byte]);}assert_eq!(*hv::INPUT.lock().unwrap(),mouse);
    // App forgets to disable capture. Releasing the lease resets peer modes.
    tty.output.clear();tui::park(f.id);tty.reconcile_matrix_selection();assert_eq!(tui::mouse_options(f.id,Some("mouse-scope")).tracking,MouseTracking::Off);
    assert!(String::from_utf8_lossy(&tty.output).contains("\x1b[?1003l"));
    assert_eq!(tui::claim(&t,8),Some(true));tty.output.clear();tty.reconcile_matrix_selection();
    assert!(!String::from_utf8_lossy(&tty.output).contains("\x1b[?1003h"));
    tui::write(&t,8,b"\x1b[?1003h\x1b[?1006h");tty.reconcile_matrix_selection();
    // Explicit disable also reconciles without a lease transition.
    tui::write(&t,8,b"\x1b[?1000l\x1b[?1002l\x1b[?1003l");tty.output.clear();tty.reconcile_matrix_selection();
    assert!(String::from_utf8_lossy(&tty.output).contains("\x1b[?1003l"));
    tui::write(&t,8,b"\x1b[?1003h");tty.reconcile_matrix_selection();tty.output.clear();
    assert_eq!(tui::release(&t,8),Some(true));tty.reconcile_matrix_selection();
    assert!(String::from_utf8_lossy(&tty.output).contains("\x1b[?1003l"));
    assert_eq!(tui::claim(&t,8),Some(true));tui::write(&t,8,b"\x1b[?1003h\x1b[?1006h");tty.reconcile_matrix_selection();tty.output.clear();
    shell2::free_name("mouse-scope");tty.reconcile_matrix_selection();
    assert!(String::from_utf8_lossy(&tty.output).contains("\x1b[?1003l"));
}

#[test] fn native_launch_receipt_follows_current_frontend_and_exact_lease() {
    let f=tui::Frontend{id:tui::new_frontend(),cols:80,rows:12};let moved=tui::Frontend{id:tui::new_frontend(),cols:60,rows:10};
    let t=shell2::target("online-handoff",1);tui::attach_native(f,&t).unwrap();tui::native_return(&t);assert!(tui::take_native_return(f.id));
    tui::request(moved,"online-handoff").unwrap();assert_eq!(tui::native_frontend(&t).unwrap().id,moved.id);
    tui::native_launch(&t,shell2::cmds::run::QueuedBlueprint{slot:"app1".into(),app:"test.bp".into(),sha256:[9;32]});
    assert!(!tui::native_visible(&t));assert!(tui::take_native_launch(f.id).is_none());
    assert_eq!(tui::take_native_launch(moved.id).unwrap().slot,"app1");assert!(tui::take_native_launch(moved.id).is_none());
    tui::request(moved,"online-handoff").unwrap();tui::native_launch(&t,shell2::cmds::run::QueuedBlueprint{slot:"stale".into(),app:"test.bp".into(),sha256:[0;32]});
    shell2::free_name("online-handoff");assert!(tui::take_native_launch(moved.id).is_none());
    let new=shell2::target("online-handoff",2);tui::attach_native(moved,&new).unwrap();
    tui::native_launch(&t,shell2::cmds::run::QueuedBlueprint{slot:"old".into(),app:"test.bp".into(),sha256:[0;32]});assert!(tui::take_native_launch(moved.id).is_none());
}
#[test] fn ssh_does_not_split_mouse_reports_into_standalone_escape_submissions(){
    let (f,t)=session("mouse-batch",9,120,40);
    let shell=Shell3 {tui_frontend:f.id,name:"mouse-batch".into(),size:(120,40),prompt:String::new(),mode:3};
    let mut tty=tty::Terminal::new_ssh(shell);assert_eq!(tui::claim(&t,9),Some(true));
    hv::SUBMISSIONS.lock().unwrap().clear();
    let reports=b"\x1b[<35;88;17M\x1b[<0;88;17M\x1b[<0;88;17m";
    tty.input(reports);
    assert_eq!(*hv::SUBMISSIONS.lock().unwrap(),vec![reports.to_vec()]);
    assert!(tui::active(f.id,Some("mouse-batch")));
    // Remote VM input remains byte-exact, including Unicode and §.
    hv::SUBMISSIONS.lock().unwrap().clear();tty.input("\x1b[Aé\x1b[Bñtail".as_bytes());
    assert_eq!(*hv::SUBMISSIONS.lock().unwrap(),vec!["\x1b[Aé\x1b[Bñtail".as_bytes().to_vec()]);
    assert!(tui::active(f.id,Some("mouse-batch")));
}
'''
    source += r'''
#[test] fn native_helpers_inherit_shell_palette_and_preserve_explicit_qr_colors() {
    let f=frontend(12,5);let t=shell2::target("native-palette",1);
    tui::attach_native(f,&t).unwrap();assert!(tui::native_pending_frame(f.id,Some("native-palette")));
    assert!(tui::snapshot(f.id,Some("native-palette")).is_none());
    assert!(tui::input(f.id,Some("native-palette"),b"\r"));
    tui::native_write(&t,b"\x1b[?25lPIC\x1b[2;1HChoose\x1b[4;1H\x1b[30;47mQR\x1b[0m");
    assert!(!tui::native_pending_frame(f.id,Some("native-palette")));
    let rows=tui::snapshot(f.id,Some("native-palette")).unwrap();
    for cell in &rows[0] {assert_eq!(cell.1.unwrap().background(),Some(update::CONTROL_BACKGROUND));assert_eq!(cell.1.unwrap().rgba(),RgbaColor::White.rgba());}
    for index in [1,2,4] {for cell in &rows[index] {assert_eq!(cell.1.unwrap().background(),Some(update::MATRIX_BACKGROUND));}}
    assert_eq!(rows[3][0].1.unwrap().rgba(),[0,0,0,255]);assert_eq!(rows[3][0].1.unwrap().background(),Some([229,229,229,255]));
    assert_eq!(rows[3][2].1.unwrap().background(),Some(update::MATRIX_BACKGROUND));
    let title=[MetaFmtStr::new("TrueOS")];let shell=update::Snapshot::new((12,5),0,[(&title,&[]),(&[],&[]),(&[],&[])],12).with_matrix(&[],0).rendered_lines();
    let patches=update::diff_rendered_lines(Some(&shell),&rows);assert!(!patches.iter().any(|patch|patch.row==SpecialRows::MatrixRow(1)),"unchanged body background must not be repainted");
    assert_eq!(tui::native_read(&t).unwrap().0,b"\r");
    tui::native_return(&t);assert!(!tui::native_pending_frame(f.id,Some("native-palette")));
    assert!(tui::select_for_navigation(f,"native-palette"));assert_eq!(tui::snapshot(f.id,Some("native-palette")).unwrap(),rows);
    let (vm_frontend,vm_target)=session("vm-palette",15,12,5);tui::claim(&vm_target,15);
    tui::write(&vm_target,15,b"\x1b[?25lVM");let rows=tui::snapshot(vm_frontend.id,Some("vm-palette")).unwrap();assert_eq!(rows[0][0].1.unwrap().background(),Some([0,0,0,255]));
}
#[test] fn native_helpers_can_return_before_the_first_paint() {
    let f=frontend(12,5);let t=shell2::target("native-unpainted",1);tui::attach_native(f,&t).unwrap();
    tui::native_return(&t);assert!(!tui::native_pending_frame(f.id,Some("native-unpainted")));assert!(tui::supports(&t));
    assert!(tui::select_for_navigation(f,"native-unpainted"));assert!(tui::native_pending_frame(f.id,Some("native-unpainted")));
    tui::native_write(&t,b"\x1b[?25lCRY");assert!(tui::snapshot(f.id,Some("native-unpainted")).is_some());
}
#[test] fn native_admin_on_ssh_keeps_cell_frames_input_scope_and_reentry() {
    let f=frontend(70,25);let t=shell2::target("native-admin-ssh",1);
    let shell=Shell3 {tui_frontend:f.id,name:"native-admin-ssh".into(),size:(70,25),prompt:String::new(),mode:3};
    let mut tty=tty::Terminal::new_ssh(shell);tty.output.clear();
    tui::attach_native(f,&t).unwrap();assert_eq!(tui::native_transport_scope(&t),Some(1));
    tui::native_write(&t,"\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[2JCRY  account & keys\x1b[23;1HAuthenticator code: ••••••".as_bytes());
    tty.reconcile_matrix_selection();let out=String::from_utf8_lossy(&tty.output);
    assert!(out.contains("CRY"));assert!(out.contains("••••••"));assert!(out.contains("?1000h"));assert!(out.contains("48;2;16;16;16m"));assert!(out.contains("48;2;24;24;24m"));
    assert!(tui::snapshot(f.id,Some("native-admin-ssh")).is_some());
    tty.input(b"123456\r");assert_eq!(tui::native_read(&t).unwrap().0,b"123456\r");
    tui::native_return(&t);tty.output.clear();tty.reconcile_matrix_selection();
    assert!(String::from_utf8_lossy(&tty.output).contains("TrueOS"));assert!(tui::supports(&t));
    tui::select(f,None);assert!(tui::select_for_navigation(f,"native-admin-ssh"));
    assert!(tui::native_visible(&t));assert_eq!(tui::native_transport_scope(&t),Some(1));
    tty.input("§".as_bytes());assert!(!tui::native_visible(&t));
}
#[test] fn blueprint_launch_notices_stay_with_the_originating_frontend() {
    let a=frontend(20,8);let b=frontend(40,12);let t=shell2::target("store-launch",1);
    tui::attach(a,&t).unwrap();
    let actual=tui::frontend_for_target(&t).unwrap();assert_eq!(actual.id,a.id);assert_eq!((actual.cols,actual.rows),(20,8));
    tui::queue_launch(a.id,service::QueuedBlueprint {slot:"downloaded".into(),app:"gridpaper.bp".into(),sha256:[0;32]});
    assert!(tui::take_native_launch(b.id).is_none());
    let receipt=tui::take_native_launch(a.id).unwrap();assert_eq!(receipt.app,"gridpaper.bp");
    assert!(tui::take_native_launch(a.id).is_none());
}
#[test] fn native_helpers_park_reenter_move_and_retire_with_their_exact_lease() {
    let f=frontend(20,8);let t=shell2::target("native",1);
    tui::attach_native(f,&t).unwrap();assert!(tui::native_slot("native"));assert!(tui::native_visible(&t));assert_eq!(tui::native_transport_scope(&t),Some(2));
    assert!(tui::attach_native(f,&t).is_err());
    assert!(tui::active(f.id,Some("native")));
    let competing=shell2::target("native-competing-vm",1);tui::attach(f,&competing).unwrap();hv::bind(14,&competing);
    assert_eq!(tui::claim(&competing,14),Some(false));
    tui::native_write(&t,b"\x1b[?25l\x1b[?1000h\x1b[?1006hPIC");
    assert_eq!(tui::snapshot(f.id,Some("native")).unwrap()[0][0].0,'P');
    tui::input(f.id,Some("native"),b"\x1b[B\r");
    assert_eq!(tui::native_read(&t).unwrap().0,b"\x1b[B\r");
    tui::native_notice(&t,"saved");assert_eq!(tui::native_read(&t).unwrap().1,vec![String::from("saved")]);
    let other=frontend(30,10);assert!(!tui::select(other,Some("native")));
    tui::native_return(&t);assert!(!tui::active(f.id,Some("native")));assert!(!tui::native_visible(&t));
    assert!(tui::supports(&t));assert!(tui::take_native_return(f.id));assert!(!tui::take_native_return(f.id));
    tui::select(f,None);assert!(tui::select(f,Some("native")));assert!(tui::active(f.id,Some("native")));
    assert_eq!(tui::snapshot(f.id,Some("native")).unwrap()[0][0].0,'P');
    tui::park(f.id);assert!(!tui::active(f.id,Some("native")));tui::select(f,Some("native"));assert!(!tui::active(f.id,Some("native")));
    assert!(tui::select_for_navigation(f,"native"));assert!(tui::active(f.id,Some("native")));assert!(tui::native_visible(&t));tui::park(f.id);
    assert!(tui::select(other,Some("native")));
    assert!(!tui::active(f.id,Some("native")));assert!(tui::active(other.id,Some("native")));
    assert_eq!(tui::surface(&t).unwrap().cols,30);tui::bind_remote_frontend(other.id);assert_eq!(tui::native_transport_scope(&t),Some(1));
    shell2::free_name("native");let replacement=shell2::target("native",2);
    assert!(tui::native_read(&t).is_none());assert!(!tui::native_visible(&t));assert!(!tui::supports(&replacement));
}
#[test] fn native_keyboard_mouse_and_exit_cleanup_use_the_same_terminal_encoding() {
    use r::keyboard::*;
    let f=frontend(20,8);let t=shell2::target("native-input",1);tui::attach_native(f,&t).unwrap();
    tui::native_write(&t,b"\x1b[?1000h\x1b[?1006h");
    let mut e=TrueosKeyboardOutputEvent::default();e.flags=KEYBOARD_OUTPUT_FLAG_PRESS;e.kind=KEYBOARD_OUTPUT_KIND_KEY;e.key_code=KEYBOARD_KEY_ENTER;e.codepoint=13;e.device_seq=7;
    assert!(tui::keyboard(f.id,Some("native-input"),&e));e.kind=KEYBOARD_OUTPUT_KIND_TEXT;assert!(tui::keyboard(f.id,Some("native-input"),&e));
    assert_eq!(tui::native_read(&t).unwrap().0,b"\r");
    let pointer=ui4::Ui4PointerEvent {source:0,window:1,x:0,y:0,local_x:6,local_y:44,dx:0,dy:0,wheel:0,buttons_down:1,buttons_pressed:1,buttons_released:0,combo_id:0,vcursor:false};
    tui::pointer(f.id,Some("native-input"),&pointer,1);
    assert_eq!(tui::native_read(&t).unwrap().0,b"\x1b[<0;2;5M");
    tui::input(f.id,Some("native-input"),b"stale");tui::native_return(&t);
    assert!(tui::native_read(&t).unwrap().0.is_empty());
    assert_eq!(tui::mouse_options(f.id,Some("native-input")).tracking,trueos_terminal::MouseTracking::Off);
    tui::detach(f.id);assert!(!tui::native_slot("native-input"));
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-tui-') as directory:
        path = Path(directory)
        (path/'spin.rs').write_text('pub struct Mutex<T>(std::sync::Mutex<T>);impl<T> Mutex<T> {pub const fn new(t:T)->Self{Self(std::sync::Mutex::new(t))}pub fn lock(&self)->std::sync::MutexGuard<\'_,T>{self.0.lock().unwrap()}}')
        for name, file in [('spin', path/'spin.rs'), ('trueos_terminal', ROOT/'crates/trueos-terminal/src/lib.rs'), ('microfont', ROOT/'vendor/microfont/src/lib.rs')]:
            subprocess.run(['rustc','--edition=2024','--crate-type=rlib','--crate-name',name,str(file),'-o',str(path/f'lib{name}.rlib')],check=True)
        tui_source=(ROOT/'src/shell3/tui.rs').read_text().replace('mod remote;', f'#[path="{ROOT}/src/shell3/tui/remote.rs"] mod remote;')
        (path/'tui.rs').write_text(tui_source)
        (path/'test.rs').write_text(source.replace(str(ROOT/'src/shell3/tui.rs'),str(path/'tui.rs')))
        args = ['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')]
        for name in ('spin','trueos_terminal','microfont'):
            args += ['--extern',f'{name}={path}/lib{name}.rlib']
        subprocess.run(args,check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
