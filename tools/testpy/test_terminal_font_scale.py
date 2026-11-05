#!/usr/bin/env python3
"""Exercise Shell3's actual font getter with real terminal OSC state."""

from pathlib import Path
import subprocess
import tempfile

from test_edit_life_native import function


ROOT = Path(__file__).resolve().parents[2]

FIXTURE = r'''
#![allow(dead_code)]
use std::sync::Mutex;
use trueos_terminal::Terminal;
mod ui4 {
    #[derive(Clone,Copy,PartialEq,Eq)] pub struct WindowId(pub u32);
    #[derive(Clone,Copy)] pub enum WindowOwner {Vm(u8)}
    #[derive(Clone,Copy)] pub struct CursorFrameKey {pub window:WindowId}
    pub mod blueprint_ui4 {pub mod collapse_api {pub fn refresh_terminal(_:super::super::WindowOwner){}}}
}
struct SpinMutex<T>(Mutex<T>);
impl<T> SpinMutex<T> {
    const fn new(value:T)->Self {Self(Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
mod routing {
use super::*;
struct Lease(&'static str);
impl Lease {fn name(&self)->&str {self.0}}
struct Route {frontend:Option<u64>,selected:bool,active:bool,lease:Lease,screen:Terminal}
impl Route {fn active(&self)->bool {self.active}}
struct Registry {routes:Vec<Route>,remote_frontends:Vec<(u64,())>,ui4_windows:Vec<(u64,ui4::WindowId,u32)>}
static ROUTES:SpinMutex<Registry>=SpinMutex::new(Registry{routes:Vec::new(),remote_frontends:Vec::new(),ui4_windows:Vec::new()});
@GETTER@
@BINDER@
@APPLIED@
fn vm_for_window(_:ui4::WindowId)->Option<u8> {None}
fn route(frontend:u64,name:&'static str,scale:Option<u32>)->Route {
    let mut screen=Terminal::new(80,24);
    if let Some(scale)=scale {screen.feed(format!("\x1b]777;font-scale;1;{scale}\x07").as_bytes());}
    Route{frontend:Some(frontend),selected:true,active:true,lease:Lease(name),screen}
}
fn reset() {*ROUTES.lock()=Registry{routes:Vec::new(),remote_frontends:Vec::new(),ui4_windows:Vec::new()};}
#[test]
fn requests_are_isolated_by_frontend_and_selected_terminal_name() {
    reset();ROUTES.lock().routes=vec![route(1,"alpha",Some(1)),route(2,"beta",Some(8))];
    assert_eq!(font_scale(1,Some("alpha")),Some(1));
    assert_eq!(font_scale(2,Some("beta")),Some(8));
    assert_eq!(font_scale(1,Some("beta")),None);
    assert_eq!(font_scale(2,Some("alpha")),None);
    assert_eq!(font_scale(1,None),None);
    assert_eq!(font_scale(3,Some("alpha")),None);
}
#[test]
fn inactive_unselected_and_remote_routes_cannot_change_a_local_shell() {
    reset();
    let mut inactive=route(1,"inactive",Some(3));inactive.active=false;
    let mut unselected=route(1,"unselected",Some(4));unselected.selected=false;
    ROUTES.lock().routes=vec![inactive,unselected,route(1,"local",Some(5)),route(2,"peer",Some(7))];
    assert_eq!(font_scale(1,Some("inactive")),None);
    assert_eq!(font_scale(1,Some("unselected")),None);
    assert_eq!(font_scale(1,Some("local")),Some(5));
    ROUTES.lock().remote_frontends.push((1,()));
    assert_eq!(font_scale(1,Some("local")),None);
    assert_eq!(font_scale(2,Some("peer")),Some(7));
}
#[test]
fn synchronized_request_commits_once_and_survives_grid_resize() {
    reset();ROUTES.lock().routes=vec![route(1,"edit",Some(2))];
    ROUTES.lock().routes[0].screen.feed(b"\x1b[?2026h\x1b]777;font-scale;1;6\x07");
    assert_eq!(font_scale(1,Some("edit")),Some(2));
    ROUTES.lock().routes[0].screen.feed(b"\x1b[?2026l");
    assert_eq!(font_scale(1,Some("edit")),Some(6));
    ROUTES.lock().routes[0].screen.resize(30,8);
    assert_eq!(font_scale(1,Some("edit")),Some(6));
}
#[test]
fn absent_request_and_fresh_owner_use_the_host_default() {
    reset();ROUTES.lock().routes=vec![route(1,"plain",None),route(2,"edit",Some(4))];
    assert_eq!(font_scale(1,Some("plain")),None);
    ROUTES.lock().routes[1].screen.reset();
    assert_eq!(font_scale(2,Some("edit")),None);
}
#[test]
fn window_bindings_track_presented_scale_instead_of_pending_requests() {
    reset();ROUTES.lock().routes=vec![route(1,"edit",Some(2)),route(2,"peer",Some(4))];
    bind_ui4_window(1,ui4::WindowId(21),2);
    bind_ui4_window(2,ui4::WindowId(22),4);
    ROUTES.lock().routes[0].screen.feed(b"\x1b]777;font-scale;1;8\x07");
    assert_eq!(font_scale(1,Some("edit")),Some(8));
    assert_eq!(font_scale_for_window(ui4::WindowId(21)),Some(2),"failed or pending paint cannot change hit testing");
    assert_eq!(font_scale_for_window(ui4::WindowId(22)),Some(4));
    assert_eq!(font_scale_for_window(ui4::WindowId(0)),None);
    bind_ui4_window(1,ui4::WindowId(21),8);
    assert_eq!(font_scale_for_window(ui4::WindowId(21)),Some(8));
    assert_eq!(font_scale_for_window(ui4::WindowId(22)),Some(4));
    bind_ui4_window(1,ui4::WindowId(23),1);
    assert_eq!(font_scale_for_window(ui4::WindowId(21)),None,"replaced window binding must disappear");
    assert_eq!(font_scale_for_window(ui4::WindowId(23)),Some(1));
}
pub(super) fn bind_for_test(frontend:u64,window:u32,scale:u32) {bind_ui4_window(frontend,ui4::WindowId(window),scale);}
pub(super) fn reset_for_test() {reset();}
}
mod shell3 {pub mod tui {pub(crate) use crate::routing::font_scale_for_window;}}
mod r {pub mod services {pub mod shell3_font_service {
    pub const DEFAULT_UI4_FONT_SCALE:u32=2;
    pub fn cell_extent(scale:u32)->Option<(u32,u32)> {Some((scale*6,scale*11))}
}}}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u8> {None}}
mod trueos_vm {pub mod vmcall {
    pub const OP_BP_UI4_DRAG_TAKE_V1:u32=0;
    pub const STATUS_OK:u32=0;
    pub fn call_with_payload(_:u32,_:u64,_:u64,_:&[u8],_:&mut[u8])->(u32,u64) {panic!("guest route unused")}
}}
mod drag_drop {
    pub const MAX_PAYLOAD_BYTES:usize=64;
    pub fn take(_:super::ui4::WindowOwner,_:super::ui4::CursorFrameKey,out:&mut[u8])->Result<usize,i32> {
        out[..16].copy_from_slice(&[0;16]);
        out[4..8].copy_from_slice(&96i32.to_le_bytes());out[8..12].copy_from_slice(&176i32.to_le_bytes());Ok(16)
    }
}
mod drag {
use super::*;
use std::sync::atomic::{AtomicU32,Ordering};
const ERROR_INVALID:i32=-1;const ERROR_UI4:i32=-2;const ERROR_CONTEXT:i32=-3;
static TARGET:AtomicU32=AtomicU32::new(21);
fn blueprint_owner()->Option<ui4::WindowOwner> {Some(ui4::WindowOwner::Vm(1))}
fn frame(_:ui4::WindowOwner,id:u32)->Result<ui4::CursorFrameKey,i32> {
    Ok(ui4::CursorFrameKey{window:ui4::WindowId(if id==0 {TARGET.load(Ordering::SeqCst)}else{id})})
}
@DRAG@
fn take(id:u32)->(i32,i32) {
    let mut bytes=[0u8;16];
    assert_eq!(trueos_cabi_ui4_drag_take_v1(id,bytes.as_mut_ptr(),bytes.len()),16);
    (i32::from_le_bytes(bytes[4..8].try_into().unwrap()),i32::from_le_bytes(bytes[8..12].try_into().unwrap()))
}
#[test]
fn drop_uses_the_target_windows_applied_scale_and_leaves_scene_pixels_unchanged() {
    routing::reset_for_test();
    routing::bind_for_test(1,21,2);routing::bind_for_test(2,22,4);
    TARGET.store(21,Ordering::SeqCst);assert_eq!(take(0),(8,8));
    TARGET.store(22,Ordering::SeqCst);assert_eq!(take(0),(4,4));
    routing::bind_for_test(2,22,8);assert_eq!(take(0),(2,2));
    TARGET.store(23,Ordering::SeqCst);assert_eq!(take(0),(8,8),"unbound terminal uses host default");
    assert_eq!(take(7),(96,176),"scene drop coordinates stay in pixels");
}
}
'''


def main() -> None:
    tui = (ROOT / "src/shell3/tui.rs").read_text()
    fixture = FIXTURE.replace("@GETTER@", function(tui, "font_scale"))
    fixture = fixture.replace("@BINDER@", function(tui, "bind_ui4_window"))
    fixture = fixture.replace("@APPLIED@", "pub(crate) " + function(tui, "font_scale_for_window"))
    drag = (ROOT / "src/ui4/blueprint_ui4/drag_drop_api.rs").read_text()
    fixture = fixture.replace("@DRAG@", function(drag, "trueos_cabi_ui4_drag_take_v1"))
    with tempfile.TemporaryDirectory(prefix="trueos-terminal-font-scale-") as temporary:
        directory = Path(temporary)
        library = directory / "libtrueos_terminal.rlib"
        source = directory / "font_scale.rs"
        binary = directory / "font_scale"
        source.write_text(fixture)
        subprocess.run(["rustc", "--edition=2024", "--crate-type=lib", "--crate-name", "trueos_terminal",
            str(ROOT / "crates/trueos-terminal/src/lib.rs"), "-o", str(library)], check=True)
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "--extern",
            f"trueos_terminal={library}", "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
