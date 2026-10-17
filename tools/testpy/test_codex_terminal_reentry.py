#!/usr/bin/env python3
"""Run Codex's native presentation-restoration adapter with a retained screen.

Only the terminal and ABI boundary are stand-ins. The restoration method is
loaded unchanged from the Codex port, including its retry/commit behavior.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ADAPTER = ROOT.parent / 'codex/codex-rs/tui/src/tui/terminal_reentry.rs'


def main():
    harness = r'''
#![allow(dead_code)]
use std::{cell::Cell,io};
std::thread_local! {
    static SNAPSHOT:Cell<(i32,u64,u32,u32)>=const {Cell::new((0,2,120,40))};
    static RESTORES:Cell<usize>=const {Cell::new(0)};
    static SETUPS:Cell<usize>=const {Cell::new(0)};
    static FAIL_SETUP:Cell<bool>=const {Cell::new(false)};
}
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_cabi_blueprint_terminal_surface_snapshot_v1(
    generation:*mut u64, columns:*mut u32, rows:*mut u32,
)->i32 {
    let (status,g,c,r)=SNAPSHOT.with(Cell::get);
    unsafe {generation.write(g);columns.write(c);rows.write(r);}
    status
}
fn restore()->io::Result<()> {RESTORES.with(|v|v.set(v.get()+1));Ok(())}
fn set_modes()->io::Result<()> {
    SETUPS.with(|v|v.set(v.get()+1));
    if FAIL_SETUP.with(Cell::get) {Err(io::Error::other("write failed"))} else {Ok(())}
}
mod terminal_program_status {pub fn invalidate_terminal_program_status() {}}
struct Terminal {cells:Vec<char>,cursor_known:bool,clears:usize}
impl Terminal {
    fn invalidate_cursor_state(&mut self) {self.cursor_known=false;}
    fn clear(&mut self)->io::Result<()> {self.cells.fill(' ');self.clears+=1;Ok(())}
}
struct Frames(Cell<usize>);
impl Frames {fn schedule_frame(&self) {self.0.set(self.0.get()+1);}}
struct Tui {terminal_generation:Option<u64>,terminal:Terminal,frame_requester:Frames,alt:bool,entries:usize}
impl Tui {
    fn new()->Self {Self {terminal_generation:Some(2),terminal:Terminal {cells:vec!['x';120*40],cursor_known:true,clears:0},frame_requester:Frames(Cell::new(0)),alt:true,entries:0}}
    fn is_alt_screen_active(&self)->bool {self.alt}
    fn leave_alt_screen_for_handoff(&mut self)->io::Result<()> {self.alt=false;Ok(())}
    fn enter_alt_screen(&mut self)->io::Result<()> {self.alt=true;self.entries+=1;Ok(())}
}
#[path="__ADAPTER__"] mod terminal_reentry;
#[test] fn same_size_new_surface_restores_modes_and_invalidates_retained_frame() {
    SNAPSHOT.with(|v|v.set((0,3,120,40)));RESTORES.with(|v|v.set(0));SETUPS.with(|v|v.set(0));
    let mut tui=Tui::new();tui.restore_terminal_surface().unwrap();
    assert!(tui.alt);assert_eq!(tui.entries,1);assert_eq!(tui.terminal_generation,Some(3));
    assert!(tui.terminal.cells.iter().all(|&c|c==' '));assert!(!tui.terminal.cursor_known);
    assert_eq!(tui.frame_requester.0.get(),1);
    assert_eq!(RESTORES.with(Cell::get),1);assert_eq!(SETUPS.with(Cell::get),1);
    tui.restore_terminal_surface().unwrap();assert_eq!(tui.terminal.clears,1);assert_eq!(tui.entries,1);
}
#[test] fn parked_or_unchanged_surface_does_not_reset_presentation() {
    let mut tui=Tui::new();
    for status in [-1,-2,-3,-4,-5,0] {
        SNAPSHOT.with(|v|v.set((status,2,120,40)));tui.restore_terminal_surface().unwrap();
        assert_eq!(tui.terminal.clears,0);assert!(tui.alt);assert_eq!(tui.terminal_generation,Some(2));
    }
}
#[test] fn failed_mode_restore_keeps_generation_pending_for_retry() {
    SNAPSHOT.with(|v|v.set((0,3,120,40)));FAIL_SETUP.with(|v|v.set(true));
    let mut tui=Tui::new();assert!(tui.restore_terminal_surface().is_err());
    assert_eq!(tui.terminal_generation,Some(2));assert_eq!(tui.terminal.clears,0);
    FAIL_SETUP.with(|v|v.set(false));tui.restore_terminal_surface().unwrap();
    assert_eq!(tui.terminal_generation,Some(3));assert_eq!(tui.terminal.clears,1);
}
#[test] fn first_observation_is_a_baseline_and_transport_failure_is_reported() {
    SNAPSHOT.with(|v|v.set((0,3,120,40)));
    let mut tui=Tui::new();tui.terminal_generation=None;tui.restore_terminal_surface().unwrap();
    assert_eq!(tui.terminal_generation,Some(3));assert_eq!(tui.terminal.clears,0);
    SNAPSHOT.with(|v|v.set((-6,0,0,0)));assert!(tui.restore_terminal_surface().is_err());
    assert_eq!(tui.terminal_generation,Some(3));
}
'''
    with tempfile.TemporaryDirectory(prefix='codex-terminal-reentry-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(harness.replace('__ADAPTER__', str(ADAPTER)))
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'),
                        '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
