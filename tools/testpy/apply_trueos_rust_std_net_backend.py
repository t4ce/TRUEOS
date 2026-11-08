#!/usr/bin/env python3
"""Select native synchronous TCP/UDP without exporting descriptor identities."""
from pathlib import Path
import argparse
ROOT=Path(__file__).resolve().parents[2]
def replace(source,old,new,path):
 if new in source:return source
 if source.count(old)!=1:raise ValueError(f'{path}: missing native net anchor {old!r}')
 return source.replace(old,new,1)
def install(root,check=False):
 lib=root/'library/std/src';changes={}
 p=lib/'sys/net/connection/trueos.rs';source=(ROOT/'tools/rust-std/trueos_net.rs').read_text()
 # NativeSocket is an explicit target API. Inherit stability inside this module.
 source=source.replace('pub struct NativeSocket(', '#[stable(feature = "trueos_native_socket", since = "1.99.0")]\npub struct NativeSocket(')
 source=source.replace('impl NativeSocket {','#[stable(feature = "trueos_native_socket", since = "1.99.0")]\nimpl NativeSocket {')
 import re
 source=re.sub(r'^([ \t]*)(pub fn )',r'\1#[stable(feature = "trueos_native_socket", since = "1.99.0")]\n\1\2',source,flags=re.M)
 changes[p]=source
 p=lib/'sys/net/connection/mod.rs';source=p.read_text();source=replace(source,'cfg_select! {','''cfg_select! {
    target_os = "trueos" => {
        mod trueos;
        #[stable(feature = "trueos_native_socket", since = "1.99.0")]
        pub use trueos::*;
        // Keep Unix-domain extension types separate from native TCP/UDP.
        // Consumers must reject unsupported Unix transports before creation.
        mod socket;
        pub use socket::{Socket, getsockopt, setsockopt};
    }''',p);changes[p]=source
 p=lib/'sys/net/mod.rs';source=p.read_text();source=replace(source,'pub use connection::*;', '#[stable(feature = "trueos_native_socket", since = "1.99.0")]\npub use connection::*;',p);changes[p]=source
 p=lib/'os/fd/owned.rs';source=p.read_text()
 for ty in ['TcpStream','TcpListener','UdpSocket']:
  for anchor in [f'impl AsFd for crate::net::{ty} {{',f'impl From<crate::net::{ty}> for OwnedFd {{',f'impl From<OwnedFd> for crate::net::{ty} {{']:
   source=replace(source,anchor,'#[cfg(not(target_os = "trueos"))]\n'+anchor,p)
 changes[p]=source
 p=lib/'os/fd/net.rs';source=p.read_text();marker='#![cfg(not(target_os = "trueos"))]\n'
 if marker not in source:source=marker+source
 changes[p]=source
 p=lib/'net/mod.rs';source=p.read_text();addition='''
#[cfg(target_os = "trueos")]
#[stable(feature = "trueos_native_socket", since = "1.99.0")]
pub use crate::sys::net::NativeSocket;
'''
 if addition not in source:source+=addition
 for ty in ['TcpStream','TcpListener','UdpSocket']:
  addition=f'''
#[cfg(target_os = "trueos")]
impl {ty} {{
    #[stable(feature = "trueos_native_socket", since = "1.99.0")]
    pub fn from_native_socket(socket: NativeSocket) -> Self {{ {{ use crate::sys::FromInner; Self::from_inner(crate::sys::net::{ty}::from_native(socket)) }} }}
    #[stable(feature = "trueos_native_socket", since = "1.99.0")]
    pub fn native_socket(&self) -> &NativeSocket {{ {{ use crate::sys::AsInner; self.as_inner().native() }} }}
    #[stable(feature = "trueos_native_socket", since = "1.99.0")]
    pub fn into_native_socket(self) -> NativeSocket {{ {{ use crate::sys::IntoInner; self.into_inner().into_native() }} }}
}}
'''
  if addition not in source:source+=addition
 changes[p]=source
 needed=[p for p,s in changes.items() if not p.exists() or p.read_text()!=s]
 if check and needed:raise ValueError('native networking backend not installed: '+', '.join(map(str,needed)))
 if not check:
  for p in needed:p.write_text(changes[p])
 print('trueos-rust-std-net: owned TCP/UDP capabilities, no descriptor conversions')
def main():
 p=argparse.ArgumentParser();p.add_argument('--check',action='store_true');p.add_argument('rust_src',type=Path);a=p.parse_args();root=a.rust_src.resolve()
 if root.name=='library':root=root.parent
 install(root,a.check)
if __name__=='__main__':main()
