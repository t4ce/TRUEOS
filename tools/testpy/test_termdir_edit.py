#!/usr/bin/env python3
"""Exercise live frontend detection, Termdir's launch wire, and Edit's startup reader."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
BLUEPRINTS = ROOT.parent / 'TRUEOS-Blueprints'
extract.ROOT = ROOT


def main():
    sdk = (BLUEPRINTS / 'crates/trueos-v/src/vshell.rs').read_text()
    sdk = sdk[sdk.index('/// Launch an app with a one-shot UTF-8 start script'):]
    sdk = sdk[:sdk.index('/// Spawn a normal Shell3 at a pixel origin')]
    source = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::atomic::{AtomicU8, Ordering};
static FRONTEND: AtomicU8 = AtomicU8::new(0);
#[macro_export]macro_rules! log {($($arg:tt)*)=>{()};}
mod hv {
    pub fn blueprint_console_target(_:u8)->Option<u8> {
        (crate::FRONTEND.load(crate::Ordering::Relaxed)!=2).then_some(7)
    }
    pub fn take_blueprint_launch_script(_:u8)->Option<String>{Some("unchanged".into())}
}
mod shell3 {pub mod tui {
    pub fn window_for_vm(_:u8)->Option<u32>{(crate::FRONTEND.load(crate::Ordering::Relaxed)==0).then_some(1)}
    pub fn frontend_for_target(_: &u8)->Option<u8>{Some(7)}
}}
mod r {pub mod restart {pub fn startup_manifest()->&'static [u8]{b"startup"}}}
mod host {
    const FS_ERR_NOT_FOUND:i32=-8;
    const TERMINAL_FRONTEND_VFILE:&str="vFile:terminal-frontend";
    const VMX_LAUNCH_SCRIPT_VFILE:&str="vFile:launch";
    const LEGACY_VMX_LAUNCH_SCRIPT_PATH:&str="/.trueos/launch";
    const STARTUP_MANIFEST_VFILE:&str="vFile:startup";
    enum RequestKind {Read{path:String}}
    static RESULT:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
    fn start_completed_read(_:u32,bytes:Vec<u8>)->i32{*RESULT.lock().unwrap()=bytes;1}
    fn start(_:u32,_:RequestKind)->i32{panic!("virtual read reached disk")}
    @HOST@
    pub fn query(owner:u32)->Result<Vec<u8>,i32>{
        let rc=start_read(owner,"vFile:terminal-frontend".into());
        if rc<0 {Err(rc)} else {Ok(RESULT.lock().unwrap().clone())}
    }
    #[test]fn frontend_is_live_repeatable_vm_local_and_has_no_disk_io(){
        for (state,text) in [(0,b"ui4".as_slice()),(1,b"remote"),(2,b"detached"),(0,b"ui4")] {
            crate::FRONTEND.store(state,crate::Ordering::Relaxed);
            assert_eq!(query(0x8000_0007).unwrap(),text);
            assert_eq!(query(0x8000_0007).unwrap(),text);
        }
        for owner in [7,0x8000_0100] {assert_eq!(query(owner),Err(-8));}
        assert!(is_virtual_read("vFile:terminal-frontend"));
        assert!(!is_virtual_read("vFile:terminal-frontend/other"));
        assert_eq!(start_read(0x8000_0007,"vFile:launch".into()),1);
        assert_eq!(*RESULT.lock().unwrap(),b"unchanged");
    }
}
mod vcabi {
    pub static PAYLOAD:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
    pub static RC:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
    pub unsafe fn trueos_cabi_blueprint_launch_script_v1(ptr:*const u8,len:usize)->i32 {
        *PAYLOAD.lock().unwrap()=unsafe{core::slice::from_raw_parts(ptr,len)}.to_vec();
        RC.load(crate::Ordering::Relaxed)
    }
}
mod trueos {
    pub mod vshell {use alloc::vec::Vec;use crate::vcabi; @SDK@ }
    pub mod async_fs {
        pub const ERR_NOT_FOUND:i32=-8;
        pub static SCRIPT:std::sync::Mutex<Result<Vec<u8>,i32>>=std::sync::Mutex::new(Err(ERR_NOT_FOUND));
        pub fn block_on<T>(value:T)->T{value}
        pub fn read_file(path:&[u8])->Result<Vec<u8>,i32>{
            if path==b"vFile:terminal-frontend" {crate::host::query(0x8000_0007)}
            else {assert_eq!(path,b"vFile:launch");SCRIPT.lock().unwrap().clone()}
        }
    }
}
mod termdir {
    use crate::trueos;
    @TERMDIR@
    #[test]fn local_launch_selects_the_builtin_new_shell_and_keeps_the_literal_path(){
        let path=std::path::Path::new("trueosfs:disc7/My notes/a; --name.txt");
        open(path).unwrap();
        assert_eq!(*crate::vcabi::PAYLOAD.lock().unwrap(),b"edit\0fs-scope trueosfs\nopen trueosfs:disc7/My notes/a; --name.txt\n\0sh3");
        crate::vcabi::RC.store(-16,crate::Ordering::Relaxed);
        assert!(open(path).is_err());
        crate::vcabi::RC.store(0,crate::Ordering::Relaxed);
        for (state,expected) in [(0,true),(1,false),(2,false)] {
            crate::FRONTEND.store(state,crate::Ordering::Relaxed);
            assert_eq!(is_local_ui4(),expected);
        }
    }
}
mod editor {
    use std::path::Path;
    use crate::trueos;
    mod launch_script {@PARSER@}
    mod apperr {pub type Result<T>=std::io::Result<T>;}
    struct Documents(Vec<std::path::PathBuf>);
    impl Documents {fn add_file_path(&mut self,path:&Path)->std::io::Result<()>{self.0.push(path.to_owned());Ok(())}}
    struct DisplayablePathBuf(std::path::PathBuf);
    impl DisplayablePathBuf {fn from_path(p:std::path::PathBuf)->Self{Self(p)}}
    struct State {documents:Documents,file_picker_pending_dir:DisplayablePathBuf}
    @READER@
    #[test]fn script_opens_the_selected_file_and_absent_scripts_keep_argv_documents(){
        let mut state=State{documents:Documents(vec!["/argv.txt".into()]),file_picker_pending_dir:DisplayablePathBuf("/".into())};
        handle_launch_script(&mut state).unwrap();
        assert_eq!(state.documents.0.len(),1);
        *trueos::async_fs::SCRIPT.lock().unwrap()=Ok(b"fs-scope trueosfs\nopen trueosfs:disc7/My notes/a; --name.txt\n".to_vec());
        handle_launch_script(&mut state).unwrap();
        assert_eq!(state.documents.0[1],Path::new("trueosfs:disc7/My notes/a; --name.txt"));
        assert_eq!(state.file_picker_pending_dir.0,Path::new("trueosfs:disc7/My notes"));
        *trueos::async_fs::SCRIPT.lock().unwrap()=Err(-2);
        assert!(handle_launch_script(&mut state).is_err());
    }
}
'''
    td = (BLUEPRINTS / 'buildins/termdir/src/edit_action.rs').read_text()
    td = '\n'.join(line for line in td.splitlines() if not line.startswith('//!'))
    parser = (BLUEPRINTS / 'buildins/edit/crates/edit/src/bin/edit/launch_script.rs').read_text()
    parser = '\n'.join(line for line in parser.splitlines() if not line.startswith('//!'))
    reader = function(BLUEPRINTS / 'buildins/edit/crates/edit/src/bin/edit/main.rs', 'handle_launch_script')
    for token, value in {
        '@HOST@': '\n'.join(extract.item('src/r/io/async_fs_cabi.rs', name) for name in ('is_virtual_read', 'start_read')),
        '@SDK@': sdk, '@TERMDIR@': td, '@PARSER@': parser, '@READER@': reader,
    }.items():
        source = source.replace(token, value)
    with tempfile.TemporaryDirectory(prefix='termdir-edit-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
