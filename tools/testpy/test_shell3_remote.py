#!/usr/bin/env python3
"""Production lease routing and remote output, with host-only kernel services."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = r'''#![allow(dead_code,non_snake_case)]
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
use alloc::{string::String, vec::Vec};
'''
    source += extract.item('src/shell3/shell3.rs', 'RgbaColor')
    source += r'''
mod update {pub const CONTROL_BACKGROUND:[u8;4]=[16,16,16,255];pub const MATRIX_BACKGROUND:[u8;4]=[24,24,24,255];pub type RenderedLine=Vec<(char,Option<crate::RgbaColor>)>;}
mod tty {pub const OUTPUT_LIMIT:usize=1024*1024;}
mod service {pub fn notify_work() {}}
mod allocators {pub fn with_host_alloc_domain<T>(f:impl FnOnce()->T)->T {f()}}
mod shell2 {
    use super::*;
    #[derive(Clone,PartialEq,Eq)] pub struct MatrixSlotLease {pub name:String,pub generation:u64}
    impl MatrixSlotLease {pub fn name(&self)->&str {&self.name}}
    pub type MatrixTarget=MatrixSlotLease;
    pub trait MatrixSlotAttachment:Send+Sync {fn on_matrix_slot_freed(&self,lease:&MatrixSlotLease);}
    pub static LIVE:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
    pub fn matrix_target_slot_lease(target:&MatrixTarget)->MatrixSlotLease {target.clone()}
    pub fn matrix_slot_is_live(lease:&MatrixSlotLease)->bool {lease.generation==LIVE.load(std::sync::atomic::Ordering::Relaxed)}
    pub fn attach_matrix_slot_resource(_: &MatrixSlotLease,_:alloc::sync::Arc<dyn MatrixSlotAttachment>)->Result<(),()> {Ok(())}
}
mod ui4 {
    pub type WindowId=u32;
    pub enum WindowOwner {Vm(u8)}
    pub mod drag_drop {pub fn release_terminal(_:super::WindowOwner,_:u32) {}}
}
mod hv {
    pub static RUN:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
    pub static INPUT:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
    pub fn vm_run_generation(_:u8)->Option<u64> {Some(RUN.load(std::sync::atomic::Ordering::Relaxed))}
    pub struct BlueprintTerminalSurfaceSnapshot {pub generation:u64,pub cols:u32,pub rows:u32}
    pub fn blueprint_console_submit_stdin_for_target(_:u8,_:&crate::shell2::MatrixTarget,_:u64,bytes:&[u8]) {INPUT.lock().unwrap().extend_from_slice(bytes);}
    pub fn blueprint_console_submit_stdin_for_lease(_:u8,_:&crate::shell2::MatrixSlotLease,_:u64,bytes:&[u8]) {INPUT.lock().unwrap().extend_from_slice(bytes);}
    pub fn blueprint_console_return_to_cli(_:u8)->bool {true}
}
mod tui {
'''
    tui = (ROOT/'src/shell3/tui.rs').read_text()
    prefix = tui[:tui.index('/// Host helpers share')]
    source += prefix.replace('//!', '//').replace('mod remote;', f'#[path="{ROOT}/src/shell3/tui/remote.rs"] mod remote;')
    for name in ('park', 'select', 'snapshot', 'color', 'input'):
        source += extract.item('src/shell3/tui.rs', name) + '\n'
    source += r'''
#[cfg(test)] mod tests {
    use super::*;
    fn reset() {
        *ROUTES.lock()=Registry::new();
        crate::hv::RUN.store(1,Ordering::Relaxed);
        crate::shell2::LIVE.store(1,Ordering::Relaxed);
        crate::hv::INPUT.lock().unwrap().clear();
    }
    fn setup(id:u64, name:&str, remote:bool)->crate::shell2::MatrixTarget {
        if remote {bind_remote_frontend(id);}
        let target=crate::shell2::MatrixSlotLease{name:name.into(),generation:1};
        attach(Frontend{id,cols:120,rows:40},&target).unwrap();
        bind_vm(&target,7);
        assert_eq!(claim(&target,7),Some(true));
        target
    }
    #[test] fn launch_backend_uses_the_frontends_own_ui4_window() {
        reset();
        bind_remote_frontend(1);
        assert!(!has_ui4_window(1));
        assert!(!has_ui4_window(2));
        bind_ui4_window(2,42);
        assert!(has_ui4_window(2));
        assert!(!has_ui4_window(1));
    }
    #[test] fn remote_bytes_bypass_cell_parser_and_host_query_responses() {
        reset();let target=setup(1,"ssh",true);
        let bytes="\x1b[38;5;8m🗺 §\x1b[6n\x1b[?1006h".as_bytes();
        assert_eq!(write(&target,7,bytes),Some(bytes.len()));
        let route=ROUTES.lock();
        assert!(route.routes[0].screen.render_rows().iter().all(|row|row.trim().is_empty()));
        drop(route);
        assert!(crate::hv::INPUT.lock().unwrap().is_empty());
        assert!(snapshot(1,Some("ssh")).is_none());
        let drain=take_remote_output(1,tty_limit()).unwrap();
        assert!(drain.active && drain.bytes.ends_with(bytes));
    }
    fn tty_limit()->usize {crate::tty::OUTPUT_LIMIT}
    #[test] fn local_ui4_still_renders_cells_and_answers_terminal_queries() {
        reset();let target=setup(2,"ui4",false);
        assert_eq!(write(&target,7,b"hi\x1b[6n"),Some(6));
        assert!(take_remote_output(2,tty_limit()).is_none());
        assert_eq!(snapshot(2,Some("ui4")).unwrap()[0][0].0,'h');
        assert_eq!(*crate::hv::INPUT.lock().unwrap(),b"\x1b[1;3R");
    }
    #[test] fn remote_input_including_unicode_and_terminal_replies_is_opaque() {
        reset();let _target=setup(1,"ssh",true);
        let bytes="§\x1b[8;40;120t\x1b[<65;2;3M".as_bytes();
        assert!(input(1,Some("ssh"),bytes));
        assert_eq!(*crate::hv::INPUT.lock().unwrap(),bytes);
    }
    #[test] fn stale_vm_run_and_released_owner_cannot_write_or_reclaim() {
        reset();let target=setup(1,"ssh",true);
        take_remote_output(1,tty_limit());
        crate::hv::RUN.store(2,Ordering::Relaxed);
        assert_eq!(write(&target,7,b"stale"),Some(0));
        assert_eq!(claim(&target,7),Some(false));
        assert_eq!(release(&target,7),Some(false));
        crate::hv::RUN.store(1,Ordering::Relaxed);
        assert_eq!(release(&target,7),Some(true));
        assert_eq!(write(&target,7,b"late"),Some(0));
        assert!(!remote_active(1));
    }
    #[test] fn fast_release_and_slot_free_keep_cleanup_after_pending_output() {
        reset();let target=setup(1,"ssh",true);
        write(&target,7,b"last frame");
        assert_eq!(release(&target,7),Some(true));
        let first=take_remote_output(1,1).unwrap();assert!(first.pending && !first.repaint);
        let rest=take_remote_output(1,tty_limit()).unwrap();assert!(rest.repaint && !rest.active);
        assert!(rest.bytes.windows(10).any(|w|w==b"last frame"));
        assert_eq!(claim(&target,7),Some(true));
        write(&target,7,b"before free");
        Attachment.on_matrix_slot_freed(&target);
        assert!(!remote_active(1));
        let drained=take_remote_output(1,tty_limit()).unwrap();
        assert!(drained.repaint && drained.bytes.windows(11).any(|w|w==b"before free"));
    }
    #[test] fn resize_updates_geometry_without_parsing_or_generating_app_output() {
        reset();let target=setup(1,"ssh",true);take_remote_output(1,tty_limit());
        assert!(select(Frontend{id:1,cols:150,rows:45},Some("ssh")));
        let geometry=surface(&target).unwrap();assert_eq!((geometry.cols,geometry.rows),(150,45));
        assert!(take_remote_output(1,tty_limit()).unwrap().bytes.is_empty());
    }
    #[test] fn separate_remote_frontends_do_not_share_output() {
        reset();let a=setup(1,"a",true);let b=setup(2,"b",true);
        take_remote_output(1,tty_limit());take_remote_output(2,tty_limit());
        write(&a,7,b"alpha");write(&b,7,b"beta");
        assert_eq!(take_remote_output(1,tty_limit()).unwrap().bytes,b"alpha");
        assert_eq!(take_remote_output(2,tty_limit()).unwrap().bytes,b"beta");
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-remote-') as directory:
        path=Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--crate-type=rlib','--crate-name','trueos_terminal',str(ROOT/'crates/trueos-terminal/src/lib.rs'),'-o',str(path/'libtrueos_terminal.rlib')],check=True)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests'),'--extern',f'trueos_terminal={path}/libtrueos_terminal.rlib'],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
