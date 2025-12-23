use spin::Once;
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
const IST_STACK_SIZE: usize = 4096;

static TSS: Once<TaskStateSegment> = Once::new();
static GDT: Once<(GlobalDescriptorTable, SegmentSelector, SegmentSelector)> = Once::new();

#[repr(align(16))]
struct AlignedStack([u8; IST_STACK_SIZE]);

static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack([0; IST_STACK_SIZE]);

pub fn install() {
    let tss = TSS.call_once(|| {
        let mut tss = TaskStateSegment::new();
        let stack_start = {
            let ptr = unsafe { core::ptr::addr_of!(DOUBLE_FAULT_STACK.0) as *const u8 };
            VirtAddr::from_ptr(ptr)
        };
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] =
            stack_start + IST_STACK_SIZE as u64;
        tss
    });

    GDT.call_once(|| {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(tss));
        (gdt, code_selector, tss_selector)
    });

    let (gdt, _code_sel, tss_sel) = GDT.get().expect("GDT initialized");
    gdt.load();
    unsafe {
        load_tss(*tss_sel);
    }
}
