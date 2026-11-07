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
mod r { pub mod net { pub mod ntp {
    pub fn current_unix_seconds() -> Option<u64> { Some(2_000_000_000) }
} } }
mod time { pub fn unix_time_seconds() -> Option<u64> { Some(2_000_000_000) } }
struct TestClock;
impl embassy_time_driver::Driver for TestClock {
    fn now(&self) -> u64 { 1 }
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
    fn reset() { *CRY_STATE.lock() = CryState::new(); }
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
        assert!(ssh_requires_public_key("t4ce")); assert!(!ssh_requires_public_key("someone-else"));
        CRY_STATE.lock().durable=None;
        assert_eq!(ssh_key_account("t4ce",&key),None); assert!(!ssh_requires_public_key("t4ce")); reset();
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
''')
        (work/'lib.rs').write_text(HARNESS.replace('__ACCOUNTS__',str(ROOT/'src/shell3/tui/accounts.rs')))
        (work/'crypt.rs').write_text((ROOT/'src/crypt.rs').read_text()+TESTS)
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
    paths=['src/crypt.rs','src/shell3/tui/accounts.rs','vendor/sunset/src/event.rs','vendor/sunset/src/native_protocol_tests.rs','src/shell3/ssh.rs','src/shell3/tui.rs','src/shell3/admin.rs','src/shell3/tty.rs','src/shell3/net.rs','src/shell3/system/cry.rs']
    sources=[{'path':str(ROOT/p),'sha256':hashlib.sha256((ROOT/p).read_bytes()).hexdigest()} for p in paths]
    (output/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':sources},indent=2)+'\n')
    print(output/'receipt.json')
if __name__=='__main__': main()
