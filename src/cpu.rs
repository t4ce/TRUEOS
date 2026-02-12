use crate::x2apic::{detect_x2apic_topology, X2ApicTopology};
use crate::{percpu, runtime, globalog, exceptions};
use ::limine::mp::Cpu as LimineCpu;
use alloc::boxed::Box;
use alloc::vec::Vec;
use embassy_executor::raw::Executor;
use embassy_time::{Duration as EmbassyDuration, Timer};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use spin::Once;

const AP_HEARTBEAT_TASK_POOL: usize = 256;

#[no_mangle]
pub unsafe extern "C" fn ap_start(cpu: &LimineCpu) -> ! {
    enable_sse();
    let slot = percpu::slot_for_lapic_id(cpu.lapic_id as u32);
    percpu::init_ap(cpu.lapic_id as u32, slot as u32);
    let ex = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    unsafe {
        (&mut *percpu::this_cpu_ptr()).set_executor_ptr(ex as *mut Executor);
    }
    let spawner = ex.spawner();
    if percpu::this_cpu().cpu_index() == 1 {
        runtime::register_first_ap_spawner(spawner);
    }
    if let Err(e) = spawner.spawn(ap_heartbeat_task()) {
        crate::log!("ap: heartbeat task spawn failed: {:?}\n", e);
    }
    crate::smp::mark_online();
    exceptions::load_this_cpu();
    runtime::run_ap_forever()
}

#[embassy_executor::task(pool_size = AP_HEARTBEAT_TASK_POOL)]
async fn ap_heartbeat_task() {
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        let slot = percpu::this_cpu().cpu_index() as u8;
        let mark = if slot < 10 {
            b'0' + slot
        } else {
            b'A' + ((slot - 10) % 26)
        };
        globalog::debugcon_write_byte_raw(mark);
    }
}

pub unsafe fn enable_sse() {
    let mut cr0 = Cr0::read();
    cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
    cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
    Cr0::write(cr0);

    let mut cr4 = Cr4::read();
    cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
    Cr4::write(cr4);
}

#[inline(always)]
pub(crate) fn long_mode_active() -> bool {
    use x86_64::registers::model_specific::Msr;
    const IA32_EFER: u32 = 0xC000_0080;
    const EFER_LMA_BIT: u64 = 1 << 10;
    let efer = unsafe { Msr::new(IA32_EFER).read() };
    (efer & EFER_LMA_BIT) != 0
}
