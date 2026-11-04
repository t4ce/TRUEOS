#!/usr/bin/env python3
"""Exercise Edit's real GPU client, private control ranges and quarantine route.

The host fixture replaces only the physical isolation call with a recorder;
priority, principal routing, storage ranges, retirement and sticky quarantine
come directly from production. No GPU commands are submitted.
"""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item
from test_edit_hex_paint import item as block


VGPU_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::physical::{EngineClass, PhysicalEngineId, PhysicalContextPriority};

    #[test]
    fn animation_is_a_normal_priority_peer_without_lowering_display_services() {
        for client in [KernelClient::GpgpuEditLife, KernelClient::GpgpuExecution,
            KernelClient::Render, KernelClient::Render1, KernelClient::Render2] {
            assert_eq!(client.physical_priority(), PhysicalContextPriority::KernelNormal);
        }
        for client in [KernelClient::GpgpuSystem, KernelClient::GpgpuFont, KernelClient::Ui4Compositor] {
            assert_eq!(client.physical_priority(), PhysicalContextPriority::KernelHigh);
        }
        assert_eq!(KernelClient::GpgpuEditLife.queue_class(), QueueClass::Compute);
        assert!(KernelClient::GpgpuEditLife.accepts_engine(PhysicalEngineId::RCS0));
        assert!(!KernelClient::GpgpuEditLife.accepts_engine(PhysicalEngineId::BCS0));
        assert!(!KernelClient::GpgpuEditLife.accepts_engine(PhysicalEngineId { class: EngineClass::RenderCompute, instance: 1 }));
    }

    #[test]
    fn edit_and_spirit_have_distinct_principals_and_exact_reverse_routes() {
        let edit = KernelClient::GpgpuEditLife;
        assert_eq!(edit.principal(), Principal::KernelGpgpuEditLife);
        assert_eq!(kernel_client_for_principal(edit.principal()), Some(edit));
        for peer in [KernelClient::GpgpuExecution, KernelClient::GpgpuSystem,
            KernelClient::GpgpuFont, KernelClient::Ui4Compositor] {
            assert_ne!(edit.principal(), peer.principal());
            assert_ne!(edit.name(), peer.name());
            assert_eq!(kernel_client_for_principal(peer.principal()), Some(peer));
        }
    }

    #[test]
    fn hardware_fault_owner_requires_one_generation_tagged_binding_on_the_right_engine() {
        assert!(physical_fault_ownership_is_exact(true, 1, 0));
        assert!(!physical_fault_ownership_is_exact(true, 0, 0));
        assert!(!physical_fault_ownership_is_exact(true, 2, 0));
        assert!(!physical_fault_ownership_is_exact(true, 1, 1));
        assert!(physical_fault_ownership_is_exact(false, 0, 0));
        assert!(!physical_fault_ownership_is_exact(false, 1, 0));
    }
}
'''


LANE_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::vgpu::KernelClient;

    fn ranges(va: DirectRcsGpuVa) -> [(u64, u64); 4] {
        [(va.ring, DIRECT_RCS_RING_BYTES as u64),
            (va.context, DIRECT_RCS_CONTEXT_BYTES as u64),
            (va.batch, (DIRECT_RCS_BATCH_BYTES * va.job_slots) as u64),
            (va.result, (DIRECT_RCS_RESULT_BYTES * va.job_slots) as u64)]
    }

    fn disjoint(a: (u64, u64), b: (u64, u64)) -> bool {
        a.0 + a.1 <= b.0 || b.0 + b.1 <= a.0
    }

    #[test]
    fn private_edit_ring_context_batch_and_result_never_alias_peer_storage() {
        let edit = ranges(EDIT_LIFE_RCS_GPU_VA);
        for (index, range) in edit.iter().copied().enumerate() {
            assert_eq!(range.0 % 4096, 0);
            for other in edit.iter().copied().skip(index + 1) { assert!(disjoint(range, other)); }
            for peer in [DIRECT_RCS_GPU_VA, FONT_RCS_GPU_VA, EXECUTION_RCS_GPU_VA,
                CODEC_RCS_GPU_VA, UI4_COMPOSITOR_RCS_GPU_VA] {
                for other in ranges(peer) { assert!(disjoint(range, other), "edit={range:?} peer={other:?}"); }
            }
        }
        assert_eq!(EDIT_LIFE_RCS_GPU_VA.job_slots, 1);
        assert!(!EDIT_LIFE_RCS_GPU_VA.map_general_auxiliary);
    }

    #[test]
    fn unavailable_optional_edit_lane_cannot_reject_core_display_boot() {
        let accepted = DirectRcsControlGgttPrewarmReport {
            system_service: true, font: true, execution: true, ui4_compositor: true,
            codec: false, edit_life: false,
        };
        assert!(accepted.accepted());
        assert!(!DirectRcsControlGgttPrewarmReport { execution: false, edit_life: true, ..accepted }.accepted());
        assert!(!DirectRcsControlGgttPrewarmReport { ui4_compositor: false, edit_life: true, ..accepted }.accepted());
        assert!(!DirectRcsControlGgttPrewarmReport { system_service: false, ..accepted }.accepted());
        assert!(!DirectRcsControlGgttPrewarmReport { font: false, ..accepted }.accepted());
    }

    #[test]
    fn an_uncertain_edit_submission_quarantines_only_edit_and_never_reopens_it() {
        assert!(direct_rcs_state_reuse_permitted(&EDIT_LIFE_RCS_CONTEXT_QUARANTINED));
        quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "test-missing-marker");
        assert!(!direct_rcs_state_reuse_permitted(&EDIT_LIFE_RCS_CONTEXT_QUARANTINED));
        for peer in [&DIRECT_RCS_CONTEXT_QUARANTINED, &FONT_RCS_CONTEXT_QUARANTINED,
            &EXECUTION_RCS_CONTEXT_QUARANTINED, &CODEC_RCS_CONTEXT_QUARANTINED] {
            assert!(direct_rcs_state_reuse_permitted(peer));
        }
        assert_eq!(*crate::gpu::vgpu::ISOLATED.lock().unwrap(), [KernelClient::GpgpuEditLife]);
        assert!(!direct_rcs_retirement_is_proven(false, 16, 16));
        assert!(!direct_rcs_retirement_is_proven(true, 12, 16));
        assert!(direct_rcs_retirement_is_proven(true, 16, 16));
        // A late proof alone neither revives this context identity nor permits
        // recycling its storage after the lane crossed its loss boundary.
        assert!(!direct_rcs_state_reuse_permitted(&EDIT_LIFE_RCS_CONTEXT_QUARANTINED));
        quarantine_direct_rcs_lane(DirectRcsLane::EditLife, "test-late-repeat");
        assert_eq!(crate::gpu::vgpu::ISOLATED.lock().unwrap().len(), 1);
    }
}
'''


