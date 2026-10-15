#!/usr/bin/env python3
"""Exercise production TLS read and std/WebSocket error adapters without traffic."""
from pathlib import Path
import subprocess
import tempfile
from test_indexed_fs_remove import extract

BLUEPRINTS = Path(__file__).resolve().parents[3] / "TRUEOS-Blueprints"
HARNESS = r'''
#![allow(dead_code)]
use std::{io::{self as std_io},task::{Context,Poll,Waker}};
mod tokio {pub mod io {pub use std::io::{Error,ErrorKind,Result};}}
use tokio::io;
type CompatIoResult<T>=std_io::Result<T>;
#[derive(Debug,PartialEq)]enum TlsFailure{UnknownIssuer}
impl std::fmt::Display for TlsFailure{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("invalid peer certificate: UnknownIssuer")}}
impl std::error::Error for TlsFailure{}
struct Session{incoming:Option<std_io::Result<usize>>,error:Option<TlsFailure>,processed:usize}
impl Session {
    fn read_tls<R>(&mut self,_:&mut R)->std_io::Result<usize>{self.incoming.take().unwrap()}
    fn process_new_packets(&mut self)->Result<(),TlsFailure>{self.processed+=1;if let Some(e)=self.error.take(){Err(e)}else{Ok(())}}
}
struct SyncReadAdapter<'a,'b>{io:&'a mut (),cx:&'a mut Context<'b>}
struct Stream<'a>{io:&'a mut (),session:&'a mut Session,alert_writes:usize}
impl Stream<'_>{
    fn write_io(&mut self,_:&mut Context)->Poll<io::Result<usize>>{self.alert_writes+=1;Poll::Ready(Err(io::Error::other("secondary alert write failure")))}
__READ_IO__
}
fn read(incoming:io::Result<usize>,error:Option<TlsFailure>)->(Poll<io::Result<usize>>,usize,usize){let mut unit=();let mut session=Session{incoming:Some(incoming),error,processed:0};let mut stream=Stream{io:&mut unit,session:&mut session,alert_writes:0};let mut cx=Context::from_waker(Waker::noop());let result=stream.read_io(&mut cx);let writes=stream.alert_writes;(result,writes,session.processed)}
fn ready_error<T:std::fmt::Debug>(result:Poll<io::Result<T>>)->io::Error{match result{Poll::Ready(Err(e))=>e,other=>panic!("expected error, got {other:?}")}}
#[test]fn tls_failure_retains_typed_source_and_primary_error_over_alert_failure(){let(result,writes,processed)=read(Ok(32),Some(TlsFailure::UnknownIssuer));let error=ready_error(result);assert_eq!(error.kind(),io::ErrorKind::InvalidData);assert_eq!(error.to_string(),"invalid peer certificate: UnknownIssuer");assert_eq!(error.get_ref().unwrap().downcast_ref::<TlsFailure>(),Some(&TlsFailure::UnknownIssuer));assert_eq!((writes,processed),(1,1));}
#[test]fn would_block_returns_pending_without_processing_or_alert(){let(result,writes,processed)=read(Err(io::ErrorKind::WouldBlock.into()),None);assert!(result.is_pending());assert_eq!((writes,processed),(0,0));}
#[test]fn socket_error_preserves_os_code_and_kind(){let(result,writes,processed)=read(Err(io::Error::from_raw_os_error(104)),None);let error=ready_error(result);assert_eq!(error.raw_os_error(),Some(104));assert_eq!(error.kind(),io::ErrorKind::ConnectionReset);assert_eq!((writes,processed),(0,0));}
#[test]fn converter_roundtrip_preserves_typed_source(){let error=io::Error::new(io::ErrorKind::InvalidData,TlsFailure::UnknownIssuer);let error=tokio_to_std_error(std_to_tokio_error(error));assert_eq!(error.kind(),io::ErrorKind::InvalidData);assert_eq!(error.get_ref().unwrap().downcast_ref::<TlsFailure>(),Some(&TlsFailure::UnknownIssuer));}
#[test]fn websocket_adapter_keeps_would_block(){let error=ready_error(map_tokio_io::<usize>(Poll::Ready(Err(io::ErrorKind::WouldBlock.into()))));assert_eq!(error.kind(),io::ErrorKind::WouldBlock);}
#[test]fn websocket_adapter_keeps_tls_source_and_pending(){let error=io::Error::new(io::ErrorKind::InvalidData,TlsFailure::UnknownIssuer);let error=ready_error(map_tokio_io::<usize>(Poll::Ready(Err(error))));assert_eq!(error.get_ref().unwrap().downcast_ref::<TlsFailure>(),Some(&TlsFailure::UnknownIssuer));assert!(map_tokio_io::<usize>(Poll::Pending).is_pending());}
#[test]fn valid_tls_read_returns_count_without_alert(){let(result,writes,processed)=read(Ok(32),None);assert!(matches!(result,Poll::Ready(Ok(32))));assert_eq!((writes,processed),(0,1));}
'''
common = (BLUEPRINTS / "vendor/tokio-rustls-0.26.4/src/common/mod.rs").read_text()
compat = (BLUEPRINTS / "vendor/tokio-tungstenite-0.29.0/src/compat.rs").read_text()
# Generic signatures can contain braces only in the function body in these adapters.
def function(source, name):
    start = source.index("fn " + name + "<") if "fn " + name + "<" in source else source.index("fn " + name + "(")
    start = source.rfind("\n", 0, start) + 1
    opening = source.index("{", start)
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]
production = "\n".join(function(common, name) for name in ["std_to_tokio_error", "tokio_to_std_error"])
production += "\n" + function(compat, "map_tokio_io")
with tempfile.TemporaryDirectory(prefix="websocket-tls-errors-") as directory:
    rust = Path(directory) / "test.rs"
    binary = Path(directory) / "test"
    rust.write_text(HARNESS.replace("__READ_IO__", extract(common, "read_io")) + production)
    subprocess.run(["rustc", "--edition=2024", "--test", "-Awarnings", str(rust), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
