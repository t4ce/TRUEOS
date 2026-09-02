use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::x86_64::__cpuid;
use core::fmt::Write;
use core::sync::atomic::{compiler_fence, Ordering};

use spin::Mutex;
use x86_64::registers::model_specific::Msr;

const CPUID_LEAF_FEATURES: u32 = 0x01;
const CPUID_LEAF_THERMAL_POWER: u32 = 0x06;
const CPUID_FEATURE_MSR: u32 = 1 << 5;
const CPUID_HFI: u32 = 1 << 19;
const CPUID_THREAD_DIRECTOR: u32 = 1 << 23;
const CPUID_TD_CLASS_SHIFT: u32 = 8;
const CPUID_HFI_PERFORMANCE: u32 = 1 << 0;
const CPUID_HFI_ENERGY_EFFICIENCY: u32 = 1 << 1;
const CPUID_HFI_PAGE_SHIFT: u32 = 8;
const CPUID_HFI_INDEX_SHIFT: u32 = 16;
const HFI_PAGE_BYTES: usize = 4096;
const HFI_MAX_TABLE_PAGES: usize = 16;
const HFI_TIMESTAMP_BYTES: usize = core::mem::size_of::<u64>();
const HFI_STABLE_CAPTURE_ATTEMPTS: usize = 32;
const HFI_INITIAL_READY_POLLS: usize = 200_000;
const HFI_INITIAL_READY_CAPTURE_ROUNDS: usize = 64;

const MSR_IA32_HW_FEEDBACK_PTR: u32 = 0x17D0;
const MSR_IA32_HW_FEEDBACK_CONFIG: u32 = 0x17D1;
const HW_FEEDBACK_PTR_VALID: u64 = 1 << 0;
const HW_FEEDBACK_CONFIG_HFI_ENABLE: u64 = 1 << 0;

const PROFILE_FLAG_LEAF_AVAILABLE: u8 = 1 << 0;
const PROFILE_FLAG_MSR_INSTRUCTION: u8 = 1 << 1;

const INTEL_VENDOR_EBX: u32 = 0x756E_6547;
const INTEL_VENDOR_EDX: u32 = 0x4965_6E69;
const INTEL_VENDOR_ECX: u32 = 0x6C65_746E;

#[derive(Clone, Copy, Debug)]
struct HfiTableState {
    phys: u64,
    virt: usize,
    bytes: usize,
    capability_mask: u8,
    header_size: usize,
    row_stride: usize,
    row_count: usize,
    enabled: bool,
    initial_ready: bool,
    initial_columns_seen: u8,
}

struct HfiTableCapture {
    timestamp: u64,
    image: Vec<u8>,
    attempts: usize,
    stable: bool,
}

static HFI_TABLE: Mutex<Option<HfiTableState>> = Mutex::new(None);

/// CPUID-only Intel Hardware Feedback Interface / Thread Director metadata for
/// one logical processor. This does not mean TRUEOS has configured an HFI table
/// or touched any `IA32_HW_FEEDBACK_*` MSR.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct IntelHfiCpuid {
    pub(crate) leaf_available: bool,
    pub(crate) msr_instruction: bool,
    pub(crate) eax: u32,
    pub(crate) ebx: u32,
    pub(crate) ecx: u32,
    pub(crate) edx: u32,
}

impl IntelHfiCpuid {
    pub(crate) const fn unavailable() -> Self {
        Self {
            leaf_available: false,
            msr_instruction: false,
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
        }
    }

    pub(crate) const fn from_profile_storage(
        flags: u8,
        eax: u32,
        ebx: u32,
        ecx: u32,
        edx: u32,
    ) -> Self {
        Self {
            leaf_available: (flags & PROFILE_FLAG_LEAF_AVAILABLE) != 0,
            msr_instruction: (flags & PROFILE_FLAG_MSR_INSTRUCTION) != 0,
            eax,
            ebx,
            ecx,
            edx,
        }
    }

    pub(crate) fn profile_storage_flags(self) -> u8 {
        let mut flags = 0;
        if self.leaf_available {
            flags |= PROFILE_FLAG_LEAF_AVAILABLE;
        }
        if self.msr_instruction {
            flags |= PROFILE_FLAG_MSR_INSTRUCTION;
        }
        flags
    }