RETIREMENT_FIXTURE = r'''
mod retirement {
    use core::sync::atomic::{AtomicBool, Ordering};
    struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
        fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
    }
    #[derive(Clone, Copy)]
    struct GpgpuRgba8Surface { gpu: u64, phys: u64, bytes: usize }
    type DirectRcsState = u64;
    struct SubmitRuntime { pending: Option<usize> }
    static EDIT_LIFE_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
    static EDIT_LIFE_RCS_SUBMIT_RUNTIME: Mutex<SubmitRuntime> = Mutex::new(SubmitRuntime { pending: None });
    static EDIT_LIFE_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
    static QUARANTINED: AtomicBool = AtomicBool::new(false);
    thread_local! {
        static EVENTS: std::cell::RefCell<Vec<(&'static str, u64, u64, u64, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
        static UNMAP_OK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
        static FLUSH_OK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    }
    fn edit_life_rcs_context_is_quarantined() -> bool { QUARANTINED.load(Ordering::Acquire) }
    fn direct_rcs_unmap_ppgtt_region_exact(state: u64, gpu: u64, phys: u64, bytes: usize) -> bool {
        EVENTS.with(|events| events.borrow_mut().push(("unmap", state, gpu, phys, bytes)));
        UNMAP_OK.with(|value| value.get())
    }
    fn direct_rcs_flush_ppgtt_pte_range(state: u64, gpu: u64, bytes: usize) -> bool {
        EVENTS.with(|events| events.borrow_mut().push(("flush", state, gpu, 0, bytes)));
        FLUSH_OK.with(|value| value.get())
    }

    #[test]
    fn retained_inputs_retire_only_after_their_own_lane_and_exact_mapping_are_safe() {
        let surface = GpgpuRgba8Surface { gpu: 0xA000000, phys: 0x12345000, bytes: 8192 };
        assert!(retire_edit_life_scratch_range(surface), "no context means no GPU mapping to retire");
        assert!(EVENTS.with(|events| events.borrow().is_empty()));
        *EDIT_LIFE_RCS_STATE.lock() = Some(0xED17);
        EDIT_LIFE_RCS_SUBMIT_RUNTIME.lock().pending = Some(1);
        assert!(!retire_edit_life_scratch_range(surface));
        assert!(EVENTS.with(|events| events.borrow().is_empty()));
        EDIT_LIFE_RCS_SUBMIT_RUNTIME.lock().pending = None;
        QUARANTINED.store(true, Ordering::Release);
        assert!(!retire_edit_life_scratch_range(surface));
        assert!(EVENTS.with(|events| events.borrow().is_empty()));
        QUARANTINED.store(false, Ordering::Release);
        assert!(retire_edit_life_scratch_range(surface));
        assert_eq!(EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut())), [
            ("unmap", 0xED17, surface.gpu, surface.phys, surface.bytes),
            ("flush", 0xED17, surface.gpu, 0, surface.bytes),
        ]);
        UNMAP_OK.with(|value| value.set(false));
        assert!(!retire_edit_life_scratch_range(surface), "wrong physical identity must retain backing");
        assert_eq!(EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut())), [
            ("unmap", 0xED17, surface.gpu, surface.phys, surface.bytes),
        ]);
        UNMAP_OK.with(|value| value.set(true));
        FLUSH_OK.with(|value| value.set(false));
        assert!(!retire_edit_life_scratch_range(surface), "unpublished PTE removal must retain backing");
        assert_eq!(EVENTS.with(|events| events.borrow().len()), 2);
    }
'''


