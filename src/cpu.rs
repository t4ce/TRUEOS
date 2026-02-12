use crate::x2apic::{detect_x2apic_topology, X2ApicTopology};
use crate::{percpu, runtime, globalog, exceptions};
use ::limine::mp::Cpu as LimineCpu;
use alloc::boxed::Box;
use alloc::vec::Vec;
use embassy_executor::raw::Executor;
use embassy_time::{Duration as EmbassyDuration, Timer};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use spin::Once;

static LOG_CPU_TOPOLOGY_ONCE: Once<()> = Once::new();
const AP_HEARTBEAT_TASK_POOL: usize = 256;

fn build_cpu_slot_lapic_order(
    resp: &::limine::response::MpResponse,
    topo: X2ApicTopology,
) -> Vec<u32> {
    // Important invariant for per-CPU mailboxes and other "slot indexed" data:
    // BSP must always be slot 0.
    let bsp_lapic_id = percpu::this_cpu().lapic_id();

    let mut items: Vec<(u32, (u32, u32, u32))> = Vec::new();
    for cpu in resp.cpus() {
        let lapic_id = cpu.lapic_id as u32;
        if lapic_id == bsp_lapic_id {
            continue;
        }
        items.push((lapic_id, topo.decode(lapic_id)));
    }

    items.sort_by(|a, b| {
        let (a_id, (a_pkg, a_core, a_smt)) = *a;
        let (b_id, (b_pkg, b_core, b_smt)) = *b;
        (a_pkg, a_core, a_smt, a_id).cmp(&(b_pkg, b_core, b_smt, b_id))
    });

    let mut lapic_order: Vec<u32> = Vec::with_capacity(items.len() + 1);
    lapic_order.push(bsp_lapic_id);

    for (lapic_id, _) in items {
        if lapic_order.iter().any(|id| *id == lapic_id) {
            continue;
        }
        lapic_order.push(lapic_id);
    }

    lapic_order
}

pub fn log_cpu_topology_once(resp: &::limine::response::MpResponse) {
    LOG_CPU_TOPOLOGY_ONCE.call_once(|| {
        let topo = detect_x2apic_topology();
        let lapic_order = build_cpu_slot_lapic_order(resp, topo);
        percpu::install_cpu_slot_lapic_order_owned(lapic_order);

        crate::log!(
            "cpu-topology: total={} bsp_lapic_id={} leaf={} smt_bits={} core_bits={}\n",
            percpu::total_slots(),
            percpu::this_cpu().lapic_id(),
            topo.leaf,
            topo.smt_bits,
            topo.core_bits
        );
        crate::log!(
            "cpu-topology: role  lapic_id  pkg  core  smt  slot\n"
        );

        let bsp_lapic_id = percpu::this_cpu().lapic_id();
        let (pkg, core, smt) = topo.decode(bsp_lapic_id);
        let bsp_slot = percpu::slot_for_lapic_id(bsp_lapic_id);
        crate::log!(
            "cpu-topology: {:<4} {:>8} {:>4} {:>5} {:>4} {:>5}\n",
            "bsp", bsp_lapic_id, pkg, core, smt, bsp_slot
        );

        for cpu in resp.cpus() {
            let lapic_id = cpu.lapic_id as u32;
            let (pkg, core, smt) = topo.decode(lapic_id);
            let slot = percpu::slot_for_lapic_id(lapic_id);
            crate::log!(
                "cpu-topology: {:<4} {:>8} {:>4} {:>5} {:>4} {:>5}\n",
                "ap", lapic_id, pkg, core, smt, slot
            );
        }
    });
}

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
