#!/usr/bin/env python3
"""Validate the real dense DirectRCS tree using an independent four-level walk."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
C = "src/intel/gpgpu/rcs/constants.rs"
R = "src/intel/gpgpu/rcs/runtime.rs"


def impl_item(name: str) -> str:
    text = (ROOT / R).read_text()
    start = text.index(f"impl {name} {{")
    opening = text.index("{", start)
    depth, end = 1, opening + 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    return text[start:end]


source = r'''
#![allow(dead_code)]
use std::cell::RefCell;
thread_local! {static FLUSHES:RefCell<Vec<(usize,usize)>>=const {RefCell::new(Vec::new())};}
fn dma_flush(virt:*mut u8,bytes:usize) {FLUSHES.with(|f|f.borrow_mut().push((virt as usize,bytes)));}
const GEN8_PAGE_PRESENT:u64=1;
#[macro_export]macro_rules! log_error {($($tt:tt)*)=>{()}}
mod direct {
use std::sync::atomic::{AtomicU64,Ordering};
static DIRECT_RCS_PPGTT_POLICY_REJECTIONS:AtomicU64=AtomicU64::new(0);
'''
for name in (
    "DIRECT_RCS_PPGTT_PD_COUNT", "DIRECT_RCS_PPGTT_PT_COUNT",
    "DIRECT_RCS_PPGTT_PML4_OFFSET", "DIRECT_RCS_PPGTT_PDPT_OFFSET",
    "DIRECT_RCS_PPGTT_PD_OFFSET", "DIRECT_RCS_PPGTT_PT_OFFSET",
    "DIRECT_RCS_PPGTT_BYTES", "DIRECT_RCS_PPGTT_LIMIT_BYTES",
    "DIRECT_RCS_RING_BYTES", "DIRECT_RCS_CONTEXT_BYTES", "DIRECT_RCS_BATCH_BYTES",
    "DIRECT_RCS_RESULT_BYTES", "GEN8_PAGE_RW", "GEN8_PAGE_PWT", "GEN8_PAGE_PCD",
):
    source += constant(C, name) + "\n"
for name in (
    "DirectRcsState", "DirectRcsGpuVa", "DirectRcsPpgttMapResult", "ExactPpgttPageMap",
    "direct_rcs_rebuild_ppgtt", "direct_rcs_map_ppgtt_region",
    "direct_rcs_map_ppgtt_region_and_publish", "direct_rcs_update_ppgtt_region",
    "direct_rcs_flush_ppgtt_pte_range", "direct_rcs_ppgtt_pte_flags",
    "validate_exact_ppgtt_page_map", "exact_ppgtt_pte_offset",
    "exact_ppgtt_phys_pages_are_valid", "direct_rcs_ppgtt_cache_policy_compatible",
    "direct_rcs_map_ppgtt_pages_and_publish", "direct_rcs_unmap_ppgtt_region_exact",
    "tests",
):
    source += item(R, name) + "\n"
source += impl_item("DirectRcsPpgttMapResult")
source += r'''
const GUARD:u64=0xfeedfacec0ffee00;
struct Fixture {words:Vec<u64>,state:DirectRcsState}
impl Fixture {
    fn new()->Self {
        let mut words=vec![GUARD;DIRECT_RCS_PPGTT_BYTES/8+2];
        let mut state:DirectRcsState=unsafe{core::mem::zeroed()};
        state.ppgtt_virt=unsafe{words.as_mut_ptr().add(1).cast()};
        state.ppgtt_phys=0x123450000;
        state.ring_phys=0x10000000;state.context_phys=0x11000000;
        state.batch_phys=0x12000000;state.result_phys=0x13000000;
        state.gpu_va=DirectRcsGpuVa{ring:0x1b00000,context:0x1b10000,
            batch:0x1c00000,result:0x1b40000,job_slots:1,map_general_auxiliary:true};
        assert!(direct_rcs_rebuild_ppgtt(state));Self{words,state}
    }
    fn raw(&self,offset:usize)->u64 {self.words[offset/8+1]}
    fn set(&mut self,offset:usize,value:u64) {self.words[offset/8+1]=value;}
    fn guards(&self) {assert_eq!(self.words[0],GUARD);assert_eq!(*self.words.last().unwrap(),GUARD);}
    // Walk from the published root as hardware does, without using any dense
    // layout constant, mapping helper, or offset calculation under test.
    fn walk(&self,gpu:u64)->Option<(u64,u64)> {
        let mut table=self.state.ppgtt_phys;
        for shift in [39,30,21,12] {
            assert_eq!(table&4095,0);
            let offset=usize::try_from(table.checked_sub(self.state.ppgtt_phys)?).ok()?;
            assert!(offset+4096<=DIRECT_RCS_PPGTT_BYTES);
            let raw=self.raw(offset+((gpu>>shift)&511) as usize*8);
            if raw&1==0 {return None;}
            if shift==12 {return Some(((raw&!4095)|(gpu&4095),raw&4095));}
            assert_eq!(raw&0x80,0,"parent must not become a large-page leaf");
            table=raw&!4095;
        }
        unreachable!()
    }
    fn flushes(&self)->Vec<(usize,usize)> {
        let base=self.state.ppgtt_virt as usize;
        crate::FLUSHES.with(|f|f.borrow().iter().map(|&(p,b)|(p-base,b)).collect())
    }
}
fn clear_flushes() {crate::FLUSHES.with(|f|f.borrow_mut().clear());}
#[test]fn rebuild_publishes_two_pd_pages_and_1024_disjoint_pt_pages() {
    clear_flushes();let f=Fixture::new();f.guards();
    assert_eq!(DIRECT_RCS_PPGTT_LIMIT_BYTES,0x80000000);
    assert_eq!(DIRECT_RCS_PPGTT_BYTES,1028*4096);
    assert_eq!(f.raw(0),f.state.ppgtt_phys+4096|0x1b);
    for pdpt in 0..2 {
        assert_eq!(f.raw(4096+pdpt*8),f.state.ppgtt_phys+8192+pdpt as u64*4096|0x1b);
    }
    for index in 2..512 {assert_eq!(f.raw(4096+index*8),0);}
    for index in 0..1024 {
        assert_eq!(f.raw(8192+index*8),f.state.ppgtt_phys+16384+index as u64*4096|0x1b);
    }
    assert_eq!(f.walk(f.state.gpu_va.ring),Some((f.state.ring_phys,3)));
    assert_eq!(f.walk(f.state.gpu_va.context),Some((f.state.context_phys,3)));
    assert_eq!(f.walk(f.state.gpu_va.batch),Some((f.state.batch_phys,3)));
    assert_eq!(f.walk(f.state.gpu_va.result),Some((f.state.result_phys,3)));
    assert_eq!(f.walk(0x40000000),None);assert_eq!(f.walk(0x80000000),None);
    assert_eq!(f.flushes(),vec![(0,DIRECT_RCS_PPGTT_BYTES)]);
}
#[test]fn low_and_high_gpu_addresses_keep_the_same_physical_and_pat_contract() {
    clear_flushes();let f=Fixture::new();clear_flushes();
    for (index,gpu) in [0x12000000,0x3ffff000,0x40000000,0x5ffff000,0x7ffff000].into_iter().enumerate() {
        let flags=if index%2==0 {3} else {0x1b};let phys=0x200000000+index as u64*4096;
        assert!(direct_rcs_map_ppgtt_region_and_publish(f.state,gpu,phys,4096,flags));
        assert_eq!(f.walk(gpu+4095),Some((phys+4095,flags)));
        let flushes=f.flushes();assert_eq!(flushes.last().unwrap().1,8);
        assert_eq!(flushes.last().unwrap().0,16384+(gpu>>12) as usize*8);
    }
    f.guards();
}
#[test]fn mapping_and_unmapping_cross_the_pdpt_boundary_without_touching_neighbors() {
    clear_flushes();let f=Fixture::new();clear_flushes();
    let gpu=0x3ffff000;let phys=0x200000000;
    assert!(direct_rcs_map_ppgtt_region_and_publish(f.state,gpu,phys,8192,0x1b));
    assert_eq!(f.walk(gpu),Some((phys,0x1b)));assert_eq!(f.walk(gpu+4096),Some((phys+4096,0x1b)));
    assert_eq!(f.walk(gpu-4096),None);assert_eq!(f.walk(gpu+8192),None);
    assert_eq!(f.flushes(),vec![(16384+(gpu>>12) as usize*8,16)]);
    assert!(direct_rcs_unmap_ppgtt_region_exact(f.state,gpu,phys,8192));
    assert!(direct_rcs_flush_ppgtt_pte_range(f.state,gpu,8192));
    assert_eq!(f.walk(gpu),None);assert_eq!(f.walk(gpu+4096),None);f.guards();
}
#[test]fn exact_noncontiguous_pages_cross_pdpt_and_unchanged_maps_do_not_republish() {
    clear_flushes();let f=Fixture::new();clear_flushes();let gpu=0x3ffff000;
    let pages=[0x234560000,0x765430000];
    assert!(direct_rcs_map_ppgtt_pages_and_publish(f.state,gpu,&pages,3));
    assert_eq!(f.walk(gpu),Some((pages[0],3)));assert_eq!(f.walk(gpu+4096),Some((pages[1],3)));
    assert_eq!(f.flushes(),vec![(16384+(gpu>>12)as usize*8,16)]);clear_flushes();
    assert!(direct_rcs_map_ppgtt_pages_and_publish(f.state,gpu,&pages,3));assert!(f.flushes().is_empty());
    f.guards();
}
#[test]fn invalid_ranges_and_physical_overflow_leave_all_leaves_and_guards_unchanged() {
    clear_flushes();let f=Fixture::new();clear_flushes();let before=f.words.clone();
    for (gpu,phys,len) in [(0x80000000,0x200000000,4096),(0x7ffff000,0x200000000,8192),
        (u64::MAX&!4095,0x200000000,8192),(0x40000000,u64::MAX&!4095,8192),
        (0x40000001,0x200000000,4096),(0x40000000,0x200000001,4096),(0,0x200000000,usize::MAX)] {
        assert!(!direct_rcs_map_ppgtt_region_and_publish(f.state,gpu,phys,len,3));
    }
    for (gpu,pages) in [(0x7ffff000,2),(0x80000000,1),(0,usize::MAX),(u64::MAX&!4095,2)] {
        assert!(validate_exact_ppgtt_page_map(gpu,pages).is_none());
    }
    assert!(!direct_rcs_flush_ppgtt_pte_range(f.state,0x7ffff000,8192));
    assert!(!direct_rcs_flush_ppgtt_pte_range(f.state,0x40000000,usize::MAX));
    assert_eq!(f.words,before);assert!(f.flushes().is_empty());f.guards();
}
#[test]fn policy_and_unmap_rejection_preflight_both_pd_pages_before_any_write() {
    clear_flushes();let f=Fixture::new();let gpu=0x3ffff000;
    assert!(direct_rcs_map_ppgtt_region(f.state,gpu,0x200000000,4096,3));
    assert!(direct_rcs_map_ppgtt_region(f.state,gpu+4096,0x200002000,4096,0x1b));
    let before=f.words.clone();clear_flushes();
    assert!(!direct_rcs_map_ppgtt_region_and_publish(f.state,gpu,0x300000000,8192,3));
    assert!(!direct_rcs_map_ppgtt_pages_and_publish(f.state,gpu,&[0x300000000,0x400000000],3));
    assert!(!direct_rcs_unmap_ppgtt_region_exact(f.state,gpu,0x200000000,8192));
    assert_eq!(f.words,before);assert!(f.flushes().is_empty());f.guards();
}
}
'''
with tempfile.TemporaryDirectory(prefix="trueos-direct-rcs-topology-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True, cwd=ROOT)
    subprocess.run([str(path / "tests"), "--test-threads=1"], check=True)