    pub(crate) fn detect_current() -> Self {
        let leaf0 = __cpuid(0);
        let vendor_intel = leaf0.ebx == INTEL_VENDOR_EBX
            && leaf0.edx == INTEL_VENDOR_EDX
            && leaf0.ecx == INTEL_VENDOR_ECX;
        if !vendor_intel || leaf0.eax < CPUID_LEAF_THERMAL_POWER {
            return Self::unavailable();
        }

        let features = __cpuid(CPUID_LEAF_FEATURES);
        let leaf6 = __cpuid(CPUID_LEAF_THERMAL_POWER);
        Self {
            leaf_available: true,
            msr_instruction: (features.edx & CPUID_FEATURE_MSR) != 0,
            eax: leaf6.eax,
            ebx: leaf6.ebx,
            ecx: leaf6.ecx,
            edx: leaf6.edx,
        }
    }

    pub(crate) const fn hfi_supported(self) -> bool {
        self.leaf_available && (self.eax & CPUID_HFI) != 0
    }

    pub(crate) const fn thread_director_supported(self) -> bool {
        self.leaf_available && (self.eax & CPUID_THREAD_DIRECTOR) != 0
    }

    pub(crate) const fn thread_director_classes(self) -> u8 {
        ((self.ecx >> CPUID_TD_CLASS_SHIFT) & 0xFF) as u8
    }

    pub(crate) const fn capability_mask(self) -> u8 {
        (self.edx & 0xFF) as u8
    }

    pub(crate) const fn performance_reporting(self) -> bool {
        self.hfi_supported() && (self.edx & CPUID_HFI_PERFORMANCE) != 0
    }

    pub(crate) const fn energy_efficiency_reporting(self) -> bool {
        self.hfi_supported() && (self.edx & CPUID_HFI_ENERGY_EFFICIENCY) != 0
    }

    pub(crate) const fn table_pages(self) -> Option<u8> {
        if self.hfi_supported() {
            Some((((self.edx >> CPUID_HFI_PAGE_SHIFT) & 0x0F) + 1) as u8)
        } else {
            None
        }
    }

    pub(crate) const fn table_bytes(self) -> Option<usize> {
        match self.table_pages() {
            Some(pages) => Some((pages as usize) * HFI_PAGE_BYTES),
            None => None,
        }
    }

    pub(crate) const fn row_index(self) -> Option<i16> {
        if self.hfi_supported() {
            Some(((self.edx >> CPUID_HFI_INDEX_SHIFT) as u16) as i16)
        } else {
            None
        }
    }
}

fn round_up_8(value: usize) -> usize {
    value.saturating_add(7) & !7
}

fn capability_layout(mask: u8) -> Option<(usize, usize, Option<usize>, Option<usize>)> {
    let count = mask.count_ones() as usize;
    if count == 0 {
        return None;
    }
    let header_size = round_up_8(count);
    let row_stride = round_up_8(count);

    let performance_offset = if (mask & CPUID_HFI_PERFORMANCE as u8) != 0 {
        Some(0)
    } else {
        None
    };
    let efficiency_offset = if (mask & CPUID_HFI_ENERGY_EFFICIENCY as u8) != 0 {
        Some(usize::from(
            (mask & ((CPUID_HFI_ENERGY_EFFICIENCY as u8) - 1)).count_ones() as u8,
        ))
    } else {
        None
    };
    Some((
        header_size,
        row_stride,
        performance_offset,
        efficiency_offset,
    ))
}

fn registered_row_count() -> Option<usize> {
    let count = crate::percpu::total_slots().max(crate::smp::cpu_count());
    let mut max_index: Option<usize> = None;
    for slot in 0..count {
        let Some(profile) = crate::cpu::CpuProfile::for_slot(slot as u32) else {
            continue;
        };
        let Some(index) = profile.hfi_cpuid().row_index() else {
            continue;
        };
        if index < 0 {
            continue;
        }
        max_index = Some(
            max_index
                .map(|current| current.max(index as usize))
                .unwrap_or(index as usize),
        );
    }
    max_index.map(|index| index.saturating_add(1))
}

