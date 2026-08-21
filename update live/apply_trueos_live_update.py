#!/usr/bin/env python3
"""Apply the TRUEOS `update live` FULLFORGET prototype.

Pinned to t4ce/TRUEOS commit ff9773b632fb04b7d54bbed92f92f0f8cc35ad0e. The patch is transactional: every exact
anchor is validated before any file is written.
"""
from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

BASE_COMMIT = 'ff9773b632fb04b7d54bbed92f92f0f8cc35ad0e'
REPLACEMENTS = [('linker.ld',
  '  .limine_requests : AT(ADDR(.limine_requests) - KERNEL_OFFSET) {\n    KEEP(*(.limine_requests))\n  } :text\n',
  '  .limine_requests : AT(ADDR(.limine_requests) - KERNEL_OFFSET) {\n'
  '    __limine_requests_start = .;\n'
  '    KEEP(*(.limine_requests))\n'
  '    __limine_requests_end = .;\n'
  '  } :text\n'),
 ('linker.ld',
  '  .got.plt : AT(ADDR(.got.plt) - KERNEL_OFFSET) {\n'
  '    *(.got.plt)\n'
  '    *(.igot.plt)\n'
  '  } :text\n'
  '\n'
  '  .bss : AT(ADDR(.bss) - KERNEL_OFFSET) {\n',
  '  .got.plt : AT(ADDR(.got.plt) - KERNEL_OFFSET) {\n'
  '    *(.got.plt)\n'
  '    *(.igot.plt)\n'
  '  } :text\n'
  '\n'
  '  /*\n'
  '   * A candidate kernel publishes a tiny fixed ABI manifest. The old\n'
  '   * generation validates this before it replaces the kernel PML4 slot.\n'
  '   */\n'
  '  .live_update_handoff ALIGN(64) : AT(ADDR(.live_update_handoff) - KERNEL_OFFSET) {\n'
  '    __live_update_handoff_start = .;\n'
  '    KEEP(*(.live_update_handoff))\n'
  '    __live_update_handoff_end = .;\n'
  '  } :text\n'
  '\n'
  '  .live_update_slot ALIGN(8) : AT(ADDR(.live_update_slot) - KERNEL_OFFSET) {\n'
  '    QUAD(0x545255454F534C55); /* TRUEOSLU */\n'
  '    QUAD(0x4C49564555504454); /* LIVEUPDT */\n'
  '    QUAD(1);                  /* ABI version */\n'
  '    QUAD(trueos_live_update_ap_entry);\n'
  '    QUAD(__live_update_handoff_start);\n'
  '    QUAD(__live_update_handoff_end - __live_update_handoff_start);\n'
  '  } :text\n'
  '\n'
  '  /* Copied verbatim into a temporary PML4 slot during FULLFORGET. */\n'
  '  .live_update_trampoline ALIGN(16) : AT(ADDR(.live_update_trampoline) - KERNEL_OFFSET) {\n'
  '    __live_update_trampoline_start = .;\n'
  '    KEEP(*(.live_update_trampoline))\n'
  '    __live_update_trampoline_end = .;\n'
  '  } :text\n'
  '\n'
  '  .bss : AT(ADDR(.bss) - KERNEL_OFFSET) {\n'),
 ('src/main.rs', 'mod limine;\nmod locale;\n', 'mod limine;\nmod live_update;\nmod locale;\n'),
 ('src/main.rs',
  '    log_os::init_log_facade();\n    crate::log_info!(\n',
  '    log_os::init_log_facade();\n    live_update::log_boot_mode();\n    crate::log_info!(\n'),
 ('src/main.rs',
  '    boot_secondary_processors(smp_resp);\n    spawn_bsp_services(spawner);\n    _loop(executor)\n',
  '    if live_update::warm_boot_active() {\n'
  '        live_update::release_warm_aps();\n'
  '    } else {\n'
  '        boot_secondary_processors(smp_resp);\n'
  '    }\n'
  '    spawn_bsp_services(spawner);\n'
  '    live_update::spawn_post_boot(spawner);\n'
  '    _loop(executor)\n'),
 ('src/exceptions.rs',
  '        crate::chronos::interrupt_install(&mut idt);\n'
  '        crate::remote_work_wake::interrupt_install(&mut idt);\n'
  '        crate::hv::control_kick::interrupt_install(&mut idt);\n',
  '        crate::chronos::interrupt_install(&mut idt);\n'
  '        crate::remote_work_wake::interrupt_install(&mut idt);\n'
  '        crate::live_update::interrupt_install(&mut idt);\n'
  '        crate::hv::control_kick::interrupt_install(&mut idt);\n'),
 ('src/cpu.rs',
  '#[unsafe(no_mangle)]\n'
  'pub unsafe extern "C" fn ap_start(cpu: &LimineCpu) -> ! {\n'
  '    enable_sse();\n'
  '    crate::microcode::apply_selected_to_current_cpu("ap");\n'
  '    let lapic_id = crate::limine::mp_cpu_id(cpu);\n'
  '    let slot = percpu::slot_for_lapic_id(lapic_id);\n'
  '    percpu::init_ap(lapic_id, slot as u32);\n'
  '    if slot > 1 {\n'
  '        crate::hv::enter_vmx_root_for_current_cpu_contract()\n'
  '            .expect("VMX core contract failed during AP startup");\n'
  '    }\n'
  '    let ex = percpu::init_executor();\n'
  '    let spawner = ex.spawner();\n'
  '    enter_ap_runtime(spawner)\n'
  '}\n',
  '#[unsafe(no_mangle)]\n'
  'pub unsafe extern "C" fn ap_start(cpu: &LimineCpu) -> ! {\n'
  '    enable_sse();\n'
  '    crate::microcode::apply_selected_to_current_cpu("ap");\n'
  '    let lapic_id = crate::limine::mp_cpu_id(cpu);\n'
  '    let slot = percpu::slot_for_lapic_id(lapic_id);\n'
  '    percpu::init_ap(lapic_id, slot as u32);\n'
  '    if slot > 1 {\n'
  '        crate::hv::enter_vmx_root_for_current_cpu_contract()\n'
  '            .expect("VMX core contract failed during AP startup");\n'
  '    }\n'
  '    let ex = percpu::init_executor();\n'
  '    let spawner = ex.spawner();\n'
  '    enter_ap_runtime(spawner)\n'
  '}\n'
  '\n'
  '/// Enter a replacement generation after the FULLFORGET trampoline has already\n'
  '/// supplied a transition stack, LAPIC identity, and CPU slot.\n'
  'pub(crate) unsafe fn warm_ap_start(lapic_id: u32, cpu_index: u32) -> ! {\n'
  '    enable_sse();\n'
  '    crate::microcode::apply_selected_to_current_cpu("warm-ap");\n'
  '    percpu::init_ap(lapic_id, cpu_index);\n'
  '    // Interrupts remain disabled across the handoff. Load the candidate IDT\n'
  '    // before this AP creates fresh VMX/executor state and opens its AP loop.\n'
  '    exceptions::load_this_cpu();\n'
  '    if cpu_index > 1 {\n'
  '        crate::hv::enter_vmx_root_for_current_cpu_contract()\n'
  '            .expect("VMX core contract failed during warm AP startup");\n'
  '    }\n'
  '    let ex = percpu::init_executor();\n'
  '    enter_ap_runtime(ex.spawner())\n'
  '}\n'),
 ('src/hv/vmx.rs',
  '        fail == 0\n    }\n}\n\npub fn vmclear(pa: u64) -> bool {\n',
  '        fail == 0\n'
  '    }\n'
  '}\n'
  '\n'
  '/// Leave VMX root operation on the current logical processor.\n'
  'pub fn vmxoff() -> bool {\n'
  '    unsafe {\n'
  '        let mut fail: u8;\n'
  '        core::arch::asm!(\n'
  '            "vmxoff",\n'
  '            "setna {fail}",\n'
  '            fail = lateout(reg_byte) fail,\n'
  '            options(nostack, preserves_flags),\n'
  '        );\n'
  '        fail == 0\n'
  '    }\n'
  '}\n'
  '\n'
  'pub fn vmclear(pa: u64) -> bool {\n'),
 ('src/hv/mod.rs',
  '    maybe_log_vmx_core_contract_summary(revision);\n'
  '    Ok(())\n'
  '}\n'
  '\n'
  'fn vm_owner_cpu_slot(vm_id: u8) -> Option<u32> {\n',
  '    maybe_log_vmx_core_contract_summary(revision);\n'
  '    Ok(())\n'
  '}\n'
  '\n'
  "/// Tear down this CPU's generation-local VMX root state before FULLFORGET.\n"
  '///\n'
  '/// VM snapshots are portable envelopes on TRUEOSFS; VMXON and VMCS pages are\n'
  '/// deliberately not carried into the replacement kernel.\n'
  "pub fn leave_vmx_root_for_current_cpu_contract() -> Result<bool, &'static str> {\n"
  '    let slot = current_vmx_slot()?;\n'
  '    if slot <= 1 || !VMX_ROOT_ACTIVE_BY_CPU[slot].load(Ordering::Acquire) {\n'
  '        return Ok(false);\n'
  '    }\n'
  '    if !vmx::vmxoff() {\n'
  '        return Err("vmxoff");\n'
  '    }\n'
  '    VMX_EXTERNAL_INTERRUPT_EXITING_BY_CPU[slot].store(false, Ordering::Release);\n'
  '    VMXON_PA_BY_CPU[slot].store(0, Ordering::Release);\n'
  '    VMX_ROOT_ACTIVE_BY_CPU[slot].store(false, Ordering::Release);\n'
  '    Ok(true)\n'
  '}\n'
  '\n'
  'fn vm_owner_cpu_slot(vm_id: u8) -> Option<u32> {\n'),
 ('src/pci/pci.rs',
  '#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]\n'
  'const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;\n',
  'const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;\n'),
 ('src/pci/pci.rs',
  '#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]\n'
  'const PCI_CAP_ID_MSI: u8 = 0x05;\n'
  'const PCI_CAP_ID_PCI_EXPRESS: u8 = 0x10;\n'
  '#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]\n'
  'const PCI_MSI_CONTROL: u16 = 0x02;\n',
  'const PCI_CAP_ID_MSI: u8 = 0x05;\n'
  'const PCI_CAP_ID_PCI_EXPRESS: u8 = 0x10;\n'
  'const PCI_CAP_ID_MSIX: u8 = 0x11;\n'
  'const PCI_MSI_CONTROL: u16 = 0x02;\n'),
 ('src/pci/pci.rs',
  '#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]\n'
  'const PCI_MSI_ENABLE: u16 = 1 << 0;\n'
  '#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]\n'
  'const PCI_MSI_MULTIPLE_MESSAGE_ENABLE: u16 = 0b111 << 1;\n',
  'const PCI_MSI_ENABLE: u16 = 1 << 0;\n'
  'const PCI_MSI_MULTIPLE_MESSAGE_ENABLE: u16 = 0b111 << 1;\n'
  'const PCI_MSIX_CONTROL: u16 = 0x02;\n'
  'const PCI_MSIX_FUNCTION_MASK: u16 = 1 << 14;\n'
  'const PCI_MSIX_ENABLE: u16 = 1 << 15;\n'),
 ('src/pci/pci.rs',
  'pub fn with_devices<R, F: FnOnce(&[PciDevice]) -> R>(f: F) -> R {\n'
  '    let lock = DEVICES.lock();\n'
  '    f(lock.as_slice())\n'
  '}\n',
  'pub fn with_devices<R, F: FnOnce(&[PciDevice]) -> R>(f: F) -> R {\n'
  '    let lock = DEVICES.lock();\n'
  '    f(lock.as_slice())\n'
  '}\n'
  '\n'
  '#[derive(Copy, Clone, Debug)]\n'
  'pub struct FullforgetPciFunction {\n'
  '    bus: u8,\n'
  '    slot: u8,\n'
  '    function: u8,\n'
  '}\n'
  '\n'
  '/// Capture an immutable BDF list while the ordinary runtime is still alive.\n'
  '/// The final FULLFORGET path consumes this list without taking `DEVICES` or the\n'
  '/// legacy PCI configuration lock after APs have been parked.\n'
  'pub fn fullforget_snapshot() -> Vec<FullforgetPciFunction, MAX_PCI_DEVICES> {\n'
  '    let mut out = Vec::new();\n'
  '    with_devices(|devices| {\n'
  '        for device in devices {\n'
  '            let _ = out.push(FullforgetPciFunction {\n'
  '                bus: device.bus,\n'
  '                slot: device.slot,\n'
  '                function: device.function,\n'
  '            });\n'
  '        }\n'
  '    });\n'
  '    out\n'
  '}\n'
  '\n'
  '#[inline]\n'
  'fn fullforget_read_u8_unlocked(bus: u8, slot: u8, function: u8, offset: u8) -> u8 {\n'
  '    let aligned = read_u32_unlocked(bus, slot, function, offset & !0x03);\n'
  '    let shift = ((offset & 0x03) as u32) * 8;\n'
  '    ((aligned >> shift) & 0xFF) as u8\n'
  '}\n'
  '\n'
  '#[inline]\n'
  'fn fullforget_read_u16_unlocked(bus: u8, slot: u8, function: u8, offset: u8) -> u16 {\n'
  '    let aligned = read_u32_unlocked(bus, slot, function, offset & !0x03);\n'
  '    let shift = ((offset & 0x03) as u32) * 8;\n'
  '    ((aligned >> shift) & 0xFFFF) as u16\n'
  '}\n'
  '\n'
  '#[inline]\n'
  'fn fullforget_write_u16_unlocked(\n'
  '    bus: u8,\n'
  '    slot: u8,\n'
  '    function: u8,\n'
  '    offset: u8,\n'
  '    value: u16,\n'
  ') {\n'
  '    let aligned_off = offset & !0x03;\n'
  '    let shift = ((offset & 0x03) as u32) * 8;\n'
  '    let current = read_u32_unlocked(bus, slot, function, aligned_off);\n'
  '    let next = (current & !(0xFFFFu32 << shift)) | ((value as u32) << shift);\n'
  '    write_u32_unlocked(bus, slot, function, aligned_off, next);\n'
  '}\n'
  '\n'
  'fn fullforget_find_capability_unlocked(\n'
  '    bus: u8,\n'
  '    slot: u8,\n'
  '    function: u8,\n'
  '    cap_id: u8,\n'
  ') -> Option<u8> {\n'
  '    let status = fullforget_read_u16_unlocked(bus, slot, function, 0x06);\n'
  '    if (status & PCI_STATUS_CAP_LIST) == 0 {\n'
  '        return None;\n'
  '    }\n'
  '    let mut pointer = fullforget_read_u8_unlocked(bus, slot, function, PCI_CAP_PTR as u8) & !0x03;\n'
  '    let mut guard = 0usize;\n'
  '    while pointer >= 0x40 && guard < 48 {\n'
  '        if fullforget_read_u8_unlocked(bus, slot, function, pointer) == cap_id {\n'
  '            return Some(pointer);\n'
  '        }\n'
  '        pointer = fullforget_read_u8_unlocked(bus, slot, function, pointer.wrapping_add(1)) & !0x03;\n'
  '        guard += 1;\n'
  '    }\n'
  '    None\n'
  '}\n'
  '\n'
  '/// Disable device-originated interrupts and bus mastering without taking any\n'
  '/// normal kernel lock. The caller must have interrupts disabled and every AP\n'
  '/// parked. BAR decoding is intentionally retained for the replacement driver.\n'
  'pub unsafe fn fullforget_quiesce_unlocked(functions: &[FullforgetPciFunction]) -> usize {\n'
  '    let mut failures = 0usize;\n'
  '    for device in functions {\n'
  '        if let Some(cap) = fullforget_find_capability_unlocked(\n'
  '            device.bus,\n'
  '            device.slot,\n'
  '            device.function,\n'
  '            PCI_CAP_ID_MSI,\n'
  '        ) {\n'
  '            let control_offset = cap.wrapping_add(PCI_MSI_CONTROL as u8);\n'
  '            let control = fullforget_read_u16_unlocked(\n'
  '                device.bus,\n'
  '                device.slot,\n'
  '                device.function,\n'
  '                control_offset,\n'
  '            );\n'
  '            fullforget_write_u16_unlocked(\n'
  '                device.bus,\n'
  '                device.slot,\n'
  '                device.function,\n'
  '                control_offset,\n'
  '                control & !(PCI_MSI_ENABLE | PCI_MSI_MULTIPLE_MESSAGE_ENABLE),\n'
  '            );\n'
  '        }\n'
  '        if let Some(cap) = fullforget_find_capability_unlocked(\n'
  '            device.bus,\n'
  '            device.slot,\n'
  '            device.function,\n'
  '            PCI_CAP_ID_MSIX,\n'
  '        ) {\n'
  '            let control_offset = cap.wrapping_add(PCI_MSIX_CONTROL as u8);\n'
  '            let control = fullforget_read_u16_unlocked(\n'
  '                device.bus,\n'
  '                device.slot,\n'
  '                device.function,\n'
  '                control_offset,\n'
  '            );\n'
  '            fullforget_write_u16_unlocked(\n'
  '                device.bus,\n'
  '                device.slot,\n'
  '                device.function,\n'
  '                control_offset,\n'
  '                (control | PCI_MSIX_FUNCTION_MASK) & !PCI_MSIX_ENABLE,\n'
  '            );\n'
  '        }\n'
  '\n'
  '        let command = fullforget_read_u16_unlocked(\n'
  '            device.bus,\n'
  '            device.slot,\n'
  '            device.function,\n'
  '            0x04,\n'
  '        );\n'
  '        fullforget_write_u16_unlocked(\n'
  '            device.bus,\n'
  '            device.slot,\n'
  '            device.function,\n'
  '            0x04,\n'
  '            (command & !PCI_COMMAND_BUS_MASTER) | PCI_COMMAND_INTX_DISABLE,\n'
  '        );\n'
  '        let readback = fullforget_read_u16_unlocked(\n'
  '            device.bus,\n'
  '            device.slot,\n'
  '            device.function,\n'
  '            0x04,\n'
  '        );\n'
  '        if (readback & PCI_COMMAND_BUS_MASTER) != 0 {\n'
  '            failures = failures.saturating_add(1);\n'
  '        }\n'
  '    }\n'
  '    failures\n'
  '}\n'),
 ('src/phys.rs',
  '        if state.add_region(start, end).is_err() {\n'
  '            crate::log!("pmm: region table full dropping 0x{:X}..0x{:X}\\n", start, end);\n'
  '        }\n',
  '        add_usable_region_excluding_warm_handoff(&mut state, start, end);\n'),
 ('src/phys.rs',
  'pub fn reserve_heap_arena(size: usize, align: usize) -> Option<HeapArena> {\n',
  '\n'
  'fn add_usable_region_excluding_warm_handoff(state: &mut PmmState, start: u64, end: u64) {\n'
  '    let mut cursor = start;\n'
  '    while cursor < end {\n'
  '        let mut cut_start = end;\n'
  '        let mut cut_end = end;\n'
  '        crate::live_update::for_each_warm_reserved_phys_range(|reserved_start, reserved_len| {\n'
  '            let Some(reserved_end) = reserved_start.checked_add(reserved_len) else {\n'
  '                return;\n'
  '            };\n'
  '            if reserved_end <= cursor || reserved_start >= end {\n'
  '                return;\n'
  '            }\n'
  '            let effective_start = reserved_start.max(cursor);\n'
  '            let effective_end = reserved_end.min(end);\n'
  '            if effective_start < cut_start {\n'
  '                cut_start = effective_start;\n'
  '                cut_end = effective_end;\n'
  '            } else if effective_start == cut_start {\n'
  '                cut_end = cut_end.max(effective_end);\n'
  '            }\n'
  '        });\n'
  '\n'
  '        if cut_start == end {\n'
  '            if state.add_region(cursor, end).is_err() {\n'
  '                crate::log!(\n'
  '                    "pmm: region table full dropping 0x{:X}..0x{:X}\\n",\n'
  '                    cursor,\n'
  '                    end\n'
  '                );\n'
  '            }\n'
  '            break;\n'
  '        }\n'
  '        if cursor < cut_start\n'
  '            && state.add_region(cursor, cut_start).is_err()\n'
  '        {\n'
  '            crate::log!(\n'
  '                "pmm: region table full dropping 0x{:X}..0x{:X}\\n",\n'
  '                cursor,\n'
  '                cut_start\n'
  '            );\n'
  '        }\n'
  '        cursor = cursor.max(cut_end);\n'
  '    }\n'
  '}\n'
  '\n'
  'pub fn reserve_heap_arena(size: usize, align: usize) -> Option<HeapArena> {\n'),
 ('src/phys.rs',
  'pub fn reserve_heap_arena_at(phys_start: u64, size: usize) -> Option<HeapArena> {\n'
  '    if size == 0 || HHDM_BASE.load(Ordering::Relaxed) == 0 {\n'
  '        return None;\n'
  '    }\n'
  '    let size_u64 = u64::try_from(size).ok()?;\n'
  '    let end = phys_start.checked_add(size_u64)?;\n'
  '    let mut guard = PMM.lock();\n'
  '    let state = guard.as_mut()?;\n'
  '    let reserved = state.allocate(size_u64, 1, phys_start, Some(end))?;\n'
  '    if reserved != phys_start {\n'
  '        let _ = state.release(reserved, size_u64);\n'
  '        return None;\n'
  '    }\n'
  '    Some(HeapArena {\n'
  '        phys_start,\n'
  '        virt_start: phys_to_virt(phys_start as usize),\n'
  '        length: size,\n'
  '    })\n'
  '}\n',
  'pub fn reserve_heap_arena_at(phys_start: u64, size: usize) -> Option<HeapArena> {\n'
  '    if size == 0 || HHDM_BASE.load(Ordering::Relaxed) == 0 {\n'
  '        return None;\n'
  '    }\n'
  '    let size_u64 = u64::try_from(size).ok()?;\n'
  '    let end = phys_start.checked_add(size_u64)?;\n'
  '    {\n'
  '        let mut guard = PMM.lock();\n'
  '        let state = guard.as_mut()?;\n'
  '        if let Some(reserved) = state.allocate(size_u64, 1, phys_start, Some(end)) {\n'
  '            if reserved == phys_start {\n'
  '                return Some(HeapArena {\n'
  '                    phys_start,\n'
  '                    virt_start: phys_to_virt(phys_start as usize),\n'
  '                    length: size,\n'
  '                });\n'
  '            }\n'
  '            let _ = state.release(reserved, size_u64);\n'
  '        }\n'
  '    }\n'
  '\n'
  '    // A warm-generation boot excluded pointer-bearing VM heaps from the new\n'
  '    // PMM. Claim an exact handoff range once instead of exposing it to normal\n'
  '    // host allocations between PMM initialization and VM restoration.\n'
  '    if !crate::live_update::claim_warm_vm_heap_range(phys_start, size_u64) {\n'
  '        return None;\n'
  '    }\n'
  '    Some(HeapArena {\n'
  '        phys_start,\n'
  '        virt_start: phys_to_virt(phys_start as usize),\n'
  '        length: size,\n'
  '    })\n'
  '}\n'),
 ('src/limine.rs',
  'pub fn hhdm_offset() -> Option<u64> {\n    let resp = HHDM_REQUEST.response()?;\n    Some(resp.offset)\n}\n',
  'pub fn hhdm_offset() -> Option<u64> {\n'
  '    if let Some(offset) = crate::live_update::warm_hhdm_offset() {\n'
  '        return Some(offset);\n'
  '    }\n'
  '    let resp = HHDM_REQUEST.response()?;\n'
  '    Some(resp.offset)\n'
  '}\n'),
 ('src/limine.rs',
  'pub fn executable_address_bases() -> Option<(u64, u64)> {\n'
  '    let resp = EXECUTABLE_ADDRESS_REQUEST.response()?;\n'
  '    Some((resp.virtual_base, resp.physical_base))\n'
  '}\n',
  'pub fn executable_address_bases() -> Option<(u64, u64)> {\n'
  '    if let Some(bases) = crate::live_update::warm_kernel_bases() {\n'
  '        return Some(bases);\n'
  '    }\n'
  '    let resp = EXECUTABLE_ADDRESS_REQUEST.response()?;\n'
  '    Some((resp.virtual_base, resp.physical_base))\n'
  '}\n'),
 ('src/limine.rs',
  "pub fn kernel_file_bytes() -> Option<&'static [u8]> {\n"
  '    let resp = EXECUTABLE_FILE_REQUEST.response()?;\n'
  '    bytes_from_limine_file(resp.executable_file())\n'
  '}\n',
  "pub fn kernel_file_bytes() -> Option<&'static [u8]> {\n"
  '    if let Some(bytes) = crate::live_update::warm_kernel_file_bytes() {\n'
  '        return Some(bytes);\n'
  '    }\n'
  '    let resp = EXECUTABLE_FILE_REQUEST.response()?;\n'
  '    bytes_from_limine_file(resp.executable_file())\n'
  '}\n'),
 ('src/shell2/backends/net_tcp_shell.rs',
  '                        crate::log!(\n'
  '                            "net-shell: tcp established handle={} ms={}\\n",\n'
  '                            handle.0,\n'
  '                            Instant::now().as_millis()\n'
  '                        );\n',
  '                        crate::log!(\n'
  '                            "net-shell: tcp established handle={} ms={}\\n",\n'
  '                            handle.0,\n'
  '                            Instant::now().as_millis()\n'
  '                        );\n'
  '                        if let Some(notice) = crate::live_update::take_shell_notice()\n'
  '                            && !net_shell_write_bytes(notice)\n'
  '                        {\n'
  '                            crate::live_update::rearm_shell_notice();\n'
  '                        }\n')]
