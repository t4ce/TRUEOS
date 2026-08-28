// Small, boot-only arithmetic probe for the Collapse5 subset-sum tree.
//
// This deliberately models a compute arena rather than a display surface.
// The AOT dispatch uses `subset_sum_collapse5_merge10.clcpp`; keeping the CPU
// oracle adjacent makes its address projection and modulo-u32 math
// independently auditable before another boot profile grows the arena.

pub(crate) const SUBSET_SUM_PROBE_WEIGHT_COUNT: usize = 10;
pub(crate) const SUBSET_SUM_PROBE_LEAF_WIDTH: usize = 5;
pub(crate) const SUBSET_SUM_PROBE_LEAF_STATES: usize = 1 << SUBSET_SUM_PROBE_LEAF_WIDTH;
pub(crate) const SUBSET_SUM_PROBE_OUTPUT_STATES: usize = 1 << SUBSET_SUM_PROBE_WEIGHT_COUNT;

/// Dynamic parameter block for the compact two-leaf tree.  The boot probe
/// selects the 5+5 profile, while a later caller may choose a smaller state
/// count and arena without tying it to any monitor resolution.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubsetSumArenaLayout {
    pub(crate) weights_offset_words: usize,
    pub(crate) left_leaf_offset_words: usize,
    pub(crate) right_leaf_offset_words: usize,
    pub(crate) output_offset_words: usize,
    pub(crate) leaf_state_count: usize,
    pub(crate) merge_state_count: usize,
    pub(crate) arena_words: usize,
}

impl SubsetSumArenaLayout {
    pub(crate) const fn two_equal_leaves(leaf_width: usize) -> Option<Self> {
        if leaf_width == 0 || leaf_width > SUBSET_SUM_PROBE_LEAF_WIDTH {
            return None;
        }
        let leaf_state_count = 1usize << leaf_width;
        let merge_state_count = leaf_state_count * leaf_state_count;
        let weights_offset_words = 0;
        let left_leaf_offset_words = 16;
        // Keep every dynamic region 16-word aligned, so changing the boot
        // profile never changes the stateful-surface packing rules.
        let right_leaf_offset_words = (left_leaf_offset_words + leaf_state_count + 15) & !15;
        let output_offset_words = (right_leaf_offset_words + leaf_state_count + 15) & !15;
        Some(Self {
            weights_offset_words,
            left_leaf_offset_words,
            right_leaf_offset_words,
            output_offset_words,
            leaf_state_count,
            merge_state_count,
            arena_words: output_offset_words + merge_state_count,
        })
    }

    pub(crate) const fn arena_bytes(self) -> usize {
        self.arena_words * core::mem::size_of::<u32>()
    }

    pub(crate) const fn arena_pages(self) -> usize {
        self.arena_bytes().div_ceil(4096)
    }
}

pub(crate) const SUBSET_SUM_PROBE_ARENA_LAYOUT: SubsetSumArenaLayout =
    match SubsetSumArenaLayout::two_equal_leaves(SUBSET_SUM_PROBE_LEAF_WIDTH) {
        Some(layout) => layout,
        None => panic!("fixed Collapse5 layout"),
    };
pub(crate) const SUBSET_SUM_PROBE_ARENA_WORDS: usize = SUBSET_SUM_PROBE_ARENA_LAYOUT.arena_words;
pub(crate) const SUBSET_SUM_PROBE_ARENA_BYTES: usize = SUBSET_SUM_PROBE_ARENA_LAYOUT.arena_bytes();
pub(crate) const SUBSET_SUM_PROBE_ARENA_PAGES: usize = SUBSET_SUM_PROBE_ARENA_LAYOUT.arena_pages();

const _: () = assert!(SUBSET_SUM_PROBE_ARENA_PAGES == 2);

/// The probe is intentionally a tiny logical 32×32 result grid.  It is not
/// derived from, allocated for, or presented by any monitor mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubsetSumProbeGeometry {
    pub(crate) weight_count: usize,
    pub(crate) leaf_state_count: usize,
    pub(crate) candidate_count: usize,
    pub(crate) logical_width: usize,
    pub(crate) logical_height: usize,
    pub(crate) arena_bytes: usize,
    pub(crate) arena_pages: usize,
}

