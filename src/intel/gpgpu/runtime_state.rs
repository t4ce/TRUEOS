static COPY_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FILL_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);

static ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static GLYPH_MASK_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static UI4_NV12_YTILE_TO_PRIMARY_XRGB_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD: Mutex<Option<UploadedKernelArtifact>> =
    Mutex::new(None);
static SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static UI4_COMPOSE_LAYERS_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static MANDEL64_WORKLIST_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SKYBOX_SAMPLE_RGB565_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CHART_SINE_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static PIXEL_PLASMA_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_OUTLINE_MESH_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_OUTLINE_COVERAGE_R8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static SCENE_AABB_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static FONT_COVERAGE_GPU_VA_CURSOR: AtomicU64 =
    AtomicU64::new(DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE);
static FONT_COVERAGE_GPU_VA_FREE: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static FONT_OUTLINE_COVERAGE_R8_SELF_TEST: Once<bool> = Once::new();
static DIRECT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static UI4_COMPOSITOR_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static SCENE_AABB_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static SCENE_AABB_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static SCENE_AABB_QUARANTINED: AtomicBool = AtomicBool::new(false);

static GPGPU_RECT_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_MANDEL64_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> = Mutex::new(None);
static GPGPU_SPRITE_QUAD_WORKLIST_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
static UI4_COMPOSITOR_SPRITE_QUAD_DESC: Mutex<Option<GpgpuRectWorklistDescBuffer>> =
    Mutex::new(None);
static RECT_WORKLIST_DESC_SUBMIT_LOCK: Mutex<()> = Mutex::new(());

static DIRECT_RCS_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static DIRECT_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_SCANOUT_PPGTT_LOGGED: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_PPGTT_POLICY_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static UI4_VIDEO_FRAME_SUBMIT_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static DIRECT_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static UI4_COMPOSITOR_RUNTIME: Mutex<Ui4CompositorRuntime> =
    Mutex::new(Ui4CompositorRuntime::new());

static SKYBOX_SAMPLE_RGB565_LOG_SEQ: AtomicU64 = AtomicU64::new(0);

static COPY_RECT_2D_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static FILL_RECT_2D_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static RESOLVE_TILE64_MSAA4_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static FONT_OUTLINE_COVERAGE_R8_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);
static GLYPH_MASK_BATCH_INCOMPLETE_SEQ: AtomicU64 = AtomicU64::new(0);

static FILL_RECT_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_RAN: AtomicBool = AtomicBool::new(false);
static FILL_RECT_WORKLIST_OK: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_OK: AtomicBool = AtomicBool::new(false);

static SPRITE_QUAD_WORKLIST_SUBMIT_FAIL_LOGS: AtomicU32 = AtomicU32::new(0);

static DIRECT_RCS_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(0);
static DIRECT_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
