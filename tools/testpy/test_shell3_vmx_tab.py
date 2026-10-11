#!/usr/bin/env python3
"""Exercise production keyboard routing for Tab leaving a VMX slot."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    shell = (ROOT/'src/shell3/shell3.rs').read_text()
    handler = re.search(r'^    fn handle_keyboard_with_latch\(.*?^    }', shell, re.M | re.S).group()
    source = r'''
extern crate alloc;
use alloc::string::String;
const OPERATOR:char='§';
mod r {pub mod keyboard {
    pub const KEYBOARD_OUTPUT_FLAG_PRESS:u8=1;
    pub const KEYBOARD_OUTPUT_KIND_KEY:u8=1;
    pub const KEYBOARD_OUTPUT_KIND_TEXT:u8=2;
    pub const KEYBOARD_KEY_TAB:u16=1;
    pub const KEYBOARD_KEY_ARROW_UP:u16=2;
    pub const KEYBOARD_KEY_ARROW_DOWN:u16=3;
    pub const KEYBOARD_KEY_ENTER:u16=4;
    pub const KEYBOARD_KEY_BACKSPACE:u16=5;
    pub struct TrueosKeyboardOutputEvent {pub flags:u8,pub kind:u8,pub codepoint:u32,pub key_code:u16}
}}
mod tui {
    use super::*;
    pub fn park(_:u64)->bool {true}
    pub fn keyboard(_:u64,name:Option<&str>,_:&r::keyboard::TrueosKeyboardOutputEvent)->bool {name.is_some()}
}
struct History {cursor:Option<usize>}
impl History {fn recall(&mut self,_:bool,_:&str)->Option<String>{None} fn end_recall(&mut self){}}
struct Prompt {text:String,cursor:usize,colors:Vec<Option<()>>}
impl Prompt {fn char_len(&self)->usize{self.text.chars().count()}}
struct Run {text:String}
struct Row {right:Vec<Run>}
struct Rows {promt:Row}
struct Shell3 {slot:Option<String>,vmx:bool,mode:u8,tui_frontend:u64,command_history:History,prompt:Prompt,rows:Rows,columns:usize}
impl Shell3 {
    fn new(vmx:bool)->Self {Self{slot:Some("app".into()),vmx,mode:2,tui_frontend:1,command_history:History{cursor:None},prompt:Prompt{text:"draft".into(),cursor:5,colors:vec![]},rows:Rows{promt:Row{right:vec![]}},columns:80}}
    fn finish_command_latch(&mut self,_:bool){}
    fn active_vmx_app(&self)->Option<()> {if self.vmx && self.slot.is_some(){Some(())}else{None}}
    fn active_matrix_slot_name(&self)->Option<String>{self.slot.clone()}
    fn select_matrix_slot_index(&mut self,index:usize)->bool{assert_eq!(index,0);self.slot=None;true}
    fn set_mode(&mut self,mode:u8)->bool{self.mode=mode;true}
    fn get_mode(&self)->u8{self.mode}
    fn latch_prompt_prefix(&mut self)->bool{false}
    fn set_prompt(&mut self,_:&str){}
    fn set_cursor(&mut self,_:usize){}
    fn replay_terminal_line(&mut self,_:&str){}
    fn submit_operator_prompt(&mut self)->bool{false}
    fn refresh_prompt_strip(&mut self){}
    fn echo_recognized_prompt(&mut self)->bool{false}
'''
    source += handler + '\n}\n'
    source += r'''
#[test] fn tab_exits_vmx_before_guest_routing_then_cycles() {
    use r::keyboard::*;
    let tab=TrueosKeyboardOutputEvent{flags:1,kind:KEYBOARD_OUTPUT_KIND_KEY,codepoint:0,key_code:KEYBOARD_KEY_TAB};
    let mut shell=Shell3::new(true);
    assert_eq!(shell.handle_keyboard_with_latch(&tab),(true,false));
    assert!(shell.slot.is_none());assert!(shell.vmx);assert_eq!(shell.mode,2);assert_eq!(shell.prompt.text,"draft");
    assert_eq!(shell.handle_keyboard_with_latch(&tab),(true,false));assert_eq!(shell.mode,3);
    assert_eq!(shell.handle_keyboard_with_latch(&tab),(true,false));assert_eq!(shell.mode,1);
}
#[test] fn release_and_other_guest_keys_preserve_selection() {
    use r::keyboard::*;
    let mut shell=Shell3::new(true);
    let release=TrueosKeyboardOutputEvent{flags:0,kind:KEYBOARD_OUTPUT_KIND_KEY,codepoint:0,key_code:KEYBOARD_KEY_TAB};
    assert_eq!(shell.handle_keyboard_with_latch(&release),(false,false));assert!(shell.slot.is_some());
    let enter=TrueosKeyboardOutputEvent{flags:1,kind:KEYBOARD_OUTPUT_KIND_KEY,codepoint:0,key_code:KEYBOARD_KEY_ENTER};
    assert_eq!(shell.handle_keyboard_with_latch(&enter),(true,false));assert!(shell.slot.is_some());
    let mut native=Shell3::new(false);
    let tab=TrueosKeyboardOutputEvent{flags:1,kind:KEYBOARD_OUTPUT_KIND_KEY,codepoint:0,key_code:KEYBOARD_KEY_TAB};
    assert_eq!(native.handle_keyboard_with_latch(&tab),(true,false));assert!(native.slot.is_some());
}
'''
    with tempfile.TemporaryDirectory(prefix='sh3-vmx-tab-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test','-Awarnings',str(path/'test.rs'),'-o',str(path/'test')],check=True)
        subprocess.run([str(path/'test')],check=True)


if __name__ == '__main__':
    main()
