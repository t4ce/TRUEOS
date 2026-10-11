#!/usr/bin/env python3
"""Exercise production initial geometry and Blueprint spawn completion polling."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def main():
    shell = (ROOT / 'src/shell3/shell3.rs').read_text()
    configure = re.search(r'    pub\(super\) fn configure_window\(.*?^    }', shell, re.M | re.S).group()
    helper = (ROOT / 'crates/trueos-v/src/vshell.rs').read_text().split('pub fn spawn_window(', 1)[1]
    source = '''#![allow(dead_code)]
const MIN_COLUMNS: usize = 20;
const MIN_ROWS: usize = 5;
mod microfont { pub const FWIDTH: usize = 6; pub const FHEIGHT: usize = 11; }
mod intel { pub fn active_scanout_dimensions() -> Option<(u32,u32)> { Some((2560,1440)) } }
mod service {
    #[derive(Clone,Copy,Default,Debug,PartialEq)]
    pub struct WindowGeometry {pub x:i32,pub y:i32,pub width:u32,pub height:u32}
    pub fn microfont_scale() -> u32 {2}
}
#[derive(Default)] struct Show { geometry:service::WindowGeometry }
impl Show { fn configure_window(&mut self,g:service::WindowGeometry) {self.geometry=g;} }
#[derive(Default)] struct Shell {show:Show,columns:usize,rows:usize}
impl Shell {
    fn set(&mut self,c:usize,r:usize) {self.columns=c.max(MIN_COLUMNS);self.rows=r.max(MIN_ROWS);}
''' + configure.replace('pub(super)', 'pub(crate)') + '''
}
mod vcabi {
    pub static CALLS: std::sync::Mutex<Vec<(i32,i32,u32,u32)>> = std::sync::Mutex::new(Vec::new());
    pub unsafe fn trueos_cabi_shell3_window_v1(x:i32,y:i32,w:u32,h:u32)->i64 {
        let mut calls = CALLS.lock().unwrap(); calls.push((x,y,w,h));
        if w!=0 {0} else if calls.len()==2 {0} else {42}
    }
}
mod vsys {pub fn poll_once() {} pub fn sleep_ms(_:u64) {}}
pub fn spawn_window(''' + helper + '''
#[test] fn initial_geometry_drives_first_text_grid() {
    let mut shell=Shell::default();
    shell.configure_window(service::WindowGeometry{x:-50,y:75,width:481,height:321});
    assert_eq!((shell.columns,shell.rows),(40,14));
    assert_eq!((shell.show.geometry.x,shell.show.geometry.y),(-50,75));
    assert_eq!((shell.show.geometry.width,shell.show.geometry.height),(481,321));
    shell.configure_window(service::WindowGeometry{x:0,y:0,width:1,height:1});
    assert_eq!((shell.show.geometry.width,shell.show.geometry.height),(240,110));
    shell.configure_window(service::WindowGeometry{x:0,y:0,width:u32::MAX,height:u32::MAX});
    assert_eq!((shell.show.geometry.width,shell.show.geometry.height),(2560,1440));
}
#[test] fn helper_waits_for_presentation_and_rejects_empty_extent() {
    assert_eq!(spawn_window(0,0,0,320),Err(-1));
    assert_eq!(spawn_window(-50,75,480,320),Ok(42));
    assert_eq!(*vcabi::CALLS.lock().unwrap(),vec![(-50,75,480,320),(0,0,0,0),(0,0,0,0)]);
}
'''
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'test')],check=True)
        subprocess.run([str(path/'test')],check=True)

if __name__ == '__main__':
    main()
