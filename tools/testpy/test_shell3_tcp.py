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
    source += '''
mod update {pub type RenderedLine=Vec<(char,Option<crate::RgbaColor>)>; pub struct Snapshot(pub Vec<RenderedLine>);impl Snapshot {pub fn rendered_lines(&self)->Vec<RenderedLine>{self.0.clone()}pub fn size(&self)->(usize,usize){(100,25)}pub fn terminal_active(&self)->bool{false}}}
const OPERATOR: char = '§';
struct Shell3 { vmx:bool, mode: u8, prompt: String, cursor: usize, messages:RefCell<Vec<String>>, parsed: RefCell<Vec<String>> }
impl Shell3 {
    fn new_terminal() -> Result<Self, ()> {
        Ok(Self { vmx:false, mode: 1, prompt: String::new(), cursor: 0, messages:RefCell::new(Vec::new()), parsed: RefCell::new(Vec::new()) })
    }
    fn reconcile_matrix_selection(&mut self) {}
    fn active_matrix_slot_name(&self) -> Option<String> { Some("sh1".into()) }
    fn stop_active_vmx(&mut self)->bool {core::mem::take(&mut self.vmx)}
    fn get_strip(&self, _: SpecialRows, _: StripSide) -> String { "TrueOS § 12:34".into() }
    fn mode(&self) -> Mode { match self.mode { 1 => Mode::HV, 2 => Mode::CMD, _ => Mode::ADM } }
    fn get_mode(&self) -> u8 { self.mode }
    fn set_mode(&mut self, mode: u8) { self.mode = mode; }
    fn set_prompt(&mut self, text: &str) { self.prompt = text.into(); }
    fn set_cursor(&mut self, cursor: usize) { self.cursor = cursor; }
    fn parse_operator(&mut self,text:&str)->bool {self.parsed.borrow_mut().push(text.into());text.starts_with(OPERATOR)}
    fn prompt(&self) -> &str { &self.prompt }
    fn cursor(&self) -> usize { self.cursor }
    fn terminal_message(&self, message: &str) {self.messages.borrow_mut().push(message.into());}
    fn capture_update_snapshot(&self) -> update::Snapshot {
        update::Snapshot(vec!["TrueOS § 12:34".chars().map(|c|(c,None)).collect(),
            "§sh1".chars().map(|c|(c,None)).collect(),
            format!("{}#",self.prompt).chars().map(|c|(c,None)).collect()])
    }
    fn replay_terminal_line(&mut self, text: &str) {
        if text != "stop" || !self.stop_active_vmx() { self.parsed.borrow_mut().push(text.into()); }
        self.prompt.clear();self.cursor=0;
    }
}
mod tty {
'''
    source += (ROOT/'src/shell3/tty.rs').read_text().replace('//!', '//')
    source += '''
#[cfg(test)] mod tests {
    use super::*;
    fn terminal() -> Terminal {
        let mut tty = Terminal::new(Shell3::new_terminal().unwrap());
        tty.output.clear(); tty
    }
    #[test] fn stop_dispatches_for_vmx_only_and_returns_to_prompt() {
        let mut tty=terminal();tty.shell.vmx=true;
        tty.input(b"stop\\r");assert!(!tty.shell.vmx);assert!(tty.shell.parsed.borrow().is_empty());
        assert!(!String::from_utf8_lossy(&tty.output).contains("not wired"));
        tty.input(b"stop\\r");assert_eq!(&*tty.shell.parsed.borrow(),&["stop"]);
    }
    #[test] fn connection_clears_once_and_positions_shared_rows() {
        let tty=Terminal::new(Shell3::new_terminal().unwrap());
        let output=String::from_utf8_lossy(&tty.output);
        assert!(output.starts_with("\\x1b]0;TrueOS §\\x07\\x1b[2J\\x1b[H"));
        assert!(output.contains("TrueOS § 12:34"));assert!(output.contains("§sh1"));
        assert!(output.contains("\\x1b[3;1H"));
        assert_eq!(output.matches("\\x1b[2J").count(),1);
    }
    #[test] fn matrix_operator_is_submitted_once_with_enter() {
        let mut tty = terminal();
        tty.input("§id§".as_bytes());assert!(tty.shell.parsed.borrow().is_empty());
        tty.input(b"\\r");tty.input(b"\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["§id§"]);
        assert_eq!(tty.shell.prompt, "");assert_eq!(tty.shell.cursor,0);
        let output=String::from_utf8_lossy(&tty.output);
        assert!(!output.contains("unknown name"));assert!(!output.contains("not wired"));
        assert!(output.contains("\\x1b[3;1H"));
    }
    #[test] fn fragmented_unicode_crlf_and_backspace() {
        let mut tty = terminal();
        tty.input(b"\\xc2"); assert_eq!(tty.shell.prompt, "");
        tty.input(b"\\xa7x\\x7f");
        assert_eq!(tty.shell.prompt, "§"); assert_eq!(tty.shell.cursor, 1);
        tty.input(b"\\r"); tty.input(b"\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["§"]);
        assert!(String::from_utf8_lossy(&tty.output).contains("\\x1b[3;1H"));
    }
    #[test] fn every_packet_split_produces_identical_results() {
        let input = "§é😀\\x7f\\tknown\\r\\nnext\\n".as_bytes();
        let mut whole = terminal(); whole.input(input);
        for split in 0..=input.len() {
            let mut tty = terminal(); tty.input(&input[..split]); tty.input(&input[split..]);
            assert_eq!(tty.presented, whole.presented, "split {split}");
            assert_eq!(*tty.shell.parsed.borrow(), *whole.shell.parsed.borrow());
            assert_eq!(tty.shell.prompt, whole.shell.prompt);
        }
    }
    #[test] fn independent_modes_and_prompts() {
        let mut first = terminal(); let mut second = terminal();
        first.input(b"\\tfirst"); second.input(b"second");
        assert_eq!(first.shell.get_mode(), 2); assert_eq!(second.shell.get_mode(), 1);
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
        assert!(tty.shell.messages.borrow().iter().any(|text|text.contains("line discarded")));
        tty.input(b"known\\n"); assert_eq!(&*tty.shell.parsed.borrow(), &["known"]);
        for _ in 0..OUTPUT_LIMIT {tty.input(b"x");}
        assert!(tty.overflow && tty.closing); assert!(tty.output.len() <= OUTPUT_LIMIT);
    }
    #[test] fn help_exit_eof_and_recognition() {
        let mut tty = terminal(); tty.input(b"known\\nhelp\\nexit\\nignored\\n");
        assert_eq!(&*tty.shell.parsed.borrow(), &["known"]);
        assert!(tty.closing);
        assert!(tty.shell.messages.borrow().iter().any(|text|text.contains("Enter replays the line")));
        let mut eof = terminal(); eof.input(b"x\\x04"); assert!(!eof.closing);
        eof.input(b"\\x7f\\x04"); assert!(eof.closing);
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
    net = (ROOT/'src/shell3/net.rs').read_text()
    source += net[net.index('const WRITE_TIMEOUT_MS'):net.index('enum WorkerEvent')]
    source += '''
#[cfg(test)] mod tests {
    use super::*;
    fn queue() -> NetQueue<NetCommand> { NetQueue { full: Cell::new(false), commands: RefCell::new(Vec::new()) } }
    #[test] fn queue_rejection_preserves_bytes_and_one_write_is_in_flight() {
        let queue = queue(); let mut connection = Connection::new(NetHandle(1), Shell3::new_terminal().unwrap());
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
        assert!(matches!(&queue.commands.borrow()[1], NetCommand::SendTcp { handle: NetHandle(1), data } if String::from_utf8_lossy(data).contains("x#")));
    }
    #[test] fn graceful_finish_waits_for_output_and_has_teardown_deadline() {
        let queue = queue(); let mut connection = Connection::new(NetHandle(2), Shell3::new_terminal().unwrap());
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
        let queue = queue(); let mut first = Connection::new(NetHandle(1), Shell3::new_terminal().unwrap());
        let mut second = Connection::new(NetHandle(2), Shell3::new_terminal().unwrap());
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