fn single_package_registered() -> bool {
    let topo = crate::x2apic::detect_x2apic_topology();
    let count = crate::percpu::total_slots().max(crate::smp::cpu_count());
    let mut package: Option<u32> = None;
    for slot in 0..count {
        let Some(profile) = crate::cpu::CpuProfile::for_slot(slot as u32) else {
            continue;
        };
        let (pkg, _, _) = topo.decode(profile.lapic_id());
        match package {
            Some(expected) if expected != pkg => return false,
            None => package = Some(pkg),
            _ => {}
        }
    }
    package.is_some()
}

fn read_u8(base: usize, offset: usize) -> u8 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u8) }
}

fn read_u64(base: usize, offset: usize) -> u64 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u64) }
}

fn used_table_bytes(state: HfiTableState) -> Option<usize> {
    HFI_TIMESTAMP_BYTES
        .checked_add(state.header_size)?
        .checked_add(state.row_count.checked_mul(state.row_stride)?)
}

fn capture_once(state: HfiTableState, attempt: usize) -> Option<HfiTableCapture> {
    let used_bytes = used_table_bytes(state)?;
    if used_bytes > state.bytes || used_bytes < HFI_TIMESTAMP_BYTES {
        return None;
    }

    let before = read_u64(state.virt, 0);
    compiler_fence(Ordering::SeqCst);
    let mut image = Vec::with_capacity(used_bytes);
    for offset in 0..used_bytes {
        image.push(read_u8(state.virt, offset));
    }
    compiler_fence(Ordering::SeqCst);
    let after = read_u64(state.virt, 0);

    let mut timestamp_bytes = [0u8; HFI_TIMESTAMP_BYTES];
    timestamp_bytes.copy_from_slice(&image[..HFI_TIMESTAMP_BYTES]);
    let copied_timestamp = u64::from_le_bytes(timestamp_bytes);
    if before == 0 || before != after || before != copied_timestamp {
        return None;
    }

    Some(HfiTableCapture {
        timestamp: before,
        image,
        attempts: attempt,
        stable: false,
    })
}

fn capture_stable(state: HfiTableState) -> Option<HfiTableCapture> {
    let mut previous: Option<HfiTableCapture> = None;
    for attempt in 1..=HFI_STABLE_CAPTURE_ATTEMPTS {
        let Some(mut current) = capture_once(state, attempt) else {
            previous = None;
            core::hint::spin_loop();
            continue;
        };

        if let Some(previous) = previous.as_ref()
            && previous.timestamp == current.timestamp
            && previous.image == current.image
        {
            current.stable = true;
            return Some(current);
        }

        previous = Some(current);
        core::hint::spin_loop();
    }
    previous
}

fn advertised_columns_ready(mask: u8, image: &[u8]) -> bool {
    let column_count = mask.count_ones() as usize;
    if column_count == 0 || image.len() < HFI_TIMESTAMP_BYTES + column_count {
        return false;
    }
    image[HFI_TIMESTAMP_BYTES..HFI_TIMESTAMP_BYTES + column_count]
        .iter()
        .all(|value| *value != 0)
}

fn capture_completes_initial_population(state: HfiTableState, capture: &HfiTableCapture) -> bool {
    state.enabled
        && capture.stable
        && capture.timestamp != 0
        && advertised_columns_ready(state.capability_mask, &capture.image)
}

