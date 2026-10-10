#!/usr/bin/env python3
"""Host tests for the production Shell3 TTY and TCP write/backpressure paths.

The kernel binary disables Rust tests. Only the Shell3 model, clock, and adapter
queues are stand-ins here; input decoding and transport flushing are real code.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = '''#![allow(dead_code)]
extern crate alloc;
use alloc::{string::String, vec::Vec};
use std::cell::{Cell, RefCell};
'''
    for name in ('Mode', 'SpecialRows', 'StripSide', 'RgbaColor'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += re.search(r'^impl RgbaColor \{.*?^}', (ROOT/'src/shell3/shell3.rs').read_text(), re.M | re.S).group()
    source += f'\n#[path="{ROOT}/src/shell3/metafmtstr.rs"] mod metafmtstr;\nuse metafmtstr::MetaFmtStr;\n'
    source += f'\n#[path="{ROOT}/src/shell3/update.rs"] mod update;\n'
    source += '''
mod r {pub mod keyboard {
    pub const KEYBOARD_OUTPUT_KIND_TEXT:u8=1;pub const KEYBOARD_OUTPUT_KIND_KEY:u8=2;
    pub const KEYBOARD_KEY_TAB:u16=2;pub const KEYBOARD_KEY_ENTER:u16=3;pub const KEYBOARD_KEY_BACKSPACE:u16=1;pub const KEYBOARD_OUTPUT_FLAG_PRESS:u32=1;
    #[derive(Default)] pub struct TrueosKeyboardOutputEvent {pub kind:u8,pub key_code:u16,pub codepoint:u32,pub flags:u32,pub utf8:[u8;4],pub utf8_len:u8}
}}
static SSH_LOGS:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
#[macro_export] macro_rules! log_info { (target: $target:literal; $($args:tt)*) => {crate::SSH_LOGS.lock().unwrap().push(format!($($args)*));}; }
mod service {pub static RELEASED:std::sync::Mutex<Vec<u32>>=std::sync::Mutex::new(Vec::new());pub fn release_shell_on_executor(slot:u32){RELEASED.lock().unwrap().push(slot);}}
const OPERATOR: char = '§';
const SpecialSeperator:char='│';
struct RowStrips {left:Vec<MetaFmtStr>}
mod tui {
#[path="__REMOTE_PATH__"] mod remote;
pub(crate) use remote::Drain as RemoteDrain;
#[derive(Default)] struct State {output:Option<remote::Output>,active:bool,input:Vec<u8>}
std::thread_local! {static STATE:std::cell::RefCell<State>=std::cell::RefCell::new(State::default());}
pub fn bind_remote_frontend(_:u64) {STATE.with(|s|*s.borrow_mut()=State {output:Some(remote::Output::new()),..Default::default()});}
pub fn remote_active(_:u64)->bool {STATE.with(|s|s.borrow().active)}
pub fn take_remote_output(_:u64,available:usize)->Option<RemoteDrain> {STATE.with(|s| {let mut s=s.borrow_mut();let active=s.active;s.output.as_mut().map(|o|o.take(available,active))})}
pub fn claim() {STATE.with(|s| {let mut s=s.borrow_mut();assert!(s.output.as_mut().unwrap().begin());s.active=true;});}
pub fn release() {STATE.with(|s| {let mut s=s.borrow_mut();s.output.as_mut().unwrap().end();s.active=false;});}
pub fn write(bytes:&[u8]) {STATE.with(|s|assert_eq!(s.borrow_mut().output.as_mut().unwrap().write(bytes),bytes.len()));}
pub fn received()->Vec<u8> {STATE.with(|s|s.borrow().input.clone())}
pub fn mouse_options(_:u64,_:Option<&str>)->trueos_terminal::MouseOptions {Default::default()}
pub fn snapshot(_:u64,_:Option<&str>)->Option<Vec<super::update::RenderedLine>> {assert!(!remote_active(1),"direct output must not snapshot the screen");None}
pub fn input(_:u64,_:Option<&str>,bytes:&[u8])->bool {STATE.with(|s| {let mut s=s.borrow_mut();if !s.active {return false;}s.input.extend_from_slice(bytes);true})}
}
struct Shell3 { tui_frontend:u64, history:Vec<String>, matrix_scroll:usize, notices:Vec<String>, pointer:Vec<(Option<usize>,bool)>, size:(usize,usize), vmx:bool, mode: u8, prompt: String, cursor: usize, parsed: RefCell<Vec<String>> }
impl Shell3 {
    fn new_terminal_reserved(_:u32,_:Option<u16>)->Self {Self::new_terminal().unwrap()}
    fn new_terminal_sized_reserved(_:u32,_:Option<u16>,_:usize,_:usize)->Self {Self::new_terminal().unwrap()}
    fn capture_controls_snapshot(&self)->update::Snapshot {
        let title=[MetaFmtStr::new("TrueOS § 12:34")];
        let status=[MetaFmtStr::new("§sh1").color(RgbaColor::Pink)];
        let prompt=[MetaFmtStr::new(self.prompt.clone()),MetaFmtStr::new(" ").color(RgbaColor::Terminal {foreground:[0,0,0,255],background:[255,255,255,255],underline:false}).blink()];
        let right=[MetaFmtStr::new("RIGHT").underline()];
        update::Snapshot::new(self.size,0,[(&title,&right),(&status,&right),(&prompt,&right)],self.size.0)
    }
    fn capture_matrix_snapshot(&self)->update::Snapshot {
        self.capture_controls_snapshot().with_matrix_offset(&self.history,0,self.matrix_scroll)
    }
    fn drag_matrix(&mut self,_:(i32,i32),_:bool,_:bool,_:bool,_:(i32,i32))->bool {false}
    fn scroll_matrix(&mut self,rows:i32)->bool {
        let old=self.matrix_scroll;let max=self.history.len().saturating_sub(self.size.1-3);
        self.matrix_scroll=if rows>=0 {old.saturating_add(rows as usize).min(max)} else {old.saturating_sub(rows.unsigned_abs() as usize)};
        old!=self.matrix_scroll
    }
    fn record_terminal_notice(&mut self,text:&str) {self.notices.push(text.into());}
    fn handle_status_pointer(&mut self,column:Option<usize>,pressed:bool)->bool {
        self.pointer.push((column,pressed));true
    }
    fn get_size(&self)->(usize,usize) {self.size}
    fn set(&mut self,cols:usize,rows:usize) {self.size=(cols,rows);}
    fn new_terminal() -> Result<Self, ()> {
        Ok(Self { tui_frontend:1, history:Vec::new(), matrix_scroll:0, notices:Vec::new(), pointer:Vec::new(), size:(100,25), vmx:false, mode: 1, prompt: String::new(), cursor: 0, parsed: RefCell::new(Vec::new()) })
    }
    fn reconcile_matrix_selection(&mut self) {}
    fn active_matrix_slot_name(&self) -> Option<String> { Some("sh1".into()) }
    fn stop_active_vmx(&mut self)->bool {core::mem::take(&mut self.vmx)}
    fn row_for_render(&self, _: SpecialRows) -> RowStrips { RowStrips {left:vec![MetaFmtStr::new("TrueOS § 12:34")]} }
    fn get_strip(&self, _: SpecialRows, _: StripSide) -> String { "TrueOS § 12:34".into() }
    fn mode(&self) -> Mode { match self.mode { 1 => Mode::HV, 2 => Mode::CMD, _ => Mode::ADM } }
    fn get_mode(&self) -> u8 { self.mode }
    fn set_mode(&mut self, mode: u8) { self.mode = mode; }
    fn set_prompt(&mut self, text: &str) { self.prompt = text.into(); }
    fn set_cursor(&mut self, cursor: usize) { self.cursor = cursor; }
    fn parse_operator(&mut self,text:&str)->bool {self.parsed.borrow_mut().push(text.into());text.starts_with(OPERATOR)}
    fn prompt(&self) -> &str { &self.prompt }
    fn handle_keyboard_with_latch(&mut self,event:&r::keyboard::TrueosKeyboardOutputEvent)->(bool,bool) {
        use r::keyboard::*;
        if event.kind==KEYBOARD_OUTPUT_KIND_TEXT {self.prompt.push(char::from_u32(event.codepoint).unwrap());self.cursor=self.prompt.chars().count();}
        else {match event.key_code {
            KEYBOARD_KEY_TAB=>{self.mode=self.mode%3+1;},
            KEYBOARD_KEY_BACKSPACE=>{self.prompt.pop();self.cursor=self.prompt.chars().count();},
            KEYBOARD_KEY_ENTER=>{let line=self.prompt.clone();self.replay_terminal_line(&line);},_=>{}
        }}
        (true,false)
    }
    fn replay_terminal_line(&mut self, text: &str) {
        if text != "stop" || !self.stop_active_vmx() { self.parsed.borrow_mut().push(text.into()); }
        self.prompt.clear();self.cursor=0;
    }
}
mod tty {
'''
    source += (ROOT/'src/shell3/tty.rs').read_text().replace('//!', '//').replace('mod input;', f'#[path="{ROOT}/src/shell3/tty/input.rs"] mod input;').replace('mod ansi;', f'#[path="{ROOT}/src/shell3/tty/ansi.rs"] mod ansi;')
    source += '''
impl Terminal {pub(crate) fn submitted(&self)->Vec<String>{self.shell.parsed.borrow().clone()}}
#[cfg(test)] mod tests {
    use super::*;
    use crate::tui;
    fn terminal() -> Terminal {
        let mut tty = Terminal::new(Shell3::new_terminal().unwrap());
        tty.output.clear(); tty
    }
    #[test] fn remote_app_bytes_and_unicode_input_bypass_shell_renderer() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());tty.output.clear();
        tui::claim();tty.reconcile_matrix_selection();tty.output.clear();
        let payload="\x1b[38;5;8m🗺 §\x1b[6n".as_bytes();
        tui::write(payload);tty.reconcile_matrix_selection();
        assert_eq!(tty.output,payload);
        tty.output.clear();tty.input("§\x1b[1;3R\x1b[<65;2;3M".as_bytes());
        assert_eq!(tui::received(),"§\x1b[1;3R\x1b[<65;2;3M".as_bytes());
        assert!(tty.output.is_empty() && tty.submitted().is_empty());
    }
    #[test] fn remote_pty_resize_does_not_emit_shell_paint_or_erase_app() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        tui::claim();tty.reconcile_matrix_selection();tty.output.clear();
        tty.resize(150,45);
        assert_eq!(tty.shell.get_size(),(150,45));assert!(tty.output.is_empty());
    }
    #[test] fn remote_release_orders_last_app_bytes_before_shell_redraw() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        tui::claim();tty.reconcile_matrix_selection();tty.output.clear();
        tui::write(b"LAST-FRAME");tui::release();tty.reconcile_matrix_selection();
        assert!(tty.output.starts_with(b"LAST-FRAME"));
        let output=String::from_utf8_lossy(&tty.output);
        assert!(output.contains("TrueOS") && output.contains("\x1b[4;25r"));
    }
    #[test] fn remote_release_waits_for_transport_capacity_before_shell_redraw() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        tui::claim();tty.reconcile_matrix_selection();tty.output=vec![b'x';OUTPUT_LIMIT];
        tui::write(b"LAST-FRAME");tui::release();tty.reconcile_matrix_selection();
        assert_eq!(tty.output.len(),OUTPUT_LIMIT);assert!(!tty.closing);
        tty.output.clear();tty.reconcile_matrix_selection();
        assert!(tty.output.starts_with(b"LAST-FRAME"));
        assert!(String::from_utf8_lossy(&tty.output).contains("TrueOS"));
    }
    #[test] fn ssh_controls_align_right_and_update_without_line_echo() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        let output=String::from_utf8_lossy(&tty.output);
        let mut screen=trueos_terminal::Terminal::new(100,25);screen.feed(&tty.output);
        for row in 0..3 {let cells=&screen.cells()[row*100+95..row*100+100];
            assert_eq!(cells.iter().map(|cell|cell.glyph).collect::<String>(),"RIGHT");
            assert!(cells.iter().all(|cell|cell.style.underline));
        }
        assert!(output.contains("TrueOS"));assert!(output.contains("12:34"));assert!(output.contains("§sh1"));
        tty.output.clear();tty.input(b"abc");
        let output=String::from_utf8_lossy(&tty.output);assert!(output.contains("\\x1b[3;1H")&&output.contains("abc"));
        assert!(!tty.output.windows(2).any(|w|w==b"\\x1b8"));
        tty.output.clear();tty.reconcile_matrix_selection();assert!(tty.output.is_empty());
        tty.resize(60,20);
        screen.resize(60,20);screen.feed(&tty.output);
        assert_eq!(screen.cells()[55..60].iter().map(|cell|cell.glyph).collect::<String>(),"RIGHT");
        tty.output.clear();
        tty.input(b"\\r");assert_eq!(tty.shell.prompt,"");
        assert_eq!(&*tty.shell.parsed.borrow(), &["abc"]);
        let output=String::from_utf8_lossy(&tty.output);assert!(output.contains("\\x1b[3;1H")&&output.contains("   "));
    }
    #[test] fn ssh_wire_pixels_match_shared_palette_after_scroll_clear_and_resize() {
        fn check(tty:&Terminal, screen:&trueos_terminal::Terminal) {
            let rows=tty.shell.capture_matrix_snapshot().rendered_lines();
            let (columns,height)=tty.shell.get_size();
            assert_eq!(screen.dimensions(),(columns,height));
            for (y,row) in rows.iter().enumerate() {for (x,(glyph,style)) in row.iter().enumerate() {
                let actual=&screen.cells()[y*columns+x];let color=style.unwrap();
                let [red,green,blue,alpha]=if color.blink() {crate::update::CONTROL_BACKGROUND} else {color.background().unwrap()};
                assert_eq!(alpha,255);assert_eq!(actual.glyph,*glyph,"cell {x},{y}");
                assert_eq!(actual.style.background,trueos_terminal::TerminalColor::Rgb {red,green,blue},"cell {x},{y}");
                if *glyph!=' ' {let [red,green,blue,_]=color.rgba();assert_eq!(actual.style.foreground,trueos_terminal::TerminalColor::Rgb {red,green,blue});}
                assert_eq!(actual.style.underline,color.underline());
            }}
        }
        let mut shell=Shell3::new_terminal().unwrap();shell.size=(12,6);
        shell.history=(0..6).map(|n|format!("line{n}")).collect();
        let mut tty=Terminal::new_ssh(shell);let mut screen=trueos_terminal::Terminal::new(12,6);
        screen.feed(&tty.output);check(&tty,&screen);
        tty.output.clear();tty.shell.history.remove(0);tty.reconcile_matrix_selection();
        screen.feed(&tty.output);check(&tty,&screen);
        tty.output.clear();tty.input(b"clear\\r");screen.feed(&tty.output);check(&tty,&screen);
        tty.output.clear();tty.resize(20,8);screen.resize(20,8);
        screen.feed(&tty.output);check(&tty,&screen);
    }
    #[test] fn ssh_mouse_reports_are_ignored_at_every_packet_split() {
        let input=b"\\x1b[<35;4;2M\\x1b[<0;4;2M\\x1b[<0;4;2m\\x1b[<32;6;2M\\x1b[<64;6;2M\\x1b[<65;6;2M\\x1b[<2;6;2M\\x1b[<35;6;3M";
        for split in 0..=input.len() {
            let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
            tty.output.clear();tty.input(&input[..split]);tty.input(&input[split..]);
            assert!(tty.shell.pointer.is_empty(),"split {split}");
            assert!(tty.line.is_empty());assert!(tty.shell.prompt.is_empty());
            assert!(tty.shell.parsed.borrow().is_empty());
        }
    }
    #[test] fn mouse_bounds_malformed_reports_and_nc_are_ignored() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        tty.input(b"\\x1b[<0;0;2M\\x1b[<0;101;2M\\x1b[<0;4;26M\\x1b[<0;4;0M\\x1b[<999;4;2M\\x1b[<0;4M\\x1b[<0;4;2;3M\\x1b[<0;-1;2M");
        let mut oversized=b"\\x1b[<".to_vec();oversized.extend_from_slice(&[b'9';100]);oversized.extend_from_slice(b";4;2M");tty.input(&oversized);
        assert!(tty.shell.pointer.is_empty());assert!(tty.line.is_empty());
        tty.input(b"\\x1b[<0;100;25Mx");
        assert!(tty.shell.pointer.is_empty());assert_eq!(tty.line,"x");
        let mut nc=terminal();nc.input(b"\\x1b[<0;4;2M");assert!(nc.shell.pointer.is_empty());assert!(nc.line.is_empty());
    }
    #[test] fn ssh_keeps_shell_mouse_reporting_off_and_cleans_up_on_exit() {
        for exit in [b"exit\\r".as_slice(),b"\\x04".as_slice()] {
            let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
            assert!(!String::from_utf8_lossy(&tty.output).contains("1003h"));
            assert!(!String::from_utf8_lossy(&tty.output).contains("1006h"));
            assert!(!String::from_utf8_lossy(&tty.output).contains("1002h"));
            tty.output.clear();tty.input(exit);
            assert!(String::from_utf8_lossy(&tty.output).contains("1002l"));
            assert!(String::from_utf8_lossy(&tty.output).contains("1006l"));assert!(tty.closing);
        }
    }
    #[test] fn ssh_reserves_matrix_rows_and_routes_notices_into_the_buffer() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[4;25r"));
        tty.output.clear();tty.resize(60,10);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[4;10r"));
        tty.output.clear();tty.input(b"help\\r");
        assert!(tty.shell.notices[0].contains("SSH types directly"));
        assert!(!String::from_utf8_lossy(&tty.output).contains("SSH types directly"));
        tty.output.clear();tty.input(b"exit\\r");
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[r"));
    }
    #[test] fn ssh_scroll_shifts_only_matrix_and_sends_exposed_rows() {
        let mut shell=Shell3::new_terminal().unwrap();shell.size=(12,6);
        shell.history=(0..6).map(|n|format!("line{n}")).collect();
        let mut tty=Terminal::new_ssh(shell);tty.output.clear();
        tty.shell.history.remove(0);tty.reconcile_matrix_selection();
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1S"));assert!(out.contains("\\x1b[6;1H")&&out.contains("line3"));
        assert!(!out.contains("line1"));assert!(!out.contains("line2"));assert!(!out.contains("TrueOS"));
        tty.output.clear();tty.shell.history.insert(0,"line0".into());tty.reconcile_matrix_selection();
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1T"));assert!(out.contains("\\x1b[4;1H")&&out.contains("line0"));
        tty.output.clear();tty.shell.history.insert(0,"newest".into());tty.reconcile_matrix_selection();
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1T"));assert!(out.contains("\\x1b[4;1H")&&out.contains("newest"));
        tty.output.clear();tty.shell.history.clear();tty.reconcile_matrix_selection();
        let output=String::from_utf8_lossy(&tty.output);assert!(output.contains("\\x1b[4;1H")&&output.contains("      "));
        assert!(tty.line.is_empty());
    }
    #[test] fn ssh_native_cursor_blinks_on_the_blank_cell_and_tracks_editing() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1 q\\x1b[?25h"));assert!(out.ends_with("\\x1b[3;1H"));
        assert!(!out.contains('#'));
        tty.output.clear();tty.input("éx".as_bytes());
        assert!(String::from_utf8_lossy(&tty.output).ends_with("\\x1b[3;3H"));
        tty.input(b"\\x7f");assert!(String::from_utf8_lossy(&tty.output).ends_with("\\x1b[3;2H"));
        tty.output.clear();tty.input(b"\\x15exit\\r");assert!(tty.closing);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[0 q"));
    }
    #[test] fn adapter_starts_in_adm_and_enter_reuses_the_existing_prompt() {
        let mut tty=terminal();assert_eq!(tty.shell.get_mode(),3);
        // A canonical nc client sends this after locally echoing '.' and Enter.
        tty.input(b".\\r");tty.input(b"\\n");
        assert_eq!(tty.output,b"\\x1b8.\\x1b8\\x1b[K\\x1b8\\x1b[K");
        assert!(tty.line.is_empty());assert!(tty.shell.prompt.is_empty());
        tty.output.clear();tty.input(b"tab\\n");assert_eq!(tty.shell.get_mode(),1);
        assert!(!String::from_utf8_lossy(&tty.output).contains("§sh1"));
    }
    #[test] fn metadata_emits_foreground_background_underline_and_resets() {
        let mut tty=terminal();
        tty.write_meta(&MetaFmtStr::new("colored").color(RgbaColor::Terminal {
            foreground:[1,2,3,255],background:[4,5,6,255],underline:true,
        }));
        tty.write_meta(&MetaFmtStr::new("plain"));
        assert_eq!(tty.output,b"\\x1b[0m\\x1b[38;2;1;2;3m\\x1b[48;2;4;5;6m\\x1b[4mcolored\\x1b[0mplain");
    }
    #[test] fn stop_dispatches_for_vmx_only_and_returns_to_prompt() {
        let mut tty=terminal();tty.shell.vmx=true;
        tty.input(b"stop\\r");assert!(!tty.shell.vmx);assert!(tty.shell.parsed.borrow().is_empty());
        assert!(!String::from_utf8_lossy(&tty.output).contains("not wired"));
        tty.input(b"stop\\r");assert_eq!(&*tty.shell.parsed.borrow(),&["stop"]);
    }
    #[test] fn connection_banner_contains_only_title_and_slot_prompt() {
        let tty=Terminal::new(Shell3::new_terminal().unwrap());
        assert_eq!(tty.output, "\\x1b[?1007s\\x1b[?1007l\\x1b[?1049h\\x1b[0m\\x1b[48;2;24;24;24m\\x1b[2J\\x1b[H\\x1b[0mTrueOS § 12:34\\r\\n\\x1b[0m\\x1b[38;2;255;105;180m§sh1\\x1b[0m \\x1b7".as_bytes());
    }
    #[test] fn clear_screen_returns_to_active_slot_prompt() {
        let mut tty=terminal();
        tty.input(b"\\t");tty.output.clear();
        tty.input(b"clear\\r");tty.input(b"\\n");
        let output=String::from_utf8_lossy(&tty.output);
        assert_eq!(output.matches("\\x1b[2J").count(),1);
        assert_eq!(output.matches("§sh1").count(),1);
        assert!(output.ends_with("\\x1b7\\x1b8\\x1b[K"));
        assert!(tty.shell.parsed.borrow().is_empty());
        assert_eq!(tty.shell.prompt, "");assert!(!tty.closing);
        tty.input(b"known\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["known"]);
    }
    #[test] fn matrix_operator_is_submitted_once_with_enter() {
        let mut tty = terminal();
        tty.input("§id§".as_bytes());assert!(tty.shell.parsed.borrow().is_empty());
        tty.input(b"\\r");tty.input(b"\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["§id§"]);
        assert_eq!(tty.shell.prompt, "");assert_eq!(tty.shell.cursor,0);
        let output=String::from_utf8_lossy(&tty.output);
        assert!(!output.contains("unknown name"));assert!(!output.contains("not wired"));
        assert_eq!(output.matches("§sh1").count(),0);
    }
    #[test] fn fragmented_unicode_crlf_and_backspace() {
        let mut tty = terminal();
        tty.input(b"\\xc2"); assert!(tty.output.is_empty());
        tty.input(b"\\xa7x\\x7f");
        assert_eq!(tty.shell.prompt, "§"); assert_eq!(tty.shell.cursor, 1);
        tty.input(b"\\r"); tty.input(b"\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["§"]);
        assert_eq!(String::from_utf8_lossy(&tty.output).matches("§sh1").count(), 0);
    }
    #[test] fn every_packet_split_produces_identical_results() {
        let input = "§é😀\\x7f\\tknown\\r\\nnext\\n".as_bytes();
        let mut whole = terminal(); whole.input(input);
        for split in 0..=input.len() {
            let mut tty = terminal(); tty.input(&input[..split]); tty.input(&input[split..]);
            assert_eq!(tty.output, whole.output, "split {split}");
            assert_eq!(*tty.shell.parsed.borrow(), *whole.shell.parsed.borrow());
            assert_eq!(tty.shell.prompt, whole.shell.prompt);
        }
    }
    #[test] fn independent_modes_and_prompts() {
        let mut first = terminal(); let mut second = terminal();
        first.input(b"\\tfirst"); second.input(b"second");
        assert_eq!(first.shell.get_mode(), 1); assert_eq!(second.shell.get_mode(), 3);
        assert_eq!(first.shell.prompt, "first"); assert_eq!(second.shell.prompt, "second");
        first.input(b"\\x03"); assert_eq!(first.shell.prompt, "");
        assert_eq!(second.shell.prompt, "second");
        second.input(b"\\x15"); assert_eq!(second.shell.prompt, "");
    }
    #[test] fn ssh_recall_csi_ss3_packet_splits_and_enter_only() {
        for arrow in [b"\\x1b[A".as_slice(), b"\\x1bOA".as_slice()] {
            for split in 0..=arrow.len() {
                let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
                tty.input(b"first\\rsecond\\rdraft");
                tty.input(&arrow[..split]);tty.input(&arrow[split..]);
                assert_eq!(tty.line,"second");assert_eq!(tty.shell.prompt,"second");
                assert_eq!(tty.submitted(),vec!["first","second"]);
                tty.input(arrow);assert_eq!(tty.line,"first");
                tty.input(arrow);assert_eq!(tty.line,"first");
                tty.input(b"\\x1bOB");assert_eq!(tty.line,"second");
                tty.input(b"\\x1b[B");assert_eq!(tty.line,"draft");
                tty.input(arrow);tty.input(b"\\r");
                assert_eq!(tty.submitted(),vec!["first","second","second"]);
                assert!(tty.line.is_empty());
            }
        }
    }
    #[test] fn ssh_recall_filters_secrets_is_bounded_and_connection_local() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        tty.input(b"cry login 123456\\rcry unlock t4ce secret\\rcry ssh add 123456 key\\r123456\\r");
        assert!(tty.history.entries.is_empty());
        for n in 0..70 {tty.input(format!("command{n}\\r").as_bytes());}
        assert_eq!(tty.history.entries.len(),64);assert_eq!(tty.history.entries[0],"command6");
        let mut other=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        other.input(b"\\x1b[A");assert!(other.line.is_empty());
        let mut plain=terminal();plain.input(b"first\\r\\x1b[A");assert!(plain.history.entries.is_empty());
        tty.input(b"\\x1b[A\\x7fX");assert_eq!(tty.line,"command6X");
        tty.input(b"\\x03\\x1b[B");assert!(tty.line.is_empty());
    }
    #[test] fn escape_sequences_and_invalid_utf8_never_enter_commands() {
        let mut tty = terminal();
        tty.input(b"\\x1b["); tty.input(b"D\\xc2A\\xffB\\x1bOC\\xc0\\xaf\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["AB"]);
    }
    #[test] fn overlong_line_is_discarded_and_output_is_bounded() {
        let mut tty = terminal(); tty.input(&vec![b'a'; LINE_LIMIT + 1]); tty.input(b"\\n");
        assert!(tty.shell.parsed.borrow().is_empty());
        assert!(String::from_utf8_lossy(&tty.output).contains("line discarded"));
        tty.input(b"known\\n"); assert_eq!(&*tty.shell.parsed.borrow(), &["known"]);
        tty.input(&vec![b'x'; OUTPUT_LIMIT * 2]);
        assert!(tty.overflow && tty.closing); assert!(tty.output.len() <= OUTPUT_LIMIT);
    }
    #[test] fn help_exit_eof_and_recognition() {
        let mut tty = terminal(); tty.input(b"known\\nhelp\\nexit\\nignored\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["known"]);
        assert!(tty.closing);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[?1049l\\x1b[?1007r"));
        assert!(String::from_utf8_lossy(&tty.output).contains("plain TCP replays on Enter"));
        let mut eof = terminal(); eof.input(b"x\\x04"); assert!(!eof.closing);
        eof.input(b"\\x7f\\x04"); assert!(eof.closing);
        assert!(String::from_utf8_lossy(&eof.output).contains("\\x1b[?1049l\\x1b[?1007r"));
    }
}
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] struct Instant(u64);
struct Duration(u64);
impl Duration { fn from_millis(ms: u64) -> Self { Self(ms) } }
impl core::ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, duration: Duration) -> Self { Self(self.0 + duration.0) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)] struct NetHandle(u32);
#[derive(Debug)] enum NetCommand { SendTcp { handle: NetHandle, data: Vec<u8> }, Close { handle: NetHandle }, FinishTcp { handle: NetHandle } }
#[derive(Default)] struct NetQueue<T> { full: Cell<bool>, commands: RefCell<Vec<T>> }
impl<T> NetQueue<T> {
    fn try_push(&self, command: T) -> Result<(), T> {
        if self.full.get() { Err(command) } else { self.commands.borrow_mut().push(command); Ok(()) }
    }
    fn push(&self, command: T) -> Result<(), ()> { self.try_push(command).map_err(|_| ()) }
}
mod net {
use super::*;
use crate::tty::Terminal;
'''
    source = source[:source.rindex('mod net {')] + (ROOT/'tools/testpy/shell3_ssh_unavailable.rs').read_text() + source[source.rindex('mod net {'):]
    net = (ROOT/'src/shell3/net.rs').read_text()
    source += net[net.index('const WRITE_TIMEOUT_MS'):net.index('enum WorkerEvent')]
    source += '''
#[cfg(test)] mod tests {
    use super::*;
    fn connection(handle:NetHandle)->Connection {let mut c=Connection::new(handle,0,None,Instant(0));c.input(b"help\\n");c}
    fn queue() -> NetQueue<NetCommand> { NetQueue { full: Cell::new(false), commands: RefCell::new(Vec::new()) } }
    #[test] fn silent_nc_gets_greeting_and_never_starts_a_shell() {
        let queue=queue();let mut c=Connection::new(NetHandle(3),0,None,Instant(0));
        assert!(c.flush(&queue,Instant(999)));assert!(queue.commands.borrow().is_empty());
        assert!(c.flush(&queue,Instant(1000)));assert!(c.terminal.is_none());assert!(c.ssh.is_none());
        assert!(matches!(&queue.commands.borrow()[0],NetCommand::SendTcp {data,..} if data==GREETING));
        c.input(b"cry key setup intruder\\n");assert!(c.terminal.is_none());
        c.in_flight=0;c.deadline=None;assert!(c.flush(&queue,Instant(1001)));assert!(c.finishing);
    }
    #[test] fn ssh_without_host_identity_never_falls_back_to_plaintext() {
        let queue=queue();let mut c=Connection::new(NetHandle(4),0,None,Instant(0));
        c.input(b"SSH-2.0-client\\r\\n");assert!(c.rejected);assert!(c.terminal.is_none());
        assert!(!c.flush(&queue,Instant(1)));
        assert!(matches!(&queue.commands.borrow()[0],NetCommand::Close {..}));
    }
    #[test] fn greeting_retries_backpressure_without_duplicate_output() {
        let queue=queue(); let mut c=connection(NetHandle(1));
        queue.full.set(true); assert!(c.flush(&queue,Instant(0)));
        assert!(!c.greeting_sent); assert_eq!(c.in_flight,0);
        queue.full.set(false); assert!(c.flush(&queue,Instant(1)));
        assert!(matches!(&queue.commands.borrow()[0],NetCommand::SendTcp {handle:NetHandle(1),data} if data==b"hello from TrueOS\\r\\n"));
        assert!(c.flush(&queue,Instant(2))); assert_eq!(queue.commands.borrow().len(),1);
        c.in_flight=1; assert!(c.flush(&queue,Instant(3))); assert!(!c.finishing);
        c.in_flight=0; c.deadline=None;
        queue.full.set(true); assert!(c.flush(&queue,Instant(4))); assert!(!c.finishing);
        queue.full.set(false); assert!(c.flush(&queue,Instant(5))); assert!(c.finishing);
        assert!(matches!(&queue.commands.borrow()[1],NetCommand::FinishTcp {handle:NetHandle(1)}));
        assert!(c.flush(&queue,Instant(6))); assert_eq!(queue.commands.borrow().len(),2);
        assert!(!c.flush(&queue,Instant(5005)));
    }
    #[test] fn stalled_peer_does_not_block_another_connection() {
        let queue=queue(); let mut a=connection(NetHandle(1)); let mut b=connection(NetHandle(2));
        assert!(a.flush(&queue,Instant(0))); assert!(b.flush(&queue,Instant(10)));
        queue.full.set(true); assert!(a.flush(&queue,Instant(30000)));
        queue.full.set(false); assert!(!a.flush(&queue,Instant(30001)));
        assert!(b.flush(&queue,Instant(30001)));
        assert!(matches!(&queue.commands.borrow()[2],NetCommand::Close {handle:NetHandle(1)}));
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-tcp-') as directory:
        path = Path(directory)
        source = source.replace('__REMOTE_PATH__', str(ROOT/'src/shell3/tui/remote.rs'))
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--crate-type=rlib','--crate-name','trueos_terminal',str(ROOT/'crates/trueos-terminal/src/lib.rs'),'-o',str(path/'libtrueos_terminal.rlib')],check=True)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path/'test.rs'), '-o', str(path/'tests'),'--extern',f'trueos_terminal={path}/libtrueos_terminal.rlib'], check=True)
        subprocess.run([str(path/'tests')], check=True)


if __name__ == '__main__':
    main()
