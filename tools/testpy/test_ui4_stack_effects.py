#!/usr/bin/env python3
"""Exercise production Slot0 transition cancellation and SURFLIVE retirement."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]
BROKER = 'src/ui4/window_broker.rs'


def method(name):
    source = (ROOT / BROKER).read_text()
    start = source.index('    fn ' + name + '(')
    end = source.index('\n    }', start) + len('\n    }')
    return source[start:end] + '\n'


def main():
    source = r'''
#![allow(dead_code)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct FrameHandle(u64);
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct FrameReadLease { frame:FrameHandle,buffer_index:u8 }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowId(u32);
mod cursor_frame_inout { pub fn selection_strip_stack_changed() {} }
mod input_broker { pub fn notify_slot4_visual_change() {} }
mod broker {
use super::*;
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum WindowPlane { Primary,Universal(u8),Interaction }
impl WindowPlane { fn slot(self)->usize { match self { Self::Primary=>0,Self::Universal(s)=>s as usize,Self::Interaction=>4 } } }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum WindowState { Ready,Closing,Closed }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct DamageRegion;
impl DamageRegion { const FULL:Self=Self; }
'''
    for name in ['WindowPlacement', 'WindowOpenTransition', 'WindowCloseTransition',
                 'WindowTransitionRetirement']:
        source += item(BROKER, name)
    production = (ROOT / BROKER).read_text()
    start = production.index('impl WindowCloseTransition {')
    end = production.index('\n#[derive', start)
    source += production[start:end] + '\n'
    for name in ['CLOSE_TRANSITION_DURATION_MS', 'OPEN_TRANSITION_SHRINK_PER_MILLE',
                 'DIRECT_PLANE_CLOSE_WAVE_DURATION_MS', 'DIRECT_PLANE_OPEN_SHRINK_PER_MILLE']:
        source += constant(BROKER, name) + '\n'
    for name in ['close_transition_placement', 'clamp_transition_to_output',
                 'scaled_fade_transition_placement', 'open_transition',
                 'open_transition_placement', 'centered_scale_coordinate', 'next_serial']:
        source += item(BROKER, name)
    source += r'''
#[derive(Clone,Copy)] struct WindowSnapshot {
    plane:WindowPlane,presentation_placement:WindowPlacement,
}
mod compositor { use super::*;
'''
    source += item('src/ui4/compositor_service.rs', 'presentation_placement')
    source += r'''
}
#[derive(Clone,Copy)] struct WindowRecord {
    generation:u16,state:WindowState,plane:WindowPlane,placement:WindowPlacement,
    open_transition:Option<WindowOpenTransition>,close_transition:Option<WindowCloseTransition>,
    replacement_presentation:Option<WindowPlacement>,damage:Option<DamageRegion>,
    revision:u64,frame:FrameHandle,publish_serial:u64,
}
impl WindowRecord {
'''
    source += method('skip_stack_transitions') + method('presentation_placement')
    source += r'''
}
struct WindowBroker { windows:Vec<WindowRecord> }
fn unpack_handle(_:u32)->Result<(usize,u16),()> { Ok((0,1)) }
impl WindowBroker {
    fn mark_composition_changed(&mut self) {}
'''
    for name in ['set_window_plane', 'advance_close_transitions', 'acknowledge']:
        source += method(name)
    source += r'''
}
fn native()->WindowPlacement {
    WindowPlacement { x:-15,y:20,width:320,height:200,z:0,opacity:255,visible:true }
}
fn broker()->WindowBroker {
    WindowBroker { windows:vec![WindowRecord {
        generation:1,state:WindowState::Ready,plane:WindowPlane::Universal(1),placement:native(),
        open_transition:None,close_transition:None,replacement_presentation:None,
        damage:None,revision:0,frame:FrameHandle(7),publish_serial:0,
    }] }
}
fn close_transition(initial:WindowPlacement)->WindowCloseTransition {
    WindowCloseTransition {
        lease:FrameReadLease { frame:FrameHandle(7),buffer_index:0 },
        initial,native:native(),started_ms:1000,delay_ms:0,duration_ms:200,
        puff_per_mille:50,retire_frame:true,
    }
}
#[test] fn primary_open_is_native_and_lease_open_keeps_its_effect() {
    assert!(open_transition(native(),WindowPlane::Primary,0).is_none());
    assert_ne!(open_transition(native(),WindowPlane::Universal(1),0).unwrap().current,native());
}
#[test] fn demotion_cancels_open_at_native_geometry() {
    let mut b=broker();
    b.windows[0].open_transition=open_transition(native(),WindowPlane::Universal(1),0);
    b.set_window_plane(WindowId(1),WindowPlane::Primary);
    assert!(b.windows[0].open_transition.is_none());
    assert_eq!(b.windows[0].presentation_placement(),native());
}
#[test] fn active_close_demotion_waits_for_ack_at_native_geometry() {
    let mut b=broker();
    let scaled=close_transition_placement(native(),100,200,50);
    b.windows[0].state=WindowState::Closing;
    b.windows[0].placement=scaled;
    b.windows[0].close_transition=Some(WindowCloseTransition { delay_ms:100,..close_transition(scaled) });
    b.set_window_plane(WindowId(1),WindowPlane::Primary);
    let terminal=WindowPlacement { opacity:0,..native() };
    assert_eq!(b.windows[0].placement,terminal);
    assert!(b.advance_close_transitions(1000,Some((100,100))).is_empty());
    assert_eq!(b.windows[0].placement,terminal,"native clipping must survive a smaller output");
    assert!(b.acknowledge(WindowId(1),0));
    assert_eq!(b.advance_close_transitions(1001,None).len(),1);
    assert_eq!(b.windows[0].state,WindowState::Closed);
}
#[test] fn ordinary_lease_close_keeps_puff_and_acknowledged_retirement() {
    let mut b=broker();
    b.windows[0].state=WindowState::Closing;
    b.windows[0].close_transition=Some(close_transition(native()));
    assert!(b.advance_close_transitions(1200,None).is_empty());
    assert_eq!(b.windows[0].placement.opacity,0);
    assert_eq!(b.windows[0].placement.width,336);
    assert!(b.acknowledge(WindowId(1),0));
    assert_eq!(b.advance_close_transitions(1201,None).len(),1);
}
#[test] fn stack_presentation_uses_backing_extent_and_preserves_position_and_alpha() {
    for opacity in [0,128,255] {
        let requested=WindowPlacement { width:1000,height:800,opacity,..native() };
        let window=WindowSnapshot { plane:WindowPlane::Primary,presentation_placement:requested };
        assert_eq!(compositor::presentation_placement(window,321,203),
            WindowPlacement { width:321,height:203,..requested });
    }
}
#[test] fn lease_presentation_keeps_scaled_geometry_and_alpha() {
    let requested=WindowPlacement { width:1000,height:800,opacity:128,..native() };
    for slot in 1..=3 {
        let window=WindowSnapshot { plane:WindowPlane::Universal(slot),presentation_placement:requested };
        assert_eq!(compositor::presentation_placement(window,321,203),requested);
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-stack-effects-') as directory:
        path = Path(directory) / 'tests.rs'
        path.write_text(source)
        binary = Path(directory) / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