NEW_FILES = {
    "src/live_update.rs": '//! RAM-only TRUEOS generation replacement (`update live`).\n//!\n//! The old kernel is treated as a disposable in-memory boot loader:\n//! 1. validate and stage a compatible TRUEOS ELF in a PMM-reserved arena;\n//! 2. checkpoint active replicatable VMX applications to TRUEOSFS;\n//! 3. park every AP through an HHDM-resident trampoline and execute VMXOFF;\n//! 4. contain PCI DMA, replace only the kernel PML4 slot, flush the TLB, and jump;\n//! 5. bring the new kernel up from copied immutable Limine boot facts and restore VMs.\n//!\n//! No candidate-kernel bytes are written to the ESP or TRUEOSFS. Persistent\n//! storage is used only for VM application checkpoints selected by this handoff.\n\nuse alloc::{format, string::String, vec::Vec};\nuse core::{\n    convert::Infallible,\n    fmt,\n    mem::{offset_of, size_of},\n    ptr,\n    sync::atomic::{AtomicBool, AtomicU64, Ordering},\n};\n\nuse embassy_executor::Spawner;\nuse embassy_time::{Duration as EmbassyDuration, Instant, Timer};\nuse x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};\n\nuse crate::shell2::{\n    MatrixTarget, matrix_target_interrupted, print_matrix_target_line,\n};\n\npub(crate) const RENDEZVOUS_VECTOR: u8 = 0x43;\n\nconst PAGE_SIZE: usize = 4096;\nconst PAGE_MASK: u64 = !(PAGE_SIZE as u64 - 1);\nconst TWO_MIB: usize = 2 * 1024 * 1024;\nconst ONE_GIB: usize = 1024 * 1024 * 1024;\nconst MAX_KERNEL_FILE_BYTES: usize = 256 * 1024 * 1024;\nconst MAX_KERNEL_SPAN_BYTES: usize = 1024 * 1024 * 1024;\nconst AP_TRANSITION_STACK_BYTES: usize = 2 * 1024 * 1024;\nconst VM_CHECKPOINT_TIMEOUT_MS: u64 = 20_000;\nconst AP_RENDEZVOUS_TIMEOUT_MS: u64 = 5_000;\nconst POST_BOOT_SERVICE_TIMEOUT_MS: u64 = 30_000;\nconst POST_BOOT_TLB_TIMEOUT_MS: u64 = 2_000;\nconst MAX_TRAMPOLINE_BYTES: usize = 64 * 1024;\nconst MAX_TRANSITION_GDT_BYTES: usize = 256;\nconst PCI_DMA_DRAIN_MS: u64 = 10;\nconst ABORT_DRAIN_TIMEOUT_MS: u64 = 2_000;\n\nconst LIVE_MANIFEST_MAGIC0: u64 = 0x5452_5545_4F53_4C55; // "TRUEOSLU"\nconst LIVE_MANIFEST_MAGIC1: u64 = 0x4C49_5645_5550_4454; // "LIVEUPDT"\nconst LIVE_ABI_VERSION: u64 = 1;\n\nconst HANDOFF_MAGIC0: u64 = 0x5452_5545_5741_524D; // "TRUEWARM"\nconst HANDOFF_MAGIC1: u64 = 0x4655_4C4C_464F_5247; // "FULLFORG"\nconst HANDOFF_STATE_COMMITTED: u64 = 1;\n\nconst TRANSITION_PARK: u64 = 1;\nconst TRANSITION_ABORT: u64 = 2;\nconst TRANSITION_SWITCH_STACKS: u64 = 3;\nconst TRANSITION_COMMIT: u64 = 4;\n\nconst VM_ID_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;\nconst CPU_SLOT_LIMIT: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;\nconst RESTORE_WORDS: usize = (VM_ID_LIMIT + 63) / 64;\n\n#[derive(Clone, Copy)]\n#[repr(C)]\npub(crate) struct WarmReservedRange {\n    pub(crate) phys_start: u64,\n    pub(crate) length: u64,\n}\n\nimpl WarmReservedRange {\n    const EMPTY: Self = Self {\n        phys_start: 0,\n        length: 0,\n    };\n\n    fn valid(self) -> bool {\n        self.length != 0 && self.phys_start.checked_add(self.length).is_some()\n    }\n}\n\nconst SHELL_NOTICE: &[u8] =\n    b"\\r\\nupdate live: hey that worked, new kernel here :)\\r\\n";\n\nunsafe extern "C" {\n    static __limine_requests_start: u8;\n    static __limine_requests_end: u8;\n    static __live_update_trampoline_start: u8;\n    static __live_update_trampoline_end: u8;\n}\n\n#[derive(Clone, Copy)]\n#[repr(C, align(64))]\nstruct WarmHandoff {\n    magic0: u64,\n    magic1: u64,\n    abi_version: u64,\n    state: u64,\n    generation: u64,\n    candidate_hash: u64,\n    arena_phys: u64,\n    arena_len: u64,\n    kernel_virt_base: u64,\n    kernel_phys_base: u64,\n    kernel_len: u64,\n    kernel_file_phys: u64,\n    kernel_file_len: u64,\n    hhdm_base: u64,\n    expected_aps: u64,\n    transition_slot: u64,\n    vm_heap_ranges: [WarmReservedRange; VM_ID_LIMIT],\n    restore_mask: [u64; RESTORE_WORDS],\n    resume_mask: [u64; RESTORE_WORDS],\n    checksum: u64,\n}\n\nimpl WarmHandoff {\n    const EMPTY: Self = Self {\n        magic0: 0,\n        magic1: 0,\n        abi_version: 0,\n        state: 0,\n        generation: 0,\n        candidate_hash: 0,\n        arena_phys: 0,\n        arena_len: 0,\n        kernel_virt_base: 0,\n        kernel_phys_base: 0,\n        kernel_len: 0,\n        kernel_file_phys: 0,\n        kernel_file_len: 0,\n        hhdm_base: 0,\n        expected_aps: 0,\n        transition_slot: 0,\n        vm_heap_ranges: [WarmReservedRange::EMPTY; VM_ID_LIMIT],\n        restore_mask: [0; RESTORE_WORDS],\n        resume_mask: [0; RESTORE_WORDS],\n        checksum: 0,\n    };\n\n    fn valid(&self) -> bool {\n        self.magic0 == HANDOFF_MAGIC0\n            && self.magic1 == HANDOFF_MAGIC1\n            && self.abi_version == LIVE_ABI_VERSION\n            && self.state == HANDOFF_STATE_COMMITTED\n            && self.arena_len != 0\n            && self.kernel_len != 0\n            && self.hhdm_base != 0\n            && self.checksum == handoff_checksum(self)\n    }\n}\n\n#[used]\n#[unsafe(link_section = ".live_update_handoff")]\nstatic mut LIVE_HANDOFF: WarmHandoff = WarmHandoff::EMPTY;\n\nstatic LIVE_UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);\nstatic WARM_APS_RELEASED: AtomicBool = AtomicBool::new(false);\nstatic SHELL_NOTICE_PENDING: AtomicBool = AtomicBool::new(false);\nstatic ACTIVE_CONTROL_HHDM: AtomicU64 = AtomicU64::new(0);\nstatic RENDEZVOUS_ISR_ACTIVE: AtomicU64 = AtomicU64::new(0);\nstatic POST_BOOT_TLB_ACTIVE: AtomicBool = AtomicBool::new(false);\nstatic POST_BOOT_TLB_ACKS: AtomicU64 = AtomicU64::new(0);\nstatic AP_TRANSITION_ENTERED: [AtomicBool; CPU_SLOT_LIMIT] =\n    [const { AtomicBool::new(false) }; CPU_SLOT_LIMIT];\nstatic WARM_VM_RANGE_CLAIMED: [AtomicBool; VM_ID_LIMIT] =\n    [const { AtomicBool::new(false) }; VM_ID_LIMIT];\n\n#[repr(C, align(64))]\nstruct TransitionControl {\n    command: AtomicU64,\n    arrived: AtomicU64,\n    stacked: AtomicU64,\n    failures: AtomicU64,\n    cr3: u64,\n    root_hhdm: u64,\n    kernel_slot: u64,\n    new_slot_entry: u64,\n    transition_slot: u64,\n    transition_slot_entry: u64,\n    stack_base_hhdm: u64,\n    stack_stride: u64,\n    bsp_entry: u64,\n    ap_entry: u64,\n    ap_park_hhdm: u64,\n    bsp_commit_hhdm: u64,\n    expected_aps: u64,\n    transition_gdt: [u8; MAX_TRANSITION_GDT_BYTES],\n    transition_gdtr: [u8; 10],\n}\n\n#[derive(Debug)]\npub enum LiveUpdateError {\n    Busy,\n    Interrupted,\n    BadElf(&\'static str),\n    Incompatible(&\'static str),\n    OutOfMemory,\n    ArithmeticOverflow,\n    VmNotReplicatable(u8),\n    VmCheckpointRequest(u8),\n    VmCheckpointTimeout(u8),\n    VmCheckpointStore(u8),\n    ApRendezvous(&\'static str),\n}\n\nimpl fmt::Display for LiveUpdateError {\n    fn fmt(&self, f: &mut fmt::Formatter<\'_>) -> fmt::Result {\n        match self {\n            Self::Busy => write!(f, "another live update is already active"),\n            Self::Interrupted => write!(f, "interrupted"),\n            Self::BadElf(reason) => write!(f, "bad candidate ELF ({reason})"),\n            Self::Incompatible(reason) => write!(f, "incompatible candidate ({reason})"),\n            Self::OutOfMemory => write!(f, "not enough contiguous RAM for candidate generation"),\n            Self::ArithmeticOverflow => write!(f, "candidate layout arithmetic overflow"),\n            Self::VmNotReplicatable(vm) => {\n                write!(f, "vm{vm} is active but not checkpoint-replicatable")\n            }\n            Self::VmCheckpointRequest(vm) => write!(f, "vm{vm} checkpoint request failed"),\n            Self::VmCheckpointTimeout(vm) => write!(f, "vm{vm} checkpoint timed out"),\n            Self::VmCheckpointStore(vm) => write!(f, "vm{vm} persistent checkpoint failed"),\n            Self::ApRendezvous(reason) => write!(f, "AP rendezvous failed ({reason})"),\n        }\n    }\n}\n\n#[derive(Clone, Copy)]\nstruct ElfSection {\n    addr: u64,\n    size: usize,\n}\n\n#[derive(Clone, Copy)]\nstruct LoadSegment {\n    vaddr: u64,\n    flags: u32,\n    offset: usize,\n    file_size: usize,\n    mem_size: usize,\n}\n\nstruct ParsedElf {\n    entry: u64,\n    min_vaddr: u64,\n    max_vaddr: u64,\n    loads: Vec<LoadSegment>,\n    limine_requests: ElfSection,\n    live_manifest: ElfSection,\n}\n\nstruct TablePool {\n    next_phys: u64,\n    end_phys: u64,\n    hhdm: u64,\n}\n\nimpl TablePool {\n    unsafe fn alloc_zeroed(&mut self) -> Result<u64, LiveUpdateError> {\n        let phys = align_up_u64(self.next_phys, PAGE_SIZE as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let end = phys\n            .checked_add(PAGE_SIZE as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        if end > self.end_phys {\n            return Err(LiveUpdateError::OutOfMemory);\n        }\n        let virt = self\n            .hhdm\n            .checked_add(phys)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)? as *mut u8;\n        ptr::write_bytes(virt, 0, PAGE_SIZE);\n        self.next_phys = end;\n        Ok(phys)\n    }\n\n    unsafe fn table_ptr(&self, phys: u64) -> Result<*mut u64, LiveUpdateError> {\n        let virt = self\n            .hhdm\n            .checked_add(phys)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        Ok(virt as *mut u64)\n    }\n\n    unsafe fn child_table(\n        &mut self,\n        parent_phys: u64,\n        index: usize,\n    ) -> Result<u64, LiveUpdateError> {\n        let parent = self.table_ptr(parent_phys)?;\n        let entry = ptr::read_volatile(parent.add(index));\n        if entry & 1 != 0 {\n            return Ok(entry & PAGE_MASK);\n        }\n        let child = self.alloc_zeroed()?;\n        ptr::write_volatile(parent.add(index), child | 0x003);\n        Ok(child)\n    }\n}\n\nstruct StagedCandidate {\n    arena_phys: u64,\n    arena_len: usize,\n    control_hhdm: u64,\n    handoff_hhdm: u64,\n    expected_aps: u64,\n    transition_installed: bool,\n    committed: bool,\n}\n\nimpl StagedCandidate {\n    fn control(&self) -> &\'static TransitionControl {\n        unsafe { &*(self.control_hhdm as *const TransitionControl) }\n    }\n\n    fn handoff_mut(&mut self) -> &\'static mut WarmHandoff {\n        unsafe { &mut *(self.handoff_hhdm as *mut WarmHandoff) }\n    }\n\n    fn set_vm_plan(\n        &mut self,\n        restore_mask: [u64; RESTORE_WORDS],\n        resume_mask: [u64; RESTORE_WORDS],\n        vm_heap_ranges: [WarmReservedRange; VM_ID_LIMIT],\n    ) {\n        let handoff = self.handoff_mut();\n        handoff.restore_mask = restore_mask;\n        handoff.resume_mask = resume_mask;\n        handoff.vm_heap_ranges = vm_heap_ranges;\n        handoff.checksum = handoff_checksum(handoff);\n    }\n\n    fn mark_committed(&mut self) {\n        let handoff = self.handoff_mut();\n        handoff.state = HANDOFF_STATE_COMMITTED;\n        handoff.checksum = handoff_checksum(handoff);\n        self.committed = true;\n    }\n}\n\nimpl Drop for StagedCandidate {\n    fn drop(&mut self) {\n        if !self.committed {\n            if self.transition_installed {\n                unsafe { clear_transition_mapping(self) };\n            }\n            let _ = crate::phys::free_phys_range(self.arena_phys, self.arena_len);\n        }\n    }\n}\n\nstruct CheckpointPlan {\n    restore_mask: [u64; RESTORE_WORDS],\n    resume_mask: [u64; RESTORE_WORDS],\n    vm_heap_ranges: [WarmReservedRange; VM_ID_LIMIT],\n    paused_by_update: Vec<u8>,\n}\n\nstruct LiveUpdateRunGuard;\n\nimpl LiveUpdateRunGuard {\n    fn acquire() -> Result<Self, LiveUpdateError> {\n        LIVE_UPDATE_IN_PROGRESS\n            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)\n            .map(|_| Self)\n            .map_err(|_| LiveUpdateError::Busy)\n    }\n}\n\nimpl Drop for LiveUpdateRunGuard {\n    fn drop(&mut self) {\n        LIVE_UPDATE_IN_PROGRESS.store(false, Ordering::Release);\n    }\n}\n\npub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {\n    idt[RENDEZVOUS_VECTOR].set_handler_fn(live_update_rendezvous_isr);\n}\n\n#[allow(non_snake_case)]\nextern "x86-interrupt" fn live_update_rendezvous_isr(_frame: InterruptStackFrame) {\n    crate::remote_work_wake::local_eoi();\n\n    if POST_BOOT_TLB_ACTIVE.load(Ordering::Acquire) {\n        unsafe { reload_cr3() };\n        POST_BOOT_TLB_ACKS.fetch_add(1, Ordering::AcqRel);\n        return;\n    }\n\n    let control_addr = ACTIVE_CONTROL_HHDM.load(Ordering::Acquire);\n    if control_addr == 0 {\n        return;\n    }\n    RENDEZVOUS_ISR_ACTIVE.fetch_add(1, Ordering::AcqRel);\n    // Pair pointer publication with abort-side invalidation. If abort won the\n    // race before this ISR became visible, do not dereference the arena.\n    if ACTIVE_CONTROL_HHDM.load(Ordering::Acquire) != control_addr {\n        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);\n        return;\n    }\n    let control = unsafe { &*(control_addr as *const TransitionControl) };\n    if control.command.load(Ordering::Acquire) != TRANSITION_PARK {\n        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);\n        return;\n    }\n\n    let slot = crate::percpu::current_slot();\n    if slot == 0 || slot >= AP_TRANSITION_ENTERED.len() {\n        control.failures.fetch_add(1, Ordering::AcqRel);\n        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);\n        return;\n    }\n    if AP_TRANSITION_ENTERED[slot].swap(true, Ordering::AcqRel) {\n        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);\n        return;\n    }\n\n    control.arrived.fetch_add(1, Ordering::AcqRel);\n    let left_vmx = match crate::hv::leave_vmx_root_for_current_cpu_contract() {\n        Ok(left) => left,\n        Err(_) => {\n            control.failures.fetch_add(1, Ordering::AcqRel);\n            AP_TRANSITION_ENTERED[slot].store(false, Ordering::Release);\n            control.arrived.fetch_sub(1, Ordering::AcqRel);\n            RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);\n            return;\n        }\n    };\n    let lapic_id = crate::percpu::this_cpu().lapic_id();\n    let park: extern "C" fn(*const TransitionControl, usize, u32) = unsafe {\n        core::mem::transmute(control.ap_park_hhdm as usize)\n    };\n    park(control, slot, lapic_id);\n\n    // The dedicated transition mapping returns only when the BSP aborts\n    // before the irreversible stack-switch phase. `arrived` is released last:\n    // once the BSP observes zero it may unmap and free the control arena.\n    if left_vmx && crate::hv::enter_vmx_root_for_current_cpu_contract().is_err() {\n        control.failures.fetch_add(1, Ordering::AcqRel);\n    }\n    AP_TRANSITION_ENTERED[slot].store(false, Ordering::Release);\n    control.arrived.fetch_sub(1, Ordering::AcqRel);\n    RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);\n}\n\n#[unsafe(no_mangle)]\n#[unsafe(link_section = ".live_update_trampoline")]\n#[unsafe(naked)]\nunsafe extern "C" fn trueos_live_update_ap_park_trampoline(\n    _control: *const TransitionControl,\n    _slot: usize,\n    _lapic_id: u32,\n) {\n    core::arch::naked_asm!(\n        "mov r8, rdi",\n        "mov r9, rsi",\n        "mov r10, rdx",\n        "1:",\n        "mov rax, qword ptr [r8 + {command}]",\n        "cmp rax, {abort}",\n        "je 9f",\n        "cmp rax, {switch_stacks}",\n        "je 3f",\n        "pause",\n        "jmp 1b",\n        "3:",\n        "mov rax, r9",\n        "inc rax",\n        "imul rax, qword ptr [r8 + {stack_stride}]",\n        "add rax, qword ptr [r8 + {stack_base}]",\n        "mov rsp, rax",\n        "and rsp, -16",\n        "lgdt [r8 + {transition_gdtr}]",\n        "lock inc qword ptr [r8 + {stacked}]",\n        "4:",\n        "mov rax, qword ptr [r8 + {command}]",\n        "cmp rax, {commit}",\n        "jne 5f",\n        // Flush global and non-global translations after the BSP replaces the\n        // shared root PML4 kernel entry.\n        "mov rcx, cr4",\n        "mov rdx, rcx",\n        "and rcx, -129",\n        "mov cr4, rcx",\n        "mov rax, qword ptr [r8 + {cr3}]",\n        "mov cr3, rax",\n        "mov cr4, rdx",\n        "mov rax, qword ptr [r8 + {ap_entry}]",\n        "mov rdi, r10",\n        "mov rsi, r9",\n        // A direct jump into an extern-C function needs the same stack\n        // alignment the callee would observe after CALL.\n        "sub rsp, 8",\n        "jmp rax",\n        "5:",\n        "pause",\n        "jmp 4b",\n        "9:",\n        "ret",\n        command = const offset_of!(TransitionControl, command),\n        stacked = const offset_of!(TransitionControl, stacked),\n        cr3 = const offset_of!(TransitionControl, cr3),\n        stack_base = const offset_of!(TransitionControl, stack_base_hhdm),\n        stack_stride = const offset_of!(TransitionControl, stack_stride),\n        ap_entry = const offset_of!(TransitionControl, ap_entry),\n        transition_gdtr = const offset_of!(TransitionControl, transition_gdtr),\n        abort = const TRANSITION_ABORT,\n        switch_stacks = const TRANSITION_SWITCH_STACKS,\n        commit = const TRANSITION_COMMIT,\n    );\n}\n\n#[unsafe(no_mangle)]\n#[unsafe(link_section = ".live_update_trampoline")]\n#[unsafe(naked)]\nunsafe extern "C" fn trueos_live_update_bsp_commit_trampoline(\n    _control: *const TransitionControl,\n) -> ! {\n    core::arch::naked_asm!(\n        "mov r8, rdi",\n        "mov rsp, qword ptr [r8 + {stack_base}]",\n        "add rsp, qword ptr [r8 + {stack_stride}]",\n        "and rsp, -16",\n        "lgdt [r8 + {transition_gdtr}]",\n        "mov rax, qword ptr [r8 + {root_hhdm}]",\n        "mov rcx, qword ptr [r8 + {kernel_slot}]",\n        "mov rdx, qword ptr [r8 + {new_slot_entry}]",\n        "mov qword ptr [rax + rcx * 8], rdx",\n        "mfence",\n        // Toggling CR4.PGE guarantees stale global kernel translations are\n        // discarded before execution enters the replacement image.\n        "mov rcx, cr4",\n        "mov rdx, rcx",\n        "and rcx, -129",\n        "mov cr4, rcx",\n        "mov rax, qword ptr [r8 + {cr3}]",\n        "mov cr3, rax",\n        "mov cr4, rdx",\n        "mov rax, {commit}",\n        "mov qword ptr [r8 + {command}], rax",\n        "mfence",\n        // Discard generation-N per-CPU identity before candidate Rust runs.\n        "mov ecx, 0xC0000101",\n        "xor eax, eax",\n        "xor edx, edx",\n        "wrmsr",\n        "mov rax, qword ptr [r8 + {bsp_entry}]",\n        "jmp rax",\n        command = const offset_of!(TransitionControl, command),\n        cr3 = const offset_of!(TransitionControl, cr3),\n        root_hhdm = const offset_of!(TransitionControl, root_hhdm),\n        kernel_slot = const offset_of!(TransitionControl, kernel_slot),\n        new_slot_entry = const offset_of!(TransitionControl, new_slot_entry),\n        stack_base = const offset_of!(TransitionControl, stack_base_hhdm),\n        stack_stride = const offset_of!(TransitionControl, stack_stride),\n        bsp_entry = const offset_of!(TransitionControl, bsp_entry),\n        transition_gdtr = const offset_of!(TransitionControl, transition_gdtr),\n        commit = const TRANSITION_COMMIT,\n    );\n}\n\n#[unsafe(no_mangle)]\npub unsafe extern "C" fn trueos_live_update_ap_entry(lapic_id: u32, slot: u32) -> ! {\n    // The candidate BSP may publish its fresh global PERCPU_READY before this\n    // AP has installed a candidate PerCpu. Clear the generation-N GS pointer\n    // so any defensive identity probe sees "not initialized" rather than an\n    // unmapped old-kernel allocation.\n    core::arch::asm!(\n        "wrmsr",\n        in("ecx") 0xC000_0101u32,\n        in("eax") 0u32,\n        in("edx") 0u32,\n        options(nostack, preserves_flags),\n    );\n    while !WARM_APS_RELEASED.load(Ordering::Acquire) {\n        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));\n    }\n    crate::cpu::warm_ap_start(lapic_id, slot)\n}\n\npub fn warm_boot_active() -> bool {\n    warm_handoff().is_some()\n}\n\npub fn warm_generation() -> Option<u64> {\n    warm_handoff().map(|handoff| handoff.generation)\n}\n\npub fn warm_hhdm_offset() -> Option<u64> {\n    warm_handoff().map(|handoff| handoff.hhdm_base)\n}\n\npub fn warm_kernel_bases() -> Option<(u64, u64)> {\n    warm_handoff().map(|handoff| (handoff.kernel_virt_base, handoff.kernel_phys_base))\n}\n\npub fn for_each_warm_reserved_phys_range(mut visit: impl FnMut(u64, u64)) {\n    let Some(handoff) = warm_handoff() else {\n        return;\n    };\n    visit(handoff.arena_phys, handoff.arena_len);\n    for range in handoff.vm_heap_ranges {\n        if range.valid() {\n            visit(range.phys_start, range.length);\n        }\n    }\n}\n\npub fn claim_warm_vm_heap_range(phys_start: u64, length: u64) -> bool {\n    let Some(handoff) = warm_handoff() else {\n        return false;\n    };\n    for (index, range) in handoff.vm_heap_ranges.iter().copied().enumerate() {\n        if range.phys_start == phys_start && range.length == length && range.valid() {\n            return WARM_VM_RANGE_CLAIMED[index]\n                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)\n                .is_ok();\n        }\n    }\n    false\n}\n\npub fn warm_kernel_file_bytes() -> Option<&\'static [u8]> {\n    let handoff = warm_handoff()?;\n    let virt = handoff.hhdm_base.checked_add(handoff.kernel_file_phys)?;\n    let len = usize::try_from(handoff.kernel_file_len).ok()?;\n    Some(unsafe { core::slice::from_raw_parts(virt as *const u8, len) })\n}\n\npub fn log_boot_mode() {\n    if let Some(handoff) = warm_handoff() {\n        crate::log_info!(\n            target: "global";\n            "live-update: generation={} candidate_hash=0x{:016X} arena=0x{:016X}+0x{:X} expected_aps={} mode=fullforget-warm\\n",\n            handoff.generation,\n            handoff.candidate_hash,\n            handoff.arena_phys,\n            handoff.arena_len,\n            handoff.expected_aps,\n        );\n    }\n}\n\npub fn release_warm_aps() {\n    if let Some(handoff) = warm_handoff() {\n        WARM_APS_RELEASED.store(true, Ordering::Release);\n        crate::log_info!(\n            target: "global";\n            "live-update: released {} parked APs into generation {}\\n",\n            handoff.expected_aps,\n            handoff.generation,\n        );\n    }\n}\n\npub fn spawn_post_boot(spawner: Spawner) {\n    let Some(handoff) = warm_handoff().copied() else {\n        return;\n    };\n\n    SHELL_NOTICE_PENDING.store(true, Ordering::Release);\n    match restore_after_live_update_task(\n        spawner,\n        handoff.restore_mask,\n        handoff.resume_mask,\n        handoff.transition_slot,\n        handoff.generation,\n    ) {\n        Ok(token) => spawner.spawn(token),\n        Err(error) => crate::log_warn!(\n            target: "global";\n            "live-update: restore task unavailable generation={} error={:?}\\n",\n            handoff.generation,\n            error,\n        ),\n    }\n}\n\npub fn take_shell_notice() -> Option<&\'static [u8]> {\n    SHELL_NOTICE_PENDING\n        .swap(false, Ordering::AcqRel)\n        .then_some(SHELL_NOTICE)\n}\n\npub fn rearm_shell_notice() {\n    if warm_boot_active() {\n        SHELL_NOTICE_PENDING.store(true, Ordering::Release);\n    }\n}\n\n#[embassy_executor::task]\nasync fn restore_after_live_update_task(\n    spawner: Spawner,\n    restore_mask: [u64; RESTORE_WORDS],\n    resume_mask: [u64; RESTORE_WORDS],\n    transition_slot: u64,\n    generation: u64,\n) {\n    let topology_deadline = Instant::now()\n        .as_millis()\n        .saturating_add(POST_BOOT_SERVICE_TIMEOUT_MS);\n    while !crate::workers::all_topology_spawners_registered()\n        && Instant::now().as_millis() < topology_deadline\n    {\n        Timer::after(EmbassyDuration::from_millis(25)).await;\n    }\n    cleanup_transition_mapping_after_boot(transition_slot as usize).await;\n\n    if restore_mask.iter().all(|word| *word == 0) {\n        crate::log_info!(\n            target: "global";\n            "live-update: generation={} has no VM checkpoints to restore\\n",\n            generation,\n        );\n        return;\n    }\n\n    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;\n    let deadline = Instant::now()\n        .as_millis()\n        .saturating_add(POST_BOOT_SERVICE_TIMEOUT_MS);\n    while !crate::hv::store::online() && Instant::now().as_millis() < deadline\n    {\n        Timer::after(EmbassyDuration::from_millis(25)).await;\n    }\n\n    for vm_id in 0..VM_ID_LIMIT {\n        if !mask_contains(&restore_mask, vm_id) {\n            continue;\n        }\n        let vm_id = vm_id as u8;\n        let name = checkpoint_name(vm_id);\n\n        let _ = crate::hv::eject(vm_id);\n        match crate::hv::try_begin_restore(vm_id) {\n            Ok(true) => {}\n            Ok(false) => {\n                crate::log_warn!(\n                    target: "global";\n                    "live-update: vm{} restore already pending name={}\\n",\n                    vm_id,\n                    name,\n                );\n                continue;\n            }\n            Err(error) => {\n                crate::log_warn!(\n                    target: "global";\n                    "live-update: vm{} restore admission failed name={} error={:?}\\n",\n                    vm_id,\n                    name,\n                    error,\n                );\n                continue;\n            }\n        }\n\n        let image = match crate::hv::store::load_persistent_async(name.as_str()).await {\n            Ok(image) => image,\n            Err(error) => {\n                crate::log_warn!(\n                    target: "global";\n                    "live-update: vm{} checkpoint load failed name={} error={:?}\\n",\n                    vm_id,\n                    name,\n                    error,\n                );\n                crate::hv::finish_restore(vm_id);\n                continue;\n            }\n        };\n        if let Err(error) = crate::hv::store::save_bytes_async(vm_id, image.snapshot.clone()).await {\n            crate::log_warn!(\n                target: "global";\n                "live-update: vm{} warm-store seed failed name={} error={:?}\\n",\n                vm_id,\n                name,\n                error,\n            );\n            crate::hv::finish_restore(vm_id);\n            continue;\n        }\n        if let Err(error) = crate::hv::restore_persistent_image(vm_id, &image, None) {\n            crate::log_warn!(\n                target: "global";\n                "live-update: vm{} envelope import failed name={} error={:?}\\n",\n                vm_id,\n                name,\n                error,\n            );\n            crate::hv::finish_restore(vm_id);\n            continue;\n        }\n\n        if mask_contains(&resume_mask, vm_id as usize) {\n            match crate::hv::start(vm_id, &spawner, None) {\n                Ok(()) => crate::log_info!(\n                    target: "global";\n                    "live-update: vm{} restored and resume scheduled name={} generation={}\\n",\n                    vm_id,\n                    name,\n                    generation,\n                ),\n                Err(error) => crate::log_warn!(\n                    target: "global";\n                    "live-update: vm{} restored but resume failed name={} error={:?}\\n",\n                    vm_id,\n                    name,\n                    error,\n                ),\n            }\n        } else {\n            crate::log_info!(\n                target: "global";\n                "live-update: vm{} restored in retained-pause state name={} generation={}\\n",\n                vm_id,\n                name,\n                generation,\n            );\n        }\n        crate::hv::finish_restore(vm_id);\n    }\n}\n\npub async fn stage_and_swap(\n    kernel: Vec<u8>,\n    spawner: Spawner,\n    target: MatrixTarget,\n) -> Result<Infallible, LiveUpdateError> {\n    let _run_guard = LiveUpdateRunGuard::acquire()?;\n    if matrix_target_interrupted(&target) {\n        return Err(LiveUpdateError::Interrupted);\n    }\n\n    print_matrix_target_line(\n        &target,\n        "update live: staging candidate in RAM; kernel disk image will not be changed",\n    );\n    let mut staged = stage_candidate(kernel.as_slice())?;\n    print_matrix_target_line(\n        &target,\n        format!(\n            "update live: candidate staged arena=0x{:016X}+{} MiB APs={}",\n            staged.arena_phys,\n            staged.arena_len / (1024 * 1024),\n            staged.expected_aps,\n        )\n        .as_str(),\n    );\n    drop(kernel);\n\n    print_matrix_target_line(\n        &target,\n        "update live: checkpointing active VMX apps to TRUEOSFS (candidate remains RAM-only)",\n    );\n    let checkpoint = checkpoint_active_vms(&spawner, &target).await?;\n    staged.set_vm_plan(\n        checkpoint.restore_mask,\n        checkpoint.resume_mask,\n        checkpoint.vm_heap_ranges,\n    );\n\n    if matrix_target_interrupted(&target) {\n        resume_checkpointed_vms(&spawner, &target, checkpoint.paused_by_update.as_slice()).await;\n        return Err(LiveUpdateError::Interrupted);\n    }\n\n    // Snapshot BDFs before APs are parked. The irreversible path uses this\n    // immutable list and never takes the ordinary PCI registry/config locks.\n    let pci_snapshot = crate::pci::fullforget_snapshot();\n    print_matrix_target_line(\n        &target,\n        format!(\n            "update live: captured {} PCI functions for lock-free DMA containment",\n            pci_snapshot.len(),\n        )\n        .as_str(),\n    );\n\n    print_matrix_target_line(\n        &target,\n        "update live: final rendezvous next; on success the TCP shell will disconnect",\n    );\n    print_matrix_target_line(\n        &target,\n        "update live: reconnect after boot for: hey that worked, new kernel here :)",\n    );\n    // Flush the final user-facing line while every normal runtime service is\n    // still schedulable. After AP rendezvous succeeds the path takes no locks,\n    // performs no allocation, and never returns to the old kernel.\n    Timer::after(EmbassyDuration::from_millis(100)).await;\n\n    if let Err(error) = rendezvous_aps(&mut staged) {\n        resume_checkpointed_vms(&spawner, &target, checkpoint.paused_by_update.as_slice()).await;\n        return Err(error);\n    }\n\n    staged.mark_committed();\n    unsafe { commit_fullforget(&staged, pci_snapshot.as_slice()) }\n}\n\nasync fn checkpoint_active_vms(\n    spawner: &Spawner,\n    target: &MatrixTarget,\n) -> Result<CheckpointPlan, LiveUpdateError> {\n    let mut restore_mask = [0u64; RESTORE_WORDS];\n    let mut resume_mask = [0u64; RESTORE_WORDS];\n    let mut vm_heap_ranges = [WarmReservedRange::EMPTY; VM_ID_LIMIT];\n    let mut paused_by_update = Vec::new();\n\n    for vm_index in 0..VM_ID_LIMIT {\n        let vm_id = vm_index as u8;\n        let state = crate::hv::vm_state(vm_id);\n        if !state.supported || !(state.running || state.starting || state.pause_latched) {\n            continue;\n        }\n\n        if state.running || state.starting {\n            mask_insert(&mut resume_mask, vm_index);\n            if !state.replicatable {\n                return checkpoint_abort(spawner, target, &paused_by_update, LiveUpdateError::VmNotReplicatable(vm_id)).await;\n            }\n            match crate::hv::request_replicatable_snapshot(vm_id) {\n                Ok(true) => {\n                    if !paused_by_update.contains(&vm_id) {\n                        paused_by_update.push(vm_id);\n                    }\n                    mask_insert(&mut resume_mask, vm_index);\n                    print_matrix_target_line(\n                        target,\n                        format!("update live: vm{} PreparePause snapshot requested", vm_id)\n                            .as_str(),\n                    );\n                }\n                Ok(false) if state.prepare_pause_pending => {}\n                Ok(false) | Err(_) => {\n                    return checkpoint_abort(spawner, target, &paused_by_update, LiveUpdateError::VmCheckpointRequest(vm_id)).await;\n                }\n            }\n        } else if !state.pause_snapshot_ready || !crate::hv::store::has_committed_vm(vm_id) {\n            return checkpoint_abort(spawner, target, &paused_by_update, LiveUpdateError::VmCheckpointRequest(vm_id)).await;\n        }\n\n        let deadline = Instant::now()\n            .as_millis()\n            .saturating_add(VM_CHECKPOINT_TIMEOUT_MS);\n        loop {\n            if matrix_target_interrupted(target) {\n                return checkpoint_abort(spawner, target, &paused_by_update, LiveUpdateError::Interrupted).await;\n            }\n            let state = crate::hv::vm_state(vm_id);\n            if state.pause_latched\n                && state.pause_snapshot_ready\n                && crate::hv::store::has_committed_vm(vm_id)\n            {\n                break;\n            }\n            if Instant::now().as_millis() >= deadline {\n                return checkpoint_abort(spawner, target, &paused_by_update, LiveUpdateError::VmCheckpointTimeout(vm_id)).await;\n            }\n            Timer::after(EmbassyDuration::from_millis(10)).await;\n        }\n\n        let name = checkpoint_name(vm_id);\n        match crate::hv::store::store_persistent_async(vm_id, name.as_str()).await {\n            Ok(bytes) => {\n                let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id) else {\n                    return checkpoint_abort(\n                        spawner,\n                        target,\n                        &paused_by_update,\n                        LiveUpdateError::VmCheckpointStore(vm_id),\n                    )\n                    .await;\n                };\n                let heap_len = stats.heap_end.saturating_sub(stats.heap_start);\n                if stats.phys_start == 0 || heap_len == 0 {\n                    return checkpoint_abort(\n                        spawner,\n                        target,\n                        &paused_by_update,\n                        LiveUpdateError::VmCheckpointStore(vm_id),\n                    )\n                    .await;\n                }\n                vm_heap_ranges[vm_index] = WarmReservedRange {\n                    phys_start: stats.phys_start as u64,\n                    length: heap_len as u64,\n                };\n                mask_insert(&mut restore_mask, vm_index);\n                print_matrix_target_line(\n                    target,\n                    format!(\n                        "update live: vm{} checkpointed as {} ({} bytes)",\n                        vm_id, name, bytes\n                    )\n                    .as_str(),\n                );\n            }\n            Err(error) => {\n                print_matrix_target_line(\n                    target,\n                    format!(\n                        "update live: vm{} persistent checkpoint failed ({:?})",\n                        vm_id, error\n                    )\n                    .as_str(),\n                );\n                return checkpoint_abort(spawner, target, &paused_by_update, LiveUpdateError::VmCheckpointStore(vm_id)).await;\n            }\n        }\n    }\n\n    Ok(CheckpointPlan {\n        restore_mask,\n        resume_mask,\n        vm_heap_ranges,\n        paused_by_update,\n    })\n}\n\nasync fn checkpoint_abort(\n    spawner: &Spawner,\n    target: &MatrixTarget,\n    touched_vms: &[u8],\n    error: LiveUpdateError,\n) -> Result<CheckpointPlan, LiveUpdateError> {\n    resume_checkpointed_vms(spawner, target, touched_vms).await;\n    Err(error)\n}\n\nasync fn resume_checkpointed_vms(\n    spawner: &Spawner,\n    target: &MatrixTarget,\n    vm_ids: &[u8],\n) {\n    for &vm_id in vm_ids {\n        // A PreparePause may cross its Ready boundary just after an update\n        // cancellation. Wait briefly for that boundary so the compensating\n        // start cannot race and leave the VM paused after this task returns.\n        let deadline = Instant::now().as_millis().saturating_add(2_000);\n        loop {\n            let state = crate::hv::vm_state(vm_id);\n            if state.pause_latched || !state.prepare_pause_pending {\n                break;\n            }\n            if Instant::now().as_millis() >= deadline {\n                break;\n            }\n            Timer::after(EmbassyDuration::from_millis(10)).await;\n        }\n\n        match crate::hv::start(vm_id, spawner, None) {\n            Ok(()) => print_matrix_target_line(\n                target,\n                format!("update live: vm{} resumed after pre-commit abort", vm_id).as_str(),\n            ),\n            Err(crate::hv::StartError::AlreadyRunning) => {}\n            Err(error) => print_matrix_target_line(\n                target,\n                format!("update live: vm{} resume after abort failed ({:?})", vm_id, error)\n                    .as_str(),\n            ),\n        }\n    }\n}\n\nfn rendezvous_aps(staged: &mut StagedCandidate) -> Result<(), LiveUpdateError> {\n    unsafe { install_transition_mapping(staged)? };\n    let control = staged.control();\n    control.arrived.store(0, Ordering::Release);\n    control.stacked.store(0, Ordering::Release);\n    control.failures.store(0, Ordering::Release);\n\n    let interrupts_were_enabled = x86_64::instructions::interrupts::are_enabled();\n    x86_64::instructions::interrupts::disable();\n    ACTIVE_CONTROL_HHDM.store(staged.control_hhdm, Ordering::Release);\n    control.command.store(TRANSITION_PARK, Ordering::Release);\n\n    for slot in 1..crate::percpu::total_slots() {\n        if !crate::remote_work_wake::send_fixed_x2apic_ipi(slot as u32, RENDEZVOUS_VECTOR) {\n            control.failures.fetch_add(1, Ordering::AcqRel);\n            abort_rendezvous(staged, interrupts_were_enabled);\n            return Err(LiveUpdateError::ApRendezvous("IPI delivery unavailable"));\n        }\n    }\n\n    // Once an AP has entered the transition trampoline it may be holding an\n    // arbitrary old-generation lock. Do not yield to Embassy or execute any\n    // normal service code from this point forward. A bounded lock-free spin is\n    // the only safe pre-commit rendezvous.\n    let timeout_ticks = AP_RENDEZVOUS_TIMEOUT_MS\n        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))\n        .saturating_add(999)\n        / 1000;\n    let deadline = embassy_time_driver::now().saturating_add(timeout_ticks);\n    while control.arrived.load(Ordering::Acquire) < staged.expected_aps {\n        if control.failures.load(Ordering::Acquire) != 0 {\n            abort_rendezvous(staged, interrupts_were_enabled);\n            return Err(LiveUpdateError::ApRendezvous("AP reported transition failure"));\n        }\n        if embassy_time_driver::now() >= deadline {\n            abort_rendezvous(staged, interrupts_were_enabled);\n            return Err(LiveUpdateError::ApRendezvous("timeout"));\n        }\n        core::hint::spin_loop();\n    }\n\n    // Success intentionally leaves BSP interrupts disabled. The caller performs\n    // only handoff bookkeeping before entering the non-returning commit path.\n    Ok(())\n}\n\nfn abort_rendezvous(staged: &mut StagedCandidate, interrupts_were_enabled: bool) {\n    let control = staged.control();\n    control.command.store(TRANSITION_ABORT, Ordering::Release);\n    // Prevent a late IPI from acquiring the control pointer while the existing\n    // ISR/trampoline population drains.\n    ACTIVE_CONTROL_HHDM.store(0, Ordering::Release);\n    let timeout_ticks = ABORT_DRAIN_TIMEOUT_MS\n        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))\n        .saturating_add(999)\n        / 1000;\n    let deadline = embassy_time_driver::now().saturating_add(timeout_ticks);\n    while control.arrived.load(Ordering::Acquire) != 0\n        || RENDEZVOUS_ISR_ACTIVE.load(Ordering::Acquire) != 0\n    {\n        if embassy_time_driver::now() >= deadline {\n            // An AP still executes transition code. Unmapping/freeing the arena\n            // would create an immediate use-after-free, so fail-stop and require\n            // the same physical reset the operator was already prepared to use.\n            loop {\n                core::arch::asm!("cli", "hlt", options(nomem, nostack));\n            }\n        }\n        core::hint::spin_loop();\n    }\n    unsafe { clear_transition_mapping(staged) };\n    if interrupts_were_enabled {\n        x86_64::instructions::interrupts::enable();\n    }\n}\n\nunsafe fn install_transition_mapping(\n    staged: &mut StagedCandidate,\n) -> Result<(), LiveUpdateError> {\n    if staged.transition_installed {\n        return Ok(());\n    }\n    let control = staged.control();\n    let root = control.root_hhdm as *mut u64;\n    let slot = usize::try_from(control.transition_slot)\n        .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;\n    if slot >= 512 {\n        return Err(LiveUpdateError::Incompatible(\n            "transition PML4 slot is invalid",\n        ));\n    }\n    let old = ptr::read_volatile(root.add(slot));\n    if old & 1 != 0 {\n        return Err(LiveUpdateError::Incompatible(\n            "transition PML4 slot became occupied",\n        ));\n    }\n    ptr::write_volatile(root.add(slot), control.transition_slot_entry);\n    core::arch::asm!("mfence", options(nostack, preserves_flags));\n    reload_cr3();\n    staged.transition_installed = true;\n    Ok(())\n}\n\nunsafe fn clear_transition_mapping(staged: &mut StagedCandidate) {\n    if !staged.transition_installed {\n        return;\n    }\n    let control = staged.control();\n    let slot = control.transition_slot as usize;\n    if slot < 512 {\n        let root = control.root_hhdm as *mut u64;\n        let current = ptr::read_volatile(root.add(slot));\n        if current == control.transition_slot_entry {\n            ptr::write_volatile(root.add(slot), 0);\n            core::arch::asm!("mfence", options(nostack, preserves_flags));\n            reload_cr3();\n        }\n    }\n    staged.transition_installed = false;\n}\n\nasync fn cleanup_transition_mapping_after_boot(slot: usize) {\n    if slot < 256 || slot >= 512 {\n        crate::log_warn!(\n            target: "global";\n            "live-update: transition mapping cleanup skipped invalid_slot={}\\n",\n            slot,\n        );\n        return;\n    }\n    let Some(hhdm) = warm_hhdm_offset() else {\n        return;\n    };\n    let cr3 = read_cr3();\n    let Some(root_hhdm) = hhdm.checked_add(cr3 & PAGE_MASK) else {\n        return;\n    };\n\n    unsafe {\n        let root = root_hhdm as *mut u64;\n        ptr::write_volatile(root.add(slot), 0);\n        core::arch::asm!("mfence", options(nostack, preserves_flags));\n        reload_cr3();\n    }\n\n    POST_BOOT_TLB_ACKS.store(0, Ordering::Release);\n    POST_BOOT_TLB_ACTIVE.store(true, Ordering::Release);\n    let expected = crate::percpu::total_slots().saturating_sub(1) as u64;\n    let mut sent = 0u64;\n    for cpu_slot in 1..crate::percpu::total_slots() {\n        if crate::remote_work_wake::send_fixed_x2apic_ipi(\n            cpu_slot as u32,\n            RENDEZVOUS_VECTOR,\n        ) {\n            sent = sent.saturating_add(1);\n        }\n    }\n    let deadline = Instant::now()\n        .as_millis()\n        .saturating_add(POST_BOOT_TLB_TIMEOUT_MS);\n    while POST_BOOT_TLB_ACKS.load(Ordering::Acquire) < sent\n        && Instant::now().as_millis() < deadline\n    {\n        Timer::after(EmbassyDuration::from_millis(1)).await;\n    }\n    POST_BOOT_TLB_ACTIVE.store(false, Ordering::Release);\n    let acknowledgements = POST_BOOT_TLB_ACKS.load(Ordering::Acquire);\n    crate::log_info!(\n        target: "global";\n        "live-update: transition mapping retired slot={} tlb_acks={}/{} topology_aps={}\\n",\n        slot,\n        acknowledgements,\n        sent,\n        expected,\n    );\n}\n\n#[inline]\nfn read_cr3() -> u64 {\n    let value: u64;\n    unsafe {\n        core::arch::asm!(\n            "mov {}, cr3",\n            out(reg) value,\n            options(nomem, nostack, preserves_flags)\n        );\n    }\n    value\n}\n\n#[inline]\nunsafe fn reload_cr3() {\n    let value = read_cr3();\n    core::arch::asm!(\n        "mov cr3, {}",\n        in(reg) value,\n        options(nostack, preserves_flags)\n    );\n}\n\nunsafe fn find_empty_transition_slot(\n    root_hhdm: u64,\n    kernel_slot: usize,\n    hhdm_slot: usize,\n) -> Result<usize, LiveUpdateError> {\n    let root = root_hhdm as *const u64;\n    for slot in (256usize..512).rev() {\n        if slot == kernel_slot || slot == hhdm_slot {\n            continue;\n        }\n        if ptr::read_volatile(root.add(slot)) & 1 == 0 {\n            return Ok(slot);\n        }\n    }\n    Err(LiveUpdateError::Incompatible(\n        "no empty high-half PML4 slot for transition trampoline",\n    ))\n}\n\nfn canonical_pml4_slot_base(slot: usize) -> u64 {\n    let low = (slot as u64) << 39;\n    if slot & 0x100 != 0 {\n        low | 0xffff_0000_0000_0000\n    } else {\n        low\n    }\n}\n\nunsafe fn commit_fullforget(\n    staged: &StagedCandidate,\n    pci_snapshot: &[crate::pci::FullforgetPciFunction],\n) -> ! {\n    let control = staged.control();\n    x86_64::instructions::interrupts::disable();\n\n    // This path deliberately bypasses the normal PCI configuration lock. Every\n    // AP is parked, so no other CPU can race the CF8/CFC transaction; bypassing\n    // also avoids deadlock if an AP happened to be interrupted while owning the\n    // normal lock.\n    let dma_failures = crate::pci::fullforget_quiesce_unlocked(pci_snapshot);\n    if dma_failures != 0 {\n        // Proceeding with a requester that still owns Bus Master Enable would\n        // let an old-generation DMA engine overwrite replacement-kernel RAM.\n        loop {\n            core::arch::asm!("cli", "hlt", options(nomem, nostack));\n        }\n    }\n    let drain_ticks = PCI_DMA_DRAIN_MS\n        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))\n        .saturating_add(999)\n        / 1000;\n    let drain_deadline = embassy_time_driver::now().saturating_add(drain_ticks);\n    while embassy_time_driver::now() < drain_deadline {\n        core::hint::spin_loop();\n    }\n\n    control\n        .command\n        .store(TRANSITION_SWITCH_STACKS, Ordering::SeqCst);\n    while control.stacked.load(Ordering::Acquire) < staged.expected_aps {\n        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));\n    }\n\n    let commit: extern "C" fn(*const TransitionControl) -> ! =\n        core::mem::transmute(control.bsp_commit_hhdm as usize);\n    commit(control)\n}\n\nfn stage_candidate(kernel: &[u8]) -> Result<StagedCandidate, LiveUpdateError> {\n    if kernel.len() > MAX_KERNEL_FILE_BYTES {\n        return Err(LiveUpdateError::Incompatible(\n            "kernel file exceeds live-update cap",\n        ));\n    }\n    let elf = parse_elf(kernel)?;\n    let span = usize::try_from(elf.max_vaddr - elf.min_vaddr)\n        .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;\n    if span == 0 || span > MAX_KERNEL_SPAN_BYTES {\n        return Err(LiveUpdateError::Incompatible(\n            "kernel PT_LOAD span exceeds cap",\n        ));\n    }\n    if !range_in_load(&elf.loads, elf.entry, 1, 0x1) {\n        return Err(LiveUpdateError::BadElf(\n            "entry is not backed by an executable PT_LOAD",\n        ));\n    }\n    if !range_in_load(\n        &elf.loads,\n        elf.limine_requests.addr,\n        elf.limine_requests.size,\n        0x2,\n    ) {\n        return Err(LiveUpdateError::Incompatible(\n            ".limine_requests is not in a writable PT_LOAD",\n        ));\n    }\n    if !range_in_load(\n        &elf.loads,\n        elf.live_manifest.addr,\n        elf.live_manifest.size,\n        0,\n    ) {\n        return Err(LiveUpdateError::Incompatible(\n            ".live_update_slot is not backed by PT_LOAD",\n        ));\n    }\n\n    let hhdm = crate::limine::hhdm_offset()\n        .ok_or(LiveUpdateError::Incompatible("missing HHDM response"))?;\n    let (current_virt, _) = crate::limine::executable_address_bases().ok_or(\n        LiveUpdateError::Incompatible("missing executable address response"),\n    )?;\n    let kernel_slot = pml4_index(elf.min_vaddr);\n    if kernel_slot != pml4_index(current_virt) {\n        return Err(LiveUpdateError::Incompatible(\n            "candidate uses a different kernel PML4 slot",\n        ));\n    }\n    let hhdm_slot = pml4_index(hhdm);\n    if hhdm_slot == kernel_slot {\n        return Err(LiveUpdateError::Incompatible(\n            "HHDM collides with kernel PML4 slot",\n        ));\n    }\n\n    let cr3: u64;\n    unsafe {\n        core::arch::asm!(\n            "mov {}, cr3",\n            out(reg) cr3,\n            options(nomem, nostack, preserves_flags)\n        );\n    }\n    let root_phys = cr3 & PAGE_MASK;\n    let root_hhdm = hhdm\n        .checked_add(root_phys)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    let transition_slot = unsafe {\n        find_empty_transition_slot(root_hhdm, kernel_slot, hhdm_slot)?\n    };\n    let transition_base = canonical_pml4_slot_base(transition_slot);\n\n    let trampoline_start = ptr::addr_of!(__live_update_trampoline_start) as usize;\n    let trampoline_end = ptr::addr_of!(__live_update_trampoline_end) as usize;\n    let trampoline_len = trampoline_end.checked_sub(trampoline_start).ok_or(\n        LiveUpdateError::Incompatible("invalid transition trampoline bounds"),\n    )?;\n    if trampoline_len == 0 || trampoline_len > MAX_TRAMPOLINE_BYTES {\n        return Err(LiveUpdateError::Incompatible(\n            "transition trampoline size is invalid",\n        ));\n    }\n    let ap_park_offset = (trueos_live_update_ap_park_trampoline as usize)\n        .checked_sub(trampoline_start)\n        .ok_or(LiveUpdateError::Incompatible(\n            "AP trampoline is outside transition section",\n        ))?;\n    let bsp_commit_offset = (trueos_live_update_bsp_commit_trampoline as usize)\n        .checked_sub(trampoline_start)\n        .ok_or(LiveUpdateError::Incompatible(\n            "BSP trampoline is outside transition section",\n        ))?;\n    if ap_park_offset >= trampoline_len || bsp_commit_offset >= trampoline_len {\n        return Err(LiveUpdateError::Incompatible(\n            "transition trampoline symbol is outside copied section",\n        ));\n    }\n\n    let cpu_count = crate::percpu::total_slots().max(1);\n    if cpu_count > CPU_SLOT_LIMIT {\n        return Err(LiveUpdateError::Incompatible(\n            "CPU topology exceeds transition table",\n        ));\n    }\n\n    let load_offset = 0usize;\n    let file_offset = align_up_usize(span, PAGE_SIZE)?;\n    let stack_offset = align_up_usize(\n        file_offset\n            .checked_add(kernel.len())\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n        PAGE_SIZE,\n    )?;\n    let stack_bytes = (cpu_count + 1)\n        .checked_mul(AP_TRANSITION_STACK_BYTES)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    let control_offset = align_up_usize(\n        stack_offset\n            .checked_add(stack_bytes)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n        PAGE_SIZE,\n    )?;\n    let trampoline_offset = align_up_usize(\n        control_offset\n            .checked_add(PAGE_SIZE)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n        PAGE_SIZE,\n    )?;\n    let trampoline_map_len = align_up_usize(trampoline_len, PAGE_SIZE)?;\n    let tables_offset = align_up_usize(\n        trampoline_offset\n            .checked_add(trampoline_map_len)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n        PAGE_SIZE,\n    )?;\n\n    let kernel_pt_pages = ceil_div(span, TWO_MIB).saturating_add(2);\n    let kernel_pd_pages = ceil_div(span, ONE_GIB).saturating_add(2);\n    let transition_pt_pages = ceil_div(trampoline_map_len, TWO_MIB).saturating_add(1);\n    let transition_pd_pages = ceil_div(trampoline_map_len, ONE_GIB).saturating_add(1);\n    let table_pages = 2usize\n        .checked_add(kernel_pt_pages)\n        .and_then(|value| value.checked_add(kernel_pd_pages))\n        .and_then(|value| value.checked_add(transition_pt_pages))\n        .and_then(|value| value.checked_add(transition_pd_pages))\n        .and_then(|value| value.checked_add(8))\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    let table_bytes = table_pages\n        .checked_mul(PAGE_SIZE)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    let arena_len = align_up_usize(\n        tables_offset\n            .checked_add(table_bytes)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n        TWO_MIB,\n    )?;\n\n    let arena_phys = crate::phys::alloc_phys_range(arena_len, TWO_MIB, 0x0100_0000, None)\n        .ok_or(LiveUpdateError::OutOfMemory)?;\n    let arena_hhdm = hhdm\n        .checked_add(arena_phys)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n\n    let staged = (|| unsafe {\n        let load_hhdm = arena_hhdm\n            .checked_add(load_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        ptr::write_bytes(load_hhdm as *mut u8, 0, span);\n        for segment in &elf.loads {\n            let dst_offset = usize::try_from(segment.vaddr - elf.min_vaddr)\n                .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;\n            let dst = load_hhdm\n                .checked_add(dst_offset as u64)\n                .ok_or(LiveUpdateError::ArithmeticOverflow)? as *mut u8;\n            ptr::copy_nonoverlapping(\n                kernel.as_ptr().add(segment.offset),\n                dst,\n                segment.file_size,\n            );\n            if segment.mem_size > segment.file_size {\n                ptr::write_bytes(\n                    dst.add(segment.file_size),\n                    0,\n                    segment.mem_size - segment.file_size,\n                );\n            }\n        }\n\n        let file_phys = arena_phys\n            .checked_add(file_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let file_hhdm = hhdm\n            .checked_add(file_phys)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        ptr::copy_nonoverlapping(kernel.as_ptr(), file_hhdm as *mut u8, kernel.len());\n\n        copy_limine_requests(&elf, load_hhdm)?;\n        let manifest_hhdm = loaded_addr(load_hhdm, elf.min_vaddr, elf.live_manifest.addr)?;\n        let manifest = core::slice::from_raw_parts(manifest_hhdm as *const u64, 6);\n        if manifest[0] != LIVE_MANIFEST_MAGIC0\n            || manifest[1] != LIVE_MANIFEST_MAGIC1\n            || manifest[2] != LIVE_ABI_VERSION\n        {\n            return Err(LiveUpdateError::Incompatible(\n                "missing live-update ABI manifest",\n            ));\n        }\n        let ap_entry = manifest[3];\n        let handoff_addr = manifest[4];\n        let handoff_size = usize::try_from(manifest[5])\n            .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;\n        if handoff_size != size_of::<WarmHandoff>() {\n            return Err(LiveUpdateError::Incompatible(\n                "handoff structure size mismatch",\n            ));\n        }\n        if !range_in_load(&elf.loads, ap_entry, 1, 0x1) {\n            return Err(LiveUpdateError::Incompatible(\n                "AP entry is not in an executable PT_LOAD",\n            ));\n        }\n        if !range_in_load(&elf.loads, handoff_addr, handoff_size, 0x2) {\n            return Err(LiveUpdateError::Incompatible(\n                "handoff slot is not in a writable PT_LOAD",\n            ));\n        }\n        let handoff_hhdm = loaded_addr(load_hhdm, elf.min_vaddr, handoff_addr)?;\n\n        let trampoline_phys = arena_phys\n            .checked_add(trampoline_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let trampoline_hhdm = hhdm\n            .checked_add(trampoline_phys)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        ptr::write_bytes(trampoline_hhdm as *mut u8, 0, trampoline_map_len);\n        ptr::copy_nonoverlapping(\n            trampoline_start as *const u8,\n            trampoline_hhdm as *mut u8,\n            trampoline_len,\n        );\n\n        let table_phys = arena_phys\n            .checked_add(tables_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let table_end = table_phys\n            .checked_add(table_bytes as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let mut pool = TablePool {\n            next_phys: table_phys,\n            end_phys: table_end,\n            hhdm,\n        };\n        let kernel_pdpt_phys = build_slot_page_tables(\n            &mut pool,\n            elf.min_vaddr,\n            elf.max_vaddr,\n            arena_phys + load_offset as u64,\n        )?;\n        let transition_pdpt_phys = build_slot_page_tables(\n            &mut pool,\n            transition_base,\n            transition_base\n                .checked_add(trampoline_map_len as u64)\n                .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n            trampoline_phys,\n        )?;\n\n        let control_phys = arena_phys\n            .checked_add(control_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let control_hhdm = hhdm\n            .checked_add(control_phys)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let stack_phys = arena_phys\n            .checked_add(stack_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let stack_base_hhdm = hhdm\n            .checked_add(stack_phys)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let ap_park_transition = transition_base\n            .checked_add(ap_park_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let bsp_commit_transition = transition_base\n            .checked_add(bsp_commit_offset as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        let (transition_gdt, transition_gdtr) =\n            snapshot_transition_gdt(control_hhdm)?;\n\n        (control_hhdm as *mut TransitionControl).write(TransitionControl {\n            command: AtomicU64::new(0),\n            arrived: AtomicU64::new(0),\n            stacked: AtomicU64::new(0),\n            failures: AtomicU64::new(0),\n            cr3,\n            root_hhdm,\n            kernel_slot: kernel_slot as u64,\n            new_slot_entry: kernel_pdpt_phys | 0x003,\n            transition_slot: transition_slot as u64,\n            transition_slot_entry: transition_pdpt_phys | 0x003,\n            stack_base_hhdm,\n            stack_stride: AP_TRANSITION_STACK_BYTES as u64,\n            bsp_entry: elf.entry,\n            ap_entry,\n            ap_park_hhdm: ap_park_transition,\n            bsp_commit_hhdm: bsp_commit_transition,\n            expected_aps: cpu_count.saturating_sub(1) as u64,\n            transition_gdt,\n            transition_gdtr,\n        });\n\n        let current_generation = warm_generation().unwrap_or(0);\n        let mut handoff = WarmHandoff {\n            magic0: HANDOFF_MAGIC0,\n            magic1: HANDOFF_MAGIC1,\n            abi_version: LIVE_ABI_VERSION,\n            state: 0,\n            generation: current_generation.saturating_add(1),\n            candidate_hash: fnv1a64(kernel),\n            arena_phys,\n            arena_len: arena_len as u64,\n            kernel_virt_base: elf.min_vaddr,\n            kernel_phys_base: arena_phys + load_offset as u64,\n            kernel_len: span as u64,\n            kernel_file_phys: file_phys,\n            kernel_file_len: kernel.len() as u64,\n            hhdm_base: hhdm,\n            expected_aps: cpu_count.saturating_sub(1) as u64,\n            transition_slot: transition_slot as u64,\n            vm_heap_ranges: [WarmReservedRange::EMPTY; VM_ID_LIMIT],\n            restore_mask: [0; RESTORE_WORDS],\n            resume_mask: [0; RESTORE_WORDS],\n            checksum: 0,\n        };\n        handoff.checksum = handoff_checksum(&handoff);\n        (handoff_hhdm as *mut WarmHandoff).write(handoff);\n\n        Ok(StagedCandidate {\n            arena_phys,\n            arena_len,\n            control_hhdm,\n            handoff_hhdm,\n            expected_aps: cpu_count.saturating_sub(1) as u64,\n            transition_installed: false,\n            committed: false,\n        })\n    })();\n\n    if staged.is_err() {\n        let _ = crate::phys::free_phys_range(arena_phys, arena_len);\n    }\n    staged\n}\n\nunsafe fn build_slot_page_tables(\n    pool: &mut TablePool,\n    min_vaddr: u64,\n    max_vaddr: u64,\n    load_phys: u64,\n) -> Result<u64, LiveUpdateError> {\n    let pdpt_phys = pool.alloc_zeroed()?;\n    let mut vaddr = min_vaddr;\n    while vaddr < max_vaddr {\n        let pdpt_index = ((vaddr >> 30) & 0x1ff) as usize;\n        let pd_index = ((vaddr >> 21) & 0x1ff) as usize;\n        let pt_index = ((vaddr >> 12) & 0x1ff) as usize;\n        let pd_phys = pool.child_table(pdpt_phys, pdpt_index)?;\n        let pt_phys = pool.child_table(pd_phys, pd_index)?;\n        let pt = pool.table_ptr(pt_phys)?;\n        let page_phys = load_phys\n            .checked_add(vaddr - min_vaddr)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        ptr::write_volatile(pt.add(pt_index), page_phys | 0x003);\n        vaddr = vaddr\n            .checked_add(PAGE_SIZE as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    }\n    Ok(pdpt_phys)\n}\n\nunsafe fn copy_limine_requests(\n    elf: &ParsedElf,\n    load_hhdm: u64,\n) -> Result<(), LiveUpdateError> {\n    let current_start = ptr::addr_of!(__limine_requests_start) as usize;\n    let current_end = ptr::addr_of!(__limine_requests_end) as usize;\n    let current_len = current_end\n        .checked_sub(current_start)\n        .ok_or(LiveUpdateError::Incompatible("invalid current Limine request bounds"))?;\n    if current_len != elf.limine_requests.size {\n        return Err(LiveUpdateError::Incompatible(\n            "Limine request layout changed; use a firmware reboot",\n        ));\n    }\n    let destination = loaded_addr(load_hhdm, elf.min_vaddr, elf.limine_requests.addr)?;\n    ptr::copy_nonoverlapping(current_start as *const u8, destination as *mut u8, current_len);\n    Ok(())\n}\n\n\n#[repr(C, packed)]\nstruct RawDescriptorTablePointer {\n    limit: u16,\n    base: u64,\n}\n\nfn snapshot_transition_gdt(\n    control_hhdm: u64,\n) -> Result<([u8; MAX_TRANSITION_GDT_BYTES], [u8; 10]), LiveUpdateError> {\n    let mut current = RawDescriptorTablePointer { limit: 0, base: 0 };\n    unsafe {\n        core::arch::asm!(\n            "sgdt [{}]",\n            in(reg) ptr::addr_of_mut!(current),\n            options(nostack, preserves_flags),\n        );\n    }\n    let current_limit = unsafe { ptr::read_unaligned(ptr::addr_of!(current.limit)) };\n    let current_base = unsafe { ptr::read_unaligned(ptr::addr_of!(current.base)) };\n    let bytes = usize::from(current_limit).saturating_add(1);\n    if bytes == 0 || bytes > MAX_TRANSITION_GDT_BYTES || current_base == 0 {\n        return Err(LiveUpdateError::Incompatible(\n            "current GDT does not fit transition contract",\n        ));\n    }\n    let mut gdt = [0u8; MAX_TRANSITION_GDT_BYTES];\n    unsafe {\n        ptr::copy_nonoverlapping(current_base as *const u8, gdt.as_mut_ptr(), bytes);\n    }\n    let copied_base = control_hhdm\n        .checked_add(offset_of!(TransitionControl, transition_gdt) as u64)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    let copied = RawDescriptorTablePointer {\n        limit: (bytes - 1) as u16,\n        base: copied_base,\n    };\n    let mut gdtr = [0u8; 10];\n    unsafe {\n        ptr::copy_nonoverlapping(\n            ptr::addr_of!(copied).cast::<u8>(),\n            gdtr.as_mut_ptr(),\n            gdtr.len(),\n        );\n    }\n    Ok((gdt, gdtr))\n}\n\nfn range_in_load(loads: &[LoadSegment], addr: u64, size: usize, required_flags: u32) -> bool {\n    let Some(end) = addr.checked_add(size as u64) else {\n        return false;\n    };\n    loads.iter().any(|load| {\n        let Some(load_end) = load.vaddr.checked_add(load.mem_size as u64) else {\n            return false;\n        };\n        addr >= load.vaddr\n            && end <= load_end\n            && (load.flags & required_flags) == required_flags\n    })\n}\n\nfn parse_elf(bytes: &[u8]) -> Result<ParsedElf, LiveUpdateError> {\n    if bytes.len() < 64 || bytes.get(0..4) != Some(b"\\x7FELF") {\n        return Err(LiveUpdateError::BadElf("missing ELF magic"));\n    }\n    if bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {\n        return Err(LiveUpdateError::BadElf("requires ELF64 little-endian v1"));\n    }\n    if read_u16(bytes, 16)? != 2 || read_u16(bytes, 18)? != 0x3e {\n        return Err(LiveUpdateError::BadElf("requires x86_64 ET_EXEC"));\n    }\n\n    let entry = read_u64(bytes, 24)?;\n    let phoff = usize_from_u64(read_u64(bytes, 32)?)?;\n    let shoff = usize_from_u64(read_u64(bytes, 40)?)?;\n    let phentsize = read_u16(bytes, 54)? as usize;\n    let phnum = read_u16(bytes, 56)? as usize;\n    let shentsize = read_u16(bytes, 58)? as usize;\n    let shnum = read_u16(bytes, 60)? as usize;\n    let shstrndx = read_u16(bytes, 62)? as usize;\n    if phentsize < 56 || phnum == 0 {\n        return Err(LiveUpdateError::BadElf("missing program headers"));\n    }\n    if shentsize < 64 || shnum == 0 || shstrndx >= shnum {\n        return Err(LiveUpdateError::BadElf("section table is required"));\n    }\n\n    let mut loads = Vec::new();\n    let mut min_vaddr = u64::MAX;\n    let mut max_vaddr = 0u64;\n    for index in 0..phnum {\n        let base = checked_table_offset(phoff, phentsize, index, bytes.len())?;\n        if read_u32(bytes, base)? != 1 {\n            continue;\n        }\n        let flags = read_u32(bytes, base + 4)?;\n        let offset = usize_from_u64(read_u64(bytes, base + 8)?)?;\n        let vaddr = read_u64(bytes, base + 16)?;\n        let file_size = usize_from_u64(read_u64(bytes, base + 32)?)?;\n        let mem_size = usize_from_u64(read_u64(bytes, base + 40)?)?;\n        if file_size > mem_size {\n            return Err(LiveUpdateError::BadElf("PT_LOAD filesz exceeds memsz"));\n        }\n        checked_range(offset, file_size, bytes.len())?;\n        let segment_end = vaddr\n            .checked_add(mem_size as u64)\n            .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n        min_vaddr = min_vaddr.min(vaddr & PAGE_MASK);\n        max_vaddr = max_vaddr.max(\n            align_up_u64(segment_end, PAGE_SIZE as u64)\n                .ok_or(LiveUpdateError::ArithmeticOverflow)?,\n        );\n        loads.push(LoadSegment {\n            vaddr,\n            flags,\n            offset,\n            file_size,\n            mem_size,\n        });\n    }\n    if loads.is_empty() || min_vaddr >= max_vaddr {\n        return Err(LiveUpdateError::BadElf("no usable PT_LOAD"));\n    }\n    if entry < min_vaddr || entry >= max_vaddr {\n        return Err(LiveUpdateError::BadElf("entry is outside PT_LOAD span"));\n    }\n    if pml4_index(min_vaddr) != pml4_index(max_vaddr - 1) {\n        return Err(LiveUpdateError::Incompatible("kernel spans more than one PML4 slot"));\n    }\n\n    let shstr_base = checked_table_offset(shoff, shentsize, shstrndx, bytes.len())?;\n    let shstr_offset = usize_from_u64(read_u64(bytes, shstr_base + 24)?)?;\n    let shstr_size = usize_from_u64(read_u64(bytes, shstr_base + 32)?)?;\n    checked_range(shstr_offset, shstr_size, bytes.len())?;\n    let shstr = &bytes[shstr_offset..shstr_offset + shstr_size];\n\n    let mut limine_requests = None;\n    let mut live_manifest = None;\n    for index in 0..shnum {\n        let base = checked_table_offset(shoff, shentsize, index, bytes.len())?;\n        let name_offset = read_u32(bytes, base)? as usize;\n        let section_type = read_u32(bytes, base + 4)?;\n        let name = elf_string(shstr, name_offset)?;\n        let addr = read_u64(bytes, base + 16)?;\n        let offset = usize_from_u64(read_u64(bytes, base + 24)?)?;\n        let size = usize_from_u64(read_u64(bytes, base + 32)?)?;\n        if section_type != 8 {\n            checked_range(offset, size, bytes.len())?;\n        }\n        let section = ElfSection { addr, size };\n        match name {\n            ".limine_requests" => limine_requests = Some(section),\n            ".live_update_slot" => live_manifest = Some(section),\n            _ => {}\n        }\n    }\n\n    let limine_requests = limine_requests\n        .ok_or(LiveUpdateError::Incompatible("candidate has no .limine_requests section"))?;\n    let live_manifest = live_manifest\n        .ok_or(LiveUpdateError::Incompatible("candidate has no .live_update_slot section"))?;\n    if live_manifest.size < 6 * size_of::<u64>() {\n        return Err(LiveUpdateError::Incompatible("live-update manifest is truncated"));\n    }\n\n    Ok(ParsedElf {\n        entry,\n        min_vaddr,\n        max_vaddr,\n        loads,\n        limine_requests,\n        live_manifest,\n    })\n}\n\nfn warm_handoff() -> Option<&\'static WarmHandoff> {\n    let handoff = unsafe { &*ptr::addr_of!(LIVE_HANDOFF) };\n    handoff.valid().then_some(handoff)\n}\n\nfn handoff_checksum(handoff: &WarmHandoff) -> u64 {\n    let mut hash = 0xcbf2_9ce4_8422_2325u64;\n    for value in [\n        handoff.magic0,\n        handoff.magic1,\n        handoff.abi_version,\n        handoff.state,\n        handoff.generation,\n        handoff.candidate_hash,\n        handoff.arena_phys,\n        handoff.arena_len,\n        handoff.kernel_virt_base,\n        handoff.kernel_phys_base,\n        handoff.kernel_len,\n        handoff.kernel_file_phys,\n        handoff.kernel_file_len,\n        handoff.hhdm_base,\n        handoff.expected_aps,\n        handoff.transition_slot,\n    ] {\n        hash = fnv1a64_value(hash, value);\n    }\n    for range in handoff.vm_heap_ranges {\n        hash = fnv1a64_value(hash, range.phys_start);\n        hash = fnv1a64_value(hash, range.length);\n    }\n    for value in handoff.restore_mask {\n        hash = fnv1a64_value(hash, value);\n    }\n    for value in handoff.resume_mask {\n        hash = fnv1a64_value(hash, value);\n    }\n    hash\n}\n\nfn fnv1a64(bytes: &[u8]) -> u64 {\n    let mut hash = 0xcbf2_9ce4_8422_2325u64;\n    for &byte in bytes {\n        hash ^= byte as u64;\n        hash = hash.wrapping_mul(0x1000_0000_01b3);\n    }\n    hash\n}\n\nfn fnv1a64_value(mut hash: u64, value: u64) -> u64 {\n    for byte in value.to_le_bytes() {\n        hash ^= byte as u64;\n        hash = hash.wrapping_mul(0x1000_0000_01b3);\n    }\n    hash\n}\n\nfn checkpoint_name(vm_id: u8) -> String {\n    format!("live-update-vm-{vm_id:02}")\n}\n\nfn mask_insert(mask: &mut [u64; RESTORE_WORDS], index: usize) {\n    if let Some(word) = mask.get_mut(index / 64) {\n        *word |= 1u64 << (index % 64);\n    }\n}\n\nfn mask_contains(mask: &[u64; RESTORE_WORDS], index: usize) -> bool {\n    mask.get(index / 64)\n        .map(|word| (*word & (1u64 << (index % 64))) != 0)\n        .unwrap_or(false)\n}\n\nfn pml4_index(addr: u64) -> usize {\n    ((addr >> 39) & 0x1ff) as usize\n}\n\nfn loaded_addr(load_hhdm: u64, min_vaddr: u64, addr: u64) -> Result<u64, LiveUpdateError> {\n    let offset = addr\n        .checked_sub(min_vaddr)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    load_hhdm\n        .checked_add(offset)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)\n}\n\nfn ceil_div(value: usize, divisor: usize) -> usize {\n    value / divisor + usize::from(value % divisor != 0)\n}\n\nfn align_up_usize(value: usize, align: usize) -> Result<usize, LiveUpdateError> {\n    if align == 0 || !align.is_power_of_two() {\n        return Err(LiveUpdateError::ArithmeticOverflow);\n    }\n    value\n        .checked_add(align - 1)\n        .map(|value| value & !(align - 1))\n        .ok_or(LiveUpdateError::ArithmeticOverflow)\n}\n\nfn align_up_u64(value: u64, align: u64) -> Option<u64> {\n    if align == 0 || !align.is_power_of_two() {\n        return None;\n    }\n    value.checked_add(align - 1).map(|value| value & !(align - 1))\n}\n\nfn usize_from_u64(value: u64) -> Result<usize, LiveUpdateError> {\n    usize::try_from(value).map_err(|_| LiveUpdateError::ArithmeticOverflow)\n}\n\nfn checked_table_offset(\n    base: usize,\n    stride: usize,\n    index: usize,\n    total: usize,\n) -> Result<usize, LiveUpdateError> {\n    let offset = stride\n        .checked_mul(index)\n        .and_then(|offset| base.checked_add(offset))\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    checked_range(offset, stride, total)?;\n    Ok(offset)\n}\n\nfn checked_range(offset: usize, length: usize, total: usize) -> Result<(), LiveUpdateError> {\n    let end = offset\n        .checked_add(length)\n        .ok_or(LiveUpdateError::ArithmeticOverflow)?;\n    if end > total {\n        return Err(LiveUpdateError::BadElf("file range is out of bounds"));\n    }\n    Ok(())\n}\n\nfn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LiveUpdateError> {\n    let slice = bytes\n        .get(offset..offset + 2)\n        .ok_or(LiveUpdateError::BadElf("truncated u16"))?;\n    Ok(u16::from_le_bytes([slice[0], slice[1]]))\n}\n\nfn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LiveUpdateError> {\n    let slice = bytes\n        .get(offset..offset + 4)\n        .ok_or(LiveUpdateError::BadElf("truncated u32"))?;\n    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))\n}\n\nfn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LiveUpdateError> {\n    let slice = bytes\n        .get(offset..offset + 8)\n        .ok_or(LiveUpdateError::BadElf("truncated u64"))?;\n    Ok(u64::from_le_bytes([\n        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],\n    ]))\n}\n\nfn elf_string(table: &[u8], offset: usize) -> Result<&str, LiveUpdateError> {\n    let tail = table\n        .get(offset..)\n        .ok_or(LiveUpdateError::BadElf("section-name offset is out of bounds"))?;\n    let end = tail.iter().position(|byte| *byte == 0).unwrap_or(tail.len());\n    core::str::from_utf8(&tail[..end])\n        .map_err(|_| LiveUpdateError::BadElf("section-name table is not UTF-8"))\n}\n',
}
FULL_FILE_REPLACEMENTS = {
    "src/shell2/cmds/update.rs": 'use core::str::SplitWhitespace;\n\nuse embassy_executor::Spawner;\nuse embassy_time::{Duration as EmbassyDuration, Timer};\n\nuse crate::shell2::shell2_cmd::ParseOutcome;\nuse crate::shell2::{\n    MatrixTarget, ShellBackend2, matrix_target_for_backend, matrix_target_interrupted,\n    print_matrix_target_line, print_shell_line, set_matrix_target_active,\n};\n\npub(crate) fn print_update_disk_table(io: &\'static dyn ShellBackend2) {\n    let choices = super::tlb_helper::collect_top_level_disk_choices();\n    super::tlb_helper::print_disk_choice_table(io, "update", "disk selection", choices.as_slice());\n}\n\npub(crate) fn try_parse(\n    spawner: &Spawner,\n    io: &\'static dyn ShellBackend2,\n    args: &mut SplitWhitespace<\'_>,\n) -> ParseOutcome {\n    let Some(arg) = args.next() else {\n        print_update_disk_table(io);\n        print_shell_line(\n            io,\n            "update: run `update <disk-id>` for a persistent install or `update live` for a RAM-only generation swap",\n        );\n        return ParseOutcome::Handled;\n    };\n    if args.next().is_some() {\n        print_shell_line(io, "update: usage `update <disk-id>|live`");\n        return ParseOutcome::Handled;\n    }\n\n    if arg == "live" {\n        submit_live_update(spawner, io);\n        return ParseOutcome::Handled;\n    }\n\n    let Some(raw_id) = super::tlb_helper::parse_disc_id_raw(arg) else {\n        print_shell_line(io, "update: invalid disk id (or use `update live`)");\n        print_update_disk_table(io);\n        return ParseOutcome::Handled;\n    };\n    let Some(disk) = super::tlb_helper::select_top_level_disk(raw_id) else {\n        print_shell_line(io, "update: no such top-level disk");\n        print_update_disk_table(io);\n        return ParseOutcome::Handled;\n    };\n\n    submit_update(spawner, io, disk);\n    ParseOutcome::Handled\n}\n\npub(crate) fn submit_update(\n    spawner: &Spawner,\n    io: &\'static dyn ShellBackend2,\n    disk: crate::disc::block::DeviceHandle,\n) {\n    let target = matrix_target_for_backend(io);\n    let info = disk.info();\n    print_matrix_target_line(\n        &target,\n        alloc::format!("update: starting on disk id={} ({})", info.id.raw(), info.id).as_str(),\n    );\n\n    set_matrix_target_active(&target, true);\n    match update_command_task(target.clone(), disk) {\n        Ok(token) => spawner.spawn(token),\n        Err(_) => {\n            set_matrix_target_active(&target, false);\n            print_shell_line(io, "update: spawn failed");\n        }\n    }\n}\n\npub(crate) fn submit_live_update(spawner: &Spawner, io: &\'static dyn ShellBackend2) {\n    let target = matrix_target_for_backend(io);\n    print_matrix_target_line(\n        &target,\n        "update live: starting RAM-only generation replacement; no kernel disk install will run",\n    );\n\n    set_matrix_target_active(&target, true);\n    match live_update_command_task(target.clone(), *spawner) {\n        Ok(token) => spawner.spawn(token),\n        Err(_) => {\n            set_matrix_target_active(&target, false);\n            print_shell_line(io, "update live: spawn failed");\n        }\n    }\n}\n\n#[embassy_executor::task(pool_size = 2)]\nasync fn update_command_task(target: MatrixTarget, disk: crate::disc::block::DeviceHandle) {\n    let task_target = target.clone();\n    async move {\n        const ISO_URL: &str = "http://trueos.eu/TrueOS.7z";\n\n        Timer::after(EmbassyDuration::from_millis(1)).await;\n\n        let log = |line: &str| {\n            print_matrix_target_line(&task_target, line);\n        };\n        let interrupted = || matrix_target_interrupted(&task_target);\n\n        let info = disk.info();\n        log("update: waiting for net");\n        crate::r::readiness::wait_for(\n            crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,\n        )\n        .await;\n        if interrupted() {\n            log("update: interrupted before download");\n            return;\n        }\n\n        log(alloc::format!(\n            "update: target id={} ({}) blocks={} bs={} writable={} label={:?}",\n            info.id.raw(),\n            info.id,\n            info.block_count,\n            info.block_size,\n            info.writable,\n            info.label,\n        )\n        .as_str());\n        if interrupted() {\n            log("update: interrupted before disk probe");\n            return;\n        }\n\n        let (status, err) = crate::r::disc::detect::detect_physical_disk_detail(disk).await;\n        log(alloc::format!(\n            "update: target status={}{}",\n            status.short(),\n            match (&status, err) {\n                (crate::r::disc::detect::DiscStatus::Unknown, Some(e)) => {\n                    alloc::format!(" (err={:?})", e)\n                }\n                _ => alloc::string::String::new(),\n            }\n        )\n        .as_str());\n        if !matches!(status, crate::r::disc::detect::DiscStatus::Trueos { .. }) {\n            log("update: install before update");\n            return;\n        }\n\n        log(alloc::format!("update: download {}", ISO_URL).as_str());\n        if interrupted() {\n            log("update: interrupted before download");\n            return;\n        }\n\n        let payload = match crate::surfer::html_shack::fetch_bytes_via_pool(\n            ISO_URL,\n            120_000,\n            128 * 1024 * 1024,\n        )\n        .await\n        {\n            Ok(fetch) => fetch.bytes,\n            Err(e) => {\n                log(alloc::format!("update: download failed ({})", e).as_str());\n                return;\n            }\n        };\n        if interrupted() {\n            log("update: interrupted after download");\n            return;\n        }\n\n        log(alloc::format!(\n            "update: downloaded payload={} bytes (7z_magic={})",\n            payload.len(),\n            crate::z7::looks_like_7z(payload.as_slice())\n        )\n        .as_str());\n\n        if !crate::z7::looks_like_7z(payload.as_slice()) {\n            log("update: refused (payload is not a 7z archive)");\n            return;\n        }\n\n        let iso = match crate::z7::extract_file_to_vec(payload.as_slice(), "trueos.iso") {\n            Ok(v) => v,\n            Err(e) => {\n                log(alloc::format!("update: extract failed ({:?})", e).as_str());\n                return;\n            }\n        };\n        drop(payload);\n        let iso_view = iso.as_slice();\n        if interrupted() {\n            log("update: interrupted before install");\n            return;\n        }\n\n        log(alloc::format!(\n            "update: extracted trueos.iso bytes={} (iso9660_magic={})",\n            iso_view.len(),\n            crate::iso9660::looks_like_iso9660(iso_view)\n        )\n        .as_str());\n\n        if !crate::iso9660::looks_like_iso9660(iso_view) {\n            log("update: refused (extracted data is not an ISO9660 image)");\n            return;\n        }\n\n        let bootx64 = match crate::iso9660::file_slice(iso_view, "/EFI/BOOT/BOOTX64.EFI") {\n            Ok(v) => v,\n            Err(_) => {\n                let efi_img = match crate::iso9660::file_slice(iso_view, "/efi.img") {\n                    Ok(v) => v,\n                    Err(e) => {\n                        log(alloc::format!("update: ISO missing efi.img ({:?})", e).as_str());\n                        return;\n                    }\n                };\n                match crate::efi_img::bootx64_from_efi_img(efi_img) {\n                    Some(v) => v,\n                    None => {\n                        log("update: efi.img missing EFI/BOOT/BOOTX64.EFI");\n                        return;\n                    }\n                }\n            }\n        };\n\n        let kernel = match crate::iso9660::file_slice(iso_view, "/TRUEOS.elf") {\n            Ok(v) => v,\n            Err(e) => {\n                log(alloc::format!("update: ISO missing TRUEOS.elf ({:?})", e).as_str());\n                return;\n            }\n        };\n\n        let bootx64_ok = bootx64.get(0..2) == Some(b"MZ");\n        let kernel_ok = kernel.get(0..4) == Some(b"\\x7FELF");\n        log(alloc::format!(\n            "update: BOOTX64.EFI={} bytes (mz={}), TRUEOS.elf={} bytes (elf={})",\n            bootx64.len(),\n            bootx64_ok,\n            kernel.len(),\n            kernel_ok\n        )\n        .as_str());\n        if !bootx64_ok || !kernel_ok {\n            log("update: refusing to install (payload format looks wrong)");\n            return;\n        }\n\n        log("update: installing onto selected TRUEOS disk");\n        match crate::disc::install::install_bootable_uefi_gpt_with_log(\n            disk,\n            bootx64,\n            kernel,\n            &mut |line| log(line),\n        )\n        .await\n        {\n            Ok(()) => match crate::r::fs::trueosfs::remount_root_async(disk).await {\n                Ok(Some(_)) => log("update: ok"),\n                Ok(None) => log("update: failed to remount TRUEOSFS"),\n                Err(e) => log(alloc::format!("update: remount failed ({:?})", e).as_str()),\n            },\n            Err(e) => log(alloc::format!("update: failed ({:?})", e).as_str()),\n        }\n    }\n    .await;\n    set_matrix_target_active(&target, false);\n}\n\n#[embassy_executor::task]\nasync fn live_update_command_task(target: MatrixTarget, spawner: Spawner) {\n    let task_target = target.clone();\n    async move {\n        const LIVE_ISO_URL: &str = "https://trueos.eu/TrueOS.7z";\n\n        Timer::after(EmbassyDuration::from_millis(1)).await;\n\n        let log = |line: &str| {\n            print_matrix_target_line(&task_target, line);\n        };\n        let interrupted = || matrix_target_interrupted(&task_target);\n\n        log("update live: waiting for net and TRUEOSFS checkpoint storage");\n        crate::r::readiness::wait_for(\n            crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,\n        )\n        .await;\n        if interrupted() {\n            log("update live: interrupted before download");\n            return;\n        }\n\n        log(alloc::format!("update live: download {}", LIVE_ISO_URL).as_str());\n        let payload = match crate::surfer::html_shack::fetch_bytes_via_pool(\n            LIVE_ISO_URL,\n            120_000,\n            128 * 1024 * 1024,\n        )\n        .await\n        {\n            Ok(fetch) => fetch.bytes,\n            Err(error) => {\n                log(alloc::format!("update live: download failed ({})", error).as_str());\n                return;\n            }\n        };\n        if interrupted() {\n            log("update live: interrupted after download");\n            return;\n        }\n        if !crate::z7::looks_like_7z(payload.as_slice()) {\n            log("update live: refused (payload is not a 7z archive)");\n            return;\n        }\n        log(alloc::format!(\n            "update live: downloaded payload={} bytes (7z_magic=true)",\n            payload.len(),\n        )\n        .as_str());\n\n        let iso = match crate::z7::extract_file_to_vec(payload.as_slice(), "trueos.iso") {\n            Ok(iso) => iso,\n            Err(error) => {\n                log(alloc::format!("update live: extract failed ({:?})", error).as_str());\n                return;\n            }\n        };\n        drop(payload);\n        if !crate::iso9660::looks_like_iso9660(iso.as_slice()) {\n            log("update live: refused (extracted data is not an ISO9660 image)");\n            return;\n        }\n        if interrupted() {\n            log("update live: interrupted before candidate extraction");\n            return;\n        }\n\n        let kernel = match crate::iso9660::file_slice(iso.as_slice(), "/TRUEOS.elf") {\n            Ok(kernel) if kernel.get(0..4) == Some(b"\\x7FELF") => kernel.to_vec(),\n            Ok(_) => {\n                log("update live: refused (TRUEOS.elf has no ELF magic)");\n                return;\n            }\n            Err(error) => {\n                log(alloc::format!("update live: ISO missing TRUEOS.elf ({:?})", error).as_str());\n                return;\n            }\n        };\n        log(alloc::format!(\n            "update live: candidate TRUEOS.elf={} bytes; disk install path skipped",\n            kernel.len(),\n        )\n        .as_str());\n        drop(iso);\n\n        match crate::live_update::stage_and_swap(kernel, spawner, task_target.clone()).await {\n            Ok(never) => match never {},\n            Err(error) => {\n                log(alloc::format!("update live: failed ({})", error).as_str());\n            }\n        }\n    }\n    .await;\n    set_matrix_target_active(&target, false);\n}\n',
}
TARGETS = sorted({path for path, _, _ in REPLACEMENTS} | set(NEW_FILES) | set(FULL_FILE_REPLACEMENTS))


