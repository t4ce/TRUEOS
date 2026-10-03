#!/usr/bin/env python3
"""Host regression tests for production Shell3 admission and UI prompt editing."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def method(text, name):
    return re.search(rf'^    (?:pub(?:\([^)]*\))? )?fn {name}\(.*?^    }}', text, re.M | re.S).group()


def main():
    shell = (ROOT/'src/shell3/shell3.rs').read_text()
    service = (ROOT/'src/shell3/service.rs').read_text()
    source = '''#![allow(dead_code)]
extern crate alloc;
use alloc::{string::{String,ToString},collections::VecDeque,vec,vec::Vec};
mod percpu { pub const CPU_SLOT_LIMIT:usize = 32; }
mod hv { pub struct BlueprintInstanceRequest; impl BlueprintInstanceRequest {pub fn default()->Self {Self}}
pub mod blueprint {pub fn prebind_required_readiness(_: &[u8])->Result<u32,String>{Ok(0)}} }
mod r {pub mod readiness {pub fn mask()->u32 {0}} pub mod keyboard {
    pub const KEYBOARD_OUTPUT_KIND_TEXT:u8=1;
    pub const KEYBOARD_OUTPUT_KIND_KEY:u8=2;
    pub const KEYBOARD_KEY_TAB:u16=2;
    pub const KEYBOARD_KEY_ENTER:u16=3;
    pub const KEYBOARD_KEY_BACKSPACE:u16=1;
    pub struct TrueosKeyboardOutputEvent { pub kind:u8,pub key_code:u16,pub codepoint:u32 }
} }
const PROMPT_CURSOR:char='#';
const OPERATOR:char='§';
'''
    for name in ('Mode', 'RgbaColor', 'SpecialRows', 'StripSide', 'PromptState', 'vmx_hash_text', 'vmx_title_meta', 'title_left_text', 'mode_title_meta', 'MatrixSlotsState', 'matrix_slots', 'matrix_slots_meta', 'matrix_slots_text', 'current_matrix_slots_text'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += re.search(r'^impl MatrixSlotsState \{.*?^}', shell, re.M | re.S).group()
    source += re.search(r'^impl MatrixSlots \{.*?^}', shell, re.M | re.S).group()
    source += """
