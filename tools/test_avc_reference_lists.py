#!/usr/bin/env python3
"""Replay the demo AVC streams through production DPB/list preparation on the host.

Requires ffmpeg only to unwrap MP4/add AUDs; parsing and DPB handling are TRUEOS.
No GPU execution is simulated or claimed by this test.
"""
import re
import subprocess
import tempfile
from pathlib import Path
from test_clip_position3_uv_texture import ROOT, constant, item


def main():
    path = 'src/intel/media/hw_pic.rs'
    source = (ROOT / path).read_text()
    harness = '#![allow(dead_code)]\nextern crate alloc;\n'
    harness += f'#[path="{ROOT}/src/intel/media/h264_cmd.rs"] pub mod h264_cmd;\n'
    harness += 'mod intel { pub use crate::h264_cmd as xelp_media_avc_decode_recipe; }\n'
    harness += constant(path, 'AVC_DPB_RETAINED_REFS') + '\n'
    for name in ['AvcDpbEntry', 'AvcDpbState', 'AvcDpbProbeLayout',
                 'avc_dpb_entry_older', 'avc_frame_num_wrap', 'avc_apply_ref_list_modifications',
                 'avc_prepare_reference_state', 'avc_commit_decoded_reference',
                 'avc_dmv_region_offset', 'avc_dmv_slot_gpu_addr']:
        harness += item(path, name) + '\n'
    harness += re.search(r'^impl AvcDpbState \{.*?^}\n', source, re.M | re.S).group()
    harness += item('tools/avc_b_slice_probe.rs', 'start_codes')
    harness += r'''
use std::sync::atomic::{AtomicU16, Ordering};
struct Lock<T>(std::sync::Mutex<T>);
impl<T> Lock<T> { fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() } }
static AVC_DPB: [Lock<AvcDpbState>; 1] = [Lock(std::sync::Mutex::new(AvcDpbState::new()))];
static AVC_PRESENTATION_HOLDS: [AtomicU16; 1] = [AtomicU16::new(0)];
fn align_up_usize(v: usize, a: usize) -> usize { (v + a - 1) & !(a - 1) }
fn main() {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).unwrap();
    let starts = start_codes(&bytes);
    let (mut sps, mut pps, mut au) = (Vec::new(), Vec::new(), Vec::new());
    let mut pictures = Vec::new();
    for (i, (start, prefix)) in starts.iter().copied().enumerate() {
        let end = starts.get(i + 1).map(|x| x.0).unwrap_or(bytes.len());
        let nal = &bytes[start..end];
        match bytes[start + prefix] & 31 {
            7 => sps = nal.to_vec(), 8 => pps = nal.to_vec(),
            9 => { if !au.is_empty() { pictures.push(std::mem::take(&mut au)); } au.extend(nal); }
            _ => au.extend(nal),
        }
    }
    if !au.is_empty() { pictures.push(au); }
    // Independent B defaults, duplicate L0 entries, and explicitly identical
    // modified lists. The default L1 swap must not happen after modification.
    let first = [&sps[..], &pps[..], &pictures[0][..]].concat();
    let mut probe = h264_cmd::parse_annexb_single_picture_plan(&first).unwrap();
    probe.picture.idr_pic = false;
    probe.picture.pic_order_cnt_type = 1;
    probe.picture.frame_num = 3;
    probe.picture.top_field_order_cnt = 20;
    probe.picture.log2_max_frame_num_minus4 = 0;
    probe.slice.class = h264_cmd::AvcSliceClass::B;
    probe.slice.num_ref_idx_l0_active_minus1 = 2;
    probe.slice.num_ref_idx_l1_active_minus1 = 2;
    for i in 0..3 {
        AVC_DPB[0].lock().entries[i] = Some(AvcDpbEntry {
            slot: i, frame_store_id: i as u8, frame_num: i as u16,
            top_field_order_cnt: i as i32 * 4, bottom_field_order_cnt: i as i32 * 4,
        });
    }
    probe.slice.ref_list_modifications_l0_count = 2;
    probe.slice.ref_list_modifications_l0[0] = h264_cmd::AvcRefListModification {
        modification_of_pic_nums_idc: 0, abs_diff_pic_num_minus1: 0,
    };
    probe.slice.ref_list_modifications_l0[1] = h264_cmd::AvcRefListModification {
        modification_of_pic_nums_idc: 0, abs_diff_pic_num_minus1: 15,
    };
    let layout = AvcDpbProbeLayout { slot_count: 16, current_slot: 0, reference_slots: 8,
        slot_bytes: probe.resources.dest_surface.byte_len, current_gpu_addr: 0,
        first_reference_gpu_addr: 0, capacity_bytes: 1 << 28 };
    let (_, refs, _, _) = avc_prepare_reference_state(0, &mut probe, layout, 0x100000000, 0x200000000).unwrap();
    assert_eq!(&refs.l0[..3], &[2, 2, 1]);
    assert_eq!(&refs.l1[..3], &[1, 2, 0]);
    probe.slice.ref_list_modifications_l1 = probe.slice.ref_list_modifications_l0;
    probe.slice.ref_list_modifications_l1_count = 2;
    let (_, refs, _, _) = avc_prepare_reference_state(0, &mut probe, layout, 0x100000000, 0x200000000).unwrap();
    assert_eq!(&refs.l1[..3], &[2, 2, 1]);
    probe.slice.num_ref_idx_l0_active_minus1 = 16;
    assert_eq!(avc_prepare_reference_state(0, &mut probe, layout, 0x100000000, 0x200000000).unwrap_err(), -30);
    probe.slice.num_ref_idx_l0_active_minus1 = 2;
    probe.slice.ref_list_modifications_l0[0].abs_diff_pic_num_minus1 = 4;
    assert_eq!(avc_prepare_reference_state(0, &mut probe, layout, 0x100000000, 0x200000000).unwrap_err(), -36);
    let mut duplicates = 0;
    // Two passes also exercise IDR reset at the loop boundary.
    for lap in 0..2 {
        for (i, au) in pictures.iter().enumerate() {
            let frame = [&sps[..], &pps[..], &au[..]].concat();
            let mut plan = h264_cmd::parse_annexb_single_picture_plan(&frame).unwrap();
            let layout = AvcDpbProbeLayout {
                slot_count: 16, current_slot: 0, reference_slots: 8, slot_bytes: plan.resources.dest_surface.byte_len,
                current_gpu_addr: 0, first_reference_gpu_addr: 0, capacity_bytes: 1 << 28,
            };
            let (slot, refs, _, _) = avc_prepare_reference_state(0, &mut plan, layout, 0x100000000, 0x200000000)
                .unwrap_or_else(|e| panic!("lap={lap} picture={i} class={:?} error={e} l0={} mods={:?}",
                    plan.slice.class, plan.slice.num_ref_idx_l0_active_minus1 + 1,
                    &plan.slice.ref_list_modifications_l0[..plan.slice.ref_list_modifications_l0_count]));
            for list in [&refs.l0[..refs.l0_count], &refs.l1[..refs.l1_count]] {
                for (j, id) in list.iter().enumerate() {
                    assert!(refs.refs[*id as usize].is_some());
                    if list[..j].contains(id) { duplicates += 1; }
                }
            }
            h264_cmd::validate_long_format_single_picture(plan, refs)
                .unwrap_or_else(|e| panic!("lap={lap} picture={i} command validation: {e:?}"));
            avc_commit_decoded_reference(0, plan, slot);
        }
    }
    assert_eq!(pictures.len(), 1071);
    assert!(duplicates > 0);
    println!("pictures={} laps=2 duplicate_entries={duplicates} result=ok", pictures.len());
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-avc-refs-') as tmp:
        rs, exe = Path(tmp) / 'probe.rs', Path(tmp) / 'probe'
        rs.write_text(harness)
        subprocess.run(['rustc', '--edition=2021', '-Awarnings', str(rs), '-o', str(exe)], check=True)
        for size in (48, 128, 256):
            asset = ROOT.parent / f'TRUEOS-Picasso-Example/Assets/Video/DSC_1879_{size}_a.mp4'
            data = subprocess.check_output(['ffmpeg', '-v', 'error', '-i', str(asset), '-c:v', 'copy',
                '-bsf:v', 'h264_mp4toannexb,h264_metadata=aud=insert', '-f', 'h264', '-'])
            print(f'{size}px:', flush=True)
            subprocess.run([str(exe)], input=data, check=True)

if __name__ == '__main__':
    main()
