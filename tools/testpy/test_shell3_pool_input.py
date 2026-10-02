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
use alloc::{string::{String,ToString},vec,vec::Vec};
mod percpu { pub const CPU_SLOT_LIMIT:usize = 32; }
mod r { pub mod keyboard {
    pub const KEYBOARD_OUTPUT_KIND_TEXT:u8=1;
    pub const KEYBOARD_OUTPUT_KIND_KEY:u8=2;
    pub const KEYBOARD_KEY_TAB:u16=2;
    pub const KEYBOARD_KEY_BACKSPACE:u16=1;
    pub struct TrueosKeyboardOutputEvent { pub kind:u8,pub key_code:u16,pub codepoint:u32 }
} }
const PROMPT_CURSOR:char='#';
const OPERATOR:char='§';
'''
    for name in ('Mode', 'RgbaColor', 'PromptState', 'title_left_text'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += re.search(r'^impl PromptState \{.*?^}', shell, re.M | re.S).group()
    source += f'\n#[path="{ROOT}/src/shell3/metafmtstr.rs"] mod metafmtstr;\nuse metafmtstr::MetaFmtStr;\n'
    source += '''struct Row {left:Vec<MetaFmtStr>,right:Vec<MetaFmtStr>}
struct Rows {promt:Row,title:Row}
struct Shell3 {prompt:PromptState,rows:Rows,columns:usize,mode:Mode,time:String}
impl Shell3 {
fn new(columns:usize)->Self {Self {prompt:PromptState::new(),rows:Rows {promt:Row {left:vec![],right:vec![]},title:Row {left:vec![],right:vec![]}},columns,mode:Mode::HV,time:"12:34".into()}}
'''
    for name in ('handle_keyboard','refresh_prompt_strip','set_mode','get_mode'):
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
        assert_eq!(a.rows.title.left[0].text,format!("TrueOS {:?} § 12:34",mode));
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
    with tempfile.TemporaryDirectory(prefix='shell3-pool-input-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests')],check=True)


if __name__ == '__main__':
    main()