def harness_source() -> str:
    vgpu = "src/gpu/vgpu.rs"
    physical = "src/gpu/physical.rs"
    runtime = "src/intel/gpgpu/rcs/runtime.rs"
    commands = "src/intel/gpgpu/rcs/commands.rs"
    constants = "src/intel/gpgpu/rcs/constants.rs"
    codec = "src/intel/gpgpu/operations/lz4_blocks.rs"
    source = "#[macro_export] macro_rules! log_error { ($($value:tt)*) => {}; }\n"
    source += "mod gpu { pub mod physical {\n"
    source += "\n".join(item(physical, name) for name in ("EngineClass", "PhysicalEngineId", "PhysicalContextPriority"))
    source += block((ROOT / physical).read_text(), "impl PhysicalEngineId")
    source += "}\npub mod vgpu { use super::physical::PhysicalContextPriority;\n"
    source += "\n".join(item(vgpu, name) for name in ("KernelClient", "Principal", "QueueClass"))
    source += block((ROOT / vgpu).read_text(), "impl KernelClient")
    source += block((ROOT / vgpu).read_text(), "impl Principal")
    source += item(vgpu, "kernel_client_for_principal")
    source += item(vgpu, "physical_fault_ownership_is_exact")
    source += "pub(crate) static ISOLATED: std::sync::Mutex<Vec<KernelClient>> = std::sync::Mutex::new(Vec::new());\n"
    source += "pub(crate) fn isolate_kernel_client(client: KernelClient) { ISOLATED.lock().unwrap().push(client); }\n"
    source += VGPU_TESTS + "}}\n"
    source += "use core::sync::atomic::{AtomicBool, Ordering};\n"
    for prefix in ("DIRECT", "FONT", "EXECUTION", "EDIT_LIFE", "CODEC"):
        source += f"static {prefix}_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);\n"
    source += item(runtime, "DirectRcsGpuVa")
    for name in ("DIRECT_RCS_RING_BYTES", "DIRECT_RCS_CONTEXT_BYTES", "DIRECT_RCS_BATCH_BYTES", "DIRECT_RCS_RESULT_BYTES",
        "EXECUTION_RCS_JOB_SLOTS", "UI4_COMPOSITOR_RCS_JOB_SLOTS"):
        source += constant(constants, name) + "\n"
    for prefix in ("DIRECT", "FONT", "EXECUTION", "EDIT_LIFE", "UI4_COMPOSITOR"):
        for storage in ("RING", "CONTEXT", "BATCH", "RESULT"):
            source += constant(constants, prefix + "_RCS_GPU_VA_" + storage + "_BASE") + "\n"
        source += constant(runtime, prefix + "_RCS_GPU_VA") + "\n"
    source += constant(codec, "CODEC_RCS_GPU_VA_RING_BASE") + "\n"
    source += constant(codec, "CODEC_RCS_GPU_VA") + "\n"
    source += item(runtime, "DirectRcsControlGgttPrewarmReport")
    source += block((ROOT / runtime).read_text(), "impl DirectRcsControlGgttPrewarmReport")
    source += item(commands, "DirectRcsLane")
    source += block((ROOT / commands).read_text(), "impl DirectRcsLane")
    source += item(commands, "quarantine_direct_rcs_lane")
    source += item(commands, "direct_rcs_state_reuse_permitted")
    source += item(commands, "direct_rcs_retirement_is_proven")
    source += LANE_TESTS
    source += RETIREMENT_FIXTURE + item(runtime, "retire_edit_life_scratch_range") + "}\n"
    return source


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-lane-") as temporary:
        directory = Path(temporary)
        fixture = directory / "lane.rs"
        fixture.write_text(harness_source())
        binary = directory / "lane"
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test", "-Adead_code", "-Aunused_variables",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(fixture), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
