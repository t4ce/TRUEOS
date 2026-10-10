#!/usr/bin/env python3
"""Real cry/envelope/key-provider code: saved accounts never replace boot's account."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(marker):
    return re.search(r'^'+re.escape(marker)+r'.*?^}', (ROOT/'src/shell2/cmds/cry.rs').read_text(), re.M|re.S).group()


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as embassy_time_driver;
use std::{collections::BTreeMap, sync::atomic::{AtomicBool,AtomicU64,Ordering}};
use spin::Mutex;
pub const TICK_HZ:u64=1000;
pub fn now()->u64{1000}
static ROOT_MOUNTED:AtomicBool=AtomicBool::new(true);
static UNIX:AtomicU64=AtomicU64::new(1800000000);
static FILES:Mutex<BTreeMap<String,Vec<u8>>>=Mutex::new(BTreeMap::new());
static FAIL_WRITE:Mutex<Option<String>>=Mutex::new(None);
mod tyche {pub fn fill_bytes(out:&mut[u8])->bool {getrandom_02::getrandom(out).is_ok()}}
mod time {pub fn unix_time_seconds()->Option<u64>{Some(crate::UNIX.load(crate::Ordering::SeqCst))}}
mod disc {pub mod block {#[derive(Clone,Copy,PartialEq,Eq)]pub struct DeviceHandle;}}
mod r {
    pub mod net {pub mod ntp {pub fn current_unix_seconds()->Option<u64>{crate::time::unix_time_seconds()}}}
    pub mod fs {pub mod trueosfs {
        use crate::disc::block::DeviceHandle;
        pub fn primary_root_handle()->Option<DeviceHandle>{crate::ROOT_MOUNTED.load(crate::Ordering::SeqCst).then_some(DeviceHandle)}
        pub async fn dir_create_all_async(_:DeviceHandle,_:&str)->Result<bool,&'static str>{Ok(true)}
        pub async fn file_write_all_async(_:DeviceHandle,path:&str,data:&[u8])->Result<bool,&'static str>{
            let mut fail=crate::FAIL_WRITE.lock();if fail.as_deref()==Some(path){*fail=None;return Err("injected write failure");}
            crate::FILES.lock().insert(path.into(),data.to_vec());Ok(true)
        }
        pub async fn file_out_async(_:DeviceHandle,path:&str)->Result<Option<Vec<u8>>,&'static str>{Ok(crate::FILES.lock().get(path).cloned())}
    }}
}
mod crypt {
@CRYPT@
    pub fn reset(){*CRY_STATE.lock()=CryState::new();}
    pub fn active_secret()->[u8;20]{CRY_STATE.lock().totp.as_ref().unwrap().secret}
    pub fn draft_secret(draft:&CryAccountDraft)->[u8;20]{draft.state.lock().totp.as_ref().unwrap().secret}
}
mod machine_key {
@MACHINE_KEY@
    pub fn test_decode(blob:&[u8])->(String,zeroize::Zeroizing<[u8;32]>){decode(blob).unwrap()}
}
mod cry {
    use alloc::{string::String,sync::Arc,vec::Vec};
    use core::fmt::Write;
    use spin::Mutex;
    use crate::crypt::{self,CryError};
    use zeroize::Zeroizing;
    use qrcodegen::{QrCode,QrCodeEcc,Version};
    const QR_QUIET_ZONE:i32=4;const QR_MAX_VERSION:Version=Version::new(10);const QR_BUFFER_BYTES:usize=QR_MAX_VERSION.buffer_len();
@CRY_ADAPTERS@
    pub fn secret(account:&AccountEnrollment)->[u8;20]{crypt::draft_secret(&account.draft)}
}
fn ready<T>(f:impl std::future::Future<Output=T>)->T{let mut f=std::pin::pin!(f);let mut cx=std::task::Context::from_waker(std::task::Waker::noop());match f.as_mut().poll(&mut cx){std::task::Poll::Ready(v)=>v,_=>panic!("host IO must be ready")}}
fn code(secret:&[u8;20])->String{format!("{:06}",trueos_crypto::generate_totp_sha1(secret,UNIX.load(Ordering::SeqCst)/30).unwrap())}
fn setup_first(){ROOT_MOUNTED.store(true,Ordering::SeqCst);crypt::reset();FILES.lock().clear();*FAIL_WRITE.lock()=None;UNIX.store(1800000000,Ordering::SeqCst);crypt::setup_root_key("alice").unwrap();crypt::begin_totp_enrollment().unwrap();let report=crypt::prepare_login(&code(&crypt::active_secret()),2).unwrap();let plan=crypt::prepare_persistence(report.challenge_sequence).unwrap();ready(cry::write_persistence(&plan)).unwrap();crypt::complete_persisted_login(plan).unwrap();UNIX.fetch_add(60,Ordering::SeqCst);}
fn check_active(original:&crypt::CryStatus,boot:&[u8]){let current=crypt::status();assert_eq!(current.username,original.username);assert_eq!(current.session,original.session);assert_eq!(current.fingerprint,original.fingerprint);assert_eq!(current.persistence,original.persistence);assert_eq!(FILES.lock().get("trueos/uncrypted.blob").unwrap(),boot);}
#[test]fn additional_accounts_keep_first_active_and_boot_blob_and_reopen_independently(){
    setup_first();let active=crypt::status();let boot=FILES.lock()["trueos/uncrypted.blob"].clone();let host_seed=*crypt::ssh_host_seed().unwrap();
    assert!(ready(cry::create_account_enrollment("alice")).is_err());
    let bob=ready(cry::create_account_enrollment("bob")).unwrap();assert!(!bob.qr_lines().unwrap().is_empty());check_active(&active,&boot);
    let recovery=ready(bob.save(&code(&cry::secret(&bob)))).unwrap();assert_eq!(recovery.len(),64);check_active(&active,&boot);assert_eq!(*crypt::ssh_host_seed().unwrap(),host_seed);
    assert!(ready(cry::create_account_enrollment("bob")).is_err());
    let carol=ready(cry::create_account_enrollment("carol")).unwrap();ready(carol.save(&code(&cry::secret(&carol)))).unwrap();check_active(&active,&boot);
    let mut public_keys=Vec::new();
    for user in ["bob","carol"] {
        let files=FILES.lock();let (name,key)=machine_key::test_decode(&files[&format!("users/{user}/secrets/uncrypted.blob")]);assert_eq!(name,user);
        let opened=trueos_credential_store::open(user,&key,&files[&format!("users/{user}/secrets/cry.v1.aes256gcm")]).unwrap();
        assert!(opened.credential.totp_active);assert!(opened.credential.last_accepted_step.is_some());assert!(files.contains_key(&format!("users/{user}/account.v1")));public_keys.push(opened.credential.public_key);
        assert!(trueos_credential_store::open("alice",&key,&files[&format!("users/{user}/secrets/cry.v1.aes256gcm")]).is_err());
    }
    assert_ne!(public_keys[0],public_keys[1]);
    let restored=ready(machine_key::unseal(crate::disc::block::DeviceHandle)).unwrap().unwrap();assert_eq!(restored.0,"alice");
    crypt::reset();crypt::unlock_persisted("alice",&restored.1,&FILES.lock()["users/alice/secrets/cry.v1.aes256gcm"]).unwrap();assert_eq!(crypt::status().username.as_deref(),Some("alice"));
}
#[test]fn failed_save_retries_the_same_verified_plan_and_recovery_key(){
    setup_first();let active=crypt::status();let boot=FILES.lock()["trueos/uncrypted.blob"].clone();let account=ready(cry::create_account_enrollment("david")).unwrap();
    assert!(ready(account.save("bad")).is_err());assert!(!account.code_verified());check_active(&active,&boot);
    *FAIL_WRITE.lock()=Some("users/david/secrets/cry.v1.aes256gcm".into());
    assert!(ready(account.save(&code(&cry::secret(&account)))).unwrap_err().contains("write"));assert!(account.code_verified());
    let saved_key=FILES.lock()["users/david/secrets/uncrypted.blob"].clone();check_active(&active,&boot);
    ready(account.save("")).unwrap();assert_eq!(FILES.lock()["users/david/secrets/uncrypted.blob"],saved_key);assert!(!account.code_verified());check_active(&active,&boot);
    let files=FILES.lock();let (_,key)=machine_key::test_decode(&saved_key);assert!(trueos_credential_store::open("david",&key,&files["users/david/secrets/cry.v1.aes256gcm"]).is_ok());
}
#[test]fn changing_the_mounted_root_writes_nothing_and_can_resume_on_the_original_root(){
    setup_first();let account=ready(cry::create_account_enrollment("grace")).unwrap();let files=FILES.lock().clone();ROOT_MOUNTED.store(false,Ordering::SeqCst);
    assert!(ready(account.save(&code(&cry::secret(&account)))).unwrap_err().contains("root changed"));assert_eq!(*FILES.lock(),files);assert!(account.code_verified());
    ROOT_MOUNTED.store(true,Ordering::SeqCst);ready(account.save("")).unwrap();assert!(FILES.lock().contains_key("users/grace/account.v1"));assert_eq!(crypt::status().username.as_deref(),Some("alice"));
}
#[test]fn stored_name_collisions_and_cancelled_drafts_do_not_change_the_first_account(){
    setup_first();let active=crypt::status();let boot=FILES.lock()["trueos/uncrypted.blob"].clone();
    FILES.lock().insert("users/eve/account.v1".into(),b"existing profile".to_vec());assert!(ready(cry::create_account_enrollment("eve")).is_err());
    let draft=ready(cry::create_account_enrollment("frank")).unwrap();drop(draft);assert!(!FILES.lock().keys().any(|path|path.starts_with("users/frank/")));check_active(&active,&boot);
}
'''


def main():
    adapters='\n'.join(item(marker) for marker in [
        'pub(crate) struct AccountEnrollment', 'impl AccountEnrollment',
        'pub(crate) async fn ensure_account_name_available(',
        'pub(crate) async fn create_account_enrollment(',
        'pub(crate) async fn write_persistence(', 'async fn write_and_verify(',
        'pub(crate) fn error_text(', 'pub(crate) fn full_hex(',
        'pub(crate) fn enrollment_qr_lines(',
    ])
    provider=(ROOT/'src/machine_key.rs').read_text().split('pub(crate) async fn restore_account(')[0].replace('//!','//')
    source=HARNESS.replace('@CRYPT@',(ROOT/'src/crypt.rs').read_text().replace('//!','//')).replace('@MACHINE_KEY@',provider).replace('@CRY_ADAPTERS@',adapters)
    with tempfile.TemporaryDirectory(prefix='cry-accounts-') as directory:
        work=Path(directory);(work/'src').mkdir();(work/'src/lib.rs').write_text(source)
        (work/'Cargo.toml').write_text(f'''[package]
name="cry-accounts-host"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
trueos-crypto={{path="{ROOT}/crates/trueos-crypto"}}
trueos-credential-store={{path="{ROOT}/crates/trueos-credential-store"}}
ed25519-dalek={{version="2.2",default-features=false,features=["zeroize"]}}
sha2={{version="0.10",default-features=false}}
spin="0.9"
zeroize={{version="1",features=["alloc"]}}
base64={{version="0.22",default-features=false,features=["alloc"]}}
getrandom_02={{package="getrandom",version="0.2"}}
qrcodegen={{package="qrcodegen-no-heap",version="1.8.1",default-features=false}}
''')
        env=dict(os.environ,CARGO_TARGET_DIR=str(ROOT/'tgt/cry-accounts-host-tests'))
        subprocess.run(['cargo','test','--offline','--quiet','--manifest-path',str(work/'Cargo.toml'),'--','--test-threads=1'],cwd=work,env=env,check=True)


if __name__=='__main__':
    main()
