

const LFM25_Q8_SUBMISSION_SIGNATURE_COUNT: usize = LFM25_Q8_SUBMISSION_SIGNATURES.len();


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
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25Q8ModelMapping {
    mapping_phys: u64,
    mapping_gpu: u64,
    mapping_bytes: usize,
    model_gpu: u64,
    model_bytes: usize,
    model_virt: *mut u8,
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
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct Lfm25Q8SubmitTimings {
    encode_us: u64,
    admission_us: u64,
    completion_us: u64,
    gpu_us: u64,
    gpu_timestamp_valid: bool,
    gpu_timestamp_hz: u32,
    phase_probe: Lfm25Q8PhaseProbeSample,
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8SubmissionSignatureSnapshot {
    buckets: [Lfm25Q8SubmissionSignatureStats; LFM25_Q8_SUBMISSION_SIGNATURE_COUNT],
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25Q8PhaseProbeSnapshot {
    buckets: [Lfm25Q8PhaseProbeStats; LFM25_Q8_SUBMISSION_SIGNATURE_COUNT],
#[derive(Copy, Clone)]
struct Lfm25Q8Buffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
struct Lfm25Q8Runtime {
    activation: Lfm25Q8Buffer,
    output: Lfm25Q8Buffer,
    bound_model: Option<Lfm25Q8ModelMapping>,
    ready: bool,
}
}
}
}
}
}
}
}
}
}
}
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



unsafe impl Send for Lfm25Q8ModelMapping {}
unsafe impl Sync for Lfm25Q8ModelMapping {}



unsafe impl Send for Lfm25Q8Buffer {}
unsafe impl Sync for Lfm25Q8Buffer {}



unsafe impl Send for Lfm25Q8Runtime {}
unsafe impl Sync for Lfm25Q8Runtime {}











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












