"""Extract actual native proxy reservation ownership and conversion branches."""
from test_native_mio import ROOT
BASE=ROOT.parent/'codex/codex-rs/network-proxy/src'
def function(text,name):
 start=text.index('fn '+name+'(');body=text.index('{',start);depth=1;end=body+1
 while depth:
  if text[end]=='{':depth+=1
  elif text[end]=='}':depth-=1
  end+=1
 return text[start:end]
def source():
 text=(BASE/'proxy.rs').read_text();start=text.index('#[derive(Debug)]\nstruct ReservedListeners');end=text.index('/// Selects how managed sandbox clients',start);owned=text[start:end]
 reserve=function(text,'reserve_loopback_ephemeral_listener');reserves=function(text,'reserve_loopback_ephemeral_listeners')
 converters=[]
 for filename,func,label in [('http_proxy.rs','run_http_proxy_with_std_listener','http'),('socks5.rs','run_socks5_with_std_listener','socks')]:
  body=function((BASE/filename).read_text(),func);begin=body.index('    #[cfg(target_os = "trueos")]');end=body.index('\n    run_',begin);conversion=body[begin:end]
  converters.append('fn convert_'+label+'(listener:tokio::net::TcpListener)->Result<rama_tcp::server::TcpListener>{'+conversion+'\nOk(listener)}')
 return 'mod proxy_reservations{\nuse anyhow::{Context,Result};use std::{net::SocketAddr,sync::{Arc,Mutex}};use tokio::net::TcpListener as StdTcpListener;use rama_tcp::server::TcpListener;\n'+owned+'\n'+reserve+'\n'+reserves+'\n'+'\n'.join(converters)+r'''
 pub async fn run(){
  use tokio::io::{AsyncReadExt,AsyncWriteExt};
  let owned=reserve_loopback_ephemeral_listeners(true).unwrap();let http_addr=owned.http_addr().unwrap();let socks_addr=owned.socks_addr("127.0.0.1:9".parse().unwrap()).unwrap();assert_ne!(http_addr.port(),socks_addr.port());assert!(StdTcpListener::bind(http_addr).await.is_err());assert!(StdTcpListener::bind(socks_addr).await.is_err());let slots=owned.into_reserved_listeners();let copy=slots.clone();drop(slots);assert!(copy.take_http().is_some());assert!(copy.take_http().is_none());assert!(copy.take_socks().is_some());assert!(copy.take_socks().is_none());drop(copy);assert_eq!(crate::network_boundary::live_sockets(),0);println!("ok exact proxy reservations retain both ports and transfer once");
  let owned=reserve_loopback_ephemeral_listeners(false).unwrap();let addr=owned.http_addr().unwrap();assert_eq!(owned.socks_addr("127.0.0.1:9".parse().unwrap()).unwrap().port(),9);let slots=owned.into_reserved_listeners();let http=convert_http(slots.take_http().unwrap()).unwrap();assert_eq!(http.local_addr().unwrap(),addr);assert!(StdTcpListener::bind(addr).await.is_err());let client=tokio::spawn(async move{let mut stream=tokio::net::TcpStream::connect(addr).await.unwrap();stream.write_all(b"http-native").await.unwrap();stream.shutdown().await.unwrap();});let(mut peer,_)=http.accept().await.unwrap();let mut bytes=Vec::new();peer.read_to_end(&mut bytes).await.unwrap();assert_eq!(bytes,b"http-native");client.await.unwrap();drop((peer,http,slots));assert_eq!(crate::network_boundary::live_sockets(),0);println!("ok actual HTTP handoff retains native listener and readiness");
  let owned=reserve_loopback_ephemeral_listeners(true).unwrap();let addr=owned.socks_addr("127.0.0.1:9".parse().unwrap()).unwrap();let slots=owned.into_reserved_listeners();let socks=convert_socks(slots.take_socks().unwrap()).unwrap();assert_eq!(socks.local_addr().unwrap(),addr);let client=tokio::spawn(async move{let mut stream=tokio::net::TcpStream::connect(addr).await.unwrap();stream.write_all(b"socks-native").await.unwrap();stream.shutdown().await.unwrap();});let(mut peer,_)=socks.accept().await.unwrap();let mut bytes=Vec::new();peer.read_to_end(&mut bytes).await.unwrap();assert_eq!(bytes,b"socks-native");client.await.unwrap();drop((peer,socks,slots));assert_eq!(crate::network_boundary::live_sockets(),0);println!("ok actual SOCKS handoff retains native listener and readiness");
  let owned=reserve_loopback_ephemeral_listeners(true).unwrap();let http=owned.http_addr().unwrap();let socks=owned.socks_addr("127.0.0.1:9".parse().unwrap()).unwrap();let slots=owned.into_reserved_listeners();let clone=slots.clone();drop(slots);assert_eq!(crate::network_boundary::live_sockets(),2);drop(clone);assert_eq!(crate::network_boundary::live_sockets(),0);let a=StdTcpListener::bind(http).await.unwrap();let b=StdTcpListener::bind(socks).await.unwrap();drop((a,b));assert_eq!(crate::network_boundary::live_sockets(),0);println!("ok canceled shared reservations release both native listener capabilities");
 }
}
'''
