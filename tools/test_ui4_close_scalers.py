#!/usr/bin/env python3
"""Check production close geometry against the display scaler admission rule."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item


def main():
    source = r'''
#![allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)] enum WindowState { Ready, Closing }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct WindowPlacement { x: i32, y: i32, width: u32, height: u32, opacity: u8 }
#[derive(Clone, Copy, Debug)] struct Plane(usize);
impl Plane { fn slot(self) -> usize { self.0 } }
#[derive(Clone, Copy, Debug)]
struct WindowSnapshot { plane: Plane, state: WindowState, presentation_placement: WindowPlacement, session: u32 }
#[derive(Clone, Copy)] struct FrameRgbaView { width: u32, height: u32 }
mod compositor { use super::*;
'''
    source += item('src/ui4/compositor_service.rs', 'resolve_close_scaler_conflicts')
    source += '\npub(super) fn resolve(w: &mut [WindowSnapshot], v: &[FrameRgbaView]) -> usize { resolve_close_scaler_conflicts(w,v,2560,1440) }\n}\n'
    for name in ['PlaneScalerMode', 'PlaneScalerFlip', 'direct_plane_scaler_id', 'plane_scaler_flips_have_unique_bindings']:
        source += item('src/intel/display.rs', name)
    source += r'''
#[derive(Clone, Copy)] struct PlaneSurfaceFlip { scaler: Option<PlaneScalerFlip> }
fn window(slot: usize, state: WindowState, scaled: bool) -> WindowSnapshot {
    WindowSnapshot { plane: Plane(slot), state, session: slot as u32 + 100,
        presentation_placement: WindowPlacement { x: 100, y: 100,
            width: if scaled { 1050 } else { 1000 }, height: if scaled { 735 } else { 700 }, opacity: 128 } }
}
fn entries(windows: &[WindowSnapshot]) -> Vec<Option<PlaneSurfaceFlip>> {
    windows.iter().filter(|w| w.plane.slot() != 0).map(|w| {
        let p = w.presentation_placement;
        let mode = if p.width == 1000 && p.height == 700 { PlaneScalerMode::Detached }
            else { PlaneScalerMode::Scaled { scaler_id: direct_plane_scaler_id(w.plane.slot()).unwrap(),
                window_pos_reg:0, window_size_reg:0, hphase_reg:0, vphase_reg:0 } };
        Some(PlaneSurfaceFlip { scaler: Some(PlaneScalerFlip { pipe_slot:0, plane_slot:w.plane.slot(), mode }) })
    }).collect()
}
fn views(count: usize) -> Vec<FrameRgbaView> { vec![FrameRgbaView { width:1000, height:700 };count] }
#[test] fn four_independent_sessions_can_close_in_one_batch() {
    let mut windows: Vec<_> = (0..4).map(|slot| window(slot, WindowState::Closing, true)).collect();
    assert!(!plane_scaler_flips_have_unique_bindings(&entries(&windows)), "original conflict must reproduce");
    assert_eq!(compositor::resolve(&mut windows, &views(4)), 1);
    assert!(plane_scaler_flips_have_unique_bindings(&entries(&windows)));
    assert_eq!(windows[3].presentation_placement.width,1000);
    for (slot,w) in windows.iter().enumerate() { assert_eq!(w.session, slot as u32+100); assert_eq!(w.presentation_placement.opacity,128); }
}
#[test] fn late_tick_to_zero_alpha_still_has_a_valid_retirement_batch() {
    let mut windows: Vec<_> = (0..4).map(|slot| window(slot, WindowState::Closing, true)).collect();
    for w in &mut windows { w.presentation_placement.opacity=0; }
    compositor::resolve(&mut windows,&views(4));
    assert!(plane_scaler_flips_have_unique_bindings(&entries(&windows)));
    assert!(windows.iter().all(|w| w.presentation_placement.opacity==0 && w.state==WindowState::Closing));
}
#[test] fn live_peer_geometry_is_preserved_for_either_plane() {
    for closing in [1,3] {
        let peer=if closing==1 {3}else{1};
        let mut windows=vec![window(closing,WindowState::Closing,true),window(peer,WindowState::Ready,true)];
        let original=windows[1].presentation_placement;
        assert_eq!(compositor::resolve(&mut windows,&views(2)),1);
        assert_eq!(windows[1].presentation_placement,original);
        assert!(plane_scaler_flips_have_unique_bindings(&entries(&windows)));
    }
}
#[test] fn arbitration_does_not_depend_on_snapshot_order() {
    let mut windows=vec![window(3,WindowState::Closing,true),window(1,WindowState::Closing,true)];
    compositor::resolve(&mut windows,&views(2));
    assert_eq!(windows[0].presentation_placement.width,1000);
    assert_eq!(windows[1].presentation_placement.width,1050);
    assert!(plane_scaler_flips_have_unique_bindings(&entries(&windows)));
}
#[test] fn uncontested_close_keeps_its_puff() {
    let mut windows=vec![window(3,WindowState::Closing,true),window(1,WindowState::Ready,false)];
    assert_eq!(compositor::resolve(&mut windows,&views(2)),0);
    assert_eq!(windows[0].presentation_placement.width,1050);
    assert!(plane_scaler_flips_have_unique_bindings(&entries(&windows)));
}
#[test] fn independent_pipes_may_use_the_same_scaler_index() {
    let windows=vec![window(1,WindowState::Closing,true),window(3,WindowState::Closing,true)];
    let mut batch=entries(&windows);
    batch[1].as_mut().unwrap().scaler.as_mut().unwrap().pipe_slot=1;
    assert!(plane_scaler_flips_have_unique_bindings(&batch));
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-close-scalers-') as tmp:
        path=Path(tmp)/'tests.rs'; path.write_text(source)
        binary=Path(tmp)/'tests'
        subprocess.run(['rustc','--edition=2024','--test',str(path),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)


if __name__ == '__main__':
    main()