struct MatrixSlots;
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {pub fn new(value:T)->Self {Self(std::sync::Mutex::new(value))} pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}}
    pub struct Once<T>(std::sync::OnceLock<T>);
    impl<T> Once<T> {pub const fn new()->Self {Self(std::sync::OnceLock::new())} pub fn call_once(&self,f:impl FnOnce()->T)->&T {self.0.get_or_init(f)}}
}
use spin::Once;
static MATRIX_SLOTS:Once<spin::Mutex<MatrixSlotsState>>=Once::new();
mod shell2 { pub mod cmds { pub mod run {
#[derive(Clone,Debug)] pub struct QueuedBlueprint { pub slot:alloc::string::String,pub app:alloc::string::String,pub sha256:[u8;32] }
} } }
mod service {
use crate::shell2::cmds::run::QueuedBlueprint;
pub static LAUNCHES:std::sync::Mutex<Vec<(String,String)>>=std::sync::Mutex::new(Vec::new());
use alloc::{vec::Vec,string::String};
pub fn notify_work(){}
fn launched(name:&str,slot:&str,app:&str)->Result<QueuedBlueprint,String> {
    if app == "missing" {return Err("apps: archive not found".into());}
    let mut launches=LAUNCHES.lock().unwrap();
    let fresh=format!("td{}",launches.len());
    launches.push((name.into(),slot.into()));
    Ok(QueuedBlueprint {slot:fresh,app:app.into(),sha256:core::array::from_fn(|i|i as u8)})
}
pub fn launch_appdb(name: &str, slot: &str)->Result<QueuedBlueprint,String>{launched(name,slot,name)}
pub fn launch_alias(name: &str, slot: &str)->Result<QueuedBlueprint,String>{launched(&format!("alias:{name}"),slot,"termdir")}

}
"""
    source += f'\n#[path="{ROOT}/src/shell3/names.rs"] mod names;\nuse names::{{HV_GROUPS,CMD_GROUPS,ADM_NAMES}};\n'
    source += re.search(r'^impl PromptState \{.*?^}', shell, re.M | re.S).group()
    source += f'\n#[path="{ROOT}/src/shell3/metafmtstr.rs"] mod metafmtstr;\nuse metafmtstr::MetaFmtStr;\n'
    source += '''type Row=RowStrips;
struct Rows {promt:Row,title:Row,status:Row}
impl Rows {fn row(&self,row:SpecialRows)->&Row {match row {SpecialRows::TitleRow=>&self.title,SpecialRows::StatusRow=>&self.status,SpecialRows::PromtRow=>&self.promt,_=>panic!()}}}
#[derive(Clone)] struct RowStrips {left:Vec<MetaFmtStr>,right:Vec<MetaFmtStr>}
impl RowStrips {fn new(left:&str,right:&str)->Self {Self {left:vec![MetaFmtStr::new(left)],right:vec![MetaFmtStr::new(right)]}}}

struct Shell3 {prompt:PromptState,rows:Rows,columns:usize,mode:Mode,time:String,aka_names:Vec<String>,appdb_names:Vec<String>,active_matrix_slot:Option<String>,active_matrix_lifetime:Option<u64>,matrix_selection_dirty:bool}
impl Shell3 {
fn new(columns:usize)->Self {Self {prompt:PromptState::new(),rows:Rows {status:Row {left:vec![],right:vec![]},promt:Row {left:vec![],right:vec![]},title:Row {left:vec![MetaFmtStr::new(title_left_text("12:34"))],right:mode_title_meta(Mode::HV,&[],&[])}},columns,mode:Mode::HV,time:"12:34".into(),aka_names:vec![],appdb_names:vec![],active_matrix_slot:None,active_matrix_lifetime:None,matrix_selection_dirty:false}}
'''
    for name in ('active_vmx_app','get_strip','row_for_render','handle_keyboard','refresh_prompt_strip','set_mode','get_mode','refresh_mode_title','echo_recognized_prompt','parse_name','set_appdb_names','select_matrix_slot_index','select_matrix_slot_name','active_matrix_slot_index','active_matrix_slot_name','reconcile_matrix_selection','submit_operator_prompt','parse_operator','set_prompt'):
        source += method(shell, name).replace("pub(super)","pub(crate)")
    source += '}\n'
    source += extract.item('src/shell3/service.rs', 'ShellOwnership')
    source += re.search(r'^impl ShellOwnership \{.*?^}', service, re.M | re.S).group()
    source += extract.item('src/shell3/service.rs', 'advance_round_robin')
    source += extract.item('src/shell3/shell3.rs', 'Shell3Error')
    source += """
const MAX_SHELL3_INSTANCES:usize=256;
#[macro_export] macro_rules! log_warn {($($args:tt)*)=>{ WARNINGS.fetch_add(1,std::sync::atomic::Ordering::Relaxed); };}
static WARNINGS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
mod admission {
use super::*;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))} fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}}
static SHELL_OWNERSHIP:Mutex<ShellOwnership>=Mutex::new(ShellOwnership::new());
struct Notify; impl Notify {fn notify_all(&self){}}
static SHELL_WORK_AVAILABLE:Notify=Notify;
fn refresh_appdb_names(){}
"""
    for name in ('request_shell3','reserve_terminal_slot','reserve_shell_on_executor','release_shell_on_executor','take_pending_for_executor','warn_instance_limit'):
        source += extract.item('src/shell3/service.rs', name)
    source += """
