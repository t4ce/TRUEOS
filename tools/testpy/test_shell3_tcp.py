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
static SSH_LOGS:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
#[macro_export] macro_rules! log_info { (target: $target:literal; $($args:tt)*) => {crate::SSH_LOGS.lock().unwrap().push(format!($($args)*));}; }
mod service {pub static RELEASED:std::sync::Mutex<Vec<u32>>=std::sync::Mutex::new(Vec::new());pub fn release_shell_on_executor(slot:u32){RELEASED.lock().unwrap().push(slot);}}
const OPERATOR: char = '§';
const SpecialSeperator:char='│';
struct RowStrips {left:Vec<MetaFmtStr>}
struct Shell3 { history:Vec<String>, matrix_scroll:usize, notices:Vec<String>, pointer:Vec<(Option<usize>,bool)>, size:(usize,usize), vmx:bool, mode: u8, prompt: String, cursor: usize, parsed: RefCell<Vec<String>> }
impl Shell3 {
    fn new_terminal_reserved(_:u32,_:Option<u16>)->Self {Self::new_terminal().unwrap()}
    fn new_terminal_sized_reserved(_:u32,_:Option<u16>,_:usize,_:usize)->Self {Self::new_terminal().unwrap()}
    fn capture_controls_snapshot(&self)->update::Snapshot {
        let title=[MetaFmtStr::new("TrueOS § 12:34")];
        let status=[MetaFmtStr::new("§sh1").color(RgbaColor::Pink)];
        let prompt=[MetaFmtStr::new(format!("{}#",self.prompt))];
        let right=[MetaFmtStr::new("RIGHT").underline()];
        update::Snapshot::new(self.size,0,[(&title,&right),(&status,&right),(&prompt,&right)],self.size.0)
    }
    fn capture_matrix_snapshot(&self)->update::Snapshot {
        self.capture_controls_snapshot().with_matrix_offset(&self.history,0,self.matrix_scroll)
    }
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
        Ok(Self { history:Vec::new(), matrix_scroll:0, notices:Vec::new(), pointer:Vec::new(), size:(100,25), vmx:false, mode: 1, prompt: String::new(), cursor: 0, parsed: RefCell::new(Vec::new()) })
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
    fn replay_terminal_line(&mut self, text: &str) {
        if text != "stop" || !self.stop_active_vmx() { self.parsed.borrow_mut().push(text.into()); }
        self.prompt.clear();self.cursor=0;
    }
}
mod tty {
'''
    source += (ROOT/'src/shell3/tty.rs').read_text().replace('//!', '//')
    source += '''
impl Terminal {pub(crate) fn submitted(&self)->Vec<String>{self.shell.parsed.borrow().clone()}}
#[cfg(test)] mod tests {
    use super::*;
    fn terminal() -> Terminal {
        let mut tty = Terminal::new(Shell3::new_terminal().unwrap());
        tty.output.clear(); tty
    }
    #[test] fn ssh_controls_align_right_and_update_without_line_echo() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        let output=String::from_utf8_lossy(&tty.output);
        for row in 1..=3 {assert!(output.contains(&format!("\\x1b[{row};96H\\x1b[0m\\x1b[38;2;255;255;255m\\x1b[4mRIGHT")));}
        assert!(output.contains("TrueOS"));assert!(output.contains("12:34"));assert!(output.contains("§sh1"));
        tty.output.clear();tty.input(b"abc");
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[3;1Habc#"));
        assert!(!tty.output.windows(2).any(|w|w==b"\\x1b8"));
        tty.output.clear();tty.reconcile_matrix_selection();assert!(tty.output.is_empty());
        tty.resize(60,20);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[1;56H"));
        tty.output.clear();
        tty.input(b"\\r");assert_eq!(tty.shell.prompt,"");
        assert_eq!(&*tty.shell.parsed.borrow(), &["abc"]);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[3;1H#   "));
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
    #[test] fn ssh_never_enables_mouse_reporting() {
        for exit in [b"exit\\r".as_slice(),b"\\x04".as_slice()] {
            let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
            assert!(!String::from_utf8_lossy(&tty.output).contains("1003"));
            assert!(!String::from_utf8_lossy(&tty.output).contains("1006"));
            tty.output.clear();tty.input(exit);
            assert!(!String::from_utf8_lossy(&tty.output).contains("1003"));assert!(tty.closing);
        }
    }
    #[test] fn ssh_reserves_matrix_rows_and_routes_notices_into_the_buffer() {
        let mut tty=Terminal::new_ssh(Shell3::new_terminal().unwrap());
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[4;25r"));
        tty.output.clear();tty.resize(60,10);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[4;10r"));
        tty.output.clear();tty.input(b"help\\r");
        assert!(tty.shell.notices[0].contains("UTF-8 line input"));
        assert!(!String::from_utf8_lossy(&tty.output).contains("UTF-8 line input"));
        tty.output.clear();tty.input(b"exit\\r");
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[r"));
    }
    #[test] fn ssh_scroll_shifts_only_matrix_and_sends_exposed_rows() {
        let mut shell=Shell3::new_terminal().unwrap();shell.size=(12,6);
        shell.history=(0..6).map(|n|format!("line{n}")).collect();
        let mut tty=Terminal::new_ssh(shell);tty.output.clear();
        tty.shell.history.remove(0);tty.reconcile_matrix_selection();
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1S"));assert!(out.contains("\\x1b[6;1Hline3"));
        assert!(!out.contains("line1"));assert!(!out.contains("line2"));assert!(!out.contains("TrueOS"));
        tty.output.clear();tty.shell.history.insert(0,"line0".into());tty.reconcile_matrix_selection();
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1T"));assert!(out.contains("\\x1b[4;1Hline0"));
        tty.output.clear();tty.shell.history.insert(0,"newest".into());tty.reconcile_matrix_selection();
        let out=String::from_utf8_lossy(&tty.output);
        assert!(out.contains("\\x1b[1T"));assert!(out.contains("\\x1b[4;1Hnewest"));
        tty.output.clear();tty.shell.history.clear();tty.reconcile_matrix_selection();
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[4;1H      "));
        assert!(tty.line.is_empty());
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
        assert_eq!(tty.output, "\\x1b[?1007s\\x1b[?1007l\\x1b[?1049h\\x1b[0m\\x1b[2J\\x1b[HTrueOS § 12:34\\r\\n\\x1b[0m\\x1b[38;2;255;105;180m§sh1\\x1b[0m \\x1b7".as_bytes());
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
        assert!(String::from_utf8_lossy(&tty.output).contains("Enter replays the line"));
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
use super::tty::Terminal;
'''
    source = source[:source.rindex('mod net {')] + (ROOT/'tools/testpy/shell3_ssh_unavailable.rs').read_text() + source[source.rindex('mod net {'):]
    net = (ROOT/'src/shell3/net.rs').read_text()
    source += net[net.index('const WRITE_TIMEOUT_MS'):net.index('enum WorkerEvent')]
    source += '''
#[cfg(test)] mod tests {
    use super::*;
    fn connection(handle:NetHandle)->Connection {
        let mut connection=Connection::new(handle,0,None,Instant(0));
        connection.open_plaintext();connection
    }
    fn queue() -> NetQueue<NetCommand> { NetQueue { full: Cell::new(false), commands: RefCell::new(Vec::new()) } }
    #[test] fn ssh_with_unavailable_credential_is_logged_and_rejected_at_every_packet_split() {
        let identification=b"SSH-2.0-OpenSSH_9.9\\r\\n";
        for split in 0..=identification.len() {
            let queue=queue();let mut c=Connection::new(NetHandle(77),7107,Some(49152),Instant(0));
            assert!(c.flush(&queue,Instant(0)));assert!(queue.commands.borrow().is_empty());
            c.input(&identification[..split]);c.input(&identification[split..]);
            assert!(c.rejected);assert!(c.terminal.is_none());
            queue.full.set(true);assert!(c.flush(&queue,Instant(10)));
            queue.full.set(false);assert!(!c.flush(&queue,Instant(11)));
            assert!(matches!(&queue.commands.borrow()[0],NetCommand::Close {handle:NetHandle(77)}));
        }
        assert!(crate::SSH_LOGS.lock().unwrap().iter().any(|line|line.contains("protocol=ssh")&&line.contains("plaintext=0")));
        assert!(crate::service::RELEASED.lock().unwrap().contains(&7107));
    }
    #[test] fn probe_preserves_plaintext_prefix_and_delays_silent_greeting() {
        let queue=queue();let mut c=Connection::new(NetHandle(88),0,None,Instant(0));
        assert!(c.flush(&queue,Instant(999)));assert!(queue.commands.borrow().is_empty());
        assert!(c.flush(&queue,Instant(1000)));assert!(c.terminal.is_some());
        assert!(matches!(&queue.commands.borrow()[0],NetCommand::SendTcp {..}));
        let mut c=Connection::new(NetHandle(89),0,None,Instant(0));
        c.input(b"S");assert!(c.terminal.is_none());
        c.input(b"how\\n");assert!(!c.rejected);
        assert_eq!(c.terminal.as_ref().unwrap().submitted(),vec!["Show"]);
    }
    #[test] fn partial_ssh_prefix_times_out_without_starting_plaintext() {
        let queue=queue();let mut c=Connection::new(NetHandle(90),7108,None,Instant(0));
        c.input(b"SSH");assert!(!c.flush(&queue,Instant(1000)));
        assert!(c.rejected);assert!(c.terminal.is_none());
        assert!(matches!(&queue.commands.borrow()[0],NetCommand::Close {..}));
    }
    #[test] fn queue_rejection_preserves_bytes_and_one_write_is_in_flight() {
        let queue = queue(); let mut connection = connection(NetHandle(1));
        let banner = connection.terminal.as_ref().unwrap().output.clone();
        queue.full.set(true); assert!(connection.flush(&queue, Instant(0)));
        assert_eq!(connection.terminal.as_ref().unwrap().output, banner);
        queue.full.set(false); assert!(connection.flush(&queue, Instant(1)));
        assert_eq!(connection.in_flight, banner.len());
        connection.terminal.as_mut().unwrap().input(b"x");
        assert!(connection.flush(&queue, Instant(2)));
        assert_eq!(queue.commands.borrow().len(), 1);
        connection.in_flight = 0; connection.deadline = None;
        assert!(connection.flush(&queue, Instant(3)));
        assert_eq!(queue.commands.borrow().len(), 2);
        assert!(matches!(&queue.commands.borrow()[1], NetCommand::SendTcp { handle: NetHandle(1), data } if data == b"\\x1b8x"));
    }
    #[test] fn graceful_finish_waits_for_output_and_has_teardown_deadline() {
        let queue = queue(); let mut connection = connection(NetHandle(2));
        connection.terminal.as_mut().unwrap().input(b"exit\\n");
        assert!(connection.flush(&queue, Instant(0)));
        assert!(matches!(&queue.commands.borrow()[0], NetCommand::SendTcp { .. }));
        assert!(connection.flush(&queue, Instant(1))); assert!(!connection.finishing);
        connection.in_flight = 0; connection.deadline = None;
        assert!(connection.flush(&queue, Instant(2))); assert!(connection.finishing);
        assert!(matches!(&queue.commands.borrow()[1], NetCommand::FinishTcp { handle: NetHandle(2) }));
        assert!(!connection.flush(&queue, Instant(5002)));
    }
    #[test] fn stalled_peer_does_not_block_another_session() {
        let queue = queue(); let mut first = connection(NetHandle(1));
        let mut second = connection(NetHandle(2));
        first.flush(&queue, Instant(0)); second.flush(&queue, Instant(10));
        assert_eq!(queue.commands.borrow().len(), 2);
        queue.full.set(true); assert!(first.flush(&queue, Instant(30000)));
        queue.full.set(false); assert!(!first.flush(&queue, Instant(30001)));
        assert!(second.flush(&queue, Instant(30001)));
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-tcp-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path/'test.rs'), '-o', str(path/'tests')], check=True)
        subprocess.run([str(path/'tests')], check=True)


if __name__ == '__main__':
    main()
