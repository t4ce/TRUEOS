#!/usr/bin/env python3
"""Run production borrowed-import retirement against exact PTE ownership stubs."""

from pathlib import Path
import subprocess
import tempfile

from test_edit_life_native import function


ROOT = Path(__file__).resolve().parents[2]

FIXTURE = r'''
#![allow(dead_code)]
use std::{collections::BTreeMap, sync::{Mutex, atomic::{AtomicBool, Ordering}}};
#[derive(Clone,Copy)] struct DirectRcsState;
#[derive(Clone,Copy)] struct GpgpuRgba8Surface { gpu:u64, phys:u64, bytes:usize }
struct Runtime { pending:Option<usize> }
struct SpinMutex<T>(Mutex<T>);
impl<T> SpinMutex<T> {
    const fn new(value:T)->Self { Self(Mutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
static EDIT_LIFE_RCS_SUBMIT_RUNTIME:SpinMutex<Runtime> = SpinMutex::new(Runtime{pending:None});
static QUARANTINED:AtomicBool = AtomicBool::new(false);
static FLUSH_OK:AtomicBool = AtomicBool::new(true);
static PAGES:Mutex<BTreeMap<u64,(u64,u32)>> = Mutex::new(BTreeMap::new());
static EVENTS:Mutex<Vec<(&str,u64,u64,usize)>> = Mutex::new(Vec::new());
fn edit_life_rcs_context_is_quarantined()->bool { QUARANTINED.load(Ordering::Relaxed) }
fn direct_rcs_unmap_ppgtt_region_exact(_:DirectRcsState,gpu:u64,phys:u64,bytes:usize)->bool {
    assert_eq!(bytes%4096,0);
    EVENTS.lock().unwrap().push(("unmap",gpu,phys,bytes));
    let mut pages=PAGES.lock().unwrap();
    // The real exact unmapper preflights the entire range before changing any
    // leaf. Missing leaves are already retired; a different physical owner
    // must be preserved regardless of its cache attribute.
    if (0..bytes).step_by(4096).any(|offset| pages.get(&(gpu+offset as u64))
        .is_some_and(|&(mapped,_)| mapped!=phys+offset as u64)) { return false; }
    for offset in (0..bytes).step_by(4096) { pages.remove(&(gpu+offset as u64)); }
    true
}
fn direct_rcs_flush_ppgtt_pte_range(_:DirectRcsState,gpu:u64,bytes:usize)->bool {
    EVENTS.lock().unwrap().push(("flush",gpu,0,bytes));
    FLUSH_OK.load(Ordering::Relaxed)
}
fn map(surface:GpgpuRgba8Surface,pat:u32)->bool {
    let bytes=(surface.bytes+4095)&!4095;
    let mut pages=PAGES.lock().unwrap();
    // Preserve the production PAT policy: a live VA cannot silently change
    // between source/WB and destination/UC roles, even for new physical pages.
    if (0..bytes).step_by(4096).any(|offset| pages.get(&(surface.gpu+offset as u64))
        .is_some_and(|&(_,old_pat)| old_pat!=pat)) { return false; }
    for offset in (0..bytes).step_by(4096) {
        pages.insert(surface.gpu+offset as u64,(surface.phys+offset as u64,pat));
    }
    true
}
@HELPER@
const SOURCE:GpgpuRgba8Surface=GpgpuRgba8Surface{gpu:0x12000000,phys:0x20000000,bytes:4097};
const DESTINATION:GpgpuRgba8Surface=GpgpuRgba8Surface{gpu:0x12010000,phys:0x20010000,bytes:4096};
fn reset() {
    PAGES.lock().unwrap().clear(); EVENTS.lock().unwrap().clear();
    QUARANTINED.store(false,Ordering::Relaxed); FLUSH_OK.store(true,Ordering::Relaxed);
    EDIT_LIFE_RCS_SUBMIT_RUNTIME.lock().pending=None;
}
#[test]
fn source_and_destination_can_exchange_recycled_addresses_without_pat_mutation() {
    reset();
    let owned=GpgpuRgba8Surface{gpu:0xA000000,phys:0x10000000,bytes:4096};
    assert!(map(owned,0)); assert!(map(SOURCE,0)); assert!(map(DESTINATION,3));
    let next_source=GpgpuRgba8Surface{gpu:DESTINATION.gpu,phys:0x30010000,bytes:4096};
    let next_destination=GpgpuRgba8Surface{gpu:SOURCE.gpu,phys:0x30000000,bytes:8192};
    assert!(!map(next_source,0),"stale PAT3 must block a PAT0 source import");
    assert!(!map(next_destination,3),"stale PAT0 must block a PAT3 destination import");
    assert!(edit_life_retire_imports(DirectRcsState,&[SOURCE,DESTINATION]));
    assert_eq!(*EVENTS.lock().unwrap(),[
        ("unmap",SOURCE.gpu,SOURCE.phys,8192),("flush",SOURCE.gpu,0,8192),
        ("unmap",DESTINATION.gpu,DESTINATION.phys,4096),("flush",DESTINATION.gpu,0,4096)]);
    assert_eq!(PAGES.lock().unwrap().len(),1,"owned font/world/kernel mappings stay resident");
    assert!(map(next_source,0)); assert!(map(next_destination,3));
}
#[test]
fn pending_or_quarantined_lane_keeps_imports_and_never_edits_ptes() {
    reset(); assert!(map(SOURCE,0));
    EDIT_LIFE_RCS_SUBMIT_RUNTIME.lock().pending=Some(1);
    assert!(!edit_life_retire_imports(DirectRcsState,&[SOURCE]));
    EDIT_LIFE_RCS_SUBMIT_RUNTIME.lock().pending=None;
    QUARANTINED.store(true,Ordering::Relaxed);
    assert!(!edit_life_retire_imports(DirectRcsState,&[SOURCE]));
    assert!(EVENTS.lock().unwrap().is_empty());
    assert_eq!(PAGES.lock().unwrap().len(),2);
}
#[test]
fn wrong_physical_owner_is_preserved_without_a_flush_or_pat_change() {
    reset(); assert!(map(SOURCE,0));
    let wrong=GpgpuRgba8Surface{phys:SOURCE.phys+4096,..SOURCE};
    assert!(!edit_life_retire_imports(DirectRcsState,&[wrong]));
    assert_eq!(*EVENTS.lock().unwrap(),[("unmap",SOURCE.gpu,wrong.phys,8192)]);
    assert_eq!(PAGES.lock().unwrap()[&SOURCE.gpu],(SOURCE.phys,0));
}
#[test]
fn clean_destination_rejection_accepts_absent_destination_leaves() {
    reset(); assert!(map(SOURCE,0));
    assert!(edit_life_retire_imports(DirectRcsState,&[SOURCE,DESTINATION]));
    assert!(PAGES.lock().unwrap().is_empty());
    assert_eq!(EVENTS.lock().unwrap().len(),4);
}
#[test]
fn destination_publication_failure_retires_its_written_leaves_too() {
    reset(); assert!(map(SOURCE,0)); assert!(map(DESTINATION,3));
    // Destination map can install leaves, then fail while publishing their
    // cache lines. No walker was submitted, so both leases remain safe to retire.
    assert!(edit_life_retire_imports(DirectRcsState,&[SOURCE,DESTINATION]));
    assert!(PAGES.lock().unwrap().is_empty());
    assert_eq!(EVENTS.lock().unwrap().len(),4);
}
#[test]
fn unpublishable_removal_stops_before_next_import_and_reports_failure() {
    reset(); assert!(map(SOURCE,0)); assert!(map(DESTINATION,3));
    FLUSH_OK.store(false,Ordering::Relaxed);
    assert!(!edit_life_retire_imports(DirectRcsState,&[SOURCE,DESTINATION]));
    assert_eq!(EVENTS.lock().unwrap().len(),2);
    assert!(PAGES.lock().unwrap().contains_key(&DESTINATION.gpu));
}
#[test]
fn overflow_or_empty_extent_never_unmaps_memory() {
    reset();
    for bytes in [0,usize::MAX] {
        assert!(!edit_life_retire_imports(DirectRcsState,&[GpgpuRgba8Surface{bytes,..SOURCE}]));
    }
    assert!(EVENTS.lock().unwrap().is_empty());
}
'''


def main() -> None:
    native = (ROOT / "src/intel/gpgpu/operations/edit_life.rs").read_text()
    helper = function(native, "edit_life_retire_imports")
    render = function(native, "render")
    assert "source_mapped && !edit_life_retire_imports(state, &[source, dst])" in render
    assert "edit_life_retire_imports(state, &[source, dst])" in render
    assert 'quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "edit-life-render-incomplete")' in render
    assert "direct_rcs_map_ppgtt" not in helper, "Retirement may not rewrite PAT or install an alias"
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-imports-") as temporary:
        source = Path(temporary) / "imports.rs"
        binary = Path(temporary) / "imports"
        source.write_text(FIXTURE.replace("@HELPER@", helper))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