#[test] fn ui_pending_and_network_share_256_cap_and_release_on_owner() {
    SHELL_OWNERSHIP.lock().worker_slots=vec![2,7];
    for expected in [2,2,2,7,7,7] {assert_eq!(request_shell3(),Ok(expected));}
    for index in 0..250 {assert_eq!(reserve_terminal_slot(),Ok(if index%2==0 {2} else {7}));}
    assert_eq!(SHELL_OWNERSHIP.lock().live_shells,250);
    assert_eq!(request_shell3(),Err(Shell3Error::InstanceLimit));
    assert_eq!(reserve_terminal_slot(),Err(Shell3Error::InstanceLimit));
    assert_eq!(WARNINGS.load(std::sync::atomic::Ordering::Relaxed),2);
    assert!(take_pending_for_executor(7));
    assert_eq!(SHELL_OWNERSHIP.lock().live_shells,251);
    release_shell_on_executor(7);
    assert_eq!(reserve_terminal_slot(),Ok(2));
    // Closing an instance does not rewind the initial fill or migrate peers.
    assert_eq!(request_shell3(),Err(Shell3Error::InstanceLimit));
}
}
"""
    source += '''
#[test] fn placement_is_three_each_then_modulo_independent_of_executor_load() {
    let mut o=ShellOwnership::new(); assert_eq!(o.preferred_slot(),None);
    o.worker_slots=vec![2,7,11];
    let sequence=(0..18).map(|_| {let slot=o.preferred_slot().unwrap(); advance_round_robin(&mut o,slot);slot}).collect::<Vec<_>>();
    assert_eq!(sequence,vec![2,2,2,7,7,7,11,11,11,2,7,11,2,7,11,2,7,11]);
    o.shells_by_slot[2]=99; o.shells_by_slot[7]=0; o.pending_by_slot[11]=42;
    assert_eq!(o.preferred_slot(),Some(2));
}
fn key(shell:&mut Shell3,kind:u8,key_code:u16,ch:char)->bool {shell.handle_keyboard(&r::keyboard::TrueosKeyboardOutputEvent {kind,key_code,codepoint:ch as u32})}
#[test] fn bounded_unicode_input_backspace_and_refill() {
    let mut s=Shell3::new(5);
    for ch in "aé😀z".chars() {assert!(key(&mut s,1,0,ch));}
    assert_eq!(s.prompt.cursor,4); assert!(!key(&mut s,1,0,'q'));
    assert_eq!(s.prompt.render(),"aé😀z#");
    assert!(key(&mut s,2,1,'\\0')); assert_eq!(s.prompt.text,"aé😀");
    assert!(key(&mut s,1,0,'q')); assert_eq!(s.prompt.render(),"aé😀q#");
    for _ in 0..4 {assert!(key(&mut s,2,1,'\\0'));}
    assert!(!key(&mut s,2,1,'\\0')); assert_eq!(s.prompt.render(),"#");
}
#[test] fn tab_updates_title_per_instance_and_enter_is_inert() {
    let mut a=Shell3::new(20); let mut b=Shell3::new(20);
    key(&mut a,1,0,'x'); key(&mut b,1,0,'y');
    for mode in [Mode::CMD,Mode::ADM,Mode::HV] {
        assert!(key(&mut a,2,2,'\\t')); assert_eq!(a.mode,mode);
        assert_eq!(a.rows.title.left[0].text,"TrueOS § 12:34");
        let legend:String=a.rows.title.right.iter().map(|run|run.text.as_str()).collect();
        assert_eq!(legend,match mode {Mode::HV=>"[online peer dl] [status pause stop] [snap preserve eject delete kick load store probe]",Mode::CMD=>"[Aka] [Media img shot vid film cam rec] [AppDB]",Mode::ADM=>"cry disc tlb xhci ram smp net bios vgpu vcpy"});
    }
    assert_eq!(b.mode,Mode::HV); assert_eq!(b.prompt.text,"y");
    assert!(!key(&mut a,2,3,'\\r')); assert!(!key(&mut a,1,0,'\\n'));
    assert_eq!(a.prompt.text,"x");
}
#[test] fn editing_at_cursor_preserves_utf8_and_respects_right_strip() {
    let mut s=Shell3::new(6); s.rows.promt.right=vec![MetaFmtStr::new("ok")];
    for ch in "aéz".chars() {assert!(key(&mut s,1,0,ch));}
    assert!(!key(&mut s,1,0,'!'));
    s.prompt.cursor=2; assert!(key(&mut s,2,1,'\\0')); assert_eq!(s.prompt.text,"az");
    assert!(key(&mut s,1,0,'😀')); assert_eq!(s.prompt.render(),"a😀#z");
}
'''
    source += '''
fn type_text(shell:&mut Shell3,text:&str) {for ch in text.chars() {assert!(key(shell,1,0,ch));}}
#[test] fn exact_mode_names_echo_without_enter_and_clear_prompt() {
    MatrixSlots::set(&["id","123"]); matrix_slots().lock().echoes.clear();
    let mut a=Shell3::new(80); let mut b=Shell3::new(80);
    assert!(a.select_matrix_slot_name("id")); assert!(b.select_matrix_slot_name("123"));
    type_text(&mut a,"onlin"); assert_eq!(a.prompt.text,"onlin"); assert!(MatrixSlots::echo_lines(Some("id")).is_empty());
    type_text(&mut a,"e"); assert_eq!(a.prompt.render(),"#"); assert_eq!(a.prompt.cursor,0);
    assert_eq!(MatrixSlots::echo_lines(Some("id")),vec!["online"]);
    type_text(&mut b,"pause"); assert_eq!(MatrixSlots::echo_lines(Some("123")),vec!["pause"]);
    assert_eq!(MatrixSlots::echo_lines(Some("id")),vec!["online"]);
    a.set_mode(3); type_text(&mut a,"net"); assert_eq!(MatrixSlots::echo_lines(Some("id")),vec!["net","online"]);
    // A valid name in another mode is still unknown here.
    type_text(&mut a,"online"); assert_eq!(a.prompt.text,"online");
    assert!(MatrixSlots::echo_lines(None).is_empty());
    let mut c=Shell3::new(80); type_text(&mut c,"dl");
    assert_eq!(MatrixSlots::echo_lines(None),vec!["dl"]);
    assert_eq!(MatrixSlots::echo_lines(Some("123")),vec!["pause"]);
}
#[test] fn appdb_name_launches_once_in_fresh_slot_without_echo() {
    service::LAUNCHES.lock().unwrap().clear();
    MatrixSlots::set(&["app"]);
    let mut s=Shell3::new(80); s.set_appdb_names(&["Demo".into()]);
    s.select_matrix_slot_name("app"); s.set_mode(2);
    type_text(&mut s,"Dem"); assert!(service::LAUNCHES.lock().unwrap().is_empty());
    type_text(&mut s,"o");
    assert_eq!(*service::LAUNCHES.lock().unwrap(),vec![("Demo".into(),"app".into())]);
    assert!(MatrixSlots::echo_lines(Some("app")).is_empty());
    assert_eq!(s.active_matrix_slot_name(),Some("td0".into()));
    assert!(MatrixSlots::echo_lines(Some("td0")).is_empty());
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Left),"TrueOS § 12:34 Demo 0001020304050607…18191a1b1c1d1e1f");
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Right),"VME tui env smp esc");
    assert!(!key(&mut s,2,2,'\\t'));assert_eq!(s.mode,Mode::CMD);
    for name in ["tui","env","smp","esc"] {assert!(s.parse_name(name));}
    assert!(!s.parse_name("Demo"));assert!(!s.parse_name("online"));
    assert!(s.parse_operator("§"));
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Left),"TrueOS § 12:34");
    assert!(key(&mut s,2,2,'\\t'));assert_eq!(s.mode,Mode::ADM);
    assert!(s.select_matrix_slot_name("td0"));
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Right),"VME tui env smp esc");
    assert_eq!(s.prompt.text,"");
}
#[test] fn aka_name_launches_once_in_cmd_with_selected_slot() {
    service::LAUNCHES.lock().unwrap().clear();
    MatrixSlots::set(&["aka"]);
    let mut s=Shell3::new(80); s.aka_names=vec!["hello".into()];
    s.select_matrix_slot_name("aka");
    type_text(&mut s,"hello"); assert!(service::LAUNCHES.lock().unwrap().is_empty());
    s.set_prompt(""); s.set_mode(2);
    type_text(&mut s,"hell"); assert!(service::LAUNCHES.lock().unwrap().is_empty());
    type_text(&mut s,"o");
    assert_eq!(*service::LAUNCHES.lock().unwrap(),vec![("alias:hello".into(),"aka".into())]);
    assert!(MatrixSlots::echo_lines(Some("aka")).is_empty());
    assert_eq!(s.active_matrix_slot_name(),Some("td0".into()));
    assert!(s.get_strip(SpecialRows::TitleRow,StripSide::Left).contains("termdir"));
    assert_eq!(s.prompt.text,"");
}
#[test] fn command_legend_uses_live_names_and_preserves_admin_colors() {
    MatrixSlots::set(&["id","123"]); matrix_slots().lock().echoes.clear();
    let mut s=Shell3::new(80); s.aka_names=vec!["hello".into()];
    s.set_appdb_names(&["Demo".into()]);
    assert!(s.rows.title.right.iter().all(|run|run.text!="Demo"));
    s.set_mode(2);
    assert_eq!(s.rows.title.right.iter().map(|run|run.text.as_str()).collect::<String>(),"[Aka hello] [Media img shot vid film cam rec] [AppDB Demo]");
    for name in ["hello","img","Demo"] {type_text(&mut s,name);assert_eq!(s.prompt.render(),"#");s.select_matrix_slot_index(0);}
    assert_eq!(MatrixSlots::echo_lines(None),vec!["img"]);
    s.set_mode(3); let admin=s.rows.title.right.clone();
    assert_eq!(admin[0].color,Some(RgbaColor::Pink));assert_eq!(admin[2].color,Some(RgbaColor::Pink));
    s.set_appdb_names(&["Other".into()]);assert_eq!(s.rows.title.right,admin);
    s.set_mode(2);assert!(s.rows.title.right.iter().any(|run|run.text=="Other"));
    assert!(s.rows.title.right.iter().all(|run|run.text!="Demo"));
}
'''
    source += '''
#[test] fn repeated_launch_keeps_previous_slot_identity_and_shared_views() {
    service::LAUNCHES.lock().unwrap().clear();MatrixSlots::set(&[] as &[&str]);
    let mut a=Shell3::new(100);a.set_mode(2);a.set_appdb_names(&["termdir".into()]);
    type_text(&mut a,"termdir");let first=a.active_matrix_slot_name().unwrap();
    a.select_matrix_slot_index(0);type_text(&mut a,"termdir");let second=a.active_matrix_slot_name().unwrap();
    assert_ne!(first,second);assert!(first.len()<=5 && second.len()<=5);
    let mut b=Shell3::new(100);b.select_matrix_slot_name(&first);
    assert_eq!(b.get_strip(SpecialRows::TitleRow,StripSide::Right),"VME tui env smp esc");
    assert!(b.row_for_render(SpecialRows::TitleRow).left[1].bold);
    assert!(!key(&mut b,2,2,'\\t'));assert_eq!(b.mode,Mode::HV);
    assert!(b.parse_operator(&format!("§{first}§")));b.parse_operator(&format!("§{first}"));
    assert!(b.active_vmx_app().is_none());assert!(key(&mut b,2,2,'\\t'));
    assert!(a.active_vmx_app().is_some());
}
#[test] fn failed_launch_keeps_selection_and_echoes_only_error() {
    MatrixSlots::set(&["old"]);let mut s=Shell3::new(80);s.set_mode(2);
    s.select_matrix_slot_name("old");s.set_appdb_names(&["missing".into()]);
    type_text(&mut s,"missing");assert_eq!(s.active_matrix_slot_name(),Some("old".into()));
    assert!(s.active_vmx_app().is_none());
    assert_eq!(MatrixSlots::echo_lines(Some("old")),vec!["apps: archive not found"]);
}
'''
    source += '''
fn submit(shell:&mut Shell3,input:&str)->bool {shell.set_prompt("");type_text(shell,input);key(shell,2,3,'\\r')}
#[test] fn operators_wait_for_enter_then_navigate_create_and_clear() {
    MatrixSlots::set(&["id","123"]);matrix_slots().lock().echoes.clear();
    let mut a=Shell3::new(80);let mut b=Shell3::new(80);
    a.set_mode(2);a.aka_names=vec!["§abc".into()];
    type_text(&mut a,"§abc");assert_eq!(a.prompt.text,"§abc");assert_eq!(a.active_matrix_slot_name(),None);
    assert!(!MatrixSlots::slot_ids().iter().any(|id|id=="abc"));assert!(MatrixSlots::echo_lines(None).is_empty());
    assert!(key(&mut a,2,3,'\\r'));assert_eq!(a.active_matrix_slot_name(),Some("abc".into()));assert_eq!(a.prompt.render(),"#");
    assert_eq!(a.prompt.cursor,0);assert_eq!(a.mode,Mode::CMD);assert_eq!(b.active_matrix_slot_name(),None);
    assert!(submit(&mut b,"§abc"));assert_eq!(MatrixSlots::slot_ids().iter().filter(|id|id.as_str()=="abc").count(),1);
    assert!(submit(&mut a,"§"));assert_eq!(a.active_matrix_slot_index(),0);assert_eq!(b.active_matrix_slot_name(),Some("abc".into()));
    assert!(MatrixSlots::echo_lines(Some("abc")).is_empty());
}
#[test] fn dropping_named_slot_frees_shared_data_and_cannot_resurrect_old_views() {
    MatrixSlots::set(&["id","123"]);matrix_slots().lock().echoes.clear();
    let mut a=Shell3::new(80);let mut b=Shell3::new(80);let mut c=Shell3::new(80);
    assert!(submit(&mut a,"§abc"));assert!(submit(&mut b,"§abc"));type_text(&mut a,"online");
    assert_eq!(MatrixSlots::echo_lines(Some("abc")),vec!["online"]);
    let old=b.active_matrix_lifetime;let generation=matrix_slots().lock().generation;
    assert!(submit(&mut c,"§abc§"));assert!(!MatrixSlots::slot_ids().iter().any(|id|id=="abc"));
    assert!(matrix_slots().lock().echoes.iter().all(|(id,_)|id.as_deref()!=Some("abc")));
    assert!(matrix_slots().lock().generation>generation);assert_eq!(a.active_matrix_slot_name(),None);assert_eq!(b.active_matrix_slot_index(),0);
    // Recreate before AP owners reconcile. An old view must still be default.
    assert!(submit(&mut c,"§abc"));assert_ne!(c.active_matrix_lifetime,old);
    assert_eq!(b.active_matrix_slot_name(),None);
    assert!(MatrixSlots::view_echo_snapshot(b.active_matrix_slot.as_deref(),b.active_matrix_lifetime).1.is_empty());
    type_text(&mut b,"pause");assert_eq!(MatrixSlots::echo_lines(None),vec!["pause"]);assert!(MatrixSlots::echo_lines(Some("abc")).is_empty());
    a.reconcile_matrix_selection();b.reconcile_matrix_selection();assert_eq!(a.active_matrix_slot,None);assert_eq!(b.active_matrix_slot,None);
    assert_eq!(a.active_matrix_lifetime,None);assert!(a.matrix_selection_dirty);assert!(b.matrix_selection_dirty);
}
#[test] fn double_section_resets_default_and_preserves_other_slots() {
    MatrixSlots::set(&["id","123"]);matrix_slots().lock().echoes.clear();
    let mut a=Shell3::new(80);let mut b=Shell3::new(80);
    type_text(&mut a,"online");assert!(submit(&mut b,"§id"));type_text(&mut b,"pause");
    assert!(submit(&mut b,"§§"));assert_eq!(b.active_matrix_slot_name(),Some("id".into()));
    assert!(MatrixSlots::echo_lines(None).is_empty());assert_eq!(MatrixSlots::echo_lines(Some("id")),vec!["pause"]);
    type_text(&mut a,"stop");assert_eq!(MatrixSlots::echo_lines(None),vec!["stop"]);
    assert!(submit(&mut a,"§§"));assert_eq!(a.prompt.render(),"#");assert_eq!(a.active_matrix_slot_index(),0);
    assert!(MatrixSlots::echo_lines(None).is_empty());type_text(&mut a,"dl");assert_eq!(MatrixSlots::echo_lines(None),vec!["dl"]);
}
#[test] fn malformed_operator_and_ordinary_enter_are_inert() {
    MatrixSlots::set(&["id","123"]);matrix_slots().lock().echoes.clear();let mut s=Shell3::new(80);
    for text in ["unknown","§abc§def","§§abc","§a b","§§§"] {
        assert!(!submit(&mut s,text));assert_eq!(s.prompt.text,text);assert_eq!(s.active_matrix_slot_name(),None);
    }
    assert!(MatrixSlots::echo_lines(None).is_empty());assert_eq!(MatrixSlots::slot_ids(),vec!["id","123"]);
}
'''
    # Exercise the production queue receipt, including a pre-existing Shell3
    # tag and reservation cleanup, rather than only mocking launch dispatch.
    source += '''
mod queue_receipt {
use super::*;
use shell2::cmds::run::QueuedBlueprint;
#[derive(Clone)] pub(crate) struct MatrixTarget {slot_id:String}
static RESERVED:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
static QUEUED:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
struct Sha256;impl Sha256 {fn digest(bytes:&[u8])->[u8;32] {assert_eq!(bytes,b"archive bytes");[42;32]}}
fn log_run_target_line(_: &MatrixTarget,_:&str) {}
fn readiness_mask_text(_:u32)->String {String::new()}
fn name_occupied_default_instance(_: &MatrixTarget,_:&str,i:hv::BlueprintInstanceRequest)->hv::BlueprintInstanceRequest {i}
fn reserve_target_for_archive(_: &MatrixTarget,archive:&str)->MatrixTarget {
    assert_eq!(archive,"termdir.bp");let mut held=RESERVED.lock().unwrap();
    let id=(0..100).map(|i|format!("td{i}")).find(|id|!held.contains(id)).unwrap();
    held.push(id.clone());MatrixTarget {slot_id:id}
}
fn release_matrix_target_vm_reservation(t:&MatrixTarget) {RESERVED.lock().unwrap().retain(|id|id!=&t.slot_id);}
fn app_label_for_archive(archive:&str)->&str {archive.trim_end_matches(".bp")}
fn app_label_for_instance(archive:&str,_:&hv::BlueprintInstanceRequest)->String {app_label_for_archive(archive).into()}
fn set_matrix_target_app_identity(t:&MatrixTarget,label:&str,hash:[u8;32]) {assert_eq!(t.slot_id,"td1");assert_eq!(label,"termdir");assert_eq!(hash,[42;32]);}
fn enqueue_blueprint_request(t:MatrixTarget,archive:String,_:&str,bytes:Vec<u8>,_:Vec<String>,_:Option<String>,_:hv::BlueprintInstanceRequest,_:bool,_:Option<()>) {
    assert_eq!(archive,"termdir.bp");assert_eq!(bytes,b"archive bytes");QUEUED.lock().unwrap().push(t.slot_id);
}
'''
    source += extract.item('src/shell2/cmds/run.rs', 'enqueue_blueprint_bytes_with_receipt')
    source += '''
#[test] fn receipt_names_actual_fresh_reserved_slot_and_archive_hash() {
    let receipt=enqueue_blueprint_bytes_with_receipt(MatrixTarget {slot_id:String::new()},"termdir.bp".into(),b"archive bytes".to_vec(),vec![],hv::BlueprintInstanceRequest::default(),None,&["td0".into()]).unwrap();
    assert_eq!(receipt.slot,"td1");assert_eq!(receipt.app,"termdir");assert_eq!(receipt.sha256,[42;32]);
    assert_eq!(*RESERVED.lock().unwrap(),vec!["td1"]);assert_eq!(*QUEUED.lock().unwrap(),vec![receipt.slot]);
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-pool-input-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
