#!/usr/bin/env python3
"""Exercise production human-session capture and remote controller lease checks."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/shell3/tui.rs").read_text()
    definitions = "\n".join(
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n" + item(source, signature)
        for signature in [
            "pub(crate) struct Frontend", "struct Owner",
            "pub(crate) struct RemoteInputOrigin", "pub(crate) struct RemoteInputAuthority",
            "enum RemoteInputOwner",
        ]
    )
    definitions += "\n" + item(source, "impl RemoteInputAuthority")
    functions = "\n".join(item(source, signature) for signature in [
        "pub(super) fn frontend_access_account(",
        "pub(super) fn capture_remote_input_origin(",
        "pub(crate) fn bind_remote_input_origin(",
        "pub(crate) fn remote_input_authority(",
        "pub(crate) fn remote_input_authority_valid(",
        "pub(crate) fn open_rdp_input_session(",
        "pub(crate) fn rdp_input_authority(",
        "pub(crate) fn close_rdp_input_session(",
    ])
    service = (ROOT / "src/shell3/service.rs").read_text()
    launches = "\n".join(item(service, signature) for signature in [
        "pub(crate) fn launch_bytes_with_script(",
        "pub(crate) fn launch_bytes_with_script_for_account(",
        "fn launch_bytes_with_args(",
    ])
    harness = r'''
extern crate alloc;
extern crate self as trueos_crypto;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountId(u64);
impl AccountId { pub const fn new(id:u64)->Self {Self(id)} }
pub struct Lock<T>(std::sync::Mutex<T>);
impl<T> Lock<T> {
    const fn new(value:T)->Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Clone, Copy)]
struct World {run:u64,running:bool,stopping:bool,slot_live:bool,account:Option<AccountId>}
static WORLD:Lock<World>=Lock::new(World{run:7,running:true,stopping:false,slot_live:true,account:Some(AccountId(1))});
mod hv {
    pub enum BlueprintConsoleSurface { Text, Terminal }
    pub(crate) mod launcher {
        pub struct LaunchRequest {
            pub archive:String,pub bytes:Vec<u8>,pub script:Option<String>,pub args:Vec<String>,
            pub access_account:Option<crate::AccountId>,
            pub remote_input_origin:Option<crate::shell3::tui::RemoteInputOrigin>,
            pub frontend:Option<crate::shell3::tui::Frontend>,
            pub console_surface:super::BlueprintConsoleSurface,
        }
        impl LaunchRequest {
            pub fn new(archive:String,bytes:Vec<u8>)->Self {
                Self{archive,bytes,script:None,args:Vec::new(),access_account:None,
                    remote_input_origin:None,frontend:None,console_surface:super::BlueprintConsoleSurface::Text}
            }
        }
        pub trait LaunchObserver {}
        pub fn enqueue(request:LaunchRequest,_:Option<Box<dyn LaunchObserver>>)->Result<(),String> {
            crate::REQUESTS.lock().push(request);Ok(())
        }
    }
    pub struct State {pub running:bool,pub stop_requested:bool}
    pub fn vm_run_generation(vm:u8)->Option<u64> {(vm==1).then(||crate::WORLD.lock().run)}
    pub fn vm_state(_:u8)->State {let w=crate::WORLD.lock();State{running:w.running,stop_requested:w.stopping}}
}
mod access_key_db {
    pub fn principal_for_vm(_:u8)->Option<crate::AccountId>{crate::WORLD.lock().account}
}
mod r { pub mod keyboard { pub fn output_event_frontiers()->(u32,u32){(100,200)} } }
mod usb2 { pub mod hid { pub fn cursor_event_frontier()->u64{300} } }
mod app_db {
    pub fn is_remote_input_controller(archive:&str, bytes:&[u8])->bool {
        archive=="commander.bp" && bytes==[1]
    }
}
static REQUESTS:Lock<Vec<hv::launcher::LaunchRequest>>=Lock::new(Vec::new());
mod shell3 {
    pub type MatrixTarget=u64;
    pub fn matrix_target_slot_lease(target:&MatrixTarget)->u64 {*target}
    pub(crate) mod os {pub fn arguments()->Vec<String>{Vec::new()}}
    pub(crate) mod service {
        use std::vec::Vec;
        type QueuedBlueprint=();
        struct ShellLaunch;
        impl crate::hv::launcher::LaunchObserver for ShellLaunch {}
        @LAUNCHES@
    }
    pub mod tui {
        use super::MatrixTarget;
        use crate::{AccountId,Lock};
        fn matrix_slot_is_live(_: &u64)->bool {crate::WORLD.lock().slot_live}
        #[path="@ACCOUNTS@"] mod accounts;
        @DEFINITIONS@
        struct Route {
            lease:u64, frontend:Option<u64>,selected:bool,vm:Option<u8>,owner:Option<Owner>,
            remote_input_origin:Option<RemoteInputOrigin>,
            remote_input_frontiers:Option<((u32,u32),u64)>,surface_generation:u64,
        }
        struct Registry {
            routes:Vec<Route>,remote_frontends:Vec<(u64,())>,accounts:accounts::Bindings,
            navigation_holds:Vec<u64>,
            native_input_sessions:Vec<RemoteInputAuthority>,
        }
        static ROUTES:Lock<Registry>=Lock::new(Registry {
            routes:Vec::new(),remote_frontends:Vec::new(),accounts:accounts::Bindings::new(),
            navigation_holds:Vec::new(),
            native_input_sessions:Vec::new(),
        });
        fn new_frontend()->u64 {
            static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1000);
            NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)
        }
        fn bind_remote_frontend(frontend:u64) { ROUTES.lock().remote_frontends.push((frontend,())); }
        @FUNCTIONS@
        #[cfg(test)] mod tests {
            use super::*;
            fn frontend()->Frontend{Frontend{id:10,cols:120,rows:40}}
            fn setup()->RemoteInputAuthority {
                *crate::WORLD.lock()=crate::World{run:7,running:true,stopping:false,slot_live:true,account:Some(AccountId(1))};
                *ROUTES.lock()=Registry{routes:Vec::new(),remote_frontends:vec![(10,())],
                    accounts:accounts::Bindings::new(),navigation_holds:Vec::new(),native_input_sessions:Vec::new()};
                ROUTES.lock().accounts.bind_human(10,Some(AccountId(1)));
                let origin=capture_remote_input_origin(frontend()).unwrap();
                ROUTES.lock().routes.push(Route{lease:90,frontend:Some(10),selected:true,
                    vm:Some(1),owner:None,remote_input_origin:None,
                    remote_input_frontiers:Some(((100,200),300)),surface_generation:4});
                bind_remote_input_origin(&90,origin);
                ROUTES.lock().routes[0].owner=Some(Owner{vm:1,run:7});
                remote_input_authority(1,7).unwrap()
            }
            #[test] fn human_controller_keeps_exact_account_and_input_frontiers(){
                let authority=setup();
                assert_eq!(authority.account,AccountId(1));
                assert_eq!(authority.keyboard_frontiers,(100,200));
                assert_eq!(authority.cursor_frontier,300);
                assert!(remote_input_authority_valid(authority));
            }
            #[test] fn native_ssh_and_commander_share_validation_but_keep_separate_lifetimes(){
                let commander=setup();
                let native=open_rdp_input_session(AccountId(0));
                let id=native.session_id().unwrap();
                assert!(remote_input_authority_valid(native));
                assert!(remote_input_authority_valid(commander));
                close_rdp_input_session(id);
                assert!(!remote_input_authority_valid(native));
                assert!(remote_input_authority_valid(commander));
                let replacement=open_rdp_input_session(AccountId(0));
                assert_ne!(native,replacement);
                assert!(!remote_input_authority_valid(native));
                close_rdp_input_session(replacement.session_id().unwrap());
            }
            #[test] fn native_ssh_account_rebinding_revokes_its_control(){
                setup(); let native=open_rdp_input_session(AccountId(1));
                let id=native.session_id().unwrap();
                ROUTES.lock().accounts.bind_human(id,Some(AccountId(2)));
                assert!(!remote_input_authority_valid(native));
                close_rdp_input_session(id);
            }
            #[test] fn inherited_guest_account_cannot_capture_human_control(){
                setup();
                let mut routes=ROUTES.lock();routes.accounts.remove(10);
                routes.accounts.bind_guest(10,Some(AccountId(1)));drop(routes);
                assert!(capture_remote_input_origin(frontend()).is_none());
                assert!(remote_input_authority(1,7).is_none());
            }
            #[test] fn account_alone_never_grants_an_unadmitted_app_control(){
                setup();ROUTES.lock().routes[0].remote_input_origin=None;
                assert!(remote_input_authority(1,7).is_none());
            }
            #[test] fn logout_reauthentication_and_detachment_expire_old_authority(){
                let authority=setup();ROUTES.lock().accounts.bind_human(10,None);
                assert!(!remote_input_authority_valid(authority));
                ROUTES.lock().accounts.bind_human(10,Some(AccountId(1)));
                assert!(!remote_input_authority_valid(authority));
                let authority=setup();ROUTES.lock().remote_frontends.clear();
                assert!(!remote_input_authority_valid(authority));
            }
            #[test] fn account_switch_and_vm_reuse_never_borrow_old_grants(){
                let authority=setup();crate::WORLD.lock().account=Some(AccountId(2));
                assert!(!remote_input_authority_valid(authority));
                let authority=setup();crate::WORLD.lock().run=8;
                assert!(!remote_input_authority_valid(authority));
                assert!(remote_input_authority(1,8).is_none());
            }
            #[test] fn stopped_parked_and_reentered_leases_retire_old_input(){
                let authority=setup();crate::WORLD.lock().stopping=true;
                assert!(!remote_input_authority_valid(authority));
                let authority=setup();ROUTES.lock().routes[0].owner=None;
                assert!(!remote_input_authority_valid(authority));
                ROUTES.lock().routes[0].owner=Some(Owner{vm:1,run:7});
                ROUTES.lock().routes[0].surface_generation+=1;
                assert!(!remote_input_authority_valid(authority));
                assert!(remote_input_authority(1,7).is_some());
            }
            #[test] fn launch_feedback_hold_matches_existing_terminal_claim_contract(){
                let authority=setup();ROUTES.lock().routes[0].selected=false;
                assert!(!remote_input_authority_valid(authority));
                ROUTES.lock().navigation_holds.push(10);
                assert!(remote_input_authority_valid(authority));
            }
            #[test] fn freed_matrix_slot_invalidates_controller_authority(){
                let authority=setup();crate::WORLD.lock().slot_live=false;
                assert!(!remote_input_authority_valid(authority));
            }
            #[test] fn origin_cannot_be_installed_into_a_foreign_frontend_or_active_run(){
                setup();let origin=capture_remote_input_origin(frontend()).unwrap();
                ROUTES.lock().routes[0].remote_input_origin=None;
                bind_remote_input_origin(&90,origin);
                assert!(remote_input_authority(1,7).is_none());
                let mut routes=ROUTES.lock();routes.routes[0].owner=None;
                routes.routes[0].frontend=Some(11);drop(routes);
                bind_remote_input_origin(&90,origin);
                assert!(ROUTES.lock().routes[0].remote_input_origin.is_none());
            }
            fn launch(archive:&str,bytes:Vec<u8>,guest:bool)->bool {
                let f=frontend();
                if guest {
                    crate::shell3::service::launch_bytes_with_script_for_account(
                        archive.into(),bytes,f,None,Some(AccountId(1))).unwrap();
                } else {
                    crate::shell3::service::launch_bytes_with_script(
                        archive.into(),bytes,f,None).unwrap();
                }
                crate::REQUESTS.lock().pop().unwrap().remote_input_origin.is_some()
            }
            #[test] fn direct_controller_launch_captures_session_but_other_apps_do_not(){
                setup();assert!(launch("commander.bp",vec![1],false));
                assert!(!launch("codex.bp",vec![1],false));
                assert!(!launch("commander.bp",vec![9],false));
            }
            #[test] fn guest_launch_to_same_human_frontend_does_not_inherit_control(){
                setup();assert!(!launch("commander.bp",vec![1],true));
                let mut routes=ROUTES.lock();routes.accounts.remove(10);
                routes.accounts.bind_guest(10,Some(AccountId(1)));drop(routes);
                assert!(!launch("commander.bp",vec![1],false));
            }
        }
    }
}
'''
    harness = harness.replace("@ACCOUNTS@", str(ROOT / "src/shell3/tui/accounts.rs"))
    harness = harness.replace("@DEFINITIONS@", definitions).replace("@FUNCTIONS@", functions)
    harness = harness.replace("@LAUNCHES@", launches)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-remote-input-session-") as directory:
        directory = Path(directory)
        source_path = directory / "tests.rs"
        source_path.write_text(harness)
        executable = directory / "tests"
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(source_path), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
