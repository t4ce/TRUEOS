#!/usr/bin/env python3
"""Exercise production app maximize routing, restore, and owner isolation."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item

HARNESS = r'''
#![allow(dead_code)]
use std::cell::RefCell;
type WindowOwner = u32;
type WindowId = u32;
#[derive(Clone, Copy, Debug, PartialEq)] enum WindowDockTarget { Maximize, Half }
#[derive(Debug, PartialEq)] enum WindowBrokerError { InteractionDenied, EmptyExtent }
#[derive(Clone, Copy)] struct WindowSnapshot { maximized:bool, dock_target:Option<WindowDockTarget> }
thread_local! {
    static STATE:RefCell<WindowSnapshot> = const { RefCell::new(WindowSnapshot{maximized:false,dock_target:None}) };
    static CALLS:RefCell<Vec<Option<WindowDockTarget>>> = const { RefCell::new(Vec::new()) };
    static OUTPUT:RefCell<Option<(u32,u32)>> = const { RefCell::new(Some((2560,1440))) };
}
fn output_dimensions()->Option<(u32,u32)> { OUTPUT.with(|v| *v.borrow()) }
mod production {
    use super::*;
    fn window_snapshot(owner:WindowOwner,id:WindowId)->Option<WindowSnapshot> {
        (owner==7 && id==8).then(|| STATE.with(|s| *s.borrow()))
    }
    fn change_window_dock(owner:WindowOwner,id:WindowId,target:Option<WindowDockTarget>,w:u32,h:u32,_:Option<()>,_:Option<()>)->Result<(),WindowBrokerError> {
        assert_eq!((owner,id,w,h),(7,8,2560,1440));
        CALLS.with(|s|s.borrow_mut().push(target));
        STATE.with(|s|*s.borrow_mut()=WindowSnapshot{maximized:target==Some(WindowDockTarget::Maximize),dock_target:target});
        Ok(())
    }
    ITEMS
}
#[test] fn maximize_then_restore_share_the_dock_transaction() {
    production::set_window_maximized(7,8,true).unwrap();
    production::set_window_maximized(7,8,false).unwrap();
    CALLS.with(|s|assert_eq!(*s.borrow(),vec![Some(WindowDockTarget::Maximize),None]));
}
#[test] fn repeated_requests_and_plain_window_restore_are_noops() {
    production::set_window_maximized(7,8,false).unwrap();
    production::set_window_maximized(7,8,true).unwrap();
    production::set_window_maximized(7,8,true).unwrap();
    CALLS.with(|s|assert_eq!(s.borrow().len(),1));
}
#[test] fn user_docked_maximize_is_visible_to_app_requests() {
    STATE.with(|s|*s.borrow_mut()=WindowSnapshot{maximized:true,dock_target:Some(WindowDockTarget::Maximize)});
    production::set_window_maximized(7,8,true).unwrap();
    production::set_window_maximized(7,8,false).unwrap();
    CALLS.with(|s|assert_eq!(*s.borrow(),vec![None]));
}
#[test] fn other_owners_and_missing_output_cannot_mutate_geometry() {
    assert_eq!(production::set_window_maximized(9,8,true),Err(WindowBrokerError::InteractionDenied));
    OUTPUT.with(|s|*s.borrow_mut()=None);
    assert_eq!(production::set_window_maximized(7,8,true),Err(WindowBrokerError::EmptyExtent));
    CALLS.with(|s|assert!(s.borrow().is_empty()));
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='ui4-app-maximize-') as directory:
        source = Path(directory) / 'tests.rs'
        source.write_text(HARNESS.replace('ITEMS', item('src/ui4/window_broker.rs', 'set_window_maximized')))
        binary = Path(directory) / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

if __name__ == '__main__':
    main()
