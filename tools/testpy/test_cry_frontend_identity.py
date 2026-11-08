#!/usr/bin/env python3
"""Test actual Cry encrypted credentials, exact-frontend bindings and SSH proofs.

The clock and entropy source are deterministic host fixtures. Production Cry,
credential-envelope crypto, frontend binding logic and Sunset protocol sources
are used unchanged. No account files, policy database or rig are modified.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
mod tyche {
    use sha2::{Digest,Sha256};
    use std::sync::atomic::{AtomicU64,Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    pub fn fill_bytes(output: &mut [u8]) -> bool {
        for chunk in output.chunks_mut(32) {
            let digest = Sha256::digest(COUNTER.fetch_add(1,Ordering::SeqCst).to_le_bytes());
            chunk.copy_from_slice(&digest[..chunk.len()]);
        }
        true
    }
}
static UNIX_SECONDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(2_000_000_000);
static TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn unix_seconds()->u64 {UNIX_SECONDS.load(std::sync::atomic::Ordering::SeqCst)}
fn reset_clock(){UNIX_SECONDS.store(2_000_000_000,std::sync::atomic::Ordering::SeqCst);TICKS.store(1,std::sync::atomic::Ordering::SeqCst);}
fn advance(seconds:u64){UNIX_SECONDS.fetch_add(seconds,std::sync::atomic::Ordering::SeqCst);TICKS.fetch_add(seconds*embassy_time_driver::TICK_HZ,std::sync::atomic::Ordering::SeqCst);}
fn poll<F:core::future::Future>(future:core::pin::Pin<&mut F>)->core::task::Poll<F::Output>{future.poll(&mut core::task::Context::from_waker(core::task::Waker::noop()))}
fn run<F:core::future::Future>(future:F)->F::Output{let mut future=std::pin::pin!(future);loop{if let core::task::Poll::Ready(value)=poll(future.as_mut()){return value;}}}
#[macro_export]macro_rules! log_warn{(target:$target:literal;$($args:tt)*)=>{{let _=format_args!($($args)*);}}}
mod r { pub mod net { pub mod ntp {
    pub fn current_unix_seconds() -> Option<u64> { Some(crate::unix_seconds()) }
} } }
mod time { pub fn unix_time_seconds() -> Option<u64> { Some(crate::unix_seconds()) } }
struct TestClock;
impl embassy_time_driver::Driver for TestClock {
    fn now(&self) -> u64 { TICKS.load(std::sync::atomic::Ordering::SeqCst) }
    fn schedule_wake(&self, _:u64, _: &core::task::Waker) {}
}
embassy_time_driver::time_driver_impl!(static CLOCK: TestClock = TestClock);
#[path="crypt.rs"] mod crypt;
#[path="__ACCOUNTS__"] mod accounts;
'''
TESTS = r'''
#[cfg(test)]
mod identity_regressions {
    use super::*;
    fn reset() { crate::reset_clock(); *CRY_STATE.lock() = CryState::new(); }
    fn enrolled(username: &str) -> CryLoginReport {
        setup_root_key(username).unwrap();
        begin_totp_enrollment().unwrap();
        let secret = CRY_STATE.lock().totp.as_ref().unwrap().secret;
        let code = trueos_crypto::generate_totp_sha1(&secret,2_000_000_000/TOTP_PERIOD_SECONDS).unwrap();
        prepare_login(&alloc::format!("{code:06}"),1).unwrap()
    }
    fn saved(username:&str, account:AccountId, role:MachineRole) -> ([u8;32], Vec<u8>) {
        reset(); enrolled(username);
        // Exercise stored descriptor semantics rather than deriving an ID at unlock.
        { let mut state=CRY_STATE.lock(); let c=state.credential.as_mut().unwrap(); c.account=account; c.maximum_role=role; }
        let sequence=CRY_STATE.lock().pending_login.as_ref().unwrap().report.challenge_sequence;
        let plan=prepare_persistence(sequence).unwrap();
        let key=*plan.recovery_key_bytes(); let envelope=plan.envelope.to_vec();
        complete_persisted_login(plan).unwrap(); (key,envelope)
    }
    #[test] fn new_credentials_have_distinct_persisted_non_root_ids() {
        reset(); let alice=enrolled("alice"); let plan=prepare_persistence(alice.challenge_sequence).unwrap();
        let opened=open_credential("alice",plan.recovery_key_bytes(),&plan.envelope).unwrap();
        assert_ne!(alice.account,AccountId::ROOT); assert_eq!(opened.credential.account_id,alice.account.raw());
        assert!(core::str::from_utf8(&plan.profile).unwrap().contains(&alloc::format!("account_id={}",alice.account.raw())));
        reset(); let bob=enrolled("bob"); assert_ne!(alice.account,bob.account); reset();
    }
    #[test] fn unlock_preserves_legacy_root_and_other_stored_account_roles() {
        for (id,role) in [(0,MachineRole::Root),(749,MachineRole::User),(993,MachineRole::Administrator)] {
            let (key,envelope)=saved("t4ce",AccountId::new(id),role); reset();
            let report=unlock_persisted("t4ce",&key,&envelope).unwrap();
            assert_eq!(report.account,AccountId::new(id)); assert_eq!(report.role,role);
            assert_eq!(CRY_STATE.lock().credential.unwrap().account,report.account);
        }
        reset();
    }
    #[test] fn envelope_username_and_tamper_cannot_select_another_account() {
        let (key,mut envelope)=saved("alice",AccountId::new(22),MachineRole::User); reset();
        assert!(unlock_persisted("bob",&key,&envelope).is_err()); assert!(CRY_STATE.lock().credential.is_none());
        let last=envelope.len()-1; envelope[last]^=1;
        assert!(unlock_persisted("alice",&key,&envelope).is_err()); assert!(CRY_STATE.lock().credential.is_none());
    }
    #[test] fn ssh_identity_requires_exact_enrolled_username_and_key() {
        saved("t4ce",AccountId::new(712),MachineRole::User);
        let key=[7;32]; CRY_STATE.lock().ssh_public_keys.push(key);
        assert_eq!(ssh_key_account("t4ce",&key),Some(AccountId::new(712)));
        assert_eq!(ssh_key_account("someone-else",&key),None); assert_eq!(ssh_key_account("t4ce",&[8;32]),None);
        assert!(ssh_requires_account_proof("t4ce")); assert!(!ssh_requires_account_proof("someone-else"));
        CRY_STATE.lock().durable=None;
        assert_eq!(ssh_key_account("t4ce",&key),None); assert!(!ssh_requires_account_proof("t4ce")); reset();
    }
    #[test] fn development_ssh_bypass_is_explicit_and_matches_only_loaded_durable_account() {
        reset();
        assert_eq!(crate::shell3::ssh_boot::account_without_proof("t4ce"),None);
        saved("t4ce",AccountId::new(712),MachineRole::User);
        assert_eq!(crate::shell3::ssh_boot::account_without_proof("t4ce"),Some(AccountId::new(712)));
        assert_eq!(crate::shell3::ssh_boot::account_without_proof("other"),None);
        assert_eq!(crate::shell3::ssh_boot::account_without_proof("T4CE"),None);
        assert_eq!(crate::shell3::ssh_boot_disabled::account_without_proof("t4ce"),None);
        assert!(ssh_requires_account_proof("t4ce"));
        CRY_STATE.lock().durable=None;
        assert_eq!(crate::shell3::ssh_boot::account_without_proof("t4ce"),None);
        reset();
    }
    #[test] fn cry_session_opens_only_after_the_same_proof_commits() {
        reset(); let report=enrolled("t4ce");
        assert_eq!(authenticated_access_account(1),None);
        let plan=prepare_persistence(report.challenge_sequence).unwrap();
        let committed=complete_persisted_login(plan).unwrap();
        assert_eq!(committed.account,report.account); assert_eq!(authenticated_access_account(1),Some(report.account));
        assert_eq!(authenticated_access_account(2),None); reset();
    }
}
'''


REMOTE_HARNESS = r"""
mod net {pub mod adapter {use alloc::{boxed::Box,collections::VecDeque,vec::Vec};use core::sync::atomic::{AtomicU32,Ordering};
@NET_QUEUE@
@NET_QUEUE_IMPL@
}}
mod shell3 {
 pub use system::cry;
 pub mod system { pub mod cry {
  use crate::crypt::{self,CryError};use alloc::{string::String,vec::Vec};use zeroize::Zeroizing;
  static MODE:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(0);
  static CALLS:core::sync::atomic::AtomicUsize=core::sync::atomic::AtomicUsize::new(0);
  struct Image{key:Zeroizing<[u8;32]>,envelope:Zeroizing<Vec<u8>>}
  static IMAGE:spin::Mutex<Option<Image>>=spin::Mutex::new(None);
  pub(crate) fn reset_store(){MODE.store(0,core::sync::atomic::Ordering::SeqCst);CALLS.store(0,core::sync::atomic::Ordering::SeqCst);*IMAGE.lock()=None;}
  pub(crate) fn fail_store(){MODE.store(1,core::sync::atomic::Ordering::SeqCst);}
  pub(crate) fn pause_store(){MODE.store(2,core::sync::atomic::Ordering::SeqCst);}
  pub(crate) fn ready_store(){MODE.store(0,core::sync::atomic::Ordering::SeqCst);}
  pub(crate) fn calls()->usize{CALLS.load(core::sync::atomic::Ordering::SeqCst)}
  pub(crate) fn has_saved_image()->bool{IMAGE.lock().is_some()}
  pub(crate) fn saved_image()->([u8;32],Vec<u8>){let image=IMAGE.lock();let image=image.as_ref().unwrap();(*image.key,image.envelope.to_vec())}
  async fn write_persistence(plan:&crypt::CryPersistencePlan)->Result<(),String>{
   CALLS.fetch_add(1,core::sync::atomic::Ordering::SeqCst);
   core::future::poll_fn(|_|if MODE.load(core::sync::atomic::Ordering::SeqCst)==2{core::task::Poll::Pending}else{core::task::Poll::Ready(())}).await;
   if MODE.load(core::sync::atomic::Ordering::SeqCst)==1{return Err("injected storage failure".into())}
   *IMAGE.lock()=Some(Image{key:Zeroizing::new(*plan.recovery_key_bytes()),envelope:Zeroizing::new(plan.envelope.to_vec())});Ok(())
  }
@PENDING@
@PENDING_DROP@
@ERROR_TEXT@
@REMOTE_LOGIN@
 }}
 pub(crate) mod ssh {
  use alloc::{string::String,sync::Arc};use core::sync::atomic::{AtomicBool,Ordering};use zeroize::Zeroizing;
@OUTCOME@
@COMPLETION@
@REQUEST@
static AUTH_REQUESTS:spin::Once<&'static crate::net::adapter::NetQueue<AuthRequest>>=spin::Once::new();
@INIT@
@SERVICE_AUTH@
  pub(crate) async fn service_for_test(){service_auth().await;}
  pub(crate) struct Ticket(Arc<AuthCompletion>);
  impl Ticket {
   pub(crate) fn cancel(&self){self.0.cancelled.store(true,Ordering::Release);}
   pub(crate) fn outcome(&self)->Option<Result<trueos_crypto::AccountId,()>>{self.0.result.lock().map(|result|match result{AuthOutcome::Denied=>Err(()),AuthOutcome::Verified(account)=>Ok(account)})}
  }
  pub(crate) fn clear_queue(){init();AUTH_REQUESTS.get().unwrap().drain(64);}
  pub(crate) fn enqueue(username:&str,code:&str,seconds:u64)->Ticket{
   init();let completion=Arc::new(AuthCompletion{result:spin::Mutex::new(None),cancelled:AtomicBool::new(false)});
   assert!(AUTH_REQUESTS.get().unwrap().push(AuthRequest{username:username.into(),code:Zeroizing::new(code.into()),deadline:trueos_time::Instant::now()+trueos_time::Duration::from_secs(seconds),completion:completion.clone()}).is_ok());Ticket(completion)
  }
 }
}
"""

def production_remote_harness():
    from test_sdk_metadata_v2 import item
    command=(ROOT/'src/shell3/system/cry.rs').read_text();ssh=(ROOT/'src/shell3/ssh.rs').read_text();queue=(ROOT/'src/net/adapter.rs').read_text()
    fields={'NET_QUEUE':item(queue,'pub struct NetQueue<T>'),'NET_QUEUE_IMPL':item(queue,'impl<T> NetQueue<T>'),
      'PENDING':re.search(r'struct PendingProof\(u64\);',command)[0],
      'PENDING_DROP':item(command,'impl Drop for PendingProof'),'ERROR_TEXT':item(command,'pub(crate) fn error_text('),'REMOTE_LOGIN':item(command,'pub(crate) async fn remote_login('),
      'OUTCOME':'#[derive(Clone,Copy)]\n'+item(ssh,'enum AuthOutcome'),'COMPLETION':item(ssh,'pub(super) struct AuthCompletion'),'REQUEST':item(ssh,'pub(super) struct AuthRequest'),
      'INIT':item(ssh,'pub(super) fn init()'),'SERVICE_AUTH':item(ssh,'pub(super) async fn service_auth()')}
    text=REMOTE_HARNESS
    for key,value in fields.items():text=text.replace('@'+key+'@',value)
    boot=(ROOT/'src/shell3/ssh_boot.rs').read_text()
    # Exercise both settings regardless of the currently selected development policy.
    policy=re.sub(r'= (?:true|false);', '= true;', re.search(
        r'pub\(super\) const DEV_SKIP_SSH_ACCOUNT_PROOF: bool = (?:true|false);',boot)[0])
    policy+='\n'+item(boot,'pub(super) fn account_without_proof(')
    policy=policy.replace('pub(super)', 'pub(crate)')
    disabled=policy.replace('DEV_SKIP_SSH_ACCOUNT_PROOF: bool = true;', 'DEV_SKIP_SSH_ACCOUNT_PROOF: bool = false;')
    text=text.replace('pub(crate) mod ssh {',
        'pub(crate) mod ssh_boot {\n'+policy+'\n}\n'
        'pub(crate) mod ssh_boot_disabled {\n'+disabled+'\n}\n'
        'pub(crate) mod ssh {')
    return text


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args(); output=args.output.resolve(); output.mkdir(parents=True,exist_ok=True)
    env=dict(os.environ)
    for key in list(env):
        if key.startswith(('CARGO_ENCODED_','CARGO_TARGET_','CARGO_PROFILE_','CARGO_CFG_','CARGO_FEATURE_')) or key in {'RUSTFLAGS','RUSTC','RUSTC_WRAPPER','CARGO_BUILD_TARGET'}: env.pop(key,None)
    env['CARGO_TARGET_DIR']=str(output/'target')
    cargo=['rustup','run',TOOLCHAIN,'cargo']; checks=[]
    with tempfile.TemporaryDirectory(prefix='trueos-cry-frontend-') as temporary:
        work=Path(temporary)
        (work/'Cargo.toml').write_text(f'''[package]
name="cry-frontend-identity-check"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
trueos-crypto={{path={json.dumps(str(ROOT/'crates/trueos-crypto'))}}}
trueos-credential-store={{path={json.dumps(str(ROOT/'crates/trueos-credential-store'))}}}
base64={{version="0.22",default-features=false,features=["alloc"]}}
ed25519-dalek={{version="2.2",default-features=false,features=["zeroize"]}}
sha2={{version="=0.10.9",default-features=false}}
spin="0.10"
zeroize={{version="1",default-features=false,features=["alloc"]}}
embassy-time-driver={{version="0.2.2",features=["tick-hz-1_000_000"]}}
trueos-time={{path={json.dumps(str(ROOT/'crates/trueos-executor/embassy-time'))},default-features=false}}
''')
        (work/'lib.rs').write_text(HARNESS.replace('__ACCOUNTS__',str(ROOT/'src/shell3/tui/accounts.rs'))+production_remote_harness())
        (work/'crypt.rs').write_text((ROOT/'src/crypt.rs').read_text()+TESTS+(ROOT/'tools/testpy/fixtures/remote_cry_tests.rs').read_text())
        def run(label,command):
            log=output/(label+'.log')
            with log.open('w') as stream: result=subprocess.run(command,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
            checks.append({'label':label,'exit':result.returncode,'log':str(log),'command':command})
            if result.returncode: raise RuntimeError(log.read_text()[-14000:])
        for mode,flags in [('debug',[]),('release',['--release'])]:
            run('cry-frontend-'+mode,cargo+['test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--','--test-threads=1'])
        sunset=work/'sunset';shutil.copytree(ROOT/'vendor/sunset',sunset)
        with (sunset/'Cargo.toml').open('a') as stream: stream.write('\n[workspace]\n')
        run('sunset-encrypted-auth',cargo+['test','--offline','--manifest-path',str(sunset/'Cargo.toml'),'--target','x86_64-unknown-linux-gnu','--features','std','--lib','native_protocol_tests','--','--test-threads=1'])
    paths=['src/crypt.rs','src/shell3/tui/accounts.rs','vendor/sunset/src/event.rs','vendor/sunset/src/native_protocol_tests.rs','src/shell3/ssh.rs','src/shell3/tui.rs','src/shell3/admin.rs','src/shell3/tty.rs','src/shell3/net.rs','src/shell3/system/cry.rs','src/net/adapter.rs','tools/testpy/fixtures/remote_cry_tests.rs','tools/testpy/test_cry_frontend_identity.py']
    paths.append('src/shell3/ssh_boot.rs')
    sources=[{'path':str(ROOT/p),'sha256':hashlib.sha256((ROOT/p).read_bytes()).hexdigest()} for p in paths]
    (output/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':sources},indent=2)+'\n')
    print(output/'receipt.json')
if __name__=='__main__': main()
