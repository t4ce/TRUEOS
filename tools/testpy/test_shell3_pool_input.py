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
    pub const KEYBOARD_KEY_ESCAPE:u16=4;
    pub const KEYBOARD_OUTPUT_FLAG_PRESS:u32=1;
    #[derive(Default)] pub struct TrueosKeyboardOutputEvent { pub kind:u8,pub key_code:u16,pub codepoint:u32,pub flags:u32,pub utf8:[u8;4],pub utf8_len:u8 }
} }
const PROMPT_CURSOR:char=' ';
const SpecialSeperator:char='│';
const OPERATOR:char='§';
'''
    for name in ('Mode', 'RgbaColor', 'SpecialRows', 'StripSide', 'PromptState', 'vmx_hash_text', 'vmx_title_meta', 'title_left_text', 'mode_title_meta', 'MatrixSlotsState', 'matrix_slots', 'matrix_slots_meta', 'matrix_slots_text', 'current_matrix_slots_text'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += re.search(r'^impl RgbaColor \{.*?^}', shell, re.M | re.S).group()
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
mod tui {
pub fn mouse_options(_:u64,_:Option<&str>)->trueos_terminal::MouseOptions {Default::default()}
pub fn snapshot(_:u64,_:Option<&str>)->Option<Vec<crate::update::RenderedLine>> {None}
pub fn input(_:u64,_:Option<&str>,_:&[u8])->bool {false}
#[derive(Clone,Copy)] pub struct Frontend {pub id:u64,pub cols:usize,pub rows:usize}
pub fn select(_:Frontend,_:Option<&str>)->bool {true}
pub fn park(_:u64)->bool {true}
pub fn keyboard(_:u64,_:Option<&str>,_:&crate::r::keyboard::TrueosKeyboardOutputEvent)->bool {false}
pub static REQUESTS:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
pub fn request(_:Frontend,name:&str)->Result<(),&'static str>{REQUESTS.lock().unwrap().push(name.into());Ok(())}
}
mod shell3 {pub mod tui {pub use crate::tui::Frontend;pub fn attach<T>(_:Frontend,_:&T)->Result<(),String>{Ok(())}}}
mod shell2 { pub mod cmds { pub mod run {
#[derive(Clone,Debug)] pub struct QueuedBlueprint { pub slot:alloc::string::String,pub app:alloc::string::String,pub sha256:[u8;32] }
} } }
mod service {
use crate::shell2::cmds::run::QueuedBlueprint;
pub static LAUNCHES:std::sync::Mutex<Vec<(String,String)>>=std::sync::Mutex::new(Vec::new());
use alloc::{vec::Vec,string::String};
pub fn notify_work(){}
pub static DROPS:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
pub fn drop_vmx_slot(name:&str) {DROPS.lock().unwrap().push(name.into());}
fn launched(name:&str,slot:&str,app:&str)->Result<QueuedBlueprint,String> {
    if app == "missing" {return Err("apps: archive not found".into());}
    let mut launches=LAUNCHES.lock().unwrap();
    let fresh=format!("td{}",launches.len());
    launches.push((name.into(),slot.into()));
    Ok(QueuedBlueprint {slot:fresh,app:app.into(),sha256:core::array::from_fn(|i|i as u8)})
}
pub fn launch_appdb(name: &str, slot: &str,_:crate::tui::Frontend)->Result<QueuedBlueprint,String>{launched(name,slot,name)}
pub fn launch_alias(name: &str, slot: &str,_:crate::tui::Frontend)->Result<QueuedBlueprint,String>{launched(&format!("alias:{name}"),slot,"termdir")}

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

struct Shell3 {matrix_scroll:usize,layout_generation:usize,status_hover:Option<status::Target>,tui_frontend:u64,rows_count:usize,prompt:PromptState,rows:Rows,columns:usize,mode:Mode,time:String,aka_names:Vec<String>,appdb_names:Vec<String>,active_matrix_slot:Option<String>,active_matrix_lifetime:Option<u64>,matrix_selection_dirty:bool}
impl Shell3 {
fn new(columns:usize)->Self {Self {matrix_scroll:0,layout_generation:0,status_hover:None,tui_frontend:1,rows_count:25,prompt:PromptState::new(),rows:Rows {status:Row {left:vec![],right:vec![]},promt:Row {left:vec![],right:vec![]},title:Row {left:vec![MetaFmtStr::new(title_left_text("12:34"))],right:mode_title_meta(Mode::HV,&[],&[])}},columns,mode:Mode::HV,time:"12:34".into(),aka_names:vec![],appdb_names:vec![],active_matrix_slot:None,active_matrix_lifetime:None,matrix_selection_dirty:false}}
'''
    for name in ('scroll_matrix','capture_matrix_snapshot','record_terminal_notice','capture_controls_snapshot','launch_named_app','tui_frontend','stop_active_vmx','active_vmx_app','get_strip','row_for_render','handle_keyboard','handle_keyboard_with_latch','any_name_matching','refresh_prompt_strip','set_mode','get_mode','refresh_mode_title','echo_recognized_prompt','parse_name','set_appdb_names','select_matrix_slot_index','select_matrix_slot_name','active_matrix_slot_index','active_matrix_slot_name','reconcile_matrix_selection','submit_operator_prompt','parse_operator','set_prompt','set_cursor','prompt','replay_terminal_line'):
        source += method(shell, name).replace("pub(super)","pub(crate)")
    source += 'fn get_size(&self)->(usize,usize) {(self.columns,self.rows_count)} fn set(&mut self,cols:usize,rows:usize) {self.columns=cols;self.rows_count=rows;} }\n'
    source += 'mod tty {\n' + (ROOT/'src/shell3/tty.rs').read_text().replace('//!','//').replace('mod input;', f'#[path="{ROOT}/src/shell3/tty/input.rs"] mod input;').replace('mod ansi;', f'#[path="{ROOT}/src/shell3/tty/ansi.rs"] mod ansi;') + '\n'
    source += r'''
#[cfg(test)] mod replay_tests {
    use super::*;
    use crate::{MatrixSlots, matrix_slots, service, Mode, StripSide, key, type_text};
    #[test] fn ssh_mouse_reports_do_not_hover_select_or_launch() {
        MatrixSlots::set(&["id","123"]);service::LAUNCHES.lock().unwrap().clear();
        let mut shell=Shell3::new(40);shell.aka_names=vec!["héllo".into()];
        let mut tty=Terminal::new_ssh(shell);tty.input(b"draft");tty.output.clear();
        tty.input(b"\x1b[<35;4;2M");
        assert!(!tty.shell.row_for_render(SpecialRows::StatusRow).left[2].underline);
        assert!(tty.output.is_empty());
        tty.input(b"\x1b[<0;4;2M\x1b[<0;4;2m");
        assert_eq!(tty.shell.active_matrix_slot_name(),None);
        tty.input(b"\x1b[<0;36;2M\x1b[<32;36;2M\x1b[<0;36;2m\x1b[<64;36;2M");
        assert!(service::LAUNCHES.lock().unwrap().is_empty());
        assert_eq!(tty.line,"draft");assert_eq!(tty.shell.prompt(),"draft");
        tty.input(b"\x1b[<35;36;3M");
        assert!(tty.shell.status_hover.is_none());
        service::LAUNCHES.lock().unwrap().clear();MatrixSlots::set(&[] as &[&str]);
    }
    #[test] fn shared_matrix_viewport_scrolls_newest_first_and_resets_on_selection() {
        MatrixSlots::set(&["history"]);
        let mut s=Shell3::new(40);s.rows_count=6;s.select_matrix_slot_name("history");
        for n in (0..7).rev() {MatrixSlots::echo(Some("history"),s.active_matrix_lifetime,format!("entry{n}"));}
        assert_eq!(s.get_strip(SpecialRows::MatrixRow(0),StripSide::Left),"entry0");
        assert!(s.scroll_matrix(1));assert_eq!(s.get_strip(SpecialRows::MatrixRow(0),StripSide::Left),"entry1");
        assert!(s.scroll_matrix(100));assert_eq!(s.matrix_scroll,4);assert!(!s.scroll_matrix(1));
        assert!(s.scroll_matrix(-100));assert_eq!(s.matrix_scroll,0);
        s.scroll_matrix(2);let mut tty=Terminal::new_ssh(s);tty.input(b"\x1b[<65;2;4M");
        assert_eq!(tty.shell.matrix_scroll,2);assert_eq!(tty.shell.get_strip(SpecialRows::MatrixRow(0),StripSide::Left),"entry2");
        tty.input(b"\x1b[<64;2;2M");assert_eq!(tty.shell.matrix_scroll,2);
        tty.shell.select_matrix_slot_index(0);assert_eq!(tty.shell.matrix_scroll,0);
        MatrixSlots::set(&[] as &[&str]);
    }
    #[test] fn ssh_typing_tab_and_operator_share_ui4_keyboard_semantics() {
        MatrixSlots::set(&[] as &[&str]);matrix_slots().lock().echoes.clear();
        let mut tty=Terminal::new_ssh(Shell3::new(100));
        let mut ui=Shell3::new(100);ui.set_mode(3);
        tty.input(b"\t");key(&mut ui,2,2,'\0');
        assert_eq!(tty.shell.get_mode(),ui.get_mode());
        tty.input(b"online");type_text(&mut ui,"online");
        assert_eq!(MatrixSlots::echo_lines(None),vec!["online","online"]);
        assert_eq!(tty.history.entries,vec!["online"]);
        assert_eq!(tty.line,"");assert_eq!(tty.shell.prompt(),ui.prompt());
        tty.input("§fresh".as_bytes());type_text(&mut ui,"§fresh");
        assert_eq!(tty.shell.active_matrix_slot_name(),None);
        assert_eq!(tty.shell.prompt(),ui.prompt());
        tty.input(b"\r");key(&mut ui,2,3,'\r');
        assert_eq!(tty.shell.active_matrix_slot_name(),ui.active_matrix_slot_name());
        assert_eq!(tty.shell.prompt(),ui.prompt());
        tty.input("éx\x7f".as_bytes());type_text(&mut ui,"éx");key(&mut ui,2,1,'\0');
        assert_eq!(tty.shell.prompt(),ui.prompt());
        // An unmatched ordinary Enter preserves text, exactly as UI4 does.
        tty.input(b"\r");key(&mut ui,2,3,'\r');assert_eq!(tty.shell.prompt(),ui.prompt());
        MatrixSlots::set(&[] as &[&str]);matrix_slots().lock().echoes.clear();
    }
    #[test] fn network_defers_until_enter_and_first_latch_discards_tail() {
        MatrixSlots::set(&[] as &[&str]);matrix_slots().lock().echoes.clear();
        let mut tty=Terminal::new(Shell3::new(100));tty.shell.set_mode(1);
        tty.input(b"onlinepause");
        assert!(MatrixSlots::echo_lines(None).is_empty());
        tty.input(b"\r");tty.input(b"\n");
        assert_eq!(MatrixSlots::echo_lines(None),vec!["online"]);
        assert_eq!(tty.shell.prompt(),"");assert_eq!(tty.line,"");
        tty.input(b"pause\n");
        assert_eq!(MatrixSlots::echo_lines(None),vec!["pause","online"]);
        matrix_slots().lock().echoes.clear();
    }
    #[test] fn replay_launches_once_and_discards_vme_tail() {
        MatrixSlots::set(&[] as &[&str]);service::LAUNCHES.lock().unwrap().clear();
        let mut shell=Shell3::new(100);shell.set_appdb_names(&["termdir".into()]);
        let mut tty=Terminal::new(shell);tty.shell.set_mode(1);
        tty.input(b"tab\n");assert_eq!(tty.shell.mode,Mode::CMD);
        tty.input(b"termdirescstop\n");
        assert_eq!(*service::LAUNCHES.lock().unwrap(),vec![("termdir".into(),"".into())]);
        assert!(tty.shell.active_vmx_app().is_some());assert_eq!(tty.line,"");
        tty.input(b"tab\n");assert_eq!(tty.shell.mode,Mode::CMD);
        tty.input(b"stoptermdir\n");assert!(tty.shell.active_vmx_app().is_none());
        assert_eq!(service::LAUNCHES.lock().unwrap().len(),1);
        tty.input(b"tab\n");assert_eq!(tty.shell.mode,Mode::ADM);
    }
    #[test] fn unicode_latch_discards_tail_and_operator_still_waits_for_enter() {
        MatrixSlots::set(&[] as &[&str]);service::LAUNCHES.lock().unwrap().clear();
        let mut shell=Shell3::new(100);shell.set_mode(2);shell.aka_names=vec!["héllo".into()];
        let mut tty=Terminal::new(shell);tty.shell.set_mode(2);
        tty.input("hélloescrest\n".as_bytes());
        assert_eq!(service::LAUNCHES.lock().unwrap()[0].0,"alias:héllo");
        assert!(tty.shell.active_vmx_app().is_some());assert_eq!(tty.line,"");
        tty.input("§new".as_bytes());assert_ne!(tty.shell.active_matrix_slot_name(),Some("new".into()));
        tty.input(b"\n");assert_eq!(tty.shell.active_matrix_slot_name(),Some("new".into()));
        assert_eq!(tty.line,"");matrix_slots().lock().echoes.clear();
    }
    #[test] fn impossible_prefix_stops_before_later_names_or_operators() {
        MatrixSlots::set(&[] as &[&str]);matrix_slots().lock().echoes.clear();
        let mut tty=Terminal::new(Shell3::new(100));tty.shell.set_mode(1);
        tty.input("onXonline§new\n".as_bytes());
        assert_eq!(tty.line,"");assert_eq!(tty.shell.prompt(),"");
        assert!(MatrixSlots::echo_lines(None).is_empty());
        assert!(!MatrixSlots::slot_ids().contains(&"new".into()));
        tty.input(b"on\n");assert_eq!(tty.line,"");
        tty.input(b"linepause\n");assert!(MatrixSlots::echo_lines(None).is_empty());
        tty.input(b"onlinepause\n");assert_eq!(MatrixSlots::echo_lines(None),vec!["online"]);
        assert_eq!(tty.line,"");matrix_slots().lock().echoes.clear();
    }
    #[test] fn dynamic_names_share_prefix_registry_and_shortest_match_wins() {
        MatrixSlots::set(&[] as &[&str]);service::LAUNCHES.lock().unwrap().clear();
        let mut shell=Shell3::new(100);shell.set_mode(2);
        shell.aka_names=vec!["hé".into(),"héllo".into()];
        let mut tty=Terminal::new(shell);tty.shell.set_mode(2);tty.input("héllo\n".as_bytes());
        assert_eq!(service::LAUNCHES.lock().unwrap()[0].0,"alias:hé");
        assert_eq!(service::LAUNCHES.lock().unwrap().len(),1);assert_eq!(tty.line,"");
        matrix_slots().lock().echoes.clear();
    }
}
}
'''
    source += f'#[path="{ROOT}/src/shell3/status.rs"] mod status;\n'
    source += f'#[path="{ROOT}/src/shell3/update.rs"] mod update;\n'
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
fn key(shell:&mut Shell3,kind:u8,key_code:u16,ch:char)->bool {shell.handle_keyboard(&r::keyboard::TrueosKeyboardOutputEvent {kind,key_code,codepoint:ch as u32,flags:1,..Default::default()})}
#[test] fn bounded_unicode_input_backspace_and_refill() {
    let mut s=Shell3::new(5);
    for ch in "aé😀z".chars() {assert!(key(&mut s,1,0,ch));}
    assert_eq!(s.prompt.cursor,4); assert!(!key(&mut s,1,0,'q'));
    assert_eq!(s.prompt.render(),"aé😀z ");
    assert!(key(&mut s,2,1,'\\0')); assert_eq!(s.prompt.text,"aé😀");
    assert!(key(&mut s,1,0,'q')); assert_eq!(s.prompt.render(),"aé😀q ");
    for _ in 0..4 {assert!(key(&mut s,2,1,'\\0'));}
    assert!(!key(&mut s,2,1,'\\0')); assert_eq!(s.prompt.render()," ");
}
#[test] fn tab_updates_title_per_instance_and_enter_is_inert() {
    let mut a=Shell3::new(20); let mut b=Shell3::new(20);
    key(&mut a,1,0,'x'); key(&mut b,1,0,'y');
    for mode in [Mode::CMD,Mode::ADM,Mode::HV] {
        assert!(key(&mut a,2,2,'\\t')); assert_eq!(a.mode,mode);
        assert_eq!(a.rows.title.left[0].text,"TrueOS § 12:34");
        let legend:String=a.rows.title.right.iter().map(|run|run.text.as_str()).collect();
        assert_eq!(legend,match mode {Mode::HV=>"[online peer dl] [status pause stop] [snap preserve eject delete kick load store probe]",Mode::CMD=>"Capture[img vid aud vaud] AppDB[]",Mode::ADM=>"cry disc tlb xhci ram smp net bios vgpu vcpy"});
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
    assert!(key(&mut s,1,0,'😀')); assert_eq!(s.prompt.render(),"a😀 z");
}
'''
    source += '''