def run_git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout.strip()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", help="TRUEOS checkout root")
    parser.add_argument("--check", action="store_true", help="validate without writing")
    parser.add_argument("--allow-base-mismatch", action="store_true")
    parser.add_argument("--allow-dirty", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(args.root).resolve()
    if not (root / ".git").exists():
        print(f"error: {root} is not a Git checkout", file=sys.stderr)
        return 2

    head = run_git(root, "rev-parse", "HEAD")
    if head != BASE_COMMIT and not args.allow_base_mismatch:
        print(
            f"error: expected base {BASE_COMMIT}, found {head}; rebase or pass --allow-base-mismatch",
            file=sys.stderr,
        )
        return 2

    if not args.allow_dirty:
        dirty = run_git(root, "status", "--porcelain", "--", *TARGETS)
        if dirty:
            print("error: targeted paths have local changes\n" + dirty, file=sys.stderr)
            return 2

    staged: dict[Path, str] = {}
    originals: dict[Path, str] = {}
    by_path: dict[str, list[tuple[str, str]]] = {}
    for path, old, new in REPLACEMENTS:
        by_path.setdefault(path, []).append((old, new))

    for relative, changes in by_path.items():
        path = root / relative
        text = path.read_text()
        originals[path] = text
        for old, new in changes:
            count = text.count(old)
            if count != 1:
                print(
                    f"error: {relative} exact anchor matched {count} times (expected 1)",
                    file=sys.stderr,
                )
                return 3
            text = text.replace(old, new, 1)
        staged[path] = text

    for relative, text in FULL_FILE_REPLACEMENTS.items():
        path = root / relative
        if not path.exists():
            print(f"error: missing {relative}", file=sys.stderr)
            return 3
        originals[path] = path.read_text()
        staged[path] = text

    for relative, text in NEW_FILES.items():
        path = root / relative
        if path.exists() and path.read_text() != text:
            print(f"error: {relative} already exists with different content", file=sys.stderr)
            return 3
        originals[path] = path.read_text() if path.exists() else ""
        staged[path] = text

    if args.check:
        print(f"ok: {len(staged)} files are applicable to {head}")
        for path in sorted(staged):
            print(path.relative_to(root))
        return 0

    written: list[Path] = []
    try:
        for path, text in staged.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            written.append(path)
    except Exception:
        for path in written:
            original = originals[path]
            if original:
                path.write_text(original)
            elif path.exists():
                path.unlink()
        raise

    print(f"applied TRUEOS update-live prototype to {len(written)} files")
    for path in sorted(written):
        print(path.relative_to(root))
    print("next: cargo fmt --all && git diff --check && run the normal TRUEOS build/test flow")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
