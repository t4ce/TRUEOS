#!/usr/bin/env python3
"""Exercise the production carrier timing/handoff code with synthetic guest work."""
from pathlib import Path
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[1]
SDK = ROOT.parent / 'TRUEOS-Blueprints/api/src'
source = (SDK / 'x86.rs').read_text()
start = source.index('#[derive(Clone, Copy, Debug, Default)]\npub struct CarrierTiming')
end = source.index('\n#[cfg(test)]\nmod tests', start)
code = '''#![allow(dead_code,unexpected_cfgs)]
extern crate alloc;
mod v { pub mod vx86 {
#[derive(Debug,Default)] pub struct Exit;
pub fn context_run(_:u64)->Result<Exit,i32>{Ok(Exit)}
pub fn context_resume(_:u64)->Result<Exit,i32>{Ok(Exit)}
}}
#[derive(Debug,PartialEq)] enum Error { CarrierUnavailable, CarrierLost, Kernel(i32) }
impl Error { fn from_kernel(e:i32)->Self {Self::Kernel(e)} }
type Exit = v::vx86::Exit;
'''
code += f'#[path="{SDK / "worker.rs"}"] mod worker;\n'
code += source[start:end]
code += '''
#[tokio::test]
async fn native_time_is_separate_from_request_queue_time() {
    let (requests, receiver) = tokio::sync::mpsc::channel(1);
    let task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        carrier_requests(receiver, |_, _| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            Ok(v::vx86::Exit)
        }).await;
    });
    let carrier = ExecutionCarrier { requests };
    let (_, t) = carrier.execute_measured(1, false).await.unwrap();
    assert!(t.request_ns >= 20_000_000, "{:?}", t);
    assert!(t.native_ns >= 10_000_000, "{:?}", t);
    drop(carrier);
    tokio::time::timeout(std::time::Duration::from_secs(1), task).await.unwrap().unwrap();
}
#[tokio::test]
async fn measurement_preserves_errors_and_abandoned_requests() {
    let (requests, receiver) = tokio::sync::mpsc::channel(1);
    let (reply, response) = tokio::sync::oneshot::channel();
    requests.send(CarrierRequest { handle: 99, resume: false, reply }).await.unwrap();
    drop(response);
    let task = tokio::spawn(carrier_requests(receiver, |handle, _| {
        assert_ne!(handle, 99);
        Err(-125)
    }));
    let carrier = ExecutionCarrier { requests };
    assert_eq!(carrier.execute(1, true).await.unwrap_err(), Error::Kernel(-125));
    drop(carrier);
    tokio::time::timeout(std::time::Duration::from_secs(1), task).await.unwrap().unwrap();
}
'''
with tempfile.TemporaryDirectory(prefix='xp-timing-') as d:
    root = Path(d); (root/'src').mkdir()
    (root/'src/lib.rs').write_text(code)
    (root/'Cargo.toml').write_text('[package]\nname="xp-timing-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\ntokio={version="=1.52.3",features=["sync","rt","macros","time"]}\n[workspace]\n')
    subprocess.run(['cargo','test','--offline','--target','x86_64-unknown-linux-gnu','--target-dir',str(ROOT/'bld/xp-timing-host-tests')],cwd=root,check=True)
