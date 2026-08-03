static COPY_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);

static ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GLYPH_MASK_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static UI4_RGBA8_TO_NV12_LINEAR_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static UI4_COMPOSE_LAYERS_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static MANDEL64_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SKYBOX_SAMPLE_RGB565_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CHART_SINE_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static PIXEL_PLASMA_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CPP_DEMO_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SHADERTOY_MANDELBROT_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SHADERTOY_CUBE_FIELD_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SHADERTOY_NGUYEN_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static PARTICLE_CRAFT_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_INSTANCE_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static LFM25_Q8_PROJECT_PACKED_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static KOKORO_QGEMM_U8_I8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static KOKORO_CONV1D_U8_U8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_OUTLINE_COVERAGE_R8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static HELIO_RETAINED_TRANSFORM_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static LAB256_MULTIPHASE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SPIRIT_VFX_BACKGROUND_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SPIRIT_VFX_SPRITE_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static LAB256_RUNTIME: Mutex<Option<Lab256Runtime>> = Mutex::new(None);
static FONT_COVERAGE_GPU_VA_CURSOR: AtomicU64 =
    AtomicU64::new(DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE);
static FONT_COVERAGE_GPU_VA_FREE: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
// `None` is available, `Some(u64::MAX)` is reserved while DMA allocation is
// being constructed, and every other value is the owning physical base. A
// fixed Font Rush VA is not reusable until the exact owner retires its Font
// PPGTT leaves and returns the matching physical token.
static FONT_RUSH_RGBA8_ATLAS_SLOTS: Mutex<[Option<u64>; GPGPU_FONT_RUSH_RGBA8_ATLAS_COUNT]> =
    Mutex::new([None; GPGPU_FONT_RUSH_RGBA8_ATLAS_COUNT]);
static PARTICLE_CRAFT_GPU_VA_CURSOR: AtomicU64 =
    AtomicU64::new(DIRECT_RCS_GPU_VA_PARTICLE_CRAFT_BASE);
static PARTICLE_CRAFT_GPU_VA_FREE: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static DIRECT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static FONT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
// The Font lane retains its private page-table topology and leaf mappings for
// the lifetime of its GuC context.  This state is deliberately separate from
// the system-service, execution, and UI4 page-table lifetimes.
static FONT_RCS_PPGTT_RUNTIME: Mutex<FontRcsPpgttRuntime> = Mutex::new(FontRcsPpgttRuntime::new());
static EXECUTION_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static LFM25_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static UI4_COMPOSITOR_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
// Global control-window PTEs are immutable for the lifetime of each state.
// `Once<bool>` makes both success and failure irreversible: a live GuC client
// can never trigger a second installation or repair a partial mapping in
// place. Failed lanes are quarantined by `direct_rcs_map_state`.
static DIRECT_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();
static FONT_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();
static EXECUTION_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();
static LFM25_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();
static UI4_COMPOSITOR_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();

static GPGPU_RECT_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_MANDEL64_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_SPRITE_QUAD_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
// Font owns a separate descriptor allocation even though its private PPGTT can
// reuse the ordinary sprite descriptor VA.  An ambiguous Font submission pins
// these exact bytes without preventing UI4 or the system-service context from
// preparing their own worklists.
static FONT_SPRITE_QUAD_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
static UI4_COMPOSITOR_SPRITE_QUAD_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
static RECT_WORKLIST_DESC_SUBMIT_LOCK: Mutex<()> = Mutex::new(());

static DIRECT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static DIRECT_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
// Font Engine is an independently scheduled GuC client. This lock protects
// only its own encoder state; it never serializes Helio, Spirit, UI4, or the
// general system-service lane.
static FONT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static FONT_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
// The execution lane permits one accepted program to outlive its issuer turn.
// Its tag, lock, state, batch, result page, PPGTT, and quarantine state are all
// independent from system-service direct-RCS work.
static EXECUTION_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static LFM25_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static EXECUTION_RCS_DETACHED_TAG: AtomicU64 = AtomicU64::new(0);
static EXECUTION_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static LFM25_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static UI4_COMPOSITOR_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_SCANOUT_PPGTT_LOGGED: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_PPGTT_POLICY_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static UI4_VIDEO_FRAME_SUBMIT_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static DIRECT_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static FONT_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static EXECUTION_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static LFM25_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static UI4_COMPOSITOR_RUNTIME: Mutex<Ui4CompositorRuntime> =
    Mutex::new(Ui4CompositorRuntime::new());

static SKYBOX_SAMPLE_RGB565_LOG_SEQ: AtomicU64 = AtomicU64::new(0);

static COPY_RECT_2D_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static FILL_RECT_2D_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static FONT_OUTLINE_COVERAGE_R8_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static GLYPH_MASK_BATCH_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static FONT_INSTANCE_BATCH_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static FILL_RECT_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static FILL_RECT_WORKLIST_OK: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_OK: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS: AtomicU32 = AtomicU32::new(0);
static FONT_SPRITE_QUAD_WORKLIST_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static DIRECT_RCS_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(0);
static DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
static FONT_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
static EXECUTION_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
static LFM25_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
