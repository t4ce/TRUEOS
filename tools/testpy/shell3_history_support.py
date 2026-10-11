"""Small host dependency/auth fixtures for production history tests."""
from pathlib import Path
import subprocess


def auth_fixture(scope):
    return r'''
mod matrix_target {pub const TRANSPORT_NET_TCP_SCOPE:u8=1;pub const TRANSPORT_LOCAL_SCOPE:u8=2;}
mod crypt {
    static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
    std::thread_local! {pub static SESSION:std::cell::Cell<Option<(u8,u64,u64)>>=std::cell::Cell::new(Some((@SCOPE@,NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed),1)));}
    pub fn authenticated_history_identity(scope:u8)->Option<(u64,u64)> {SESSION.with(|session|session.get().filter(|identity|identity.0==scope).map(|(_,account,sequence)|(account,sequence)))}
}
mod user_input_record {
    std::thread_local! {pub static CAPTURED:std::cell::RefCell<Vec<(u8,String)>>=const {std::cell::RefCell::new(Vec::new())};}
    pub fn capture(scope:u8,text:&str) {CAPTURED.with(|records|records.borrow_mut().push((scope,text.into())));}
}
'''.replace('@SCOPE@', str(scope))


def dependencies(path):
    zeroize = next((Path.home()/'.cargo/registry/src').glob('*/zeroize-*/src/lib.rs'))
    subprocess.run(['rustc','--edition=2021','--crate-type=rlib','--crate-name','zeroize','--cfg','feature="alloc"',str(zeroize),'-o',str(path/'libzeroize.rlib')],check=True)
    (path/'spin.rs').write_text("pub struct Mutex<T>(std::sync::Mutex<T>);impl<T> Mutex<T>{pub const fn new(value:T)->Self{Self(std::sync::Mutex::new(value))}pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}")
    subprocess.run(['rustc','--edition=2024','--crate-type=rlib','--crate-name','spin',str(path/'spin.rs'),'-o',str(path/'libspin.rlib')],check=True)
    return ['--extern',f'zeroize={path}/libzeroize.rlib','--extern',f'spin={path}/libspin.rlib']
