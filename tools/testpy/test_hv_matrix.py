#!/usr/bin/env python3
"""Exercise production Matrix reservation, deletion and VM ownership on the host."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
shell = (ROOT / 'src/shell3/shell3.rs').read_text()
state = re.search(r'^struct MatrixSlotsState \{.*?^}', shell, re.M | re.S).group()
state_impl = re.search(r'^impl MatrixSlotsState \{.*?^}', shell, re.M | re.S).group()
methods = '\n'.join(re.search(r'^    (?:pub\((?:crate|super)\) )?fn '+name+r'\(.*?^    }', shell, re.M | re.S).group()
                    for name in ('fresh_slot', 'fresh_app_slot', 'allocate_slot', 'drop_lease', 'drop_slot', 'drop_slot_inner', 'set_background_active', 'background_active_slots'))
source = r'''#![allow(dead_code,non_upper_case_globals)]
extern crate alloc;
mod r {pub mod input_timing {pub fn now_ms()->u64 {0}}}
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
        pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
    }
}
mod shell3 {
    use alloc::{collections::VecDeque,string::{String,ToString},vec::Vec};
    use crate::spin;
    #[path="MATRIX_TARGET"]pub mod matrix_target;
    pub(crate) use matrix_target::{MatrixSlotLease,MatrixTarget,matrix_target_slot_lease};
    @STATE@
    @STATE_IMPL@
    fn matrix_slots()->&'static spin::Mutex<MatrixSlotsState> {
        static STATE:std::sync::OnceLock<spin::Mutex<MatrixSlotsState>>=std::sync::OnceLock::new();
        STATE.get_or_init(||spin::Mutex::new(MatrixSlotsState::new()))
    }
    pub struct MatrixSlots;
    impl MatrixSlots {
        @METHODS@
        pub fn echo_output(_: &MatrixSlotLease,_:String) {}
        pub fn echo(_:Option<&str>,_:Option<u64>,_:String) {}
    }
    pub mod service {pub fn notify_work(){}}
    pub mod tui {
        pub fn reserve(_: &super::MatrixTarget)->Result<(),String> {Ok(())}
        pub fn bind_vm(_: &super::MatrixTarget,_:u8) {}
        pub fn release(_: &super::MatrixTarget,_:u8)->Option<bool> {Some(true)}
    }
    pub fn count()->usize {matrix_slots().lock().ids.len()}
    #[test] fn background_activity_follows_the_exact_slot_lifetime() {
        let (name, lifetime) = MatrixSlots::fresh_slot(None);
        let lease = MatrixSlotLease::from_identity(name.clone(), lifetime);
        assert!(!MatrixSlots::background_active_slots().contains(&name));
        assert!(MatrixSlots::set_background_active(&lease, true));
        assert!(MatrixSlots::set_background_active(&lease, true));
        assert_eq!(MatrixSlots::background_active_slots().iter().filter(|id| *id == &name).count(), 1);
        assert!(MatrixSlots::set_background_active(&lease, false));
        assert!(!MatrixSlots::background_active_slots().contains(&name));
        assert!(MatrixSlots::set_background_active(&lease, true));
        MatrixSlots::drop_lease(&lease);
        assert!(!MatrixSlots::background_active_slots().contains(&name));
        let (new_name, new_lifetime) = MatrixSlots::fresh_slot(None);
        assert_eq!(new_name, name);
        assert_ne!(new_lifetime, lifetime);
        let new_lease = MatrixSlotLease::from_identity(new_name, new_lifetime);
        assert!(MatrixSlots::set_background_active(&new_lease, true));
        assert!(!MatrixSlots::set_background_active(&lease, false));
        assert!(MatrixSlots::background_active_slots().contains(&name));
        MatrixSlots::drop_lease(&new_lease);
    }
}
mod hv {
    use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
    use crate::{spin,shell3::MatrixSlotLease};
    struct ExecutionPause;
    impl ExecutionPause {fn paused(&self)->bool {false}}
    struct Vm {matrix_owner:spin::Mutex<Option<MatrixSlotLease>>,running:AtomicBool,pause_latched:AtomicBool,execution_pause:ExecutionPause,run_generation:AtomicU64}
    impl Vm {const fn new()->Self {Self {matrix_owner:spin::Mutex::new(None),running:AtomicBool::new(false),pause_latched:AtomicBool::new(false),execution_pause:ExecutionPause,run_generation:AtomicU64::new(1)}}}
    static trueos_vm_ids:[Vm;2]=[const {Vm::new()};2];
    fn vm_slot(vm:u8)->Option<&'static Vm> {trueos_vm_ids.get(vm as usize)}
    static KILLED:spin::Mutex<Vec<u8>>=spin::Mutex::new(Vec::new());
    fn kill_for_matrix_slot(vm:u8,lease:&MatrixSlotLease)->Result<bool,()> {
        let vm_state=vm_slot(vm).unwrap();
        if vm_state.matrix_owner.lock().as_ref()!=Some(lease) {return Ok(false);}
        KILLED.lock().push(vm);vm_state.running.store(false,Ordering::Release);Ok(true)
    }
    #[path="MATRIX"]pub mod matrix;
    struct TerminalLease;
    impl TerminalLease {fn accepts_console_input(&self,_:bool)->bool {true}}
    struct Context {console_attached:bool,app_command_passthrough:bool,terminal_lease:TerminalLease,console_target:Option<crate::shell3::MatrixTarget>,console_input:alloc::collections::VecDeque<u8>,console_input_since_ms:Option<u64>}
    static BLUEPRINT_PROCESS_CONTEXTS:[spin::Mutex<Option<Context>>;2]=[const {spin::Mutex::new(None)};2];
    pub fn vm_run_generation(vm:u8)->Option<u64> {vm_slot(vm).map(|vm|vm.run_generation.load(Ordering::Acquire))}
    fn notify_blueprint_console_input(_:u8) {}
    fn blueprint_console_exit_reason(_:u8)->Option<String> {None}
    @STDIN_METHODS@
    @PROMPT_TEST@

    #[test] fn one_app_identity_survives_reentry_and_pause_and_retires_on_eject() {
        let before=crate::shell3::count();
        let (target,receipt)=matrix::reserve("voxy.bp",b"voxy").unwrap();
        assert_eq!(crate::shell3::count(),before+1);
        matrix::bind(0,"voxy.bp",b"voxy",Some(target.clone())).unwrap();
        // Reconstructing a process reuses exactly this reservation.
        let again=matrix::bind(0,"voxy.bp",b"voxy",None).unwrap();
        assert_eq!(crate::shell3::matrix_target_slot_lease(&target),crate::shell3::matrix_target_slot_lease(&again));
        assert_eq!(crate::shell3::count(),before+1);
        assert_eq!(matrix::vm_for_slot(&receipt.slot,receipt.lifetime),Some(0));
        trueos_vm_ids[0].running.store(true,Ordering::Release);
        assert_eq!(matrix::working_slots(),vec![receipt.slot.clone()]);
        trueos_vm_ids[0].pause_latched.store(true,Ordering::Release);
        assert!(matrix::working_slots().is_empty());
        assert!(matrix::app(&receipt.slot,Some(receipt.lifetime)).is_some());
        assert!(matrix::bind(1,"voxy.bp",b"voxy",Some(target.clone())).is_err());
        matrix::retire_vm(0);
        assert!(matrix::app(&receipt.slot,Some(receipt.lifetime)).is_none());
        assert_eq!(crate::shell3::count(),before);
        assert!(KILLED.lock().is_empty());
        trueos_vm_ids[0].running.store(false,Ordering::Release);
        trueos_vm_ids[0].pause_latched.store(false,Ordering::Release);
    }
    #[test] fn expired_reservation_cannot_bind_and_old_slot_cannot_kill_reused_vm() {
        let (old,old_receipt)=matrix::reserve("old.bp",b"old").unwrap();
        matrix::bind(0,"old.bp",b"old",Some(old.clone())).unwrap();
        // A new run takes a new lease; the old app keeps its final output slot.
        let (new,new_receipt)=matrix::reserve("new.bp",b"new").unwrap();
        matrix::bind(0,"new.bp",b"new",Some(new.clone())).unwrap();
        matrix::discard(&old);
        assert!(KILLED.lock().is_empty());
        assert!(matrix::app(&new_receipt.slot,Some(new_receipt.lifetime)).is_some());
        assert!(matrix::bind(1,"old.bp",b"old",Some(old)).is_err());
        assert!(matrix::app(&new_receipt.slot,Some(old_receipt.lifetime)).is_none());
        matrix::discard(&new);
        assert_eq!(*KILLED.lock(),vec![0]);
        *trueos_vm_ids[0].matrix_owner.lock()=None;KILLED.lock().clear();
        // The allocator reuses the name, but never its lifetime.
        let (replacement,receipt)=matrix::reserve("old.bp",b"old").unwrap();
        assert_eq!(receipt.slot,old_receipt.slot);
        assert!(!crate::shell3::MatrixSlots::drop_lease(&crate::shell3::MatrixSlotLease::from_identity(old_receipt.slot,old_receipt.lifetime)));
        assert!(matrix::app(&receipt.slot,Some(receipt.lifetime)).is_some());
        matrix::discard(&replacement);
    }
    #[test] fn app_names_use_four_characters_and_number_collisions() {
        let mut targets=Vec::new();
        for (archive,expected) in [("/apps/appstore.bp","apps"),("appstore.bp","apps1"),("appsmith.bp","apps2"),("os.bp","os"),("éclair.bp","écla")] {
            let (target,receipt)=matrix::reserve(archive,b"app").unwrap();
            assert_eq!(receipt.slot,expected);
            targets.push(target);
        }
        matrix::discard(&targets.remove(0));
        let (reused,receipt)=matrix::reserve("appstore.bp",b"app").unwrap();
        assert_eq!(receipt.slot,"apps");
        matrix::discard(&reused);
        for target in targets {matrix::discard(&target);}
    }
    #[test] fn slot_deletion_and_admission_cannot_leave_an_unowned_running_app() {
        for _ in 0..32 {
            let (target,receipt)=matrix::reserve("racing.bp",b"racing").unwrap();
            let barrier=std::sync::Barrier::new(2);
            let admitted=std::thread::scope(|scope| {
                let thread=scope.spawn(|| {barrier.wait();matrix::bind(0,"racing.bp",b"racing",Some(target.clone()))});
                barrier.wait();matrix::discard(&target);thread.join().unwrap().is_ok()
            });
            assert!(matrix::app(&receipt.slot,Some(receipt.lifetime)).is_none());
            assert_eq!(!KILLED.lock().is_empty(),admitted);
            *trueos_vm_ids[0].matrix_owner.lock()=None;KILLED.lock().clear();
        }
    }
    #[test] fn rollback_releases_failed_admission_but_commit_transfers_the_slot() {
        let before=crate::shell3::count();
        let (target,receipt)=matrix::reserve("bad.bp",b"bad").unwrap();
        {let mut admission=matrix::Admission::new(0);matrix::bind(0,"bad.bp",b"bad",Some(target.clone())).unwrap();admission.bound(&target);}
        assert!(matrix::app(&receipt.slot,Some(receipt.lifetime)).is_none());
        assert_eq!(crate::shell3::count(),before);
        let (target,_)=matrix::reserve("good.bp",b"good").unwrap();
        {let mut admission=matrix::Admission::new(0);matrix::bind(0,"good.bp",b"good",Some(target.clone())).unwrap();admission.bound(&target);admission.commit();}
        assert_eq!(crate::shell3::count(),before+1);matrix::retire_vm(0);
        assert!(matrix::bind(64,"invalid.bp",b"",None).is_err());assert_eq!(crate::shell3::count(),before);
    }
}
'''
stdin_source = (ROOT/'src/hv/mod.rs').read_text()
stdin_methods = '\n'.join(re.search(r'^'+prefix+name+r'\(.*?^}', stdin_source,re.M|re.S).group()
    for prefix,name in [(r'pub\(crate\) fn ','blueprint_console_submit_stdin_for_lease'),('fn ','blueprint_console_submit_stdin_inner')])
prompt_method = re.search(r'^    fn submit_app_prompt\(.*?^    }', shell,re.M|re.S).group()
prompt_test = r'''
    #[test] fn prompt_input_reaches_only_the_vm_bound_to_the_selected_lifetime() {
        use crate::shell3::{MatrixSlotLease,MatrixSlots};
        const OPERATOR:char='§';
        struct Prompt {text:String}
        #[derive(Default)]struct History {lines:Vec<String>}
        impl History {fn remember(&mut self,s:&str){self.lines.push(s.into());}fn end_recall(&mut self){}}
        struct Shell3 {prompt:Prompt,command_history:History,active_matrix_slot:Option<String>,active_matrix_lifetime:Option<u64>}
        impl Shell3 {
            fn set_prompt(&mut self,s:&str){self.prompt.text=s.into();}
            fn active_vmx_app(&self)->Option<()> {Some(())}
            @PROMPT_METHOD@
        }
        let (target,receipt)=matrix::reserve("voxy.bp",b"voxy").unwrap();
        matrix::bind(0,"voxy.bp",b"voxy",Some(target.clone())).unwrap();
        *BLUEPRINT_PROCESS_CONTEXTS[0].lock()=Some(Context {console_attached:true,app_command_passthrough:false,terminal_lease:TerminalLease,console_target:Some(target.clone()),console_input:alloc::collections::VecDeque::new(),console_input_since_ms:None});
        let mut shell=Shell3 {prompt:Prompt{text:"hello world".into()},command_history:History::default(),active_matrix_slot:Some(receipt.slot.clone()),active_matrix_lifetime:Some(receipt.lifetime)};
        assert!(shell.submit_app_prompt());
        assert_eq!(BLUEPRINT_PROCESS_CONTEXTS[0].lock().as_ref().unwrap().console_input.iter().copied().collect::<Vec<_>>(),b"hello world\n");
        assert!(shell.prompt.text.is_empty());assert_eq!(shell.command_history.lines,vec!["hello world"]);
        shell.set_prompt("§other");assert!(!shell.submit_app_prompt());
        shell.set_prompt("stale");shell.active_matrix_lifetime=Some(receipt.lifetime+1);assert!(!shell.submit_app_prompt());
        // A recycled VM ID must not route input from its previous output slot.
        let (new,_)=matrix::reserve("other.bp",b"other").unwrap();matrix::bind(0,"other.bp",b"other",Some(new.clone())).unwrap();
        shell.active_matrix_lifetime=Some(receipt.lifetime);assert!(!shell.submit_app_prompt());
        matrix::discard(&target);matrix::retire_vm(0);KILLED.lock().clear();*BLUEPRINT_PROCESS_CONTEXTS[0].lock()=None;
    }
'''.replace('@PROMPT_METHOD@',prompt_method)
source = source.replace('@STDIN_METHODS@',stdin_methods).replace('@PROMPT_TEST@',prompt_test)
source = source.replace('MATRIX_TARGET',str(ROOT/'src/shell3/matrix_target.rs')).replace('MATRIX',str(ROOT/'src/hv/matrix.rs'))
source = source.replace('@STATE_IMPL@',state_impl).replace('@STATE@',state).replace('@METHODS@',methods)
with tempfile.TemporaryDirectory(prefix='hv-matrix-') as directory:
    path = Path(directory)
    matrix = (ROOT/'src/hv/matrix.rs').read_text()
    matrix = matrix[:matrix.index('use alloc::')] + 'use crate::spin;\n' + matrix[matrix.index('use alloc::'):]
    (path/'matrix.rs').write_text(matrix)
    (path/'test.rs').write_text(source.replace(str(ROOT/'src/hv/matrix.rs'),str(path/'matrix.rs')))
    (path/'Cargo.toml').write_text('[package]\nname="hv-matrix-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nsha2="0.10.9"\n[lib]\npath="test.rs"\n')
    subprocess.run(['cargo','test','--offline','--target','x86_64-unknown-linux-gnu','--manifest-path',str(path/'Cargo.toml'),'--target-dir','/tmp/trueos-hv-matrix-test-target','--','--test-threads=1'],cwd='/tmp',check=True)
