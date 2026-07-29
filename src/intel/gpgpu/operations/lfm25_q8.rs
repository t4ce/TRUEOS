#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Lfm25Q8ProjectError {
    UnsupportedTarget,
    InvalidModel,
    NonContiguousModel,
    InvalidShape,
    Allocation,
    RuntimeUnavailable,
    MappingFailed,
    EncodeFailed,
    SubmitFailed,
    CompletionTimeout,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8ProjectStats {
    pub(crate) available: bool,
    pub(crate) ready: bool,
    pub(crate) launches: u64,
    pub(crate) submissions: u64,
    pub(crate) failures: u64,
    pub(crate) total_submit_ms: u64,
    pub(crate) total_encode_us: u64,
    pub(crate) total_admission_us: u64,
    pub(crate) total_completion_us: u64,
    pub(crate) total_gpu_us: u64,
    pub(crate) gpu_timestamp_samples: u64,
    pub(crate) gpu_timestamp_hz: u32,
    pub(crate) last_rows: u32,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Lfm25Q8SubmissionSignature {
    ShortconvInput = 0,
    Hidden = 1,
    AttentionQkv = 2,
    FfnGateUp = 3,
    FfnDown = 4,
    Vocabulary = 5,
    Unknown = 6,
}

impl Lfm25Q8SubmissionSignature {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ShortconvInput => "shortconv-in",
            Self::Hidden => "hidden",
            Self::AttentionQkv => "attention-qkv",
            Self::FfnGateUp => "ffn-gate-up",
            Self::FfnDown => "ffn-down",
            Self::Vocabulary => "vocabulary",
            Self::Unknown => "unknown",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

pub(crate) const LFM25_Q8_SUBMISSION_SIGNATURES: [Lfm25Q8SubmissionSignature; 7] = [
    Lfm25Q8SubmissionSignature::ShortconvInput,
    Lfm25Q8SubmissionSignature::Hidden,
    Lfm25Q8SubmissionSignature::AttentionQkv,
    Lfm25Q8SubmissionSignature::FfnGateUp,
    Lfm25Q8SubmissionSignature::FfnDown,
    Lfm25Q8SubmissionSignature::Vocabulary,
    Lfm25Q8SubmissionSignature::Unknown,
];
const LFM25_Q8_SUBMISSION_SIGNATURE_COUNT: usize = LFM25_Q8_SUBMISSION_SIGNATURES.len();

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8SubmissionSignatureStats {
    pub(crate) submissions: u64,
    pub(crate) projections: u64,
    pub(crate) submit_ms: u64,
    pub(crate) completion_us: u64,
    pub(crate) gpu_us: u64,
    pub(crate) gpu_samples: u64,
    pub(crate) submit_min_ms: u64,
    pub(crate) submit_max_ms: u64,
    pub(crate) completion_min_us: u64,
    pub(crate) completion_max_us: u64,
    pub(crate) gpu_min_us: u64,
    pub(crate) gpu_max_us: u64,
    /// Exact extrema are derivable from two cumulative snapshots only while
    /// the earlier snapshot has no samples for the relevant signature.
    pub(crate) submission_extrema_valid: bool,
    pub(crate) gpu_extrema_valid: bool,
}

impl Lfm25Q8SubmissionSignatureStats {
    fn delta_since(self, before: Self) -> Self {
        let submissions = self.submissions.saturating_sub(before.submissions);
        let gpu_samples = self.gpu_samples.saturating_sub(before.gpu_samples);
        let submission_extrema_valid = submissions != 0 && before.submissions == 0;
        let gpu_extrema_valid = gpu_samples != 0 && before.gpu_samples == 0;
        Self {
            submissions,
            projections: self.projections.saturating_sub(before.projections),
            submit_ms: self.submit_ms.saturating_sub(before.submit_ms),
            completion_us: self.completion_us.saturating_sub(before.completion_us),
            gpu_us: self.gpu_us.saturating_sub(before.gpu_us),
            gpu_samples,
            submit_min_ms: if submission_extrema_valid {
                self.submit_min_ms
            } else {
                0
            },
            submit_max_ms: if submission_extrema_valid {
                self.submit_max_ms
            } else {
                0
            },
            completion_min_us: if submission_extrema_valid {
                self.completion_min_us
            } else {
                0
            },
            completion_max_us: if submission_extrema_valid {
                self.completion_max_us
            } else {
                0
            },
            gpu_min_us: if gpu_extrema_valid {
                self.gpu_min_us
            } else {
                0
            },
            gpu_max_us: if gpu_extrema_valid {
                self.gpu_max_us
            } else {
                0
            },
            submission_extrema_valid,
            gpu_extrema_valid,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8SubmissionSignatureSnapshot {
    buckets: [Lfm25Q8SubmissionSignatureStats; LFM25_Q8_SUBMISSION_SIGNATURE_COUNT],
}

impl Lfm25Q8SubmissionSignatureSnapshot {
    pub(crate) const fn bucket(
        &self,
        signature: Lfm25Q8SubmissionSignature,
    ) -> Lfm25Q8SubmissionSignatureStats {
        self.buckets[signature.index()]
    }

    pub(crate) fn delta_since(self, before: Self) -> Self {
        let mut buckets =
            [Lfm25Q8SubmissionSignatureStats::default(); LFM25_Q8_SUBMISSION_SIGNATURE_COUNT];
        let mut index = 0;
        while index < LFM25_Q8_SUBMISSION_SIGNATURE_COUNT {
            buckets[index] = self.buckets[index].delta_since(before.buckets[index]);
            index += 1;
        }
        Self { buckets }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8PhaseProbeStats {
    pub(crate) samples: u64,
    pub(crate) valid_samples: u64,
    pub(crate) queue_to_batch_us: u64,
    pub(crate) preamble_us: u64,
    pub(crate) walkers_us: u64,
    pub(crate) epilogue_us: u64,
    pub(crate) release_to_observe_us: u64,
    pub(crate) queue_to_observe_us: u64,
    pub(crate) gt_start_active_samples: u64,
    pub(crate) gt_end_active_samples: u64,
    pub(crate) gt_start_ratio_sum: u64,
    pub(crate) gt_end_ratio_sum: u64,
}

impl Lfm25Q8PhaseProbeStats {
    fn delta_since(self, before: Self) -> Self {
        Self {
            samples: self.samples.saturating_sub(before.samples),
            valid_samples: self.valid_samples.saturating_sub(before.valid_samples),
            queue_to_batch_us: self
                .queue_to_batch_us
                .saturating_sub(before.queue_to_batch_us),
            preamble_us: self.preamble_us.saturating_sub(before.preamble_us),
            walkers_us: self.walkers_us.saturating_sub(before.walkers_us),
            epilogue_us: self.epilogue_us.saturating_sub(before.epilogue_us),
            release_to_observe_us: self
                .release_to_observe_us
                .saturating_sub(before.release_to_observe_us),
            queue_to_observe_us: self
                .queue_to_observe_us
                .saturating_sub(before.queue_to_observe_us),
            gt_start_active_samples: self
                .gt_start_active_samples
                .saturating_sub(before.gt_start_active_samples),
            gt_end_active_samples: self
                .gt_end_active_samples
                .saturating_sub(before.gt_end_active_samples),
            gt_start_ratio_sum: self
                .gt_start_ratio_sum
                .saturating_sub(before.gt_start_ratio_sum),
            gt_end_ratio_sum: self
                .gt_end_ratio_sum
                .saturating_sub(before.gt_end_ratio_sum),
        }
    }

    pub(crate) fn accumulate(&mut self, other: Self) {
        self.samples = self.samples.saturating_add(other.samples);
        self.valid_samples = self.valid_samples.saturating_add(other.valid_samples);
        self.queue_to_batch_us = self
            .queue_to_batch_us
            .saturating_add(other.queue_to_batch_us);
        self.preamble_us = self.preamble_us.saturating_add(other.preamble_us);
        self.walkers_us = self.walkers_us.saturating_add(other.walkers_us);
        self.epilogue_us = self.epilogue_us.saturating_add(other.epilogue_us);
        self.release_to_observe_us = self
            .release_to_observe_us
            .saturating_add(other.release_to_observe_us);
        self.queue_to_observe_us = self
            .queue_to_observe_us
            .saturating_add(other.queue_to_observe_us);
        self.gt_start_active_samples = self
            .gt_start_active_samples
            .saturating_add(other.gt_start_active_samples);
        self.gt_end_active_samples = self
            .gt_end_active_samples
            .saturating_add(other.gt_end_active_samples);
        self.gt_start_ratio_sum = self
            .gt_start_ratio_sum
            .saturating_add(other.gt_start_ratio_sum);
        self.gt_end_ratio_sum = self.gt_end_ratio_sum.saturating_add(other.gt_end_ratio_sum);
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8PhaseProbeSnapshot {
    buckets: [Lfm25Q8PhaseProbeStats; LFM25_Q8_SUBMISSION_SIGNATURE_COUNT],
}

impl Lfm25Q8PhaseProbeSnapshot {
    pub(crate) const fn bucket(
        &self,
        signature: Lfm25Q8SubmissionSignature,
    ) -> Lfm25Q8PhaseProbeStats {
        self.buckets[signature.index()]
    }

    pub(crate) fn delta_since(self, before: Self) -> Self {
        let mut buckets = [Lfm25Q8PhaseProbeStats::default(); LFM25_Q8_SUBMISSION_SIGNATURE_COUNT];
        let mut index = 0;
        while index < LFM25_Q8_SUBMISSION_SIGNATURE_COUNT {
            buckets[index] = self.buckets[index].delta_since(before.buckets[index]);
            index += 1;
        }
        Self { buckets }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25Q8ProjectSpec {
    pub(crate) weight_offset: u32,
    pub(crate) columns: u32,
    pub(crate) rows: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Lfm25Q8WeightLayout {
    NativeQ8,
    PackedQ8x16Pair,
}

impl Lfm25Q8WeightLayout {
    const fn label(self) -> &'static str {
        match self {
            Self::NativeQ8 => "native-q8",
            Self::PackedQ8x16Pair => "pair1088-x16-dp4a",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25Q8ModelMapping {
    mapping_phys: u64,
    mapping_gpu: u64,
    mapping_bytes: usize,
    model_gpu: u64,
    model_bytes: usize,
    model_virt: *mut u8,
    layout: Lfm25Q8WeightLayout,
}

unsafe impl Send for Lfm25Q8ModelMapping {}
unsafe impl Sync for Lfm25Q8ModelMapping {}

#[derive(Copy, Clone)]
struct Lfm25Q8Buffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for Lfm25Q8Buffer {}
unsafe impl Sync for Lfm25Q8Buffer {}

struct Lfm25Q8Runtime {
    activation: Lfm25Q8Buffer,
    output: Lfm25Q8Buffer,
    bound_model: Option<Lfm25Q8ModelMapping>,
    ready: bool,
}

unsafe impl Send for Lfm25Q8Runtime {}
unsafe impl Sync for Lfm25Q8Runtime {}

#[derive(Copy, Clone)]
struct Lfm25Q8ProjectParams {
    weights_gpu: u64,
    activation_gpu: u64,
    output_gpu: u64,
    model_bytes: usize,
    activation_bytes: usize,
    output_bytes: usize,
    weight_offset: u32,
    columns: u32,
    rows: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct Lfm25Q8PhaseProbeSample {
    sampled: bool,
    valid: bool,
    queue_to_batch_us: u64,
    preamble_us: u64,
    walkers_us: u64,
    epilogue_us: u64,
    release_to_observe_us: u64,
    queue_to_observe_us: u64,
    gt_start_ratio: u32,
    gt_end_ratio: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct Lfm25Q8SubmitTimings {
    encode_us: u64,
    admission_us: u64,
    completion_us: u64,
    gpu_us: u64,
    gpu_timestamp_valid: bool,
    gpu_timestamp_hz: u32,
    phase_probe: Lfm25Q8PhaseProbeSample,
}

struct Lfm25Q8SubmissionSignatureCounters {
    submissions: AtomicU64,
    projections: AtomicU64,
    submit_ms: AtomicU64,
    completion_us: AtomicU64,
    gpu_us: AtomicU64,
    gpu_samples: AtomicU64,
    submit_min_ms: AtomicU64,
    submit_max_ms: AtomicU64,
    completion_min_us: AtomicU64,
    completion_max_us: AtomicU64,
    gpu_min_us: AtomicU64,
    gpu_max_us: AtomicU64,
}

struct Lfm25Q8PhaseProbeCounters {
    samples: AtomicU64,
    valid_samples: AtomicU64,
    queue_to_batch_us: AtomicU64,
    preamble_us: AtomicU64,
    walkers_us: AtomicU64,
    epilogue_us: AtomicU64,
    release_to_observe_us: AtomicU64,
    queue_to_observe_us: AtomicU64,
    gt_start_active_samples: AtomicU64,
    gt_end_active_samples: AtomicU64,
    gt_start_ratio_sum: AtomicU64,
    gt_end_ratio_sum: AtomicU64,
}

impl Lfm25Q8PhaseProbeCounters {
    const fn new() -> Self {
        Self {
            samples: AtomicU64::new(0),
            valid_samples: AtomicU64::new(0),
            queue_to_batch_us: AtomicU64::new(0),
            preamble_us: AtomicU64::new(0),
            walkers_us: AtomicU64::new(0),
            epilogue_us: AtomicU64::new(0),
            release_to_observe_us: AtomicU64::new(0),
            queue_to_observe_us: AtomicU64::new(0),
            gt_start_active_samples: AtomicU64::new(0),
            gt_end_active_samples: AtomicU64::new(0),
            gt_start_ratio_sum: AtomicU64::new(0),
            gt_end_ratio_sum: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> Lfm25Q8PhaseProbeStats {
        Lfm25Q8PhaseProbeStats {
            samples: self.samples.load(Ordering::Relaxed),
            valid_samples: self.valid_samples.load(Ordering::Relaxed),
            queue_to_batch_us: self.queue_to_batch_us.load(Ordering::Relaxed),
            preamble_us: self.preamble_us.load(Ordering::Relaxed),
            walkers_us: self.walkers_us.load(Ordering::Relaxed),
            epilogue_us: self.epilogue_us.load(Ordering::Relaxed),
            release_to_observe_us: self.release_to_observe_us.load(Ordering::Relaxed),
            queue_to_observe_us: self.queue_to_observe_us.load(Ordering::Relaxed),
            gt_start_active_samples: self.gt_start_active_samples.load(Ordering::Relaxed),
            gt_end_active_samples: self.gt_end_active_samples.load(Ordering::Relaxed),
            gt_start_ratio_sum: self.gt_start_ratio_sum.load(Ordering::Relaxed),
            gt_end_ratio_sum: self.gt_end_ratio_sum.load(Ordering::Relaxed),
        }
    }

    fn record(&self, sample: Lfm25Q8PhaseProbeSample) {
        if !sample.sampled {
            return;
        }
        if sample.gt_start_ratio != 0 {
            self.gt_start_active_samples.fetch_add(1, Ordering::Relaxed);
            self.gt_start_ratio_sum
                .fetch_add(u64::from(sample.gt_start_ratio), Ordering::Relaxed);
        }
        if sample.gt_end_ratio != 0 {
            self.gt_end_active_samples.fetch_add(1, Ordering::Relaxed);
            self.gt_end_ratio_sum
                .fetch_add(u64::from(sample.gt_end_ratio), Ordering::Relaxed);
        }
        if sample.valid {
            self.queue_to_batch_us
                .fetch_add(sample.queue_to_batch_us, Ordering::Relaxed);
            self.preamble_us
                .fetch_add(sample.preamble_us, Ordering::Relaxed);
            self.walkers_us
                .fetch_add(sample.walkers_us, Ordering::Relaxed);
            self.epilogue_us
                .fetch_add(sample.epilogue_us, Ordering::Relaxed);
            self.release_to_observe_us
                .fetch_add(sample.release_to_observe_us, Ordering::Relaxed);
            self.queue_to_observe_us
                .fetch_add(sample.queue_to_observe_us, Ordering::Relaxed);
            self.valid_samples.fetch_add(1, Ordering::Relaxed);
        }
        self.samples.fetch_add(1, Ordering::Relaxed);
    }
}

impl Lfm25Q8SubmissionSignatureCounters {
    const fn new() -> Self {
        Self {
            submissions: AtomicU64::new(0),
            projections: AtomicU64::new(0),
            submit_ms: AtomicU64::new(0),
            completion_us: AtomicU64::new(0),
            gpu_us: AtomicU64::new(0),
            gpu_samples: AtomicU64::new(0),
            submit_min_ms: AtomicU64::new(u64::MAX),
            submit_max_ms: AtomicU64::new(0),
            completion_min_us: AtomicU64::new(u64::MAX),
            completion_max_us: AtomicU64::new(0),
            gpu_min_us: AtomicU64::new(u64::MAX),
            gpu_max_us: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> Lfm25Q8SubmissionSignatureStats {
        let submissions = self.submissions.load(Ordering::Relaxed);
        let gpu_samples = self.gpu_samples.load(Ordering::Relaxed);
        Lfm25Q8SubmissionSignatureStats {
            submissions,
            projections: self.projections.load(Ordering::Relaxed),
            submit_ms: self.submit_ms.load(Ordering::Relaxed),
            completion_us: self.completion_us.load(Ordering::Relaxed),
            gpu_us: self.gpu_us.load(Ordering::Relaxed),
            gpu_samples,
            submit_min_ms: if submissions == 0 {
                0
            } else {
                self.submit_min_ms.load(Ordering::Relaxed)
            },
            submit_max_ms: self.submit_max_ms.load(Ordering::Relaxed),
            completion_min_us: if submissions == 0 {
                0
            } else {
                self.completion_min_us.load(Ordering::Relaxed)
            },
            completion_max_us: self.completion_max_us.load(Ordering::Relaxed),
            gpu_min_us: if gpu_samples == 0 {
                0
            } else {
                self.gpu_min_us.load(Ordering::Relaxed)
            },
            gpu_max_us: self.gpu_max_us.load(Ordering::Relaxed),
            submission_extrema_valid: submissions != 0,
            gpu_extrema_valid: gpu_samples != 0,
        }
    }

    fn record(&self, projections: u64, submit_ms: u64, timings: Lfm25Q8SubmitTimings) {
        self.projections.fetch_add(projections, Ordering::Relaxed);
        self.submit_ms.fetch_add(submit_ms, Ordering::Relaxed);
        self.completion_us
            .fetch_add(timings.completion_us, Ordering::Relaxed);
        self.submit_min_ms.fetch_min(submit_ms, Ordering::Relaxed);
        self.submit_max_ms.fetch_max(submit_ms, Ordering::Relaxed);
        self.completion_min_us
            .fetch_min(timings.completion_us, Ordering::Relaxed);
        self.completion_max_us
            .fetch_max(timings.completion_us, Ordering::Relaxed);
        if timings.gpu_timestamp_valid {
            self.gpu_us.fetch_add(timings.gpu_us, Ordering::Relaxed);
            self.gpu_min_us.fetch_min(timings.gpu_us, Ordering::Relaxed);
            self.gpu_max_us.fetch_max(timings.gpu_us, Ordering::Relaxed);
            self.gpu_samples.fetch_add(1, Ordering::Relaxed);
        }
        self.submissions.fetch_add(1, Ordering::Relaxed);
    }
}

static LFM25_Q8_RUNTIME: Mutex<Option<Lfm25Q8Runtime>> = Mutex::new(None);
static LFM25_Q8_READY: AtomicBool = AtomicBool::new(false);
static LFM25_Q8_LAUNCHES: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_SUBMISSIONS: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_FAILURES: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_TOTAL_SUBMIT_MS: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_TOTAL_ENCODE_US: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_TOTAL_ADMISSION_US: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_TOTAL_COMPLETION_US: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_TOTAL_GPU_US: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_GPU_TIMESTAMP_SAMPLES: AtomicU64 = AtomicU64::new(0);
static LFM25_Q8_GPU_TIMESTAMP_HZ: AtomicU32 = AtomicU32::new(0);
static LFM25_Q8_LAST_ROWS: AtomicU32 = AtomicU32::new(0);
static LFM25_Q8_SUBMISSION_SIGNATURE_STATS: [Lfm25Q8SubmissionSignatureCounters;
    LFM25_Q8_SUBMISSION_SIGNATURE_COUNT] =
    [const { Lfm25Q8SubmissionSignatureCounters::new() }; LFM25_Q8_SUBMISSION_SIGNATURE_COUNT];
static LFM25_Q8_PHASE_PROBE_STATS: [Lfm25Q8PhaseProbeCounters;
    LFM25_Q8_SUBMISSION_SIGNATURE_COUNT] =
    [const { Lfm25Q8PhaseProbeCounters::new() }; LFM25_Q8_SUBMISSION_SIGNATURE_COUNT];

pub(crate) const LFM25_Q8_PROJECTIONS_PER_TOKEN: u64 = 93;
pub(crate) const LFM25_Q8_SUBMISSIONS_PER_TOKEN: u64 = 65;
pub(crate) const LFM25_Q8_PROJECTIONS_PER_PREFILL_TOKEN: u64 = LFM25_Q8_PROJECTIONS_PER_TOKEN - 1;
pub(crate) const LFM25_Q8_SUBMISSIONS_PER_PREFILL_TOKEN: u64 = LFM25_Q8_SUBMISSIONS_PER_TOKEN - 1;
const _: () = {
    let shortconv = trueos_lfm25_model::lfm25_decode::SHORTCONV_STATE_COUNT as u64;
    let attention = trueos_lfm25_model::lfm25_decode::KV_CACHE_COUNT as u64;
    let layers = trueos_lfm25_model::lfm25::MODEL_LAYER_COUNT as u64;
    assert!(shortconv * 2 + attention * 4 + layers * 3 + 1 == LFM25_Q8_PROJECTIONS_PER_TOKEN);
    assert!(shortconv * 2 + attention * 2 + layers * 2 + 1 == LFM25_Q8_SUBMISSIONS_PER_TOKEN);
    assert!(shortconv * 2 + attention * 4 + layers * 3 == LFM25_Q8_PROJECTIONS_PER_PREFILL_TOKEN);
    assert!(shortconv * 2 + attention * 2 + layers * 2 == LFM25_Q8_SUBMISSIONS_PER_PREFILL_TOKEN);
};

pub(crate) fn lfm25_q8_project_supported() -> bool {
    lfm25_q8_layout_supported(Lfm25Q8WeightLayout::NativeQ8)
}

pub(crate) fn lfm25_q8_packed_project_supported() -> bool {
    lfm25_q8_layout_supported(Lfm25Q8WeightLayout::PackedQ8x16Pair)
}

fn lfm25_q8_layout_supported(layout: Lfm25Q8WeightLayout) -> bool {
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    lfm25_q8_artifact(layout)
        .target_policy
        .supports(dev.device_id, dev.revision_id)
        && !lfm25_rcs_context_is_quarantined()
}

const fn lfm25_q8_artifact(layout: Lfm25Q8WeightLayout) -> GpgpuKernelArtifact {
    match layout {
        Lfm25Q8WeightLayout::NativeQ8 => LFM25_Q8_PROJECT_ADLS_ARTIFACT,
        Lfm25Q8WeightLayout::PackedQ8x16Pair => LFM25_Q8_PROJECT_PACKED_ADLS_ARTIFACT,
    }
}

fn upload_lfm25_q8_layout_kernel(layout: Lfm25Q8WeightLayout) -> Option<UploadedKernelArtifact> {
    match layout {
        Lfm25Q8WeightLayout::NativeQ8 => upload_lfm25_q8_project_kernel(),
        Lfm25Q8WeightLayout::PackedQ8x16Pair => upload_lfm25_q8_project_packed_kernel(),
    }
}

pub(crate) fn lfm25_q8_project_stats() -> Lfm25Q8ProjectStats {
    Lfm25Q8ProjectStats {
        available: lfm25_q8_packed_project_supported(),
        ready: LFM25_Q8_READY.load(Ordering::Acquire),
        launches: LFM25_Q8_LAUNCHES.load(Ordering::Relaxed),
        submissions: LFM25_Q8_SUBMISSIONS.load(Ordering::Relaxed),
        failures: LFM25_Q8_FAILURES.load(Ordering::Relaxed),
        total_submit_ms: LFM25_Q8_TOTAL_SUBMIT_MS.load(Ordering::Relaxed),
        total_encode_us: LFM25_Q8_TOTAL_ENCODE_US.load(Ordering::Relaxed),
        total_admission_us: LFM25_Q8_TOTAL_ADMISSION_US.load(Ordering::Relaxed),
        total_completion_us: LFM25_Q8_TOTAL_COMPLETION_US.load(Ordering::Relaxed),
        total_gpu_us: LFM25_Q8_TOTAL_GPU_US.load(Ordering::Relaxed),
        gpu_timestamp_samples: LFM25_Q8_GPU_TIMESTAMP_SAMPLES.load(Ordering::Relaxed),
        gpu_timestamp_hz: LFM25_Q8_GPU_TIMESTAMP_HZ.load(Ordering::Relaxed),
        last_rows: LFM25_Q8_LAST_ROWS.load(Ordering::Relaxed),
    }
}

pub(crate) fn lfm25_q8_submission_signature_snapshot() -> Lfm25Q8SubmissionSignatureSnapshot {
    let mut buckets =
        [Lfm25Q8SubmissionSignatureStats::default(); LFM25_Q8_SUBMISSION_SIGNATURE_COUNT];
    let mut index = 0;
    while index < LFM25_Q8_SUBMISSION_SIGNATURE_COUNT {
        buckets[index] = LFM25_Q8_SUBMISSION_SIGNATURE_STATS[index].snapshot();
        index += 1;
    }
    Lfm25Q8SubmissionSignatureSnapshot { buckets }
}

pub(crate) fn lfm25_q8_phase_probe_snapshot() -> Lfm25Q8PhaseProbeSnapshot {
    let mut buckets = [Lfm25Q8PhaseProbeStats::default(); LFM25_Q8_SUBMISSION_SIGNATURE_COUNT];
    let mut index = 0;
    while index < LFM25_Q8_SUBMISSION_SIGNATURE_COUNT {
        buckets[index] = LFM25_Q8_PHASE_PROBE_STATS[index].snapshot();
        index += 1;
    }
    Lfm25Q8PhaseProbeSnapshot { buckets }
}

pub(crate) fn bind_lfm25_q8_model(
    model: &[u8],
) -> Result<Lfm25Q8ModelMapping, Lfm25Q8ProjectError> {
    bind_lfm25_q8_model_layout(model, Lfm25Q8WeightLayout::NativeQ8)
}

pub(crate) fn bind_lfm25_q8_packed_model(
    model: &[u8],
) -> Result<Lfm25Q8ModelMapping, Lfm25Q8ProjectError> {
    bind_lfm25_q8_model_layout(model, Lfm25Q8WeightLayout::PackedQ8x16Pair)
}

fn bind_lfm25_q8_model_layout(
    model: &[u8],
    layout: Lfm25Q8WeightLayout,
) -> Result<Lfm25Q8ModelMapping, Lfm25Q8ProjectError> {
    if !lfm25_q8_layout_supported(layout) {
        return Err(Lfm25Q8ProjectError::UnsupportedTarget);
    }
    if model.len() != trueos_lfm25_model::lfm25::PINNED_NATIVE_IMAGE_BYTES as usize
        || model.is_empty()
    {
        return Err(Lfm25Q8ProjectError::InvalidModel);
    }
    let model_virt = model.as_ptr() as *mut u8;
    let required_alignment = match layout {
        Lfm25Q8WeightLayout::NativeQ8 => 4usize,
        Lfm25Q8WeightLayout::PackedQ8x16Pair => 64usize,
    };
    if model_virt as usize % required_alignment != 0 {
        return Err(Lfm25Q8ProjectError::InvalidModel);
    }
    let start_phys =
        crate::phys::virt_to_phys_checked(model_virt).ok_or(Lfm25Q8ProjectError::InvalidModel)?;
    for offset in (0..model.len()).step_by(4096) {
        let observed = crate::phys::virt_to_phys_checked(unsafe { model_virt.add(offset) })
            .ok_or(Lfm25Q8ProjectError::InvalidModel)?;
        let expected = start_phys
            .checked_add(offset as u64)
            .ok_or(Lfm25Q8ProjectError::InvalidModel)?;
        if observed != expected {
            return Err(Lfm25Q8ProjectError::NonContiguousModel);
        }
    }
    let last_virt = unsafe { model_virt.add(model.len() - 1) };
    let last_phys =
        crate::phys::virt_to_phys_checked(last_virt).ok_or(Lfm25Q8ProjectError::InvalidModel)?;
    if last_phys != start_phys + (model.len() - 1) as u64 {
        return Err(Lfm25Q8ProjectError::NonContiguousModel);
    }
    let page_offset = (start_phys & 0xFFF) as usize;
    let mapping_bytes = page_offset
        .checked_add(model.len())
        .and_then(|bytes| bytes.checked_add(4095))
        .map(|bytes| bytes & !4095usize)
        .ok_or(Lfm25Q8ProjectError::InvalidModel)?;
    let mapping = Lfm25Q8ModelMapping {
        mapping_phys: start_phys & !0xFFF,
        mapping_gpu: LFM25_Q8_MODEL_MAPPING_GPU_BASE,
        mapping_bytes,
        model_gpu: LFM25_Q8_MODEL_MAPPING_GPU_BASE + page_offset as u64,
        model_bytes: model.len(),
        model_virt,
        layout,
    };
    if mapping.model_gpu % required_alignment as u64 != 0
        || mapping.mapping_gpu + mapping.mapping_bytes as u64 > LFM25_Q8_ACTIVATION_GPU
    {
        return Err(Lfm25Q8ProjectError::InvalidModel);
    }

    // The sealed bytes become immutable for the lifetime of the decoder.
    // Publish them once before the persistent PPGTT mapping is installed.
    super::dma_flush(model_virt, model.len());
    let _guard = LFM25_RCS_SUBMIT_LOCK.lock();
    let mut runtime_slot = LFM25_Q8_RUNTIME.lock();
    ensure_lfm25_q8_runtime(&mut runtime_slot)?;
    prepare_lfm25_q8_runtime(runtime_slot.as_mut().unwrap(), mapping)?;
    Ok(mapping)
}

pub(crate) fn lfm25_q8_project(
    model: Lfm25Q8ModelMapping,
    weight_offset: u32,
    columns: u32,
    rows: u32,
    activation: &[u8],
    output: &mut [f32],
) -> Result<u64, Lfm25Q8ProjectError> {
    let specs = [Lfm25Q8ProjectSpec {
        weight_offset,
        columns,
        rows,
    }];
    let mut outputs = [output];
    lfm25_q8_project_batch(model, &specs, activation, &mut outputs)
}

pub(crate) fn lfm25_q8_project_batch(
    model: Lfm25Q8ModelMapping,
    specs: &[Lfm25Q8ProjectSpec],
    activation: &[u8],
    outputs: &mut [&mut [f32]],
) -> Result<u64, Lfm25Q8ProjectError> {
    if specs.is_empty()
        || specs.len() > LFM25_Q8_MAX_BATCH_PROJECTIONS
        || outputs.len() != specs.len()
    {
        return Err(Lfm25Q8ProjectError::InvalidShape);
    }
    let columns = specs[0].columns;
    let row_bytes = usize::try_from(columns)
        .ok()
        .and_then(|columns| columns.checked_div(32))
        .and_then(|blocks| blocks.checked_mul(34))
        .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
    if activation.len() != row_bytes {
        return Err(Lfm25Q8ProjectError::InvalidShape);
    }
    let activation_payload_bytes = match model.layout {
        Lfm25Q8WeightLayout::NativeQ8 => row_bytes,
        Lfm25Q8WeightLayout::PackedQ8x16Pair => {
            trueos_lfm25_cpu::packed_q8x16_activation_bytes(columns as usize)
                .map_err(|_| Lfm25Q8ProjectError::InvalidShape)?
        }
    };
    if activation_payload_bytes > LFM25_Q8_ACTIVATION_ALLOC_BYTES {
        return Err(Lfm25Q8ProjectError::InvalidShape);
    }
    let mut output_bytes = 0usize;
    for (spec, output) in specs.iter().zip(outputs.iter()) {
        let matrix_bytes = row_bytes
            .checked_mul(spec.rows as usize)
            .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
        let matrix_end = (spec.weight_offset as usize)
            .checked_add(matrix_bytes)
            .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
        output_bytes = align_up(output_bytes, 64)
            .and_then(|offset| offset.checked_add(spec.rows as usize * core::mem::size_of::<f32>()))
            .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
        if spec.columns != columns
            || !lfm25_q8_admitted_shape(spec.columns, spec.rows)
            || output.len() != spec.rows as usize
            || matrix_end > model.model_bytes
            || !lfm25_q8_exact_tensor_spec(spec, matrix_bytes)
        {
            return Err(Lfm25Q8ProjectError::InvalidShape);
        }
    }
    if output_bytes > LFM25_Q8_OUTPUT_ALLOC_BYTES {
        return Err(Lfm25Q8ProjectError::InvalidShape);
    }

    let _guard = LFM25_RCS_SUBMIT_LOCK.lock();
    let mut runtime_slot = LFM25_Q8_RUNTIME.lock();
    ensure_lfm25_q8_runtime(&mut runtime_slot)?;
    let runtime = runtime_slot.as_mut().unwrap();
    prepare_lfm25_q8_runtime(runtime, model)?;

    let activation_destination = unsafe {
        core::slice::from_raw_parts_mut(runtime.activation.virt, activation_payload_bytes)
    };
    match model.layout {
        Lfm25Q8WeightLayout::NativeQ8 => activation_destination.copy_from_slice(activation),
        Lfm25Q8WeightLayout::PackedQ8x16Pair => {
            trueos_lfm25_cpu::pack_q8x16_activation(
                activation,
                columns as usize,
                activation_destination,
            )
            .map_err(|_| Lfm25Q8ProjectError::InvalidShape)?;
        }
    }
    super::dma_flush(runtime.activation.virt, activation_payload_bytes);

    let mut params = Vec::new();
    params
        .try_reserve_exact(specs.len())
        .map_err(|_| Lfm25Q8ProjectError::Allocation)?;
    let mut output_offset = 0usize;
    for spec in specs {
        output_offset = align_up(output_offset, 64).ok_or(Lfm25Q8ProjectError::InvalidShape)?;
        let bytes = spec.rows as usize * core::mem::size_of::<f32>();
        params.push(Lfm25Q8ProjectParams {
            weights_gpu: model.model_gpu,
            activation_gpu: runtime.activation.gpu,
            output_gpu: runtime.output.gpu + output_offset as u64,
            model_bytes: model.model_bytes,
            activation_bytes: activation_payload_bytes,
            output_bytes: bytes,
            weight_offset: spec.weight_offset,
            columns: spec.columns,
            rows: spec.rows,
        });
        output_offset = output_offset
            .checked_add(bytes)
            .ok_or(Lfm25Q8ProjectError::InvalidShape)?;
    }

    let signature = lfm25_q8_submission_signature(specs);
    // `_guard` serializes this ordinal read through the successful counter
    // publication below, so two completed submissions cannot claim one sample
    // ordinal. A rejected submission deliberately retries the same ordinal.
    // Keep the profile gate in this hot path so disabling it compile-folds the
    // complete sampler, including the ordinal load, back to the legacy path.
    let phase_probe_ordinal = if crate::log_os::flags::LUMEN_PERF_DIAG_PROFILE_ENABLED {
        lfm25_q8_phase_probe_next_ordinal(signature)
    } else {
        0
    };
    let phase_probe_sampled = crate::log_os::flags::LUMEN_PERF_DIAG_PROFILE_ENABLED
        && lfm25_q8_phase_probe_sample_ordinal(phase_probe_ordinal);
    let started = direct_rcs_now_tick();
    let result = submit_lfm25_q8_project(runtime, model, &params, phase_probe_sampled);
    let elapsed_ms = direct_rcs_elapsed_ms_since(started);
    match result {
        Ok(timings) => {
            for (params, output) in params.iter().zip(outputs.iter_mut()) {
                let offset = (params.output_gpu - runtime.output.gpu) as usize;
                let source = unsafe { runtime.output.virt.add(offset) };
                super::dma_flush(source, params.output_bytes);
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        source as *const f32,
                        output.as_mut_ptr(),
                        output.len(),
                    );
                }
            }
            let projection_count = specs.len() as u64;
            debug_assert!(
                !crate::log_os::flags::LUMEN_PERF_DIAG_PROFILE_ENABLED
                    || phase_probe_ordinal
                        == LFM25_Q8_SUBMISSION_SIGNATURE_STATS[signature.index()]
                            .submissions
                            .load(Ordering::Relaxed)
                            .checked_add(1)
                            .expect("LFM submission ordinal overflow")
            );
            LFM25_Q8_SUBMISSION_SIGNATURE_STATS[signature.index()].record(
                projection_count,
                elapsed_ms,
                timings,
            );
            if crate::log_os::flags::LUMEN_PERF_DIAG_PROFILE_ENABLED {
                LFM25_Q8_PHASE_PROBE_STATS[signature.index()].record(timings.phase_probe);
            }
            let launches = LFM25_Q8_LAUNCHES
                .fetch_add(projection_count, Ordering::Relaxed)
                .saturating_add(projection_count);
            let submissions = LFM25_Q8_SUBMISSIONS
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            LFM25_Q8_TOTAL_SUBMIT_MS.fetch_add(elapsed_ms, Ordering::Relaxed);
            LFM25_Q8_TOTAL_ENCODE_US.fetch_add(timings.encode_us, Ordering::Relaxed);
            LFM25_Q8_TOTAL_ADMISSION_US.fetch_add(timings.admission_us, Ordering::Relaxed);
            LFM25_Q8_TOTAL_COMPLETION_US.fetch_add(timings.completion_us, Ordering::Relaxed);
            if timings.gpu_timestamp_valid {
                LFM25_Q8_TOTAL_GPU_US.fetch_add(timings.gpu_us, Ordering::Relaxed);
                LFM25_Q8_GPU_TIMESTAMP_SAMPLES.fetch_add(1, Ordering::Relaxed);
                LFM25_Q8_GPU_TIMESTAMP_HZ.store(timings.gpu_timestamp_hz, Ordering::Relaxed);
            }
            LFM25_Q8_LAST_ROWS.store(specs.last().unwrap().rows, Ordering::Relaxed);
            if submissions == 1 && crate::log_os::flags::LUMEN_PERF_DIAG_PROFILE_ENABLED {
                super::log_gen12_mocs_readback("first-lfm-retire");
            }
            if submissions == 1 || submissions.is_power_of_two() {
                crate::log_info!(
                    target: "gpgpu";
                    "intel/gpgpu: lfm25-q8 batch ok launches={} submissions={} batch_projections={} columns={} last_rows={} last_weight_offset=0x{:X} submit_ms={} phase_us=encode:{},admission:{},completion:{},gpu:{} gpu_timestamp_valid={} gpu_timestamp_hz={} lane=lfm25 weight_layout={} artifact={}\n",
                    launches,
                    submissions,
                    specs.len(),
                    columns,
                    specs.last().unwrap().rows,
                    specs.last().unwrap().weight_offset,
                    elapsed_ms,
                    timings.encode_us,
                    timings.admission_us,
                    timings.completion_us,
                    timings.gpu_us,
                    timings.gpu_timestamp_valid as u8,
                    timings.gpu_timestamp_hz,
                    model.layout.label(),
                    lfm25_q8_artifact(model.layout).name,
                );
            }
            Ok(elapsed_ms)
        }
        Err(error) => {
            LFM25_Q8_FAILURES.fetch_add(1, Ordering::Relaxed);
            Err(error)
        }
    }
}

const fn lfm25_q8_submission_signature(specs: &[Lfm25Q8ProjectSpec]) -> Lfm25Q8SubmissionSignature {
    match specs {
        [spec] if spec.columns == 1_024 && spec.rows == 3_072 => {
            Lfm25Q8SubmissionSignature::ShortconvInput
        }
        [spec] if spec.columns == 1_024 && spec.rows == 1_024 => Lfm25Q8SubmissionSignature::Hidden,
        [query, key, value]
            if query.columns == 1_024
                && query.rows == 1_024
                && key.columns == 1_024
                && key.rows == 512
                && value.columns == 1_024
                && value.rows == 512 =>
        {
            Lfm25Q8SubmissionSignature::AttentionQkv
        }
        [gate, up]
            if gate.columns == 1_024
                && gate.rows == 4_608
                && up.columns == 1_024
                && up.rows == 4_608 =>
        {
            Lfm25Q8SubmissionSignature::FfnGateUp
        }
        [spec] if spec.columns == 4_608 && spec.rows == 1_024 => {
            Lfm25Q8SubmissionSignature::FfnDown
        }
        [spec] if spec.columns == 1_024 && spec.rows == 65_536 => {
            Lfm25Q8SubmissionSignature::Vocabulary
        }
        _ => Lfm25Q8SubmissionSignature::Unknown,
    }
}

const fn lfm25_q8_phase_probe_sample_ordinal(ordinal: u64) -> bool {
    ordinal != 0 && ordinal.is_power_of_two()
}

const fn lfm25_q8_phase_probe_samples_through(submissions: u64) -> u64 {
    let mut samples = 0u64;
    let mut ordinal = 1u64;
    while ordinal <= submissions {
        samples += 1;
        if ordinal > u64::MAX / 2 {
            break;
        }
        ordinal *= 2;
    }
    samples
}

fn lfm25_q8_phase_probe_next_ordinal(signature: Lfm25Q8SubmissionSignature) -> u64 {
    let completed = LFM25_Q8_SUBMISSION_SIGNATURE_STATS[signature.index()]
        .submissions
        .load(Ordering::Relaxed);
    completed
        .checked_add(1)
        .expect("LFM submission ordinal overflow")
}

const _: () = {
    const fn spec(columns: u32, rows: u32) -> Lfm25Q8ProjectSpec {
        Lfm25Q8ProjectSpec {
            weight_offset: 0,
            columns,
            rows,
        }
    }

    assert!(
        lfm25_q8_submission_signature(&[spec(1_024, 3_072)]) as u8
            == Lfm25Q8SubmissionSignature::ShortconvInput as u8
    );
    assert!(
        lfm25_q8_submission_signature(&[spec(1_024, 1_024)]) as u8
            == Lfm25Q8SubmissionSignature::Hidden as u8
    );
    assert!(
        lfm25_q8_submission_signature(&[spec(1_024, 1_024), spec(1_024, 512), spec(1_024, 512),])
            as u8
            == Lfm25Q8SubmissionSignature::AttentionQkv as u8
    );
    assert!(
        lfm25_q8_submission_signature(&[spec(1_024, 4_608), spec(1_024, 4_608)]) as u8
            == Lfm25Q8SubmissionSignature::FfnGateUp as u8
    );
    assert!(
        lfm25_q8_submission_signature(&[spec(4_608, 1_024)]) as u8
            == Lfm25Q8SubmissionSignature::FfnDown as u8
    );
    assert!(
        lfm25_q8_submission_signature(&[spec(1_024, 65_536)]) as u8
            == Lfm25Q8SubmissionSignature::Vocabulary as u8
    );
    assert!(
        lfm25_q8_submission_signature(&[spec(1_024, 512), spec(1_024, 1_024), spec(1_024, 512),])
            as u8
            == Lfm25Q8SubmissionSignature::Unknown as u8
    );
    assert!(!lfm25_q8_phase_probe_sample_ordinal(0));
    assert!(lfm25_q8_phase_probe_sample_ordinal(1));
    assert!(lfm25_q8_phase_probe_sample_ordinal(2));
    assert!(!lfm25_q8_phase_probe_sample_ordinal(3));
    assert!(lfm25_q8_phase_probe_sample_ordinal(4));
    assert!(!lfm25_q8_phase_probe_sample_ordinal(5));
    assert!(lfm25_q8_phase_probe_samples_through(0) == 0);
    assert!(lfm25_q8_phase_probe_samples_through(10) == 4);
    assert!(lfm25_q8_phase_probe_samples_through(114) == 7);
    assert!(lfm25_q8_phase_probe_samples_through(190) == 8);
    assert!(lfm25_q8_phase_probe_samples_through(304) == 9);
    let canonical_hi_samples = lfm25_q8_phase_probe_samples_through(190)
        + lfm25_q8_phase_probe_samples_through(304)
        + lfm25_q8_phase_probe_samples_through(114)
        + lfm25_q8_phase_probe_samples_through(304)
        + lfm25_q8_phase_probe_samples_through(304)
        + lfm25_q8_phase_probe_samples_through(10);
    assert!(canonical_hi_samples == 46);
    assert!(canonical_hi_samples * 100 < 1_226 * 4);
};

fn ensure_lfm25_q8_runtime(slot: &mut Option<Lfm25Q8Runtime>) -> Result<(), Lfm25Q8ProjectError> {
    if slot.is_some() {
        return Ok(());
    }
    let (activation_phys, activation_virt) =
        crate::dma::alloc(LFM25_Q8_ACTIVATION_ALLOC_BYTES, 4096)
            .ok_or(Lfm25Q8ProjectError::Allocation)?;
    let (output_phys, output_virt) = crate::dma::alloc(LFM25_Q8_OUTPUT_ALLOC_BYTES, 4096)
        .ok_or(Lfm25Q8ProjectError::Allocation)?;
    unsafe {
        core::ptr::write_bytes(activation_virt, 0, LFM25_Q8_ACTIVATION_ALLOC_BYTES);
        core::ptr::write_bytes(output_virt, 0, LFM25_Q8_OUTPUT_ALLOC_BYTES);
    }
    super::dma_flush(activation_virt, LFM25_Q8_ACTIVATION_ALLOC_BYTES);
    super::dma_flush(output_virt, LFM25_Q8_OUTPUT_ALLOC_BYTES);
    *slot = Some(Lfm25Q8Runtime {
        activation: Lfm25Q8Buffer {
            phys: activation_phys,
            gpu: LFM25_Q8_ACTIVATION_GPU,
            virt: activation_virt,
            bytes: LFM25_Q8_ACTIVATION_ALLOC_BYTES,
        },
        output: Lfm25Q8Buffer {
            phys: output_phys,
            gpu: LFM25_Q8_OUTPUT_GPU,
            virt: output_virt,
            bytes: LFM25_Q8_OUTPUT_ALLOC_BYTES,
        },
        bound_model: None,
        ready: false,
    });
    Ok(())
}

fn prepare_lfm25_q8_runtime(
    runtime: &mut Lfm25Q8Runtime,
    model: Lfm25Q8ModelMapping,
) -> Result<(), Lfm25Q8ProjectError> {
    if runtime.ready && runtime.bound_model == Some(model) {
        return Ok(());
    }
    let dev = super::claimed_device().ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let upload = upload_lfm25_q8_layout_kernel(model.layout)
        .ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let state = lfm25_rcs_state_once(dev).ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let mapped = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        && direct_rcs_map_ppgtt_kernel(
            state,
            model.mapping_gpu,
            model.mapping_phys,
            model.mapping_bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime.activation.gpu,
            runtime.activation.phys,
            runtime.activation.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime.output.gpu,
            runtime.output.phys,
            runtime.output.bytes,
        );
    if !mapped {
        runtime.ready = false;
        LFM25_Q8_READY.store(false, Ordering::Release);
        return Err(Lfm25Q8ProjectError::MappingFailed);
    }
    runtime.bound_model = Some(model);
    runtime.ready = true;
    LFM25_Q8_READY.store(true, Ordering::Release);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: lfm25-q8 runtime ready model_phys=0x{:X} model_gpu=0x{:X} model_bytes=0x{:X} mapped_bytes=0x{:X} activation_gpu=0x{:X} output_gpu=0x{:X} lane=lfm25 weight_layout={} artifact={} target={} device=0x{:04X} revision=0x{:02X}\n",
        model.mapping_phys,
        model.model_gpu,
        model.model_bytes,
        model.mapping_bytes,
        runtime.activation.gpu,
        runtime.output.gpu,
        model.layout.label(),
        lfm25_q8_artifact(model.layout).name,
        lfm25_q8_artifact(model.layout).target,
        dev.device_id,
        dev.revision_id,
    );
    Ok(())
}

fn submit_lfm25_q8_project(
    runtime: &Lfm25Q8Runtime,
    model: Lfm25Q8ModelMapping,
    params: &[Lfm25Q8ProjectParams],
    phase_probe_sampled: bool,
) -> Result<Lfm25Q8SubmitTimings, Lfm25Q8ProjectError> {
    let dev = super::claimed_device().ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let upload = upload_lfm25_q8_layout_kernel(model.layout)
        .ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    let state = lfm25_rcs_state_once(dev).ok_or(Lfm25Q8ProjectError::RuntimeUnavailable)?;
    if !runtime.ready || runtime.bound_model != Some(model) {
        return Err(Lfm25Q8ProjectError::RuntimeUnavailable);
    }
    let encode_started = direct_rcs_now_tick();
    if !direct_rcs_forcewake(dev) {
        return Err(Lfm25Q8ProjectError::RuntimeUnavailable);
    }
    if !direct_rcs_encode_lfm25_q8_batch(state, upload, params, phase_probe_sampled) {
        return Err(Lfm25Q8ProjectError::EncodeFailed);
    }
    let encode_us = direct_rcs_elapsed_us_since(encode_started);
    let gpu_host_pre_submit = if phase_probe_sampled {
        direct_rcs_read_render_timestamp(dev)
    } else {
        0
    };
    let gt_start_ratio = if phase_probe_sampled {
        super::gen12_actual_gt_ratio(dev)
    } else {
        0
    };
    let admission_started = direct_rcs_now_tick();
    if !lfm25_rcs_submit_batch(dev, state) {
        return Err(Lfm25Q8ProjectError::SubmitFailed);
    }
    let admission_us = direct_rcs_elapsed_us_since(admission_started);
    let completion_started = direct_rcs_now_tick();
    let (observed, gpu_host_observe) = if phase_probe_sampled {
        lfm25_rcs_poll_result_slot_timeout_ms_with_timestamp(
            dev,
            state,
            LFM25_Q8_POST_MARKER_SLOT,
            LFM25_Q8_POST_MARKER,
            LFM25_Q8_COMPLETION_TIMEOUT_MS,
        )
    } else {
        (
            lfm25_rcs_poll_result_slot_timeout_ms(
                state,
                LFM25_Q8_POST_MARKER_SLOT,
                LFM25_Q8_POST_MARKER,
                LFM25_Q8_COMPLETION_TIMEOUT_MS,
            ),
            0,
        )
    };
    if observed != LFM25_Q8_POST_MARKER {
        return Err(Lfm25Q8ProjectError::CompletionTimeout);
    }
    let gt_end_ratio = if phase_probe_sampled {
        super::gen12_actual_gt_ratio(dev)
    } else {
        0
    };
    let completion_us = direct_rcs_elapsed_us_since(completion_started);
    let gpu_start = direct_rcs_read_result_qword(state, LFM25_Q8_GPU_START_TIMESTAMP_SLOT);
    let gpu_end = direct_rcs_read_result_qword(state, LFM25_Q8_GPU_END_TIMESTAMP_SLOT);
    let gpu_timestamp_hz = direct_rcs_timestamp_frequency_hz(dev);
    let gpu_interval =
        direct_rcs_timestamp_interval_us(gpu_start, gpu_end, u64::from(gpu_timestamp_hz));
    let phase_probe = if phase_probe_sampled {
        lfm25_q8_read_phase_probe_sample(
            state,
            gpu_host_pre_submit,
            gpu_start,
            gpu_end,
            gpu_host_observe,
            u64::from(gpu_timestamp_hz),
            gt_start_ratio,
            gt_end_ratio,
        )
    } else {
        Lfm25Q8PhaseProbeSample::default()
    };
    debug_assert!(
        !phase_probe.valid
            || gpu_interval
                .map(|(_, gpu_us)| gpu_us == phase_probe.walkers_us)
                .unwrap_or(false)
    );
    Ok(Lfm25Q8SubmitTimings {
        encode_us,
        admission_us,
        completion_us,
        gpu_us: gpu_interval.map(|(_, us)| us).unwrap_or(0),
        gpu_timestamp_valid: gpu_interval.is_some(),
        gpu_timestamp_hz,
        phase_probe,
    })
}

fn lfm25_q8_read_phase_probe_sample(
    state: DirectRcsState,
    gpu_host_pre_submit: u64,
    gpu_start: u64,
    gpu_end: u64,
    gpu_host_observe: u64,
    gpu_timestamp_hz: u64,
    gt_start_ratio: u32,
    gt_end_ratio: u32,
) -> Lfm25Q8PhaseProbeSample {
    let mut sample = Lfm25Q8PhaseProbeSample {
        sampled: true,
        gt_start_ratio,
        gt_end_ratio,
        ..Lfm25Q8PhaseProbeSample::default()
    };
    let gpu_batch_enter =
        direct_rcs_read_result_qword(state, LFM25_Q8_GPU_BATCH_ENTER_TIMESTAMP_SLOT);
    let gpu_post_release =
        direct_rcs_read_result_qword(state, LFM25_Q8_GPU_POST_RELEASE_TIMESTAMP_SLOT);
    let queue_to_batch =
        direct_rcs_timestamp_interval_us(gpu_host_pre_submit, gpu_batch_enter, gpu_timestamp_hz);
    let preamble = direct_rcs_timestamp_interval_us(gpu_batch_enter, gpu_start, gpu_timestamp_hz);
    let walkers = direct_rcs_timestamp_interval_us(gpu_start, gpu_end, gpu_timestamp_hz);
    let epilogue = direct_rcs_timestamp_interval_us(gpu_end, gpu_post_release, gpu_timestamp_hz);
    let release_to_observe =
        direct_rcs_timestamp_interval_us(gpu_post_release, gpu_host_observe, gpu_timestamp_hz);
    let queue_to_observe =
        direct_rcs_timestamp_interval_us(gpu_host_pre_submit, gpu_host_observe, gpu_timestamp_hz);
    if let (
        Some((queue_to_batch_ticks, queue_to_batch_us)),
        Some((preamble_ticks, preamble_us)),
        Some((walkers_ticks, walkers_us)),
        Some((epilogue_ticks, epilogue_us)),
        Some((release_to_observe_ticks, release_to_observe_us)),
        Some((queue_to_observe_ticks, queue_to_observe_us)),
    ) = (queue_to_batch, preamble, walkers, epilogue, release_to_observe, queue_to_observe)
    {
        let phase_sum_ticks = queue_to_batch_ticks
            .saturating_add(preamble_ticks)
            .saturating_add(walkers_ticks)
            .saturating_add(epilogue_ticks)
            .saturating_add(release_to_observe_ticks);
        if phase_sum_ticks == queue_to_observe_ticks {
            sample.valid = true;
            sample.queue_to_batch_us = queue_to_batch_us;
            sample.preamble_us = preamble_us;
            sample.walkers_us = walkers_us;
            sample.epilogue_us = epilogue_us;
            sample.release_to_observe_us = release_to_observe_us;
            sample.queue_to_observe_us = queue_to_observe_us;
        }
    }
    sample
}

const fn lfm25_q8_admitted_shape(columns: u32, rows: u32) -> bool {
    (columns == 1_024 && matches!(rows, 512 | 1_024 | 3_072 | 4_608 | 65_536))
        || (columns == 4_608 && rows == 1_024)
}

fn lfm25_q8_exact_tensor_spec(spec: &Lfm25Q8ProjectSpec, matrix_bytes: usize) -> bool {
    trueos_lfm25_model::lfm25::generated::TENSORS
        .iter()
        .any(|descriptor| {
            descriptor.rank == 2
                && trueos_lfm25_model::lfm25::TensorFormat::from_raw(descriptor.format)
                    == Some(trueos_lfm25_model::lfm25::TensorFormat::Q8_0)
                && descriptor.native_offset == spec.weight_offset
                && descriptor.native_bytes as usize == matrix_bytes
                && descriptor.ggml_ne0 == spec.columns
                && descriptor.ggml_ne1 == spec.rows
        })
}
