#!/usr/bin/env python3
"""Real Cry gate, history policy, AEAD recorder, and append/readback on the host."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    crypt = (ROOT/'src/crypt.rs').read_text().replace('//!', '//')
    history = (ROOT/'src/shell3/command_history.rs').read_text().replace('//!', '//')
    record = (ROOT/'src/user_input_record.rs').read_text().replace('//!', '//')
    record = record[:record.index('#[trueos_executor::task]')].replace('use trueos_time::{Duration as EmbassyDuration, Timer};', '')
    source = r'''
#![allow(dead_code,unused_variables)]
extern crate alloc;
extern crate self as embassy_time_driver;
use std::{collections::BTreeMap,sync::atomic::{AtomicBool,AtomicU64,Ordering}};
static UNIX:AtomicU64=AtomicU64::new(1800000000);
static FAIL_APPEND:AtomicBool=AtomicBool::new(false);
static FILES:spin::Mutex<BTreeMap<String,Vec<u8>>>=spin::Mutex::new(BTreeMap::new());
pub const TICK_HZ:u64=1000;
pub fn now()->u64 {1000}
#[macro_export] macro_rules! log {($($args:tt)*)=>{};}
mod tyche {pub fn fill_bytes(bytes:&mut[u8])->bool {getrandom_02::getrandom(bytes).is_ok()}}
mod time {pub fn unix_time_seconds()->Option<u64> {Some(crate::UNIX.load(crate::Ordering::Relaxed))}}
mod matrix_target {pub const TRANSPORT_LOCAL_SCOPE:u8=2;}
mod r {
pub mod net {pub mod ntp {pub fn current_unix_seconds()->Option<u64> {crate::time::unix_time_seconds()}}}
pub mod fs {pub mod trueosfs {
pub struct Info {pub data_len:u64}
pub fn primary_root_handle()->Option<()> {Some(())}
pub async fn file_append_async(_:(),path:&str,bytes:&[u8])->Result<bool,&'static str> {
if crate::FAIL_APPEND.swap(false,crate::Ordering::Relaxed) {return Err("injected failure");}
crate::FILES.lock().entry(path.into()).or_default().extend_from_slice(bytes);Ok(true)
}
pub async fn file_info_async(_:(),path:&str)->Result<Option<Info>,&'static str> {Ok(crate::FILES.lock().get(path).map(|bytes|Info {data_len:bytes.len() as u64}))}
pub async fn file_read_range_async(_:(),path:&str,offset:u64,out:&mut[u8])->Result<Option<usize>,&'static str> {
let files=crate::FILES.lock();let Some(bytes)=files.get(path) else {return Ok(None);};let slice=&bytes[offset as usize..offset as usize+out.len()];out.copy_from_slice(slice);Ok(Some(out.len()))
}
}}
}
mod crypt {
@CRYPT@
pub fn test_login(scope:u8) {
if !status().configured {setup_root_key("alice").unwrap();begin_totp_enrollment().unwrap();}
crate::UNIX.fetch_add(60,crate::Ordering::Relaxed);
let secret=CRY_STATE.lock().totp.as_ref().unwrap().secret;
let code=alloc::format!("{:06}",trueos_crypto::generate_totp_sha1(&secret,crate::UNIX.load(crate::Ordering::Relaxed)/30).unwrap());
let report=prepare_login(&code,scope).unwrap();let plan=prepare_persistence(report.challenge_sequence).unwrap();complete_persisted_login(plan).unwrap();
}
}
mod command_history {@HISTORY@}
mod user_input_record {
@RECORD@
pub async fn test_flush() {flush_once().await;}
pub fn test_records(scope:u8)->Vec<String> {
let context=crate::crypt::authenticated_user_input_record_key(scope).unwrap();
let cipher=ChaCha20Poly1305::new_from_slice(context.key_bytes()).unwrap();
crate::FILES.lock().get(PATH).unwrap().chunks_exact(RECORD_BYTES).map(|record| {
assert_eq!(&record[..4],b"TUIR");
let mut plaintext=Zeroizing::new(record[HEADER_BYTES..RECORD_BYTES-TAG_BYTES].to_vec());
cipher.decrypt_in_place_detached(Nonce::from_slice(&record[20..32]),&record[..HEADER_BYTES],plaintext.as_mut_slice(),chacha20poly1305::Tag::from_slice(&record[RECORD_BYTES-TAG_BYTES..])).unwrap();
assert_eq!(u64::from_le_bytes(plaintext[8..16].try_into().unwrap()),context.account.raw());
let length=u16::from_le_bytes(plaintext[32..34].try_into().unwrap()) as usize;
String::from_utf8(plaintext[40..40+length].to_vec()).unwrap()
}).collect()
}
}
fn ready<F:core::future::Future>(future:F)->F::Output {let waker=std::task::Waker::noop();let mut context=std::task::Context::from_waker(waker);let mut future=std::pin::pin!(future);match future.as_mut().poll(&mut context) {std::task::Poll::Ready(value)=>value,_=>panic!("unexpected pending")}}
#[test] fn cry_history_encrypts_redacted_records_retries_and_shares_only_authorized_recall() {
let mut local=command_history::CommandHistory::default();local.remember("before-login");ready(user_input_record::test_flush());assert!(FILES.lock().is_empty());
crypt::test_login(2);local.remember("status");local.remember("cry login malformed extra");local.remember("cry unlock alice recovery-secret");local.remember("cry ssh add 123456 key");
FAIL_APPEND.store(true,Ordering::Relaxed);ready(user_input_record::test_flush());assert!(FILES.lock().is_empty());
ready(user_input_record::test_flush());let records=user_input_record::test_records(2);
assert_eq!(records,vec!["status","cry login ******","cry unlock ******","cry ssh add ******"]);
let disk=FILES.lock()[user_input_record::PATH].clone();assert!(!disk.windows(6).any(|bytes|bytes==b"status"));assert!(!disk.windows(15).any(|bytes|bytes==b"recovery-secret"));
assert_eq!(local.recall(true,"draft"),Some("cry ssh add ******".into()));crypt::logout(2);local.sync();assert!(local.entries.is_empty());assert!(local.draft.is_empty());
let mut remote=command_history::CommandHistory::default();remote.set_scope(1);remote.remember("remote-before-login");ready(user_input_record::test_flush());assert_eq!(FILES.lock()[user_input_record::PATH],disk);
crypt::test_login(1);assert_eq!(remote.recall(true,""),Some("cry ssh add ******".into()));assert_eq!(local.recall(true,""),None);
remote.remember("net");ready(user_input_record::test_flush());assert_eq!(user_input_record::test_records(1).last().unwrap(),"net");
}
'''
    source = source.replace('@CRYPT@', crypt).replace('@HISTORY@', history).replace('@RECORD@', record)
    with tempfile.TemporaryDirectory(prefix='shell3-history-recording-') as directory:
        work = Path(directory)
        (work/'src').mkdir()
        (work/'src/lib.rs').write_text(source)
        (work/'Cargo.toml').write_text(f'''[package]
name="shell3-history-recording-host"
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
chacha20poly1305={{version="0.10.1",default-features=false}}
''')
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT/'tgt/shell3-history-recording-tests'))
        subprocess.run(['cargo','test','--offline','--quiet','--manifest-path',str(work/'Cargo.toml'),'--','--test-threads=1'],cwd=work,env=env,check=True)

if __name__ == '__main__':
    main()