fn type_text(shell:&mut Shell3,text:&str) {for ch in text.chars() {assert!(key(shell,1,0,ch));}}
#[test] fn terminal_slots_skip_existing_names_and_allocate_atomically() {
    MatrixSlots::set(&["sh1", "sh3", "custom"]);
    let (name, lifetime)=MatrixSlots::fresh_terminal_slot(None);
    assert_eq!(name,"sh2");
    assert_eq!(MatrixSlots::fresh_terminal_slot(None).0,"sh4");
    assert_eq!(MatrixSlots::fresh_terminal_slot(Some(49152)).0,"49152");
    assert_eq!(MatrixSlots::fresh_terminal_slot(Some(49152)).0,"sh5");
    assert_eq!(MatrixSlots::fresh_terminal_slot(Some(0)).0,"sh6");
    let threads:Vec<_>=(0..8).map(|_|std::thread::spawn(|| MatrixSlots::fresh_terminal_slot(Some(65535)))).collect();
    let mut names:Vec<_>=threads.into_iter().map(|t|t.join().unwrap().0).collect();
    assert!(names.iter().any(|name|name=="65535"));
    names.sort();names.dedup();assert_eq!(names.len(),8);
    assert_eq!(matrix_slots().lock().lifetimes.iter().find(|(id,_)|id==&name).unwrap().1,lifetime);
}
#[test] fn exact_mode_names_echo_without_enter_and_clear_prompt() {
    MatrixSlots::set(&["id","123"]); matrix_slots().lock().echoes.clear();
    let mut a=Shell3::new(80); let mut b=Shell3::new(80);
    assert!(a.select_matrix_slot_name("id")); assert!(b.select_matrix_slot_name("123"));
    type_text(&mut a,"onlin"); assert_eq!(a.prompt.text,"onlin"); assert!(MatrixSlots::echo_lines(Some("id")).is_empty());
    type_text(&mut a,"e"); assert_eq!(a.prompt.render()," "); assert_eq!(a.prompt.cursor,0);
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
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Right),"VME tui env smp esc stop");
    assert!(!key(&mut s,2,2,'\\t'));assert_eq!(s.mode,Mode::CMD);
    for name in ["tui","env","smp","esc","stop"] {assert!(s.parse_name(name));}
    assert!(!s.parse_name("Demo"));assert!(!s.parse_name("online"));
    assert!(s.parse_operator("§"));
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Left),"TrueOS § 12:34");
    assert!(key(&mut s,2,2,'\\t'));assert_eq!(s.mode,Mode::ADM);
    assert!(s.select_matrix_slot_name("td0"));
    assert_eq!(s.get_strip(SpecialRows::TitleRow,StripSide::Right),"VME tui env smp esc stop");
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
    assert_eq!(s.rows.title.right.iter().map(|run|run.text.as_str()).collect::<String>(),"Capture[img vid aud vaud] AppDB[Demo]");
    for name in ["hello","img","Demo"] {type_text(&mut s,name);assert_eq!(s.prompt.render()," ");s.select_matrix_slot_index(0);}
    assert_eq!(MatrixSlots::echo_lines(None),vec!["img"]);
    s.set_mode(3); let admin=s.rows.title.right.clone();
    assert_eq!(admin[0].color,Some(RgbaColor::Pink));assert_eq!(admin[2].color,Some(RgbaColor::Pink));
    s.set_appdb_names(&["Other".into()]);assert_eq!(s.rows.title.right,admin);
    s.set_mode(2);assert!(s.rows.title.right.iter().any(|run|run.text=="Other"));
    assert!(s.rows.title.right.iter().all(|run|run.text!="Demo"));
}
#[test] fn status_links_hover_navigate_and_launch_without_changing_prompt() {
    MatrixSlots::set(&["id","123"]);
    service::LAUNCHES.lock().unwrap().clear();
    let mut s=Shell3::new(40);s.aka_names=vec!["héllo".into()];s.set_prompt("draft");
    assert_eq!(s.get_strip(SpecialRows::StatusRow,StripSide::Right),"Aka[héllo]");
    assert!(s.handle_status_pointer(Some(3),false));
    let status=s.row_for_render(SpecialRows::StatusRow);
    assert!(status.left[2].underline && status.left[3].underline);
    assert!(!status.left[0].underline);assert!(!status.right.iter().any(|run|run.underline));
    s.handle_status_pointer(Some(3),true);assert_eq!(s.active_matrix_slot_name(),Some("id".into()));
    s.handle_status_pointer(Some(0),true);assert_eq!(s.active_matrix_slot_name(),None);
    assert!(s.handle_status_pointer(Some(35),false));
    assert!(s.row_for_render(SpecialRows::StatusRow).right[1].underline);
    s.handle_status_pointer(Some(35),true);
    assert_eq!(*service::LAUNCHES.lock().unwrap(),vec![("alias:héllo".into(),"".into())]);
    assert_eq!(s.active_matrix_slot_name(),Some("td0".into()));assert_eq!(s.prompt.text,"draft");
    assert!(s.handle_status_pointer(None,false));
    assert!(!s.row_for_render(SpecialRows::StatusRow).right.iter().any(|run|run.underline));
}
#[test] fn status_targets_follow_right_alignment_and_clipping() {
    let ids=vec!["abcdef".into()];let aliases=vec!["héllo".into(),"other".into()];
    assert_eq!(status::hit(&ids,&aliases,40,29),Some(status::Target::Alias("héllo".into())));
    assert_eq!(status::hit(&ids,&aliases,40,33),None); // space between aliases
    assert_eq!(status::hit(&ids,&aliases,10,4),None); // clipped-strip separator
    assert_eq!(status::hit(&ids,&aliases,10,9),Some(status::Target::Alias("héllo".into()))); // first alias cell fits
    assert_eq!(status::hit(&ids,&aliases,12,11),Some(status::Target::Alias("héllo".into())));
    assert_eq!(status::hit(&ids,&aliases,10,10),None);
    assert_eq!(status::hit(&ids,&aliases,0,0),None);
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
    assert_eq!(b.get_strip(SpecialRows::TitleRow,StripSide::Right),"VME tui env smp esc stop");
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
#[test] fn vme_stop_and_operator_drop_share_cleanup_and_retire_other_views() {
    service::LAUNCHES.lock().unwrap().clear();service::DROPS.lock().unwrap().clear();
    MatrixSlots::set(&["plain"]);
    let mut a=Shell3::new(100);a.set_mode(2);a.set_appdb_names(&["termdir".into()]);
    type_text(&mut a,"termdir");let first=a.active_matrix_slot_name().unwrap();
    let mut b=Shell3::new(100);b.select_matrix_slot_name(&first);
    type_text(&mut a,"stop");
    assert_eq!(*service::DROPS.lock().unwrap(),vec![first.clone()]);
    assert_eq!(a.active_matrix_slot_name(),None);assert_eq!(a.prompt.text,"");
    assert!(!MatrixSlots::slot_ids().contains(&first));assert!(MatrixSlots::echo_lines(None).is_empty());
    assert!(matrix_slots().lock().vmx_apps.is_empty());
    b.reconcile_matrix_selection();assert_eq!(b.active_matrix_slot_name(),None);
    assert!(key(&mut b,2,2,'\\t'));
    type_text(&mut a,"termdir");let second=a.active_matrix_slot_name().unwrap();
    assert!(b.parse_operator(&format!("§{second}§")));
    assert_eq!(*service::DROPS.lock().unwrap(),vec![first,second]);
    a.reconcile_matrix_selection();assert_eq!(a.active_matrix_slot_name(),None);
    assert!(b.parse_operator("§plain§"));assert!(b.parse_operator("§§"));
    assert_eq!(service::DROPS.lock().unwrap().len(),2);
}
'''
    source += '''