fn wait_for_initial_population(state: &mut HfiTableState) {
    if state.initial_ready || !state.enabled {
        return;
    }

    let column_count = state.capability_mask.count_ones() as usize;
    for _ in 0..HFI_INITIAL_READY_POLLS {
        let before = read_u64(state.virt, 0);
        let mut columns_ready = before != 0;
        for column in 0..column_count {
            columns_ready &= read_u8(state.virt, HFI_TIMESTAMP_BYTES + column) != 0;
        }
        let after = read_u64(state.virt, 0);
        if columns_ready
            && before == after
            && let Some(capture) = capture_stable(*state)
            && capture_completes_initial_population(*state, &capture)
        {
            state.initial_ready = true;
            return;
fn updated_columns_mask(capability_mask: u8, image: &[u8]) -> u8 {
    let column_count = capability_mask.count_ones() as usize;
    if column_count == 0 || image.len() < HFI_TIMESTAMP_BYTES + column_count {
        return 0;
    }

    let mut ordinal = 0usize;
    let mut updated = 0u8;
    for bit in 0..u8::BITS {
        let bit_mask = 1u8 << bit;
        if (capability_mask & bit_mask) == 0 {
            continue;
        }
        if image[HFI_TIMESTAMP_BYTES + ordinal] != 0 {
            updated |= bit_mask;
        }
        ordinal += 1;
    }
    updated
}

fn initial_columns_complete(state: HfiTableState) -> bool {
    state.capability_mask != 0
        && (state.initial_columns_seen & state.capability_mask) == state.capability_mask
}

fn initial_population_ready(state: HfiTableState) -> bool {
    initial_columns_complete(state)
}

fn observe_stable_capture(state: &mut HfiTableState, capture: &HfiTableCapture) {
    if !state.enabled || !capture.stable || capture.timestamp == 0 {
        return;
    }
    state.initial_columns_seen |= updated_columns_mask(state.capability_mask, &capture.image);
}

fn wait_for_initial_population(state: &mut HfiTableState) {
    if initial_population_ready(*state) || !state.enabled {
        return;
    }

    for _ in 0..HFI_INITIAL_READY_CAPTURE_ROUNDS {
        if let Some(capture) = capture_stable(*state) {
            observe_stable_capture(state, &capture);
            if initial_population_ready(*state) {
                return;
            }
        }
        core::hint::spin_loop();
    }
}

/// Explicitly configure the package-0 HFI memory table on the BSP.
///
/// This is intentionally never called from boot or scheduling paths. The shell
/// diagnostic has to request it explicitly. TRUEOS currently has no recoverable
/// #GP wrapper around RDMSR/WRMSR, so the CPUID and environment gates below are
/// deliberately strict.
pub(crate) fn enable_table_explicit() -> Result<(), &'static str> {
    if crate::percpu::current_slot() != 0 {
        return Err("HFI table bring-up must run on the BSP");
    }
    if crate::intel::is_emulator_environment() {
        return Err("HFI MSR bring-up is disabled in emulator environments");
    }
    if !single_package_registered() {
        return Err("HFI table bring-up currently requires exactly one registered package");
    }

    let cpuid = IntelHfiCpuid::detect_current();
    if !cpuid.hfi_supported() || !cpuid.msr_instruction {
        return Err("Intel HFI/MSR capability is unavailable on the current CPU");
    }
    let pages = cpuid.table_pages().ok_or("HFI table size unavailable")? as usize;
    if pages == 0 || pages > HFI_MAX_TABLE_PAGES {
        return Err("HFI table page count is outside the defensive limit");
    }
    let bytes = pages
        .checked_mul(HFI_PAGE_BYTES)
        .ok_or("HFI table byte size overflow")?;
    let mask = cpuid.capability_mask();
    let Some((header_size, row_stride, _, _)) = capability_layout(mask) else {
        return Err("HFI reports no decodable capability columns");
    };
    let row_count = registered_row_count().ok_or("no non-negative registered HFI row indexes")?;
    let data_offset = HFI_TIMESTAMP_BYTES
        .checked_add(header_size)
        .ok_or("HFI table header overflow")?;
    let required = data_offset
        .checked_add(
            row_count
                .checked_mul(row_stride)
                .ok_or("HFI row span overflow")?,
        )
        .ok_or("HFI table span overflow")?;
    if required > bytes {
        return Err("registered HFI rows do not fit the CPUID-advertised table size");
    }

    let mut state = HFI_TABLE.lock();
    if let Some(existing) = state.as_mut() {
        if existing.bytes != bytes || existing.capability_mask != mask {
            return Err("existing HFI allocation does not match current CPUID layout");
        }
        if !existing.enabled {
            unsafe {
                Msr::new(MSR_IA32_HW_FEEDBACK_PTR)
                    .write(existing.phys | HW_FEEDBACK_PTR_VALID);
                let mut config = Msr::new(MSR_IA32_HW_FEEDBACK_CONFIG);
                let value = config.read();
                config.write(value | HW_FEEDBACK_CONFIG_HFI_ENABLE);
            }
            existing.enabled = true;
        }
        wait_for_initial_population(existing);
        return Ok(());
    }

    let phys = crate::phys::alloc_phys_range(bytes, HFI_PAGE_BYTES, 0x0010_0000, None)
        .ok_or("PMM could not reserve the HFI table")?;
    if !phys.is_multiple_of(HFI_PAGE_BYTES as u64) {
        let _ = crate::phys::free_phys_range(phys, bytes);
        return Err("PMM returned a non-page-aligned HFI table");
    }
    let virt = crate::phys::phys_to_virt(phys as usize);
    unsafe { core::ptr::write_bytes(virt as *mut u8, 0, bytes) };
    compiler_fence(Ordering::SeqCst);

    unsafe {
        Msr::new(MSR_IA32_HW_FEEDBACK_PTR).write(phys | HW_FEEDBACK_PTR_VALID);
        let mut config = Msr::new(MSR_IA32_HW_FEEDBACK_CONFIG);
        let value = config.read();
        config.write(value | HW_FEEDBACK_CONFIG_HFI_ENABLE);
    }
    compiler_fence(Ordering::SeqCst);

    *state = Some(HfiTableState {
        phys,
        virt,
        bytes,
        capability_mask: mask,
        header_size,
        row_stride,
        row_count,
        enabled: true,
        initial_ready: false,
        initial_columns_seen: 0,
    });
    if let Some(existing) = state.as_mut() {
        wait_for_initial_population(existing);
    }
    Ok(())
}

/// Disable HFI table generation without releasing the programmed physical
/// memory. Intel documents implementations that retain table-address state, so
/// the allocation remains pinned for the kernel generation.
pub(crate) fn disable_table_explicit() -> Result<(), &'static str> {
    if crate::percpu::current_slot() != 0 {
        return Err("HFI table disable must run on the BSP");
    }
    let mut state = HFI_TABLE.lock();
    let Some(existing) = state.as_mut() else {
        return Err("HFI table has not been configured by TRUEOS");
    };
    unsafe {
        let mut config = Msr::new(MSR_IA32_HW_FEEDBACK_CONFIG);
        let value = config.read();
        config.write(value & !HW_FEEDBACK_CONFIG_HFI_ENABLE);
    }
    existing.enabled = false;
    Ok(())
}

pub(crate) fn table_snapshot_text() -> String {
    let mut out = String::new();
    let (state, capture, initial_ready) = {
        let mut state = HFI_TABLE.lock();
        let Some(state) = state.as_mut() else {
            out.push_str(
                "HFI table: unconfigured (use `tlb hfi enable` for explicit MSR/table bring-up)\n",
            );
            return out;
        };

        let capture = capture_stable(*state);
        if !state.initial_ready
            && let Some(capture) = capture.as_ref()
            && capture_completes_initial_population(*state, capture)
        {
            state.initial_ready = true;
        }
        (*state, capture, state.initial_ready)
        if let Some(capture) = capture.as_ref() {
            observe_stable_capture(state, capture);
        }
        let initial_ready = initial_population_ready(*state);
        (*state, capture, initial_ready)
    };

    let Some((_, _, performance_offset, efficiency_offset)) =
        capability_layout(state.capability_mask)
    else {
        out.push_str("HFI table: configured but capability layout is not decodable\n");
        return out;
    };

    let Some(capture) = capture else {
        writeln!(
            out,
            "HFI table: enabled={} capture_status=unstable coherent=no stable=no phys=0x{:016X} bytes=0x{:X} rows={} stride={} header={} mask=0x{:02X}",
            if state.enabled { "yes" } else { "no" },
            state.phys,
            state.bytes,
            state.row_count,
            state.row_stride,
            state.header_size,
            state.capability_mask
        )
        .unwrap();
        out.push_str(
            "HFI rows: withheld because a timestamp-consistent table image could not be captured; scheduler_consumer=none\n",
        );
        return out;
    };

    let capture_status = if !state.enabled {
        "disabled-retained"
    } else if !initial_ready {
        "initializing"
    } else if capture.stable {
        "ready"
    } else {
        "changing"
    };
    let data_offset = HFI_TIMESTAMP_BYTES + state.header_size;
    let perf_updated = performance_offset
        .and_then(|offset| capture.image.get(HFI_TIMESTAMP_BYTES + offset).copied());
    let eff_updated = efficiency_offset
        .and_then(|offset| capture.image.get(HFI_TIMESTAMP_BYTES + offset).copied());

    writeln!(
        out,
        "HFI table: enabled={} capture_status={} coherent=yes stable={} attempts={} phys=0x{:016X} bytes=0x{:X} timestamp=0x{:016X} rows={} stride={} header={} mask=0x{:02X}",
        if state.enabled { "yes" } else { "no" },
        capture_status,
        if capture.stable { "yes" } else { "no" },
        capture.attempts,
        state.phys,
        state.bytes,
        capture.timestamp,
        state.row_count,
        state.row_stride,
        state.header_size,
        state.capability_mask
    )
    .unwrap();
    writeln!(
        out,
        "HFI header: performance_updated={} efficiency_updated={} initial_population={} update_interrupt=not-configured scheduler_consumer=none",
        "HFI header: performance_updated={} efficiency_updated={} initial_population={} initial_columns_seen=0x{:02X}/0x{:02X} update_interrupt=not-configured scheduler_consumer=none",
        perf_updated
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-")),
        eff_updated
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-")),
        if initial_ready { "ready" } else { "incomplete" },
        state.initial_columns_seen,
        state.capability_mask,
    )
    .unwrap();
    if !initial_ready {
        out.push_str(
            "HFI note: advertised capability columns are not fully populated; rows are diagnostic-only and not eligible for consumers.\n",
        );
    } else if !capture.stable {
        out.push_str(
            "HFI note: the image is timestamp-consistent but changed between capture attempts; rows are a coherent point-in-time diagnostic.\n",
        );
    }

    writeln!(out, "Row  Performance  Efficiency  Raw8").unwrap();
    for row in 0..state.row_count {
        let row_base = data_offset + row * state.row_stride;
        let performance = performance_offset
            .and_then(|offset| capture.image.get(row_base + offset).copied());
        let efficiency = efficiency_offset
            .and_then(|offset| capture.image.get(row_base + offset).copied());
        let mut raw = [0u8; 8];
        let raw_len = state.row_stride.min(raw.len());
        if let Some(bytes) = capture.image.get(row_base..row_base + raw_len) {
            raw[..raw_len].copy_from_slice(bytes);
        }
        writeln!(
            out,
            "{:<4} {:<12} {:<11} {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            row,
            performance
                .map(|value| value.to_string())
                .unwrap_or_else(|| String::from("-")),
            efficiency
                .map(|value| value.to_string())
                .unwrap_or_else(|| String::from("-")),
            raw[0],
            raw[1],
            raw[2],
            raw[3],
            raw[4],
            raw[5],
            raw[6],
            raw[7]
        )
        .unwrap();
    }
    out
}

pub(crate) fn explicit_bringup_text() -> String {
    let mut out = String::new();
    match enable_table_explicit() {
        Ok(()) => {
            out.push_str("HFI explicit bring-up: enabled; scheduler remains disconnected\n");
            out.push_str(&table_snapshot_text());
        }
        Err(error) => {
            writeln!(out, "HFI explicit bring-up: refused: {error}").unwrap();
        }
    }
    out
}

pub(crate) fn explicit_disable_text() -> String {
    let mut out = String::new();
    match disable_table_explicit() {
        Ok(()) => {
            out.push_str("HFI explicit bring-up: disabled; pinned table retained\n");
            out.push_str(&table_snapshot_text());
        }
        Err(error) => {
            writeln!(out, "HFI explicit disable: refused: {error}").unwrap();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_hfi_and_thread_director_metadata() {
        let raw = IntelHfiCpuid {
            leaf_available: true,
            msr_instruction: true,
            eax: CPUID_HFI | CPUID_THREAD_DIRECTOR,
            ebx: 0,
            ecx: 4 << CPUID_TD_CLASS_SHIFT,
            edx: CPUID_HFI_PERFORMANCE
                | CPUID_HFI_ENERGY_EFFICIENCY
                | (2 << CPUID_HFI_PAGE_SHIFT)
                | (7 << CPUID_HFI_INDEX_SHIFT),
        };

        assert!(raw.hfi_supported());
        assert!(raw.thread_director_supported());
        assert_eq!(raw.thread_director_classes(), 4);
        assert_eq!(raw.capability_mask(), 0x03);
        assert!(raw.performance_reporting());
        assert!(raw.energy_efficiency_reporting());
        assert_eq!(raw.table_pages(), Some(3));
        assert_eq!(raw.table_bytes(), Some(3 * HFI_PAGE_BYTES));
        assert_eq!(raw.row_index(), Some(7));
        assert_eq!(
            IntelHfiCpuid::from_profile_storage(
                raw.profile_storage_flags(),
                raw.eax,
                raw.ebx,
                raw.ecx,
                raw.edx,
            ),
            raw
        );
    }

    #[test]
    fn preserves_signed_hfi_row_index() {
        let raw = IntelHfiCpuid {
            leaf_available: true,
            msr_instruction: true,
            eax: CPUID_HFI,
            ebx: 0,
            ecx: 0,
            edx: CPUID_HFI_PERFORMANCE | (0xFFFF << CPUID_HFI_INDEX_SHIFT),
        };

        assert_eq!(raw.row_index(), Some(-1));
    }

    #[test]
    fn basic_hfi_layout_rounds_to_eight_bytes() {
        let (header, stride, perf, eff) = capability_layout(0x03).unwrap();
        assert_eq!(header, 8);
        assert_eq!(stride, 8);
        assert_eq!(perf, Some(0));
        assert_eq!(eff, Some(1));
    }

    #[test]
    fn initial_population_requires_every_advertised_column() {
        let mut image = [0u8; 16];
        image[HFI_TIMESTAMP_BYTES] = 1;
        assert!(!advertised_columns_ready(0x03, &image));
        image[HFI_TIMESTAMP_BYTES + 1] = 1;
        assert!(advertised_columns_ready(0x03, &image));
    fn update_indications_map_back_to_advertised_capability_bits() {
        let mut image = [0u8; 16];
        image[HFI_TIMESTAMP_BYTES] = 1;
        assert_eq!(updated_columns_mask(0x03, &image), 0x01);
        image[HFI_TIMESTAMP_BYTES + 1] = 1;
        assert_eq!(updated_columns_mask(0x03, &image), 0x03);
    }

    #[test]
    fn initial_population_accumulates_separate_column_updates() {
        let mut state = HfiTableState {
            phys: 0,
            virt: 0,
            bytes: 4096,
            capability_mask: 0x03,
            header_size: 8,
            row_stride: 8,
            row_count: 8,
            enabled: true,
            initial_columns_seen: 0,
        };
        let mut perf_image = alloc::vec![0u8; 16];
        perf_image[HFI_TIMESTAMP_BYTES] = 1;
        observe_stable_capture(
            &mut state,
            &HfiTableCapture {
                timestamp: 1,
                image: perf_image,
                attempts: 2,
                stable: true,
            },
        );
        assert_eq!(state.initial_columns_seen, 0x01);
        assert!(!initial_population_ready(state));

        let mut efficiency_image = alloc::vec![0u8; 16];
        efficiency_image[HFI_TIMESTAMP_BYTES + 1] = 1;
        observe_stable_capture(
            &mut state,
            &HfiTableCapture {
                timestamp: 2,
                image: efficiency_image,
                attempts: 2,
                stable: true,
            },
        );
        assert_eq!(state.initial_columns_seen, 0x03);
        assert!(initial_population_ready(state));
    }

    #[test]
    fn row_zero_values_do_not_define_initial_readiness() {
        let mut image = [0u8; 24];
        image[HFI_TIMESTAMP_BYTES] = 1;
        image[HFI_TIMESTAMP_BYTES + 1] = 1;
        assert!(advertised_columns_ready(0x03, &image));
        assert_eq!(updated_columns_mask(0x03, &image), 0x03);
    }
}
