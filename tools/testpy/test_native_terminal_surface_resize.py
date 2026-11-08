#!/usr/bin/env python3
"""Run actual Shell3 selection and SSH prompt code across geometry changes.

The route registry/admission fixtures isolate selection from hardware. Screen
resizing uses the real terminal model; the entire SSH terminal module and its
inline tests run unchanged with host Tokio. This does not simulate a live rig.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_edit_life_native import function
from test_native_mio import ROOT, TOOLCHAIN, environment, info

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
pub type Result<T> = core::result::Result<T, String>;
#[path = "@SSH@"] mod ssh_terminal;
mod routing {
use std::collections::VecDeque;
use spin::Mutex;
use trueos_terminal::Terminal;
#[derive(Clone, Copy)] struct Frontend {id:u64, cols:usize, rows:usize}
struct Lease(&'static str);
impl Lease {fn name(&self)->&str{self.0}}
struct Native {active:bool,return_to_default:bool,input:VecDeque<u8>}
struct Route {
 lease:Lease,frontend:Option<u64>,selected:bool,native:Option<Native>,
 owner:Option<u64>,screen:Terminal,surface_generation:u64,suppressed_text:Option<()>,
}
impl Route {fn active(&self)->bool{self.owner.is_some()||self.native.as_ref().is_some_and(|n|n.active)}}
struct Registry {routes:Vec<Route>,changes:Vec<Option<u64>>}
impl Registry {fn changed(&mut self,id:impl Into<Option<u64>>){self.changes.push(id.into())}}
static ROUTES:Mutex<Registry>=Mutex::new(Registry{routes:Vec::new(),changes:Vec::new()});
fn authorize_route(_:Frontend,name:&str)->Result<(),()>{if name=="denied"{Err(())}else{Ok(())}}
fn park(id:u64)->bool {
 for route in ROUTES.lock().routes.iter_mut().filter(|r|r.frontend==Some(id)) {
  route.owner=None;if let Some(native)=route.native.as_mut(){native.active=false;}
 }
 true
}
@SELECT@
fn route(name:&'static str,frontend:u64)->Route {
 let mut screen=Terminal::new(80,24);screen.feed(b"retained");
 Route{lease:Lease(name),frontend:Some(frontend),selected:true,
 native:Some(Native{active:true,return_to_default:false,input:b"typeahead".iter().copied().collect()}),
 owner:None,screen,surface_generation:7,suppressed_text:None}
}
fn reset(routes:Vec<Route>){*ROUTES.lock()=Registry{routes,changes:Vec::new()};}
#[test]fn resize_keeps_identity_and_pending_native_input() {
 reset(vec![route("codex",1)]);
 assert!(select(Frontend{id:1,cols:132,rows:48},Some("codex")));
 let r=ROUTES.lock();let route=&r.routes[0];
 assert_eq!(route.screen.dimensions(),(132,48));assert_eq!(route.surface_generation,7);
 assert_eq!(route.native.as_ref().unwrap().input.iter().copied().collect::<Vec<_>>(),b"typeahead");
 assert!(route.native.as_ref().unwrap().active);assert_eq!(r.changes,vec![Some(1)]);
}
#[test]fn repeated_geometry_does_not_emit_spurious_changes() {
 reset(vec![route("codex",1)]);
 assert!(select(Frontend{id:1,cols:80,rows:24},Some("codex")));
 let r=ROUTES.lock();assert_eq!(r.routes[0].surface_generation,7);assert!(r.changes.is_empty());
}
#[test]fn resizing_selected_route_does_not_resize_or_retire_another_frontend() {
 reset(vec![route("codex",1),route("edit",2)]);
 assert!(select(Frontend{id:1,cols:100,rows:30},Some("codex")));
 let r=ROUTES.lock();let other=&r.routes[1];assert_eq!(other.screen.dimensions(),(80,24));
 assert_eq!(other.surface_generation,7);assert_eq!(other.native.as_ref().unwrap().input.len(),9);
}
#[test]fn resize_cannot_take_another_frontends_live_surface() {
 reset(vec![route("codex",2)]);
 assert!(!select(Frontend{id:1,cols:100,rows:30},Some("codex")));
 let r=ROUTES.lock();assert_eq!(r.routes[0].frontend,Some(2));assert_eq!(r.routes[0].surface_generation,7);
 assert_eq!(r.routes[0].screen.dimensions(),(80,24));assert!(r.changes.is_empty());
}
#[test]fn switching_routes_still_retires_inactive_native_input() {
 let mut next=route("next",1);next.selected=false;next.native.as_mut().unwrap().active=false;
 reset(vec![route("codex",1),next]);
 assert!(select(Frontend{id:1,cols:100,rows:30},Some("next")));
 let r=ROUTES.lock();assert!(!r.routes[0].selected);assert!(r.routes[0].native.as_ref().unwrap().input.is_empty());
 assert!(r.routes[1].selected);assert_eq!(r.routes[1].screen.dimensions(),(100,30));
}
#[test]fn denied_route_leaves_input_and_identity_untouched() {
 reset(vec![route("denied",1)]);
 assert!(!select(Frontend{id:1,cols:100,rows:30},Some("denied")));
 let r=ROUTES.lock();assert_eq!(r.routes[0].screen.dimensions(),(80,24));assert_eq!(r.routes[0].surface_generation,7);
 assert_eq!(r.routes[0].native.as_ref().unwrap().input.len(),9);assert!(r.changes.is_empty());
}
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    tui = ROOT / 'src/shell3/tui.rs'
    ssh = ROOT.parent / 'TRUEOS-Buildins/ssh/src/terminal.rs'
    with tempfile.TemporaryDirectory(prefix='trueos-surface-resize-') as temporary:
        work = Path(temporary)
        (work / 'lib.rs').write_text(HARNESS.replace('@SSH@', str(ssh)).replace('@SELECT@', function(tui.read_text(), 'select')))
        (work / 'Cargo.toml').write_text('''[package]
name="trueos-surface-resize-proof"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
spin="=0.10.1"
zeroize={version="1",features=["alloc"]}
tokio={version="=1.54.1",features=["rt","time","macros"]}
trueos-terminal={path=''' + json.dumps(str(ROOT / 'crates/trueos-terminal')) + '''}
''')
        checks = []
        for profile, flags in [('debug', []), ('release', ['--release'])]:
            command = ['rustup', 'run', TOOLCHAIN, 'cargo', 'test', '--offline', '--lib', '--target', 'x86_64-unknown-linux-gnu', *flags, '--', '--test-threads=1']
            result = subprocess.run(command, cwd=work, env=environment(out / 'target'), text=True, capture_output=True, timeout=180)
            log = out / (profile + '.log')
            log.write_text(result.stdout + result.stderr)
            if result.returncode:
                raise RuntimeError(log.read_text()[-10000:])
            passed = int(re.search(r'test result: ok\. (\d+) passed', result.stdout)[1])
            assert passed == 14, passed
            checks.append({'profile': profile, 'passed': passed, 'log': str(log)})
    (out / 'receipt.json').write_text(json.dumps({'status': 'passed', 'boundary': __doc__, 'checks': checks, 'sources': [info(tui), info(ssh)]}, indent=2) + '\n')
    print(out / 'receipt.json')


if __name__ == '__main__':
    main()
