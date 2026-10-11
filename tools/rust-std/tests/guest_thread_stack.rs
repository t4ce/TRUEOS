#![allow(dead_code)]
use std::sync::OnceLock;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
const PAGE_SIZE_4K: usize = 4096;
const GUEST_HIGH_IMAGE_PT_COUNT: usize = 32;
const PT_ENTRY_PRESENT: u64 = 1;
const PT_ENTRY_WRITABLE: u64 = 2;
const PT_ENTRY_LARGE_PAGE: u64 = 128;
const PT_ENTRY_NO_EXECUTE: u64 = 1 << 63;
#[repr(C, align(4096))]
struct GuestPage([u64;512]);
struct GuestTables { pml4: GuestPage, image_pts: [GuestPage;GUEST_HIGH_IMAGE_PT_COUNT] }
static TABLES: [OnceLock<usize>;2] = [const { OnceLock::new() };2];
static GUEST_DYNAMIC_PT_NEXT: [Mutex<usize>;2] = [const { Mutex::new(0) };2];
fn guest_tables_ptr_for_vm(vm:u8) -> Result<*mut GuestTables,&'static str> {
    let cell = TABLES.get(vm as usize).ok_or("VM")?;
    Ok(*cell.get_or_init(|| {
        let layout = std::alloc::Layout::new::<GuestTables>();
        unsafe { std::alloc::alloc_zeroed(layout) as usize }
    }) as *mut GuestTables)
}
fn host_va_to_pa(va:u64) -> Option<u64> { Some(va) }
unsafe fn zero_guest_page(page:*mut [u64;512]) { unsafe { (*page).fill(0) }; }
fn pml4_index(va:u64)->usize { ((va>>39)&511) as usize }
fn pdpt_index(va:u64)->usize { ((va>>30)&511) as usize }
fn pd_index(va:u64)->usize { ((va>>21)&511) as usize }
fn pt_index(va:u64)->usize { ((va>>12)&511) as usize }
mod phys {pub fn phys_to_virt(pa:usize)->usize {pa}}
mod pci {pub mod mmio {pub fn with_page_table_lock<T>(f:impl FnOnce()->T)->T {f()}}}
mod allocators {
    pub struct HeapStats {pub initialized:bool,pub phys_start:usize,pub heap_start:usize,pub heap_end:usize}
    pub fn hv_guest_heap_stats_if_configured(vm:u8)->Option<HeapStats> {
        (vm<2).then_some(HeapStats {initialized:true,phys_start:0x10000000+vm as usize*0x1000000,heap_start:0,heap_end:0x1000000})
    }
}
mod thread_stack;
fn entry(vm:u8,va:u64)->u64 {
    let mut page = unsafe { &(*guest_tables_ptr_for_vm(vm).unwrap()).pml4.0 };
    for index in [pml4_index(va),pdpt_index(va),pd_index(va)] {
        let value = page[index];
        if value & PT_ENTRY_PRESENT == 0 {return 0;}
        page = unsafe { &*((value&0x000F_FFFF_FFFF_F000) as *const [u64;512]) };
    }
    page[pt_index(va)]
}
#[test]
fn private_stack_aliases_have_nx_guards_and_verified_retirement() {
    let base=0xFFFF_FE00_0020_0000;
    let physical=0x10002000;
    thread_stack::map_thread_stack(0,base,physical,8192).unwrap();
    assert_eq!(entry(0,base),physical|PT_ENTRY_PRESENT|PT_ENTRY_WRITABLE|PT_ENTRY_NO_EXECUTE);
    assert_eq!(entry(0,base+4096),(physical+4096)|PT_ENTRY_PRESENT|PT_ENTRY_WRITABLE|PT_ENTRY_NO_EXECUTE);
    assert_eq!(entry(0,base-4096),0);
    assert_eq!(entry(0,base+8192),0);
    assert_eq!(entry(1,base),0,"another VM must not inherit a worker stack");
    assert!(thread_stack::map_thread_stack(1,base,physical,8192).is_err(),"backing belongs to VM 0");
    assert!(thread_stack::map_thread_stack(0,base,physical,8192).is_err());
    assert!(thread_stack::unmap_thread_stack(0,base,physical+4096,8192).is_err());
    assert_ne!(entry(0,base),0,"failed ownership validation must preserve the entire mapping");
    thread_stack::unmap_thread_stack(0,base,physical,8192).unwrap();
    assert_eq!(entry(0,base),0);
    assert_eq!(entry(0,base+4096),0);
    assert!(thread_stack::unmap_thread_stack(0,base,physical,8192).is_err());
    *GUEST_DYNAMIC_PT_NEXT[1].lock()=GUEST_HIGH_IMAGE_PT_COUNT;
    assert!(thread_stack::map_thread_stack(1,base,0x11002000,8192).is_err());
    assert_eq!(entry(1,base),0,"table exhaustion must not publish a partial stack");
}