#[test] fn tui_requests_reentry_and_typed_esc_restores_default() {
    service::LAUNCHES.lock().unwrap().clear();service::DROPS.lock().unwrap().clear();tui::REQUESTS.lock().unwrap().clear();
    MatrixSlots::set(&[] as &[&str]);let mut s=Shell3::new(100);s.set_mode(2);s.set_appdb_names(&["termdir".into()]);
    type_text(&mut s,"termdir");let name=s.active_matrix_slot_name().unwrap();
    type_text(&mut s,"tui");assert_eq!(*tui::REQUESTS.lock().unwrap(),vec![name.clone()]);
    assert!(service::DROPS.lock().unwrap().is_empty());assert_eq!(s.active_matrix_slot_name(),Some(name.clone()));
    assert!(!key(&mut s,2,4,'\\0'));assert_eq!(s.active_matrix_slot_name(),Some(name.clone()));
    type_text(&mut s,"esc");assert_eq!(s.active_matrix_slot_name(),None);
    assert!(MatrixSlots::slot_ids().contains(&name));assert!(service::DROPS.lock().unwrap().is_empty());
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
    assert!(key(&mut a,2,3,'\\r'));assert_eq!(a.active_matrix_slot_name(),Some("abc".into()));assert_eq!(a.prompt.render()," ");
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
    assert!(submit(&mut a,"§§"));assert_eq!(a.prompt.render()," ");assert_eq!(a.active_matrix_slot_index(),0);
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
    let receipt=enqueue_blueprint_bytes_with_receipt(MatrixTarget {slot_id:String::new()},"termdir.bp".into(),b"archive bytes".to_vec(),vec![],hv::BlueprintInstanceRequest::default(),None,&["td0".into()],None).unwrap();
    assert_eq!(receipt.slot,"td1");assert_eq!(receipt.app,"termdir");assert_eq!(receipt.sha256,[42;32]);
    assert_eq!(*RESERVED.lock().unwrap(),vec!["td1"]);assert_eq!(*QUEUED.lock().unwrap(),vec![receipt.slot]);
}
}
'''
    source += '''
