#!/usr/bin/env python3
"""Exercise production launch payloads, pullbot parsing, and new-shell handoff."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
BLUEPRINTS = ROOT.parent / "TRUEOS-Blueprints"
extract.ROOT = ROOT


def main():
    sdk = (BLUEPRINTS / "crates/trueos-v/src/vshell.rs").read_text()
    sdk = sdk[sdk.index("/// Launch an app with a one-shot UTF-8 start script"):]
    pullbot = (BLUEPRINTS / "buildins/appstore/src/pullbot.rs").read_text()
    pullbot = pullbot[:pullbot.index("pub fn report")]
    startup = (ROOT / "src/shell3/startup.rs").read_text()
    startup = startup[:startup.index("/// Preserve ordinary kernel progress")]
    service = (ROOT / "src/shell3/service.rs").read_text()
    admission = extract.item("src/shell3/service.rs", "ShellOwnership")
    admission += re.search(r"^impl ShellOwnership \{.*?^}", service, re.M | re.S).group()
    for name in ("advance_round_robin", "request_shell3_with_startup"):
        admission += extract.item("src/shell3/service.rs", name)
    source = r'''#![allow(dead_code)]
extern crate alloc;
mod percpu {pub const CPU_SLOT_LIMIT:usize=32;}
mod vcabi {
    pub static PAYLOAD:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
    pub static RC:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
    pub unsafe fn trueos_cabi_blueprint_launch_script_v1(ptr:*const u8,len:usize)->i32 {
        *PAYLOAD.lock().unwrap()=unsafe {core::slice::from_raw_parts(ptr,len)}.to_vec();
        RC.load(std::sync::atomic::Ordering::Relaxed)
    }
}
mod sdk {use alloc::vec::Vec;use crate::vcabi;
SDK_SOURCE
}
mod wire {
WIRE_SOURCE
}
mod pullbot {
PULLBOT_SOURCE
}
mod shell3 {
    use alloc::{string::String,vec::Vec,collections::VecDeque};
    const MAX_SHELL3_INSTANCES:usize=256;
    #[derive(Debug,PartialEq,Eq)]pub enum Shell3Error {NoExecutor,InstanceLimit}
    struct App {slot:String}
    struct MatrixSlots;
    impl MatrixSlots {fn echo(_:Option<&str>,_:Option<u64>,message:String) {ERRORS.lock().unwrap().push(message);}}
    static ERRORS:std::sync::Mutex<Vec<String>>=std::sync::Mutex::new(Vec::new());
    pub struct Shell3 {active_matrix_slot:Option<String>,active_matrix_lifetime:Option<u64>,events:Vec<String>}
    impl Shell3 {
        fn tui_frontend(&self)->u64 {42}
        fn select_queued_app(&mut self,app:App) {self.events.push(format!("attach:{}",app.slot));}
        fn select_matrix_slot_name(&mut self,name:&str)->bool {self.events.push(format!("enter:{name}"));true}
    }
    mod service {
        use super::*;
        struct Mutex<T>(std::sync::Mutex<T>);
        impl<T> Mutex<T> {const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))} fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}}
        static SHELL_OWNERSHIP:Mutex<ShellOwnership>=Mutex::new(ShellOwnership::new());
        struct Notify;impl Notify {fn notify_all(&self){}}
        static SHELL_WORK_AVAILABLE:Notify=Notify;
        fn refresh_appdb_names(){}
        fn warn_instance_limit(){}
ADMISSION_SOURCE
        pub static CALLS:std::sync::Mutex<Vec<(String,Vec<u8>,u64,Option<String>)>>=std::sync::Mutex::new(Vec::new());
        pub static FAIL:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
        pub fn launch_bytes_with_script(archive:String,bytes:Vec<u8>,frontend:u64,script:Option<String>)->Result<App,String> {
            CALLS.lock().unwrap().push((archive,bytes,frontend,script));
            if FAIL.load(std::sync::atomic::Ordering::Relaxed) {return Err("no VM".into());}
            Ok(App {slot:"vm-19".into()})
        }
        #[test] fn one_new_shell_reservation_carries_the_blueprint_and_respects_the_pool_cap() {
            assert_eq!(request_shell3_with_startup(None),Err(Shell3Error::NoExecutor));
            {let mut pool=SHELL_OWNERSHIP.lock();pool.worker_slots=vec![2];pool.live_shells=255;}
            let startup=super::startup::Startup::Blueprint {archive:"Frog.bp".into(),bytes:vec![9],script:Some("weather 13 51\n".into())};
            assert_eq!(request_shell3_with_startup(Some(startup)),Ok(2));
            assert_eq!(request_shell3_with_startup(None),Err(Shell3Error::InstanceLimit));
            let mut pool=SHELL_OWNERSHIP.lock();
            assert_eq!(pool.live_shells,255);assert_eq!(pool.pending_by_slot.iter().sum::<usize>(),1);
            assert_eq!(pool.startup_requests.len(),1);
            let (slot,startup)=pool.startup_requests.pop_front().unwrap();assert_eq!(slot,2);
            match startup.unwrap() {
                super::startup::Startup::Blueprint {archive,bytes,script} => {
                    assert_eq!(archive,"Frog.bp");assert_eq!(bytes,vec![9]);assert_eq!(script.as_deref(),Some("weather 13 51\n"));
                }, _=>panic!("wrong startup"),
            }
        }
    }
    pub mod startup {
STARTUP_SOURCE
    }
    #[test] fn new_shell_handoff_launches_once_and_enters_the_returned_slot() {
        let mut shell=Shell3 {active_matrix_slot:None,active_matrix_lifetime:None,events:Vec::new()};
        startup::Startup::Blueprint {archive:"Frog.bp".into(),bytes:vec![1,2,3],script:Some("weather -74 40\n".into())}.launch(&mut shell);
        assert_eq!(shell.events,vec!["attach:vm-19","enter:vm-19"]);
        assert_eq!(*service::CALLS.lock().unwrap(),vec![("Frog.bp".into(),vec![1,2,3],42,Some("weather -74 40\n".into()))]);
        service::FAIL.store(true,std::sync::atomic::Ordering::Relaxed);
        shell.events.clear();
        startup::Startup::Blueprint {archive:"bad.bp".into(),bytes:vec![],script:None}.launch(&mut shell);
        assert!(shell.events.is_empty());assert_eq!(*ERRORS.lock().unwrap(),vec!["no VM"]);
    }
}
#[test] fn legacy_payload_and_routed_scripts_round_trip_without_rewriting() {
    sdk::launch_with_script("Frog","weather 13 51\n").unwrap();
    assert_eq!(*vcabi::PAYLOAD.lock().unwrap(),b"Frog\0weather 13 51\n");
    let script="weather -74 40\nopen --sh3 something -- tail\n";
    for (destination,expected) in [(sdk::LaunchDestination::CurrentShell,wire::Destination::CurrentShell),
        (sdk::LaunchDestination::NewShell3,wire::Destination::NewShell3),
        (sdk::LaunchDestination::Headless,wire::Destination::Headless)] {
        sdk::launch_with_destination("common/dl/Frog.bp",script,destination).unwrap();
        let payload=vcabi::PAYLOAD.lock().unwrap();
        assert_eq!(wire::parse(&payload).unwrap(),wire::Request {app:"common/dl/Frog.bp",script,destination:expected});
    }
    vcabi::RC.store(-16,std::sync::atomic::Ordering::Relaxed);
    assert_eq!(sdk::launch_with_destination("appstore","",sdk::LaunchDestination::Headless),Err(-16));
    vcabi::RC.store(0,std::sync::atomic::Ordering::Relaxed);
}
#[test] fn malformed_destinations_and_embedded_nuls_are_rejected() {
    for payload in [&b"Frog"[..],b"\0script",b"Frog\0script\0unknown",b"Frog\0\0sh3\0extra",b"Frog\0\xff"] {
        assert!(wire::parse(payload).is_err());
    }
    for (app,script) in [("",""),("Frog\0x",""),("Frog","weather\0x")] {
        assert_eq!(sdk::launch_with_destination(app,script,sdk::LaunchDestination::NewShell3),Err(-1));
    }
}
'''
    wire = (ROOT / "src/r/io/launch_request.rs").read_text()
    source = source.replace("SDK_SOURCE", sdk).replace("WIRE_SOURCE", wire)
    source = source.replace("PULLBOT_SOURCE", pullbot).replace("STARTUP_SOURCE", startup)
    source = source.replace("ADMISSION_SOURCE", admission)
    with tempfile.TemporaryDirectory(prefix="blueprint-launch-destination-") as directory:
        path = Path(directory)
        (path / "test.rs").write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "tests")], check=True)
        subprocess.run([str(path / "tests"), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