impl SubsetSumProbeGeometry {
    pub(crate) const fn for_weight_count(weight_count: usize) -> Option<Self> {
        if weight_count == 0
            || !weight_count.is_multiple_of(2)
            || weight_count > SUBSET_SUM_PROBE_WEIGHT_COUNT
        {
            return None;
        }
        let layout = match SubsetSumArenaLayout::two_equal_leaves(weight_count / 2) {
            Some(layout) => layout,
            None => return None,
        };
        Some(Self {
            weight_count,
            leaf_state_count: layout.leaf_state_count,
            candidate_count: layout.merge_state_count,
            logical_width: layout.leaf_state_count,
            logical_height: layout.leaf_state_count,
            arena_bytes: layout.arena_bytes(),
            arena_pages: layout.arena_pages(),
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SubsetSumProbeDispatchState {
    /// The deterministic CPU oracle has not yet been run.
    NotStarted,
    /// The tree and direct CPU oracle disagreed; GPU submission is forbidden.
    CpuOracleMismatch,
    UnsupportedTarget,
    AllocationFailed,
    RuntimeUnavailable,
    MappingFailed,
    EncodeFailed,
    SubmitFailed,
    CompletionTimeout,
    GpuMismatch,
    GpuVerified,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubsetSumBootProbeReport {
    pub(crate) state: SubsetSumProbeDispatchState,
    pub(crate) geometry: SubsetSumProbeGeometry,
    pub(crate) cpu_verified: bool,
    pub(crate) gpu_attempted: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) gpu_verified: bool,
    pub(crate) first_mismatch: Option<usize>,
    pub(crate) pre_marker: u32,
    pub(crate) post_marker: u32,
    pub(crate) allocated_bytes: usize,
    pub(crate) artifact_verified: bool,
}

impl Default for SubsetSumBootProbeReport {
    fn default() -> Self {
        Self {
            state: SubsetSumProbeDispatchState::NotStarted,
            geometry: SubsetSumProbeGeometry::for_weight_count(SUBSET_SUM_PROBE_WEIGHT_COUNT)
                .expect("fixed subset-sum boot geometry"),
            cpu_verified: false,
            gpu_attempted: false,
            submitted: false,
            retired: false,
            gpu_verified: false,
            first_mismatch: None,
            pre_marker: 0,
            post_marker: 0,
            allocated_bytes: 0,
            artifact_verified: false,
        }
    }
}

static SUBSET_SUM_BOOT_PROBE_REPORT: spin::Mutex<SubsetSumBootProbeReport> =
    spin::Mutex::new(SubsetSumBootProbeReport {
        state: SubsetSumProbeDispatchState::NotStarted,
        geometry: SubsetSumProbeGeometry {
            weight_count: SUBSET_SUM_PROBE_WEIGHT_COUNT,
            leaf_state_count: SUBSET_SUM_PROBE_LEAF_STATES,
            candidate_count: SUBSET_SUM_PROBE_OUTPUT_STATES,
            logical_width: SUBSET_SUM_PROBE_LEAF_STATES,
            logical_height: SUBSET_SUM_PROBE_LEAF_STATES,
            arena_bytes: SUBSET_SUM_PROBE_ARENA_BYTES,
            arena_pages: SUBSET_SUM_PROBE_ARENA_PAGES,
        },
        cpu_verified: false,
        gpu_attempted: false,
        submitted: false,
        retired: false,
        gpu_verified: false,
        first_mismatch: None,
        pre_marker: 0,
        post_marker: 0,
        allocated_bytes: 0,
        artifact_verified: false,
    });

#[derive(Copy, Clone)]
struct SubsetSumProbeArena {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for SubsetSumProbeArena {}
unsafe impl Sync for SubsetSumProbeArena {}

static SUBSET_SUM_PROBE_ARENA: spin::Mutex<Option<SubsetSumProbeArena>> =
    spin::Mutex::new(None);

pub(crate) fn subset_sum_boot_probe_report() -> SubsetSumBootProbeReport {
    *SUBSET_SUM_BOOT_PROBE_REPORT.lock()
}

fn collapse5(weights: &[u32; SUBSET_SUM_PROBE_LEAF_WIDTH]) -> [u32; SUBSET_SUM_PROBE_LEAF_STATES] {
    let mut output = [0u32; SUBSET_SUM_PROBE_LEAF_STATES];
    let mut candidate = 0usize;
    while candidate < output.len() {
        let mut sum = 0u32;
        let mut bit = 0usize;
        while bit < SUBSET_SUM_PROBE_LEAF_WIDTH {
            if candidate & (1usize << bit) != 0 {
                sum = sum.wrapping_add(weights[bit]);
            }
            bit += 1;
        }
        output[candidate] = sum;
        candidate += 1;
    }
    output
}

fn merge_cartesian(
    left: &[u32; SUBSET_SUM_PROBE_LEAF_STATES],
    right: &[u32; SUBSET_SUM_PROBE_LEAF_STATES],
) -> [u32; SUBSET_SUM_PROBE_OUTPUT_STATES] {
    let mut output = [0u32; SUBSET_SUM_PROBE_OUTPUT_STATES];
    let mut candidate = 0usize;
    while candidate < output.len() {
        // This is the same address projection the kernel uses for an
        // internal tree node: p[0..5] selects left and p[5..10] selects right.
        let left_index = candidate & (SUBSET_SUM_PROBE_LEAF_STATES - 1);
        let right_index = candidate >> SUBSET_SUM_PROBE_LEAF_WIDTH;
        output[candidate] = left[left_index].wrapping_add(right[right_index]);
        candidate += 1;
    }
    output
}

fn direct_oracle(
    weights: &[u32; SUBSET_SUM_PROBE_WEIGHT_COUNT],
) -> [u32; SUBSET_SUM_PROBE_OUTPUT_STATES] {
    let mut output = [0u32; SUBSET_SUM_PROBE_OUTPUT_STATES];
    let mut candidate = 0usize;
    while candidate < output.len() {
        let mut sum = 0u32;
        let mut bit = 0usize;
        while bit < weights.len() {
            if candidate & (1usize << bit) != 0 {
                sum = sum.wrapping_add(weights[bit]);
            }
            bit += 1;
        }
        output[candidate] = sum;
        candidate += 1;
    }
    output
}

fn deterministic_probe_weights() -> [u32; SUBSET_SUM_PROBE_WEIGHT_COUNT] {
    // Non-trivial values deliberately exercise byte carries; this rules out
    // accidental RGBA saturating blend semantics.
    [
        0x0000_00FF,
        0x0000_FF01,
        0x00FF_0101,
        0x7F00_00F1,
        0x8000_010F,
        0x0101_0101,
        0xFE00_00FF,
        0x0001_0001,
        0x7FFF_FFFF,
        0x8000_0001,
    ]
}

fn subset_sum_probe_arena_once() -> Option<SubsetSumProbeArena> {
    let mut slot = SUBSET_SUM_PROBE_ARENA.lock();
    if let Some(arena) = *slot {
        return Some(arena);
    }
    let (phys, virt) = crate::dma::alloc(SUBSET_SUM_PROBE_ARENA_ALLOC_BYTES, 4096)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, SUBSET_SUM_PROBE_ARENA_ALLOC_BYTES);
    }
    super::dma_flush(virt, SUBSET_SUM_PROBE_ARENA_ALLOC_BYTES);
    let arena = SubsetSumProbeArena {
        phys,
        gpu: SUBSET_SUM_PROBE_ARENA_GPU,
        virt,
        bytes: SUBSET_SUM_PROBE_ARENA_ALLOC_BYTES,
    };
    *slot = Some(arena);
    Some(arena)
}

fn publish_subset_sum_probe_report(report: SubsetSumBootProbeReport) -> SubsetSumBootProbeReport {
    *SUBSET_SUM_BOOT_PROBE_REPORT.lock() = report;
    report
}

/// Run the CPU oracle, dispatch all three ordered RCS0 stages, wait for the
/// release marker, and compare every GPU result word with the exact oracle.
pub(crate) fn run_subset_sum_boot_probe_once() -> SubsetSumBootProbeReport {
    let weights = deterministic_probe_weights();
    let left = collapse5(&weights[..5].try_into().expect("fixed left leaf"));
    let right = collapse5(&weights[5..].try_into().expect("fixed right leaf"));
    let tree = merge_cartesian(&left, &right);
    let oracle = direct_oracle(&weights);
    let first_mismatch = tree.iter().zip(oracle.iter()).position(|(a, b)| a != b);
    let cpu_verified = first_mismatch.is_none();
    let mut report = SubsetSumBootProbeReport {
        state: if cpu_verified {
            SubsetSumProbeDispatchState::NotStarted
        } else {
            SubsetSumProbeDispatchState::CpuOracleMismatch
        },
        geometry: SubsetSumProbeGeometry::for_weight_count(weights.len())
            .expect("fixed boot weight count"),
        cpu_verified,
        gpu_attempted: false,
        submitted: false,
        retired: false,
        gpu_verified: false,
        first_mismatch,
        pre_marker: 0,
        post_marker: 0,
        allocated_bytes: 0,
        artifact_verified: false,
    };
    if !cpu_verified {
        return publish_subset_sum_probe_report(report);
    }

    let _submit_guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        report.state = SubsetSumProbeDispatchState::UnsupportedTarget;
        return publish_subset_sum_probe_report(report);
    };
    if !SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_ARTIFACT
        .target_policy
        .supports(dev.device_id, dev.revision_id)
        || direct_rcs_context_is_quarantined()
    {
        report.state = SubsetSumProbeDispatchState::UnsupportedTarget;
        return publish_subset_sum_probe_report(report);
    }

    let Some(arena) = subset_sum_probe_arena_once() else {
        report.state = SubsetSumProbeDispatchState::AllocationFailed;
        return publish_subset_sum_probe_report(report);
    };
    report.allocated_bytes = arena.bytes;
    unsafe {
        // A non-zero seed makes a retired-but-non-writing kernel fail the
        // exact readback instead of accidentally matching zero-valued states.
        core::ptr::write_bytes(arena.virt, 0xA5, arena.bytes);
        core::ptr::copy_nonoverlapping(
            weights.as_ptr(),
            (arena.virt as *mut u32).add(SUBSET_SUM_PROBE_ARENA_LAYOUT.weights_offset_words),
            weights.len(),
        );
    }
    super::dma_flush(arena.virt, arena.bytes);

    let Some(upload) = upload_subset_sum_collapse5_merge10_kernel() else {
        report.state = SubsetSumProbeDispatchState::RuntimeUnavailable;
        return publish_subset_sum_probe_report(report);
    };
    report.artifact_verified = upload.verified;
    let Some(state) = direct_rcs_state_once(dev) else {
        report.state = SubsetSumProbeDispatchState::RuntimeUnavailable;
        return publish_subset_sum_probe_report(report);
    };
    if !direct_rcs_forcewake(dev) {
        report.state = SubsetSumProbeDispatchState::RuntimeUnavailable;
        return publish_subset_sum_probe_report(report);
    }
    if !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        || !direct_rcs_map_ppgtt_kernel(state, arena.gpu, arena.phys, arena.bytes)
    {
        report.state = SubsetSumProbeDispatchState::MappingFailed;
        return publish_subset_sum_probe_report(report);
    }
    report.gpu_attempted = true;
    if !direct_rcs_encode_subset_sum_probe_batch(
        state,
        upload,
        arena.gpu,
        arena.bytes,
        SUBSET_SUM_PROBE_LEAF_WIDTH,
        SUBSET_SUM_PROBE_ARENA_LAYOUT,
    ) {
        report.state = SubsetSumProbeDispatchState::EncodeFailed;
        return publish_subset_sum_probe_report(report);
    }
    report.submitted = direct_rcs_submit_batch(dev, state);
    if !report.submitted {
        report.state = SubsetSumProbeDispatchState::SubmitFailed;
        return publish_subset_sum_probe_report(report);
    }
    report.post_marker = direct_rcs_poll_result_slot_timeout_ms(
        state,
        SUBSET_SUM_PROBE_POST_MARKER_SLOT,
        SUBSET_SUM_PROBE_POST_MARKER,
        SUBSET_SUM_PROBE_COMPLETION_TIMEOUT_MS,
    );
    report.pre_marker = direct_rcs_read_result_slot(state, SUBSET_SUM_PROBE_PRE_MARKER_SLOT);
    report.retired = report.post_marker == SUBSET_SUM_PROBE_POST_MARKER;
    if !report.retired {
        quarantine_direct_rcs_context("subset-sum-marker-timeout");
        report.state = SubsetSumProbeDispatchState::CompletionTimeout;
        return publish_subset_sum_probe_report(report);
    }

    super::dma_flush(arena.virt, arena.bytes);
    let gpu_output = unsafe {
        core::slice::from_raw_parts(
            (arena.virt as *const u32).add(SUBSET_SUM_PROBE_ARENA_LAYOUT.output_offset_words),
            SUBSET_SUM_PROBE_OUTPUT_STATES,
        )
    };
    report.first_mismatch = gpu_output
        .iter()
        .zip(tree.iter())
        .position(|(observed, expected)| observed != expected);
    report.gpu_verified = report.first_mismatch.is_none();
    report.state = if report.gpu_verified {
        SubsetSumProbeDispatchState::GpuVerified
    } else {
        SubsetSumProbeDispatchState::GpuMismatch
    };
    publish_subset_sum_probe_report(report)
}

/// Dedicated, once-per-boot probe task.  The small settle period leaves the
/// early display bring-up path alone; this task owns no display surface, plane,
/// writeback, or BCS resource.
#[trueos_executor::task]
pub(crate) async fn subset_sum_boot_probe_task() {
    trueos_time::Timer::after(trueos_time::Duration::from_millis(250)).await;
    let report = run_subset_sum_boot_probe_once();
    let geometry = report.geometry;
    crate::log_once!(target: "gpgpu";
        "intel/gpgpu: subset-sum boot-probe state={:?} weights={} leaves=2 leaf_states={} merge_states={} logical_grid={}x{} logical_bytes={} allocated_bytes={} arena_pages={} cpu_exact={} first_mismatch={:?} gpu_attempted={} gpu_exact={} dispatch={} retirement={} pre=0x{:08X} post=0x{:08X} artifact_verified={} lane=kernel-gpgpu-execution priority=normal physical_engine=rcs0 display_pipes=untouched wd0=untouched bcs0=untouched scanout_allocations=0\n",
        report.state,
        geometry.weight_count,
        geometry.leaf_state_count,
        geometry.candidate_count,
        geometry.logical_width,
        geometry.logical_height,
        geometry.arena_bytes,
        report.allocated_bytes,
        geometry.arena_pages,
        report.cpu_verified as u8,
        report.first_mismatch,
        report.gpu_attempted as u8,
        report.gpu_verified as u8,
        if report.submitted { "submitted" } else { "not-submitted" },
        if report.retired { "retired" } else { "not-retired" },
        report.pre_marker,
        report.post_marker,
        report.artifact_verified as u8,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse5_tree_exactly_matches_direct_ten_weight_oracle() {
        let weights = deterministic_probe_weights();
        let left = collapse5(&weights[..5].try_into().unwrap());
        let right = collapse5(&weights[5..].try_into().unwrap());
        let tree = merge_cartesian(&left, &right);
        let oracle = direct_oracle(&weights);
        assert_eq!(tree, oracle);

        let geometry = SubsetSumProbeGeometry::for_weight_count(weights.len()).unwrap();
        assert_eq!(geometry.candidate_count, 1024);
        assert_eq!(geometry.logical_width * geometry.logical_height, 1024);
        assert_eq!(geometry.arena_pages, 2);
    }

    #[test]
    fn geometry_is_a_compute_grid_not_a_monitor_mode() {
        assert_eq!(
            SubsetSumProbeGeometry::for_weight_count(10)
                .unwrap()
                .logical_width,
            32
        );
        assert!(SubsetSumProbeGeometry::for_weight_count(9).is_none());
        assert_eq!(SubsetSumProbeGeometry::for_weight_count(6).unwrap().candidate_count, 64);
    }

    #[test]
    fn smaller_dynamic_profile_scales_arena_with_state_count() {
        let layout = SubsetSumArenaLayout::two_equal_leaves(3).unwrap();
        assert_eq!(layout.leaf_state_count, 8);
        assert_eq!(layout.merge_state_count, 64);
        assert_eq!(layout.right_leaf_offset_words, 32);
        assert_eq!(layout.output_offset_words, 48);
        assert_eq!(layout.arena_words, 112);
    }
}