#[macro_export] macro_rules! log {($($args:tt)*)=>{};}
mod launch_lifecycle {
use super::*;
use std::sync::atomic::{AtomicBool,AtomicUsize,Ordering};
static EXPIRED:AtomicBool=AtomicBool::new(false);
static STARTS:AtomicUsize=AtomicUsize::new(0);
static STOPS:AtomicUsize=AtomicUsize::new(0);
static BINDS:AtomicUsize=AtomicUsize::new(0);
static UNBINDS:AtomicUsize=AtomicUsize::new(0);
static CASE:AtomicUsize=AtomicUsize::new(0);
struct Spawner;
#[derive(Clone)] struct MatrixTarget;
#[derive(Clone)] struct Instance;impl Instance {fn is_default(&self)->bool {false}}
struct AppVmLaunchRequest {instance:Instance,archive:String,module_bytes:Vec<u8>,app_args:Vec<String>,launch_script:Option<String>,target:MatrixTarget}
struct BlueprintLaunchPlan {console_surface:u8}
fn app_label_for_archive(a:&str)->&str {a}
fn matrix_target_interrupted(_: &MatrixTarget)->bool {EXPIRED.load(Ordering::Relaxed)}
mod matrix {
use super::*;
pub fn matrix_target_slot_lease(_: &MatrixTarget) {}
pub fn bind_matrix_target_vm(_: &MatrixTarget,id:u8)->bool {assert_eq!(id,7);BINDS.fetch_add(1,Ordering::Relaxed);!matrix_target_interrupted(&MatrixTarget)}
pub fn unbind_matrix_target_vm(_: &MatrixTarget,id:u8) {assert_eq!(id,7);UNBINDS.fetch_add(1,Ordering::Relaxed);}
}
mod hv {
use super::*;
pub fn default_app_instance_vm(_: &str)->Option<u8> {None}
pub fn named_app_instance_vms(_: &str)->Vec<(u8,String)> {vec![]}
pub fn first_free_vm_id()->Option<u8> {Some(7)}
pub fn start_blueprint_app_vm(_:u8,_:&Spawner,_:String,_:Vec<u8>,_:Vec<String>,_:Option<String>,_:Instance,_:Option<MatrixTarget>,_:u8)->Result<(),()> {
    // Ownership must be published before VM launch, without claiming input.
    assert_eq!(BINDS.load(Ordering::Relaxed),1);STARTS.fetch_add(1,Ordering::Relaxed);
    match CASE.load(Ordering::Relaxed) {2=>{EXPIRED.store(true,Ordering::Relaxed);Ok(())},3=>Err(()),_=>Ok(())}
}
pub fn kill_for_matrix_slot(id:u8,_:&())->Result<bool,()> {stop(id)}
pub fn stop(id:u8)->Result<bool,()> {assert_eq!(id,7);STOPS.fetch_add(1,Ordering::Relaxed);Ok(true)}
}
'''
    source += extract.item('src/shell2/cmds/run.rs', 'start_blueprint_launch').replace('crate::hv::','hv::').replace('crate::shell2::','matrix::')
    source += '''
#[test] fn vm_ownership_precedes_launch_and_freed_target_cancels_start() {
    let request=AppVmLaunchRequest {instance:Instance,archive:"termdir.bp".into(),module_bytes:vec![],app_args:vec![],launch_script:None,target:MatrixTarget};
    for (case,expected_starts,expected_stops,expected_unbinds) in [(0,1,0,0),(1,0,0,0),(2,1,1,0),(3,1,0,1)] {
        CASE.store(case,Ordering::Relaxed);EXPIRED.store(case==1,Ordering::Relaxed);
        for counter in [&STARTS,&STOPS,&BINDS,&UNBINDS] {counter.store(0,Ordering::Relaxed);}
        start_blueprint_launch(&Spawner,&request,BlueprintLaunchPlan {console_surface:1},&|_|{});
        assert_eq!(BINDS.load(Ordering::Relaxed),1);
        assert_eq!(STARTS.load(Ordering::Relaxed),expected_starts);
        assert_eq!(STOPS.load(Ordering::Relaxed),expected_stops);
        assert_eq!(UNBINDS.load(Ordering::Relaxed),expected_unbinds);
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-pool-input-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--crate-type=rlib','--crate-name','trueos_terminal',str(ROOT/'crates/trueos-terminal/src/lib.rs'),'-o',str(path/'libtrueos_terminal.rlib')],check=True)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests'),'--extern',f'trueos_terminal={path}/libtrueos_terminal.rlib'],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
