#!/usr/bin/env python3
"""Exercise Edit's production stdin loop through a byte-free re-entry wake."""
from pathlib import Path
import subprocess
import tempfile
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
EDIT = ROOT.parent / 'TRUEOS-Blueprints/buildins/edit/crates/edit/src/sys/unix.rs'


def main():
    production = '\n'.join(function(EDIT, name) for name in (
        'check_terminal_resize', 'take_terminal_restore', 'read_stdin'))
    # The host rustc also defines target_os=linux. Exercise only the native
    # TRUEOS poll branch, while retaining every lifecycle/read decision.
    production = production.replace('target_os = "linux"', 'target_os = "unused_linux"')
    harness = r'''
#![allow(dead_code,non_camel_case_types)]
use std::{time,cell::RefCell,ops::{Deref,DerefMut}};
struct Arena;
fn scratch_arena(_:Option<&Arena>)->Box<Arena>{Box::new(Arena)}
struct BVec(Vec<u8>);
impl BVec {
    fn empty()->Self{Self(Vec::new())}
    fn reserve(&mut self,_:&Arena,n:usize){self.0.reserve(n)}
    unsafe fn set_len(&mut self,n:usize){unsafe{self.0.set_len(n)}}
    fn extend_from_slice(&mut self,_:&Arena,b:&[u8]){self.0.extend_from_slice(b)}
}
impl Deref for BVec {type Target=Vec<u8>;fn deref(&self)->&Vec<u8>{&self.0}}
impl DerefMut for BVec {fn deref_mut(&mut self)->&mut Vec<u8>{&mut self.0}}
struct BString<'a>(String,std::marker::PhantomData<&'a Arena>);
impl<'a> BString<'a> {
    fn from_utf8_lossy(_:&'a Arena,b:BVec)->Self{Self(String::from_utf8_lossy(&b.0).into_owned(),Default::default())}
    fn replace_range(&mut self,_:&Arena,r:std::ops::Range<usize>,s:&str){self.0.replace_range(r,s)}
}
macro_rules! arena_format {($arena:expr,$($args:tt)*)=>{format!($($args)*)}}
const KIBI:usize=1024;
struct State {stdin:i32,inject_resize:bool,restore_terminal:bool,utf8_len:usize,utf8_buf:[u8;4]}
static mut STATE:State=State{stdin:0,inject_resize:false,restore_terminal:false,utf8_len:0,utf8_buf:[0;4]};
#[derive(Clone,Copy)]enum Action{Reentry,Byte,Timeout}
struct Fixture {generation:u64,active:bool,action:Action,polls:Vec<i32>,reads:usize,nonblocking:Vec<bool>}
std::thread_local! {static F:RefCell<Fixture>=RefCell::new(Fixture{generation:1,active:true,action:Action::Timeout,polls:vec![],reads:0,nonblocking:vec![]});}
unsafe fn trueos_cabi_blueprint_terminal_surface_snapshot_v1(g:*mut u64,c:*mut u32,r:*mut u32)->i32 {
    F.with(|f|{let f=f.borrow();if !f.active{return -1} unsafe{g.write(f.generation);c.write(120);r.write(40)};0})
}
fn get_window_size()->(u16,u16){(120,40)}
fn set_tty_nonblocking(value:bool){F.with(|f|f.borrow_mut().nonblocking.push(value))}
fn errno()->i32{libc::EAGAIN}
mod libc {
    pub type c_int=i32;pub const POLLIN:i16=1;pub const EINTR:i32=4;pub const EAGAIN:i32=11;
    pub struct pollfd {pub fd:i32,pub events:i16,pub revents:i16}
    pub unsafe fn poll(_: *mut pollfd,_:usize,timeout:i32)->i32 {
        super::F.with(|f|{let mut f=f.borrow_mut();f.polls.push(timeout);match f.action {
            super::Action::Reentry=>{f.active=true;f.generation+=1;0},
            super::Action::Byte=>1,super::Action::Timeout=>0,
        }})
    }
    pub unsafe fn read(_:i32,p:*mut u8,_:usize)->isize {
        super::F.with(|f|{f.borrow_mut().reads+=1});unsafe{p.write(b'x')};1
    }
}
#[test]fn idle_editor_observes_reentry_without_a_keystroke_and_keeps_typed_bytes_separate(){
    check_terminal_resize();unsafe{STATE.inject_resize=false};assert!(!take_terminal_restore());
    F.with(|f|{let mut f=f.borrow_mut();f.active=false;f.action=Action::Reentry});
    let arena=Arena;let input=read_stdin(&arena,time::Duration::MAX).expect("no EOF");
    assert_eq!(input.0,"\x1b[8;40;120t");assert!(take_terminal_restore());assert!(!take_terminal_restore());
    F.with(|f|{let f=f.borrow();assert_eq!(f.polls,vec![-1]);assert_eq!(f.reads,0)});
    F.with(|f|{let mut f=f.borrow_mut();f.active=false;f.action=Action::Timeout});
    assert_eq!(read_stdin(&arena,time::Duration::MAX).unwrap().0,"");assert!(!take_terminal_restore());
    F.with(|f|{let mut f=f.borrow_mut();f.active=true;f.action=Action::Byte});
    assert_eq!(read_stdin(&arena,time::Duration::MAX).unwrap().0,"x");assert!(!take_terminal_restore());
    F.with(|f|{let f=f.borrow();assert_eq!(f.nonblocking,vec![true]);assert_eq!(f.reads,1)});
    F.with(|f|{f.borrow_mut().action=Action::Timeout});
    assert_eq!(read_stdin(&arena,time::Duration::from_millis(u64::MAX)).unwrap().0,"");
    F.with(|f|assert_eq!(f.borrow().polls.last(),Some(&i32::MAX)));
}
'''
    with tempfile.TemporaryDirectory(prefix='edit-reentry-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(harness + production)
        subprocess.run(['rustc', '--edition=2024', '--cfg', 'target_os="trueos"',
                        '-Aexplicit_builtin_cfgs_in_flags', '--test', str(path/'test.rs'),
                        '-o', str(path/'test')], check=True)
        subprocess.run([str(path/'test'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
