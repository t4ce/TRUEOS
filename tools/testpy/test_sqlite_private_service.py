#!/usr/bin/env python3
"""Exercise the production private SQLite service/SQLx configuration and guard.

The async filesystem boundary supplies explicit metadata/error responses. The
actual native guard is compiled unchanged apart from selecting its target cfg.
Native authorization and transport admission have their own real-engine tests.
"""
from pathlib import Path
import argparse
import hashlib
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BP = ROOT.parent / 'TRUEOS-Blueprints'
HARNESS = r'''
extern crate self as trueos;
use std::{cell::RefCell, collections::BTreeMap, future::Future, sync::Arc, task::{Context, Poll, Wake, Waker}};
#[derive(Default)]
struct State { private_error: Option<i32>, metadata: BTreeMap<String, Result<async_fs::NodeKind,i32>>, calls: Vec<String> }
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
pub mod async_fs {
    pub const ERR_NOT_FOUND: i32 = -8;
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum NodeKind { File, Directory }
    pub struct Metadata { pub kind: NodeKind }
    pub async fn assert_private_path(path: &[u8]) -> Result<(),i32> {
        super::STATE.with(|state| {let mut state=state.borrow_mut();state.calls.push(format!("private:{}",String::from_utf8_lossy(path)));state.private_error.map_or(Ok(()),Err)})
    }
    pub async fn metadata(path: &[u8]) -> Result<Metadata,i32> {
        super::STATE.with(|state| {let mut state=state.borrow_mut();let path=String::from_utf8(path.to_vec()).unwrap();state.calls.push(path.clone());state.metadata.get(&path).cloned().unwrap_or(Err(ERR_NOT_FOUND)).map(|kind| Metadata{kind})})
    }
}
struct Noop;
impl Wake for Noop { fn wake(self:Arc<Self>) {} }
fn wait<T>(future:impl Future<Output=T>)->T {let waker=Waker::from(Arc::new(Noop));let mut context=Context::from_waker(&waker);let mut future=std::pin::pin!(future);match future.as_mut().poll(&mut context){Poll::Ready(value)=>value,Poll::Pending=>panic!("fixture boundary unexpectedly yielded")}}
#[path="@STORAGE@"] mod private_storage;
#[path="@ENDPOINT@"] mod private_endpoint;
fn metadata(path:&str,value:Result<async_fs::NodeKind,i32>){STATE.with(|state|{state.borrow_mut().metadata.insert(path.into(),value);});}
fn directory(path:&str){metadata(path,Ok(async_fs::NodeKind::Directory));}
fn verify()->Result<(),String>{wait(private_storage::verify("codex",std::path::Path::new(private_storage::DIRECTORY)))}
fn calls()->Vec<String>{STATE.with(|state|state.borrow().calls.clone())}
#[test]
fn unprotected_or_wrong_directory_refuses_before_any_metadata_probe(){
    STATE.with(|state|state.borrow_mut().private_error=Some(-13));
    assert!(verify().unwrap_err().contains("not delegated private storage"));
    assert_eq!(calls(),vec![format!("private:{}",private_storage::DIRECTORY)]);
    STATE.with(|state|*state.borrow_mut()=State::default());
    assert!(wait(private_storage::verify("codex",std::path::Path::new("/common/sqlite-service/codex"))).is_err());
    assert!(calls().is_empty());
}
#[test]
fn absent_legacy_ancestors_are_proved_without_accessing_missing_descendants(){
    directory("/common");
    assert!(verify().is_ok());
    assert_eq!(calls(),vec![format!("private:{}",private_storage::DIRECTORY),"/common".into(),"/common/sqlite-service".into()]);
}
#[test]
fn legacy_shared_directory_refuses_instead_of_starting_with_empty_databases(){
    for legacy in ["/common/sqlite-service/codex"] {
        STATE.with(|state|*state.borrow_mut()=State::default());
        let mut prefix=String::new();for part in legacy.trim_start_matches('/').split('/') {prefix.push('/');prefix.push_str(part);directory(&prefix);}
        let error=verify().unwrap_err();assert!(error.contains(legacy),"{error}");assert!(error.contains("trusted private migration must complete"));
    }
}
#[test]
fn unavailable_storage_and_nondirectory_ancestors_are_never_treated_as_absent(){
    metadata("/common",Err(-1));assert!(verify().unwrap_err().contains("TRUEOSFS -1"));
    STATE.with(|state|*state.borrow_mut()=State::default());metadata("/common",Ok(async_fs::NodeKind::File));
    assert!(verify().unwrap_err().contains("not a directory"));
}
#[test]
fn private_service_endpoint_matches_the_reserved_instance_port(){
    let hash="codex".bytes().fold(2166136261u32,|hash,byte|(hash^u32::from(byte)).wrapping_mul(16777619));
    assert_eq!(20000+hash%40000,42116);
    assert_eq!(private_endpoint::ENDPOINT,private_storage::ENDPOINT);
}
'''


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    storage = BP / 'apps/sqlite/src/private_storage.rs'
    endpoint = BP / 'vendor/sqlx-sqlite-0.9.0/src/remote/private_endpoint.rs'
    checks = []
    with tempfile.TemporaryDirectory(prefix='private-service-', dir=out) as tmp:
        work = Path(tmp)
        selected = work / 'private_storage.rs'
        selected.write_text(storage.read_text().replace('#[cfg(target_os = "trueos")]', '#[cfg(all())]'))
        harness = work / 'proof.rs'
        harness.write_text(HARNESS.replace('@STORAGE@', str(selected)).replace('@ENDPOINT@', str(endpoint)))
        for profile, flags in [('debug', []), ('release', ['-O'])]:
            binary = work / ('proof-' + profile)
            command = ['rustc', '--edition=2024', '--test', *flags, str(harness), '-o', str(binary)]
            result = subprocess.run(command, cwd=work, text=True, capture_output=True, timeout=60)
            log = out / (profile + '.log')
            log.write_text(result.stdout + result.stderr)
            if result.returncode:
                raise RuntimeError(log.read_text())
            result = subprocess.run([str(binary)], cwd=work, text=True, capture_output=True, timeout=60)
            with log.open('a') as stream:
                stream.write(result.stdout + result.stderr)
            if result.returncode:
                raise RuntimeError(log.read_text())
            assert '10 passed' in result.stdout, result.stdout
            checks.append({'profile': profile, 'tests': 10, 'log': str(log), 'compile': command})
    receipt = out / 'receipt.json'
    receipt.write_text(json.dumps({'scope': __doc__, 'sources': {str(path): digest(path) for path in [storage, endpoint]}, 'checks': checks}, indent=2) + '\n')
    print(receipt)


if __name__ == '__main__':
    main()
