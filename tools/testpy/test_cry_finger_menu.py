#!/usr/bin/env python3
"""Exercise the production Cry Finger menu with deterministic host boundaries.

Actual menu navigation, input decoding and layout methods are compiled unchanged.
The USB scenarios are fixtures: these tests verify that choosing a scenario calls
the selected diagnostic once, without submitting authentication or disk work.
No account files, enrollment records or hardware are accessed.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
use alloc::{format, string::{String, ToString}, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::ops::{Deref, DerefMut};

struct Zeroizing<T>(T);
impl<T> Zeroizing<T> { fn new(value:T)->Self {Self(value)} }
impl<T> Deref for Zeroizing<T> { type Target=T; fn deref(&self)->&T {&self.0} }
impl<T> DerefMut for Zeroizing<T> { fn deref_mut(&mut self)->&mut T {&mut self.0} }
#[derive(Clone,Copy)] struct MatrixTarget;
#[derive(Clone,Copy)] struct DeviceHandle;
struct DiskId; impl DiskId {fn raw(&self)->u32 {1}}
struct DiskInfo { id:DiskId, label:Option<String>, writable:bool, block_count:u64, block_size:u32 }
impl DeviceHandle {fn info(self)->DiskInfo {DiskInfo{id:DiskId,label:None,writable:false,block_count:1,block_size:512}}}

static AUTH_CALLS: AtomicUsize=AtomicUsize::new(0);
static REQUESTS: AtomicUsize=AtomicUsize::new(0);
static RETURNS: AtomicUsize=AtomicUsize::new(0);
static SUMMARY_CALLS: AtomicUsize=AtomicUsize::new(0);
static SCENARIO_RUNS: std::sync::Mutex<Vec<cry::finger::Scenario>>=std::sync::Mutex::new(Vec::new());

mod tui {
    use super::*;
    pub fn native_frontend_id(_: &MatrixTarget)->Option<u64>{Some(71)}
    pub fn bind_authenticated_frontend(_:u64,_:Option<u64>)->bool{AUTH_CALLS.fetch_add(1,Ordering::SeqCst);true}
    pub fn native_visible(_: &MatrixTarget)->bool{RETURNS.load(Ordering::SeqCst)==0}
    pub fn native_return(_: &MatrixTarget){RETURNS.fetch_add(1,Ordering::SeqCst);}
    pub fn native_transport_scope(_: &MatrixTarget)->Option<u8>{Some(1)}
}
mod matrix_target {pub const TRANSPORT_LOCAL_SCOPE:u8=0;}
mod capture {pub fn error_result(message:&str)->String {format!("ERROR: {message}")}}
mod crypt {
    use super::*;
    #[derive(PartialEq)] pub enum CryTwoFactorState {Active}
    pub struct Status {pub two_factor:CryTwoFactorState}
    pub fn status()->Status {Status{two_factor:CryTwoFactorState::Active}}
    pub fn ssh_authorized_keys()->Vec<[u8;32]>{Vec::new()}
    pub fn ssh_key_fingerprint(_: &[u8;32])->String {"fixture-key".into()}
    pub fn authenticated_recovery_key(_:u8)->Option<Zeroizing<[u8;32]>>{None}
    pub fn logout(_:u8){AUTH_CALLS.fetch_add(1,Ordering::SeqCst);}
}
mod cry {
    use super::*;
    pub struct AccountEnrollment {pub username:String}
    impl AccountEnrollment {
        pub fn qr_lines(&self)->Result<Zeroizing<Vec<String>>,String>{Ok(Zeroizing::new(vec!["QR".into()]))}
        pub fn code_verified(&self)->bool{false}
    }
    pub fn full_hex(_: &[u8])->String {"fixture-recovery".into()}
    pub mod finger {
        use super::*;
        #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Scenario {UsbStatus,UsbReadiness}
        pub const SCENARIOS:&[Scenario]=&[Scenario::UsbStatus,Scenario::UsbReadiness];
        impl Scenario {
            pub fn label(self)->&'static str {match self {Self::UsbStatus=>"USB status",Self::UsbReadiness=>"USB readiness"}}
            pub fn run(self)->Vec<String>{
                SCENARIO_RUNS.lock().unwrap().push(self);
                vec![self.label().into(),"Reader 06cb:00e9".into(),"Fixture result".into()]
            }
        }
        pub fn summary()->Vec<String>{SUMMARY_CALLS.fetch_add(1,Ordering::SeqCst);vec!["Reader 06cb:00e9".into(),"Diagnostic tests".into()]}
    }
}
mod tlb_helper {
    use super::*;
    pub struct DiskChoice {pub handle:DeviceHandle}
    impl DiskChoice {
        pub fn raw_id(&self)->u32{1}
        pub fn label_text(&self)->&str{"disk"}
        pub fn size_text(&self)->&str{"512 bytes"}
        pub fn mode_text(&self)->&str{"read-only"}
    }
    pub fn collect_top_level_disk_choices()->Vec<DiskChoice>{Vec::new()}
}
#[derive(Default)] struct Screen;
impl Screen {
    fn invalidate(&mut self){}
    fn paint(&mut self,_:&MatrixTarget,_:&[String],_:usize,_:usize){}
}
type Reply=();
enum Request {Enrollment,Ramdisc(u64)}

mod helper {
use super::*;
@ACTION@
@INPUT@
@INPUT_IMPL@
@DECODE@
}
use helper::{Action,Input};

mod admin {
use super::*;
@KIND@
@KIND_IMPL@
@FIELD@
@FIELD_IMPL@
@FORM@
@FORM_DEFAULT@
@FORM_IMPL@
@PAGE@
@PAGE_IMPL@
@VIEW@
impl View {
@NEW@
@SET_PAGE@
@LABELS@
@BODY@
@CHOOSE@
@INPUT_METHOD@
@MENU_START@
@LEAVE@
@FORMAT_FORM@
    fn account_lines(_:u8)->Vec<String>{vec!["Automatic account: fixture".into(),"Session: fixture".into(),"Persistence: fixture".into()]}
    fn problem(&mut self,message:&str){self.result=super::capture::error_result(message);}
    fn scope(target:&MatrixTarget)->u8{super::tui::native_transport_scope(target).unwrap()}
    fn submit(&mut self,_:&MatrixTarget,_:Request){REQUESTS.fetch_add(1,Ordering::SeqCst);}
    fn submit_form(&mut self,_:&MatrixTarget){panic!("Finger navigation entered an unrelated form")}
    fn cancel_login(&mut self){}
}

#[cfg(test)] mod tests {
use super::*;
fn reset(){
    AUTH_CALLS.store(0,Ordering::SeqCst);REQUESTS.store(0,Ordering::SeqCst);
    RETURNS.store(0,Ordering::SeqCst);SUMMARY_CALLS.store(0,Ordering::SeqCst);
    SCENARIO_RUNS.lock().unwrap().clear();
}
fn view(draft:bool)->View {
    reset();let mut view=View::new(Kind::Cry);
    if draft {view.draft=Some(Arc::new(cry::AccountEnrollment{username:"second".into()}));}
    view
}
fn select(view:&mut View,label:&str){
    view.selected=view.labels().iter().position(|entry|entry==label).unwrap_or_else(||panic!("missing {label}"));
    view.choose(&MatrixTarget);
}
fn open(view:&mut View){select(view,"Finger");assert!(matches!(view.page,Page::Finger(_)));}
fn no_authentication_or_jobs(){assert_eq!(AUTH_CALLS.load(Ordering::SeqCst),0);assert_eq!(REQUESTS.load(Ordering::SeqCst),0);}

#[test] fn finger_entry_preserves_home_actions_with_and_without_an_account_draft(){
    for draft in [false,true] {
        let mut view=view(draft);let labels=view.labels();
        assert_eq!(labels[7],"Sign out");assert_eq!(labels[8],"Finger");assert_eq!(labels.last().unwrap(),"Return");
        if draft {assert_eq!(labels[9],"Close unfinished enrollment");}
        open(&mut view);no_authentication_or_jobs();
    }
    let disc=View::new(Kind::Disc);assert!(!disc.labels().iter().any(|label|label=="Finger"));
}

#[test] fn opening_finger_shows_reader_summary_and_available_scenarios_without_running_one(){
    let mut view=view(false);open(&mut view);
    assert_eq!(SUMMARY_CALLS.load(Ordering::SeqCst),1);
    assert!(view.body(&MatrixTarget).iter().any(|line|line.contains("06cb:00e9")));
    let mut expected:Vec<String>=cry::finger::SCENARIOS.iter().map(|scenario|scenario.label().into()).collect();
    expected.extend(["Back".into(),"Return".into()]);assert_eq!(view.labels(),expected);
    assert!(SCENARIO_RUNS.lock().unwrap().is_empty());no_authentication_or_jobs();
}

#[test] fn each_scenario_runs_once_and_has_a_result_page_with_back_and_return(){
    for scenario in cry::finger::SCENARIOS {
        let mut view=view(false);open(&mut view);select(&mut view,scenario.label());
        assert!(matches!(view.page,Page::FingerResult(_)));
        assert_eq!(*SCENARIO_RUNS.lock().unwrap(),[*scenario]);
        let body=view.body(&MatrixTarget);assert_eq!(body[0],scenario.label());assert!(body.len()<=4);
        assert_eq!(view.labels(),["Back","Return"]);no_authentication_or_jobs();
    }
}

#[test] fn results_back_reopens_finger_and_finger_back_returns_home(){
    let mut view=view(false);open(&mut view);select(&mut view,cry::finger::SCENARIOS[0].label());
    select(&mut view,"Back");assert!(matches!(view.page,Page::Finger(_)));
    assert_eq!(SUMMARY_CALLS.load(Ordering::SeqCst),2);assert_eq!(view.selected,0);
    select(&mut view,"Back");assert!(matches!(view.page,Page::Home));
    assert_eq!(RETURNS.load(Ordering::SeqCst),0);no_authentication_or_jobs();
}

#[test] fn return_from_both_finger_pages_returns_the_current_terminal_without_authentication(){
    for result in [false,true] {
        let mut view=view(false);open(&mut view);
        if result {select(&mut view,cry::finger::SCENARIOS[1].label());}
        select(&mut view,"Return");assert_eq!(RETURNS.load(Ordering::SeqCst),1);no_authentication_or_jobs();
    }
}

#[test] fn finger_round_trip_preserves_draft_and_its_close_action_remains_at_nine(){
    let mut view=view(true);let draft=view.draft.clone().unwrap();open(&mut view);
    select(&mut view,cry::finger::SCENARIOS[0].label());select(&mut view,"Back");select(&mut view,"Back");
    assert!(Arc::ptr_eq(view.draft.as_ref().unwrap(),&draft));assert_eq!(view.labels()[9],"Close unfinished enrollment");
    view.selected=9;view.choose(&MatrixTarget);assert!(view.draft.is_none());assert_eq!(view.result,"Enrollment closed.");
    assert_eq!(view.selected,0);no_authentication_or_jobs();
}

#[test] fn sign_out_at_seven_still_invokes_the_existing_signout_action(){
    for draft in [false,true] {
        let mut view=view(draft);view.selected=7;view.choose(&MatrixTarget);
        assert_eq!(AUTH_CALLS.load(Ordering::SeqCst),2);assert_eq!(view.result,"Signed out.");
        assert_eq!(REQUESTS.load(Ordering::SeqCst),0);assert!(matches!(view.page,Page::Home));
    }
}

#[test] fn keyboard_and_mouse_can_select_the_second_test_in_a_five_row_terminal(){
    let mut view=view(false);open(&mut view);
    for rows in 5..=24 {
        let start=view.menu_start(rows);assert!(start>=2);assert!(rows.saturating_sub(start+2)>=1);
    }
    view.input(&MatrixTarget,b"j",0,5);assert_eq!(view.selected,1);
    // Only the highlighted row fits. Click its first visible cell.
    let mouse=format!("\x1b[<0;1;{}M",view.menu_start(5)+1);
    view.input(&MatrixTarget,mouse.as_bytes(),1,5);
    assert!(matches!(view.page,Page::FingerResult(_)));
    assert_eq!(*SCENARIO_RUNS.lock().unwrap(),[cry::finger::SCENARIOS[1]]);no_authentication_or_jobs();
}
}
}
'''


def main():
    admin = (ROOT / "src/shell3/admin.rs").read_text()
    helper = (ROOT / "src/shell3/helper.rs").read_text()
    fields = {
        "ACTION": item(helper, "pub(super) enum Action"),
        "INPUT": "#[derive(Default)]\n" + item(helper, "pub(super) struct Input"),
        "INPUT_IMPL": item(helper, "impl Input"),
        "DECODE": item(helper, "fn decode_cell_sequence("),
        "KIND": "#[derive(Clone, Copy, PartialEq, Eq)]\n" + item(admin, "enum Kind"),
        "KIND_IMPL": item(admin, "impl Kind"),
        "FIELD": item(admin, "enum Field"),
        "FIELD_IMPL": item(admin, "impl Field"),
        "FORM": item(admin, "struct Form"),
        "FORM_DEFAULT": item(admin, "impl Default for Form"),
        "FORM_IMPL": item(admin, "impl Form"),
        "PAGE": item(admin, "enum Page"),
        "PAGE_IMPL": item(admin, "impl Page"),
        "VIEW": item(admin, "struct View"),
    }
    fields["ACTION"] = "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n" + fields["ACTION"]
    for name, signature in (
        ("NEW", "fn new(kind: Kind)"),
        ("SET_PAGE", "fn set_page("),
        ("LABELS", "fn labels("),
        ("BODY", "fn body("),
        ("CHOOSE", "fn choose("),
        ("INPUT_METHOD", "fn input("),
        ("MENU_START", "fn menu_start("),
        ("LEAVE", "fn leave("),
        ("FORMAT_FORM", "fn format_form("),
    ):
        fields[name] = item(admin, signature)
    source = HARNESS
    for name, value in fields.items():
        source = source.replace("@" + name + "@", value)
    assert not re.search(r"@[A-Z_]+@", source), "unexpanded harness marker"
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-cry-finger-menu-") as directory:
        directory = Path(directory)
        code = directory / "tests.rs"
        code.write_text(source)
        binary = directory / "tests"
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(code), "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=directory, check=True)


if __name__ == "__main__":
    main()
