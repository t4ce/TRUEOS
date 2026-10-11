#!/usr/bin/env python3
"""Test production UI4 keyboard recall and the history shared with SSH."""
from pathlib import Path
import re
import subprocess
import tempfile
import shell3_history_support as history_support

ROOT = Path(__file__).resolve().parents[2]


def main():
    shell = (ROOT/'src/shell3/shell3.rs').read_text()
    source = f'''#![allow(dead_code)]
extern crate alloc;
use alloc::{{string::String,vec::Vec}};
use zeroize::Zeroize;
#[path="{ROOT}/src/shell3/command_history.rs"] mod command_history;
const OPERATOR:char='§';
mod r {{pub mod keyboard {{
pub const KEYBOARD_OUTPUT_FLAG_PRESS:u32=1;
pub const KEYBOARD_OUTPUT_KIND_KEY:u8=1;pub const KEYBOARD_OUTPUT_KIND_TEXT:u8=2;
pub const KEYBOARD_KEY_ENTER:u16=1;pub const KEYBOARD_KEY_TAB:u16=2;pub const KEYBOARD_KEY_BACKSPACE:u16=3;
pub const KEYBOARD_KEY_ARROW_UP:u16=4;pub const KEYBOARD_KEY_ARROW_DOWN:u16=5;
#[derive(Default)] pub struct TrueosKeyboardOutputEvent {{pub flags:u32,pub kind:u8,pub key_code:u16,pub codepoint:u32}}
}}}}
mod tui {{pub fn park(_:u64)->bool {{true}} pub fn keyboard(_:u64,_:Option<&str>,_:&crate::r::keyboard::TrueosKeyboardOutputEvent)->bool {{false}}}}
struct Prompt {{text:String,cursor:usize,colors:Vec<Option<()>>}}
impl Prompt {{fn char_len(&self)->usize {{self.text.chars().count()}}}}
struct Run {{text:String}}
struct Strip {{right:Vec<Run>}}
struct Rows {{promt:Strip}}
struct Shell3 {{command_history:command_history::CommandHistory,prompt:Prompt,rows:Rows,columns:usize,tui_frontend:u64,executed:Vec<String>}}
impl Shell3 {{
fn new()->Self {{Self {{command_history:Default::default(),prompt:Prompt {{text:String::new(),cursor:0,colors:vec![]}},rows:Rows {{promt:Strip {{right:vec![]}}}},columns:80,tui_frontend:1,executed:vec![]}}}}
fn finish_command_latch(&mut self,_:bool) {{}}
fn active_matrix_slot_name(&self)->Option<&str> {{None}}
fn get_mode(&self)->u8 {{1}} fn set_mode(&mut self,_:u8)->bool {{true}}
fn refresh_prompt_strip(&mut self) {{}}
fn set_prompt(&mut self,text:&str) {{self.prompt.text=text.into();self.prompt.cursor=self.prompt.char_len();}}
fn set_cursor(&mut self,index:usize)->bool {{self.prompt.cursor=index;true}}
fn parse_operator(&mut self,text:&str)->bool {{if text.starts_with('§') {{self.executed.push(text.into());true}} else {{false}}}}
fn replay_terminal_line(&mut self,text:&str) {{self.executed.push(text.into());self.command_history.remember(text);self.set_prompt("");}}
fn echo_recognized_prompt(&mut self)->bool {{false}}
'''
    source = source.replace('const OPERATOR:', history_support.auth_fixture(2) + '\nconst OPERATOR:')
    for name in ('handle_keyboard_with_latch', 'submit_operator_prompt'):
        source += re.search(rf'^    (?:pub(?:\([^)]*\))? )?fn {name}\(.*?^    }}', shell, re.M | re.S).group()
    source += '''}
fn key(shell:&mut Shell3,key:u16)->bool {use r::keyboard::*;shell.handle_keyboard_with_latch(&TrueosKeyboardOutputEvent {flags:KEYBOARD_OUTPUT_FLAG_PRESS,kind:KEYBOARD_OUTPUT_KIND_KEY,key_code:key,..Default::default()}).0}
'''
    history = (ROOT/'src/shell3/command_history.rs').read_text().replace('//!', '//')
    source = source.replace(f'#[path="{ROOT}/src/shell3/command_history.rs"] mod command_history;', 'mod command_history {' + history + '\npub fn test_count(account:u64,text:&str)->u64 {LIVE.lock().entries.iter().find(|entry|entry.account==account && entry.text.as_str()==text).unwrap().count}\n}')
    source += r'''
#[test] fn cry_gate_scope_changes_and_logout_control_recording_and_recall() {
    crypt::SESSION.with(|session|session.set(None));let mut history=command_history::CommandHistory::default();
    history.remember("pre-login");assert!(history.entries.is_empty());assert_eq!(history.recall(true,"draft"),None);
    crypt::SESSION.with(|session|session.set(Some((1,1001,1))));history.remember("wrong-scope");assert!(history.entries.is_empty());
    crypt::SESSION.with(|session|session.set(Some((2,1001,2))));history.remember("status");
    history.remember("status");assert_eq!(history.entries,vec!["status"]);assert_eq!(command_history::test_count(1001,"status"),2);
    let mut other=command_history::CommandHistory::default();assert_eq!(other.recall(true,"draft"),Some("status".into()));
    crypt::SESSION.with(|session|session.set(Some((2,1002,3))));assert_eq!(other.recall(true,"draft"),None);assert!(other.draft.is_empty());
    crypt::SESSION.with(|session|session.set(None));history.sync();assert!(history.entries.is_empty());assert!(history.cursor.is_none());
    user_input_record::CAPTURED.with(|records|assert_eq!(*records.borrow(),vec![(2,"status".into()),(2,"status".into())]));
}
#[test] fn recording_redacts_malformed_secrets_before_shared_cache_and_writer() {
    crypt::SESSION.with(|session|session.set(Some((2,1003,1))));let mut history=command_history::CommandHistory::default();
    for line in ["cry login malformed extra", "CRY unlock alice recovery extra", "cry ssh add 123456 key", "cry ssh remove 123456 key", "123456"] {history.remember(line);}
    assert_eq!(history.entries,vec!["cry login ******","cry unlock ******","cry ssh add ******","cry ssh remove ******","******"]);
    user_input_record::CAPTURED.with(|records|assert!(records.borrow().iter().all(|(_,text)|!text.contains("malformed") && !text.contains("recovery") && !text.contains("123456"))));
    let before=user_input_record::CAPTURED.with(|records|records.borrow().len());history.set_recording(false);history.remember("duplicate-dispatch");
    assert_eq!(before,user_input_record::CAPTURED.with(|records|records.borrow().len()));
}
#[test] fn arrows_recall_without_running_and_down_restores_unicode_draft() {
    use r::keyboard::*;let mut shell=Shell3::new();shell.command_history.remember("first");shell.command_history.remember("second");shell.set_prompt("dräft");
    assert!(key(&mut shell,KEYBOARD_KEY_ARROW_UP));assert_eq!(shell.prompt.text,"second");assert_eq!(shell.prompt.cursor,6);
    key(&mut shell,KEYBOARD_KEY_ARROW_UP);assert_eq!(shell.prompt.text,"first");assert!(shell.executed.is_empty());
    key(&mut shell,KEYBOARD_KEY_ARROW_DOWN);assert_eq!(shell.prompt.text,"second");
    key(&mut shell,KEYBOARD_KEY_ARROW_DOWN);assert_eq!(shell.prompt.text,"dräft");assert_eq!(shell.prompt.cursor,5);
    assert!(key(&mut shell,KEYBOARD_KEY_ARROW_DOWN));assert!(shell.prompt.text.is_empty());assert_eq!(shell.prompt.cursor,0);
    assert!(!key(&mut shell,KEYBOARD_KEY_ARROW_DOWN));
    key(&mut shell,KEYBOARD_KEY_ARROW_UP);assert_eq!(shell.prompt.text,"second");
}
#[test] fn enter_runs_recalled_command_and_editing_ends_recall() {
    use r::keyboard::*;let mut shell=Shell3::new();shell.command_history.remember("termdir");
    key(&mut shell,KEYBOARD_KEY_ARROW_UP);key(&mut shell,KEYBOARD_KEY_ENTER);assert_eq!(shell.executed,vec!["termdir"]);assert!(shell.prompt.text.is_empty());
    key(&mut shell,KEYBOARD_KEY_ARROW_UP);key(&mut shell,KEYBOARD_KEY_BACKSPACE);assert_eq!(shell.prompt.text,"termdi");
    key(&mut shell,KEYBOARD_KEY_ENTER);assert_eq!(shell.executed.len(),1);
    assert!(shell.command_history.cursor.is_none());
}
#[test] fn operator_submissions_are_remembered_and_recall_is_bounded_and_shared() {
    use r::keyboard::*;let mut shell=Shell3::new();shell.set_prompt("§sh1");key(&mut shell,KEYBOARD_KEY_ENTER);
    key(&mut shell,KEYBOARD_KEY_ARROW_UP);assert_eq!(shell.prompt.text,"§sh1");
    let mut history=command_history::CommandHistory::default();for secret in ["cry login secret","§cry unlock secret","123456",""] {history.remember(secret);}
    assert!(history.entries.iter().all(|entry|!entry.contains("secret") && !entry.contains("123456")));for n in 0..70 {history.remember(&format!("command{n}"));}
    assert_eq!(history.entries.len(),10);assert_eq!(history.entries[0],"command60");
    let mut other=command_history::CommandHistory::default();assert_eq!(other.recall(true,"draft"),Some("command69".into()));
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-command-history-') as directory:
        path = Path(directory)
        dependency_args = history_support.dependencies(path)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests'),*dependency_args],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)

if __name__ == '__main__':
    main()
