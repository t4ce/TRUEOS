#!/usr/bin/env python3
"""Exercise production Edit registration failure/pause lifecycle on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

FIXTURE = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod wait { pub struct EmbassySpinRawMutex; }
struct Signal<M,T>(core::marker::PhantomData<(M,T)>);
impl<M,T> Signal<M,T> {
    const fn new() -> Self { Self(core::marker::PhantomData) }
    fn signal(&self, _:T) { WAKES.fetch_add(1, Ordering::SeqCst); }
}
#[macro_export] macro_rules! log_error { ($($tokens:tt)*) => {}; }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowId(u32);
impl WindowId { fn raw(self) -> u32 { self.0 } }
type FrameHandle = u32;
#[derive(Clone,Copy,PartialEq,Eq)] struct Rect;
struct Page { width:u32, height:u32, viewport:Rect, cell_width:u32, cell_height:u32 }
impl Page { fn valid(&self) -> bool { self.width != 0 && self.height != 0 } }
const MAX_PAGES:usize = 8;
static WAKES:AtomicUsize = AtomicUsize::new(0);
static RESTORE_OK:AtomicBool = AtomicBool::new(true);
static RESTORES:Mutex<Vec<(WindowId,FrameHandle)>> = Mutex::new(Vec::new());
static ATTACHED:AtomicUsize = AtomicUsize::new(0);
static RETIRED:Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
fn retire_frame_when_released(frame:FrameHandle) { RETIRED.lock().push(frame); }
mod window_broker {
    pub struct Snapshot { pub frame:super::FrameHandle }
    pub fn window_snapshot(_:(),_:super::WindowId) -> Option<Snapshot> {
        let frame=super::ATTACHED.load(super::Ordering::SeqCst) as super::FrameHandle;
        (frame!=0).then_some(Snapshot{frame})
    }
}
fn show_source(window:WindowId, source:FrameHandle) -> Result<(),()> {
    RESTORES.lock().push((window,source));
    if RESTORE_OK.load(Ordering::SeqCst) { Ok(()) } else { Err(()) }
}
@PRODUCTION@
mod effects {
    use super::*;
    const OWNER:()=();
    struct Effect { quarantined:bool, input:Registration, frame:FrameHandle }
    @DROP@
    #[test]
    fn drop_requires_detachment_as_well_as_gpu_retirement() {
        reset();
        publish_page(WindowId(1),10,page());
        let input=PAGES.lock()[0].clone();
        ATTACHED.store(90,Ordering::SeqCst);
        drop(Effect{quarantined:false,input:input.clone(),frame:90});
        assert!(RETIRED.lock().is_empty(),"attached frame must remain allocated");
        ATTACHED.store(10,Ordering::SeqCst);
        drop(Effect{quarantined:false,input:input.clone(),frame:90});
        assert_eq!(*RETIRED.lock(),[90]);
        drop(Effect{quarantined:true,input,frame:91});
        assert_eq!(*RETIRED.lock(),[90],"uncertain GPU output must stay allocated");
    }
}
fn page() -> Arc<Page> {
    Arc::new(Page { width:80,height:40,viewport:Rect,cell_width:6,cell_height:11 })
}
fn reset() {
    PAGES.lock().clear(); RESTORES.lock().clear();
    WAKES.store(0,Ordering::SeqCst); RESTORE_OK.store(true,Ordering::SeqCst);
    ATTACHED.store(0,Ordering::SeqCst); RETIRED.lock().clear();
}
#[test]
fn failure_revokes_old_input_restores_source_and_leaves_peer_running() {
    reset();
    publish_page(WindowId(1),10,page());
    publish_page(WindowId(2),20,page());
    let input=PAGES.lock()[0].clone();
    let peer=PAGES.lock()[1].clone();
    PAGES.lock()[0].displayed=Some(90);
    assert!(input_is_active(&input));
    stop(WindowId(1),"test-native-failure");
    assert!(is_failed(WindowId(1)));
    assert!(!input_is_active(&input));
    assert!(input_is_active(&peer));
    assert!(!is_failed(WindowId(2)));
    assert_eq!(*RESTORES.lock(),[(WindowId(1),10)]);
    let current=PAGES.lock()[0].clone();
    assert_ne!(current.serial,input.serial);
    assert!(current.paused);
    assert_eq!(current.displayed,None);
}
#[test]
fn new_text_and_menu_round_trip_cannot_restart_failed_producer() {
    reset();
    let retained=page();
    publish_page(WindowId(1),10,Arc::clone(&retained));
    stop(WindowId(1),"test-native-failure");
    let stopped=PAGES.lock()[0].clone();
    let wake_count=WAKES.load(Ordering::SeqCst);
    publish_page(WindowId(1),11,page());
    assert_eq!(WAKES.load(Ordering::SeqCst),wake_count);
    assert_eq!(PAGES.lock()[0].serial,stopped.serial);
    pause(WindowId(1),12);
    publish_page(WindowId(1),12,page());
    let current=PAGES.lock()[0].clone();
    assert!(is_failed(WindowId(1)));
    assert!(!input_is_active(&current));
    assert_eq!(current.source,12);
    assert!(Arc::ptr_eq(&retained,&current.page));
}
#[test]
fn repeated_stop_does_not_rewake_or_change_identity() {
    reset();
    publish_page(WindowId(1),10,page());
    stop(WindowId(1),"test-native-failure");
    let serial=PAGES.lock()[0].serial;
    let wake_count=WAKES.load(Ordering::SeqCst);
    for _ in 0..3 { stop(WindowId(1),"test-native-failure"); }
    assert_eq!(PAGES.lock()[0].serial,serial);
    assert_eq!(WAKES.load(Ordering::SeqCst),wake_count);
}
#[test]
fn stop_restores_newest_registered_source_after_an_older_render_fails() {
    reset();
    publish_page(WindowId(1),10,page());
    let stale=PAGES.lock()[0].clone();
    PAGES.lock()[0].displayed=Some(90);
    publish_page(WindowId(1),11,page());
    stop(stale.window,"test-old-render-failed");
    assert_eq!(PAGES.lock()[0].source,11);
    assert_eq!(*RESTORES.lock(),[(WindowId(1),11)]);
}
#[test]
fn ordinary_redraw_retries_failed_source_restore_without_reviving_effect() {
    reset();
    publish_page(WindowId(1),10,page());
    PAGES.lock()[0].displayed=Some(90);
    RESTORE_OK.store(false,Ordering::SeqCst);
    stop(WindowId(1),"test-native-failure");
    assert_eq!(PAGES.lock()[0].displayed,Some(90));
    RESTORE_OK.store(true,Ordering::SeqCst);
    pause(WindowId(1),11);
    let current=PAGES.lock()[0].clone();
    assert_eq!(current.displayed,None);
    assert!(current.failed);
    assert!(!input_is_active(&current));
    assert_eq!(*RESTORES.lock(),[(WindowId(1),10),(WindowId(1),11)]);
}
#[test]
fn successful_restore_wakes_cleanup_even_when_source_and_pause_are_unchanged() {
    reset();
    publish_page(WindowId(1),10,page());
    PAGES.lock()[0].displayed=Some(90);
    RESTORE_OK.store(false,Ordering::SeqCst);
    stop(WindowId(1),"test-native-failure");
    let wakes=WAKES.load(Ordering::SeqCst);
    RESTORE_OK.store(true,Ordering::SeqCst);
    pause(WindowId(1),10);
    assert_eq!(PAGES.lock()[0].displayed,None);
    assert_eq!(WAKES.load(Ordering::SeqCst),wakes+1);
    assert!(is_failed(WindowId(1)));
}
#[test]
fn explicit_removal_allows_fresh_registration_without_affecting_peer() {
    reset();
    publish_page(WindowId(1),10,page());
    publish_page(WindowId(2),20,page());
    let peer=PAGES.lock()[1].clone();
    stop(WindowId(1),"test-native-failure");
    let old=PAGES.lock()[0].clone();
    remove(WindowId(1));
    assert!(!is_failed(WindowId(1)));
    publish_page(WindowId(1),11,page());
    let current=PAGES.lock().iter().find(|r| r.window==WindowId(1)).unwrap().clone();
    assert!(input_is_active(&current));
    assert!(!input_is_active(&old));
    assert!(input_is_active(&peer));
}
'''


def main() -> None:
    source = (ROOT / "src/ui4/edit_life.rs").read_text()
    start = source.index("#[derive(Clone)]\nstruct Registration")
    production = source[start:source.index("\nfn show_source(", start)]
    start = source.index("impl Drop for Effect {")
    drop = source[start:source.index("\nimpl Effect {", start)]
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-failure-") as temporary:
        fixture = Path(temporary) / "tests.rs"
        binary = Path(temporary) / "tests"
        fixture.write_text(FIXTURE.replace("@PRODUCTION@", production).replace("@DROP@", drop))
        subprocess.run(["rustc", "--edition=2024", "--test", str(fixture), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
