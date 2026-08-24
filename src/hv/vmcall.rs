//! VMX hypercall protocol — host side.
//!
//! Three roles in one compact module (to be split when the op table grows):
//!   vmx-comm  : shared CommPage layout + op/status codes
//!   vmx-trans : read request / write response helpers
//!   vmx-exec  : dispatch table executed by the host vmexit loop
//!
//! Guest writes request fields then issues `vmcall`.
//! Host reads, executes, writes response, then vmresumes.
//! The vmcall is synchronous from the guest's point of view.

use crate::hv::memory::kernel_va_to_pa;
use crate::hv::{hvlogf, hvwarnf};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use log_os_core::LogLevel;

const BLUEPRINT_AUDIO_LOG_SAMPLE_EVERY: u32 = 1_000;
const BLUEPRINT_TEXT_SCENE_BUSY_LOG_SAMPLE_EVERY: u32 = 1_000;

static BLUEPRINT_AUDIO_WRITE_LOG_SEQ: AtomicU32 = AtomicU32::new(0);
static BLUEPRINT_AUDIO_POLL_LOG_SEQ: AtomicU32 = AtomicU32::new(0);
static BLUEPRINT_TEXT_SCENE_BUSY_LOG_SEQ: AtomicU32 = AtomicU32::new(0);

fn sampled_log(counter: &AtomicU32) -> bool {
    counter.fetch_add(1, Ordering::Relaxed) % BLUEPRINT_AUDIO_LOG_SAMPLE_EVERY == 0
}

fn sampled_text_scene_busy_log() -> bool {
    BLUEPRINT_TEXT_SCENE_BUSY_LOG_SEQ.fetch_add(1, Ordering::Relaxed)
        % BLUEPRINT_TEXT_SCENE_BUSY_LOG_SAMPLE_EVERY
        == 0
}

fn vm_has_trueosfs_scope(vm_id: u8) -> bool {
    crate::hv::blueprint_process_env_var(vm_id, "TRUEOS_FS_SCOPE").as_deref() == Some("trueosfs")
}

fn vm_mount_selector_allowed(vm_id: u8, path: &str) -> bool {
    !path.starts_with("trueosfs:disc") || vm_has_trueosfs_scope(vm_id)
}

// ── op codes (u32, written by guest before vmcall) ──────────────────────────
pub const OP_PRESERVE: u32 = 0x01; // snapshot + stop
pub const OP_PING: u32 = 0x02; // response_data = 0xCAFE_BABE
pub const OP_UNIX_TIME: u32 = 0x03; // response_data = unix seconds
pub const OP_YIELD: u32 = 0x04; // cooperative host yield point
pub const OP_SLEEP_MS: u32 = 0x05; // cooperative host sleep before resume
pub const OP_RAND_BYTES: u32 = 0x06; // arg0 requested bytes, response payload is random bytes
pub const OP_BP_CPU_COUNT: u32 = 0x07; // response is app-visible CPU/service lane count
pub const OP_MONOTONIC_NANOS: u32 = 0x08; // response_data = host monotonic nanos
pub const OP_LIFECYCLE_PAUSE: u32 = 0x09; // legacy request; begins PreparePause only
pub const OP_BP_REL_IMAGE_EXEC_ENABLE: u32 = 0x0A; // trusted loader: arg0 GVA,arg1 bytes
pub const OP_BP_REL_IMAGE_EXEC_DISABLE: u32 = 0x0B; // trusted loader: exact active range
pub const OP_BP_LIFECYCLE_POLL: u32 = 0x0C; // response operation + deadline/reason when PreparePause is pending
pub const OP_BP_LIFECYCLE_READY: u32 = 0x0D; // arg0 operation,arg1 checkpoint version -> pause/snapshot at exact boundary
pub const OP_BP_LIFECYCLE_IDENTITY: u32 = 0x0E; // response generation/flags + instance/lineage UUID bytes
pub const OP_LIFECYCLE_SNAPSHOT: u32 = 0x0F; // request PreparePause with warm snapshot disposition
pub const OP_BP_RAPL_SNAPSHOT_READ: u32 = 0x91; // arg0 offset, arg1 cap -> latest RAPL snapshot text
pub const OP_BP_RAPL_HISTORY_READ: u32 = 0x92; // arg0 offset, arg1 cap -> capped RAPL history text
pub const OP_BP_PCI_SNAPSHOT_READ: u32 = 0x93; // arg0 offset, arg1 cap -> latest PCI snapshot text
pub const OP_BP_THERMAL_SNAPSHOT_READ: u32 = 0x94; // arg0 offset, arg1 cap -> latest thermal snapshot text
pub const OP_BP_VGPU_VVIDEO_CREATE: u32 = 0x95; // arg0 device,arg1 guest VA,payload bytes+usage -> buffer
pub const OP_BP_VGPU_VVIDEO_FLUSH: u32 = 0x96; // arg0 device,arg1 buffer,payload offset+bytes -> rc
pub const OP_BP_VGPU_VVIDEO_INVALIDATE: u32 = 0x97; // arg0 device,arg1 buffer,payload offset+bytes -> rc
pub const OP_BP_VGPU_DEVICE_DIAGNOSTICS: u32 = 0x90; // arg0 device -> counters + mapping proof
pub const OP_BP_SYSTEM_SERVICES_SNAPSHOT_READ: u32 = 0xA4; // arg0 offset, arg1 cap -> task registry snapshot
pub const OP_BP_VGPU_OPEN: u32 = 0xA5; // arg0 requested caps -> opaque device/rc
pub const OP_BP_VGPU_CLOSE: u32 = 0xA6; // arg0 device -> rc
pub const OP_BP_VGPU_DEVICE_INFO: u32 = 0xA7; // arg0 device -> DeviceInfo payload
pub const OP_BP_VGPU_BUFFER_CREATE: u32 = 0xA8; // arg0 device,arg1 bytes,payload usage -> buffer/rc
pub const OP_BP_VGPU_BUFFER_DESTROY: u32 = 0xA9; // arg0 device,arg1 buffer -> rc
pub const OP_BP_VGPU_BUFFER_INFO: u32 = 0xAA; // arg0 device,arg1 buffer -> BufferInfo payload
pub const OP_BP_VGPU_QUEUE_CREATE: u32 = 0xAB; // arg0 device,arg1 class -> queue/rc
pub const OP_BP_VGPU_QUEUE_DESTROY: u32 = 0xAC; // arg0 device,arg1 queue -> rc
pub const OP_BP_VGPU_SUBMIT_CONTROL_NOP: u32 = 0xAD; // arg0 device,arg1 queue -> TimelinePoint
pub const OP_BP_VGPU_TIMELINE: u32 = 0xAE; // arg0 device,arg1 queue -> TimelineStatus
pub const OP_BP_VGPU_WAIT: u32 = 0xAF; // arg0 device,arg1 queue,payload value -> rc
pub const OP_BP_VGPU_BUFFER_WRITE: u32 = 0xB0; // arg0 device,arg1 buffer,payload offset+bytes
pub const OP_BP_VGPU_BUFFER_READ: u32 = 0xB1; // arg0 device,arg1 buffer,payload offset+len
pub const OP_BP_VGPU_UI4_SURFACE_ACQUIRE: u32 = 0x124; // arg0 device,arg1 window -> SurfaceInfo
pub const OP_BP_VGPU_UI4_SURFACE_DISCARD: u32 = 0x125; // arg0 device,arg1 surface -> rc
pub const OP_BP_VGPU_UI4_SURFACE_CLEAR_SUBMIT: u32 = 0x126; // arg0 device,arg1 queue,payload surface+rgba -> TimelinePoint
pub const OP_BP_VGPU_SHADER_MODULE_CREATE: u32 = 0x127; // arg0 device,arg1 package digest -> shader
pub const OP_BP_VGPU_SHADER_MODULE_DESTROY: u32 = 0x128; // arg0 device,arg1 shader -> rc
pub const OP_BP_VGPU_RENDER_PIPELINE_CREATE: u32 = 0x129; // arg0 device,arg1 shader,payload stride+position -> pipeline
pub const OP_BP_VGPU_RENDER_PIPELINE_DESTROY: u32 = 0x12A; // arg0 device,arg1 pipeline -> rc
pub const OP_BP_VGPU_UI4_INDEXED_SUBMIT: u32 = 0x12B; // arg0 device,arg1 queue,payload IndexedDraw -> TimelinePoint
pub const OP_BP_ASYNC_FS_RECORD_KEY_START: u32 = 0x12C; // payload resolved file path -> operation id/rc
// arg0 window,arg1 x/y,payload w/h -> rc. The active BlueprintScene producer
// is identified by its release proof, not by a UI toolkit or render engine.
pub const OP_BP_UI4_SCENE_COMPUTE_FRAME_PUBLISH: u32 = 0x12D;
pub const OP_BP_UI4_SOLARA_FONT_SIZES: u32 = 0xB2; // arg0 cap -> count + FontSize payload
pub const OP_BP_UI4_SOLARA_FRAME_OPEN: u32 = 0xB3; // arg0 x/y,arg1 width/height -> window
pub const OP_BP_UI4_SOLARA_FRAME_BEGIN: u32 = 0xB4; // arg0 window,arg1 clear RGBA -> rc
pub const OP_BP_UI4_SOLARA_TEXT_ROWS: u32 = 0xB5; // arg0 window,arg1 font/scale,payload rows -> rc
pub const OP_BP_UI4_SOLARA_FRAME_PUBLISH: u32 = 0xB6; // arg0 window,arg1 x/y,payload w/h -> rc
pub const OP_BP_UI4_SOLARA_FRAME_CLOSE: u32 = 0xB7; // arg0 window,arg1 close flags -> rc
pub const OP_BP_UI4_SOLARA_TEXT_SCENE: u32 = 0xB8; // arg0 window,arg1 font,payload viewport/rows -> rc
pub const OP_BP_GRIDPAPER_SNAPSHOT_SUBMIT: u32 = 0xB9; // arg0 generation,arg1 instance:scale,payload fixed page -> rc
pub const OP_BP_GRIDPAPER_CLOSE: u32 = 0xBA; // arg0 instance, detach calling VM's producer -> rc
pub const OP_BP_GRIDPAPER_TEXT_ANIMATIONS_SUBMIT: u32 = 0xBB; // arg0 instance,payload CSS-like text color programs -> rc
pub const OP_BP_PRINTER_SNAPSHOT_READ: u32 = 0xBC; // arg0 offset, arg1 cap -> IPP printer registry
pub const OP_BP_PRINT2D_SUBMIT: u32 = 0xBD; // arg0 document kind,arg1 subject,payload compact document -> job/rc
pub const OP_BP_PRINT2D_STATUS: u32 = 0xBE; // arg0 job id -> PrintJobState/rc
pub const OP_BP_GRIDPAPER_PRINT_REQUEST_TAKE: u32 = 0xBF; // arg0 instance, focused Print Screen request token
pub const OP_BP_UI4_SCENE_SKYBOX_UPLOAD_BEGIN: u32 = 0xC0; // arg0 window,arg1 width/height -> rc
pub const OP_BP_UI4_SCENE_SKYBOX_UPLOAD_CHUNK: u32 = 0xC1; // arg0 window,arg1 byte offset,payload RGB565 -> rc
pub const OP_BP_UI4_SCENE_SKYBOX_UPLOAD_FINISH: u32 = 0xC2; // arg0 window -> rc
pub const OP_BP_UI4_SCENE_SKYBOX_RENDER: u32 = 0xC3; // arg0 window,payload render params -> rc
pub const OP_BP_UI4_SCENE_WRITE_OPAQUE_RGBA8: u32 = 0xC4; // arg0 window,arg1 byte offset,payload RGBA8 -> rc
pub const OP_BP_UI4_SCENE_FRAME_SET_POSITION: u32 = 0xC5; // arg0 window,arg1 x/y -> rc
pub const OP_BP_UI4_SCENE_FRAME_RESIZE: u32 = 0xC6; // arg0 window,arg1 width/height -> rc
pub const OP_BP_UI4_SCENE_FRAME_OPEN_STREAMING: u32 = 0xC7; // arg0 x/y,arg1 width/height -> window
pub const OP_BP_SHELL_ATTACHED_READ: u32 = 0xCB; // arg0 cap -> attached-shell input payload
pub const OP_BP_INPUT_KEYBOARD_OUTPUT_POP: u32 = 0xCC; // response payload is one keyboard event
pub const OP_BP_INPUT_KEYBOARD_OUTPUT_SINCE: u32 = 0xCD; // arg0 read seq,arg1 cap -> payload events
pub const OP_BP_ASYNC_FS_READ_START: u32 = 0xCE; // payload resolved path -> operation id/rc
pub const OP_BP_ASYNC_FS_REMOVE_START: u32 = 0xCF; // payload resolved path -> operation id/rc
pub const OP_BP_ASYNC_FS_STATUS: u32 = 0xD0; // arg0 operation id -> pending/ready/rc
pub const OP_BP_ASYNC_FS_RESULT_LEN: u32 = 0xD1; // arg0 operation id -> result length/rc
pub const OP_BP_ASYNC_FS_RESULT_READ: u32 = 0xD2; // arg0 id,arg1 offset:cap -> payload bytes
pub const OP_BP_ASYNC_FS_DISCARD: u32 = 0xD3; // arg0 operation id -> rc
pub const OP_BP_UI4_SCENE_PAN_EVENT_TAKE: u32 = 0xD4; // arg0 window -> rc + PanEvent payload
pub const OP_BP_ASYNC_FS_WRITE_BEGIN: u32 = 0xD5; // arg0 total length, payload path -> operation id/rc
pub const OP_BP_ASYNC_FS_WRITE_CHUNK: u32 = 0xD6; // arg0 id, arg1 offset, payload bytes -> rc
pub const OP_BP_ASYNC_FS_WRITE_COMMIT: u32 = 0xD7; // arg0 operation id -> rc
pub const OP_BP_ASYNC_FS_CREATE_DIR_ALL_START: u32 = 0xD8; // payload resolved path -> operation id/rc
pub const OP_BP_ASYNC_FS_STAT_START: u32 = 0xD9; // payload resolved path -> operation id/rc
pub const OP_BP_ASYNC_FS_LIST_DIR_START: u32 = 0xDA; // payload resolved path -> operation id/rc
pub const OP_BP_ASYNC_FS_LIST_MOUNTS_START: u32 = 0x139; // no payload -> mounted TRUEOSFS roots
pub const OP_BP_ASYNC_FS_RENAME_START: u32 = 0x13A; // payload src-len + resolved src/dst -> operation id/rc
pub const OP_BP_SHELL_ATTACHED_WAIT_READABLE: u32 = 0x13B; // arg0 timeout ms -> event-driven terminal wake
// Generic same-archive child-Hull service.  Child handle 0 names the worker's
// parent endpoint; nonzero handles are opaque parent-owned values.
pub const OP_BP_CHILD_SPAWN_V1: u32 = 0x13C; // payload initial message -> child handle
pub const OP_BP_CHILD_SEND_V1: u32 = 0x13D; // arg0 handle, payload message -> bytes/rc
pub const OP_BP_CHILD_RECEIVE_V1: u32 = 0x13E; // arg0 handle -> one queued message
pub const OP_BP_CHILD_STATUS_V1: u32 = 0x13F; // arg0 handle -> lifecycle state/rc
pub const OP_BP_CHILD_TERMINATE_V1: u32 = 0x140; // arg0 child handle -> rc
pub const OP_BP_VGPU_UI4_INDEXED_BATCH_SUBMIT: u32 = 0x141; // arg0 device,arg1 queue,payload IndexedDrawBatch -> TimelinePoint
pub const OP_BP_VGPU_CLOUD_WORK_GRAPH_CREATE: u32 = 0x149;
pub const OP_BP_VGPU_CLOUD_WORK_GRAPH_DESTROY: u32 = 0x14A;
pub const OP_BP_VGPU_CLOUD_FRAME_SUBMIT: u32 = 0x14B;
const _: () = {
    assert!(OP_BP_VGPU_CLOUD_WORK_GRAPH_CREATE == 0x149);
    assert!(OP_BP_VGPU_CLOUD_WORK_GRAPH_DESTROY == 0x14A);
    assert!(OP_BP_VGPU_CLOUD_FRAME_SUBMIT == 0x14B);
    assert!(core::mem::size_of::<v::vgpu::CloudWorkGraphDescriptor>() <= PAYLOAD_CAP);
    assert!(core::mem::size_of::<v::vgpu::CloudFrameSubmit>() <= PAYLOAD_CAP);
    assert!(core::mem::size_of::<v::vgpu::CloudFrameTelemetry>() <= PAYLOAD_CAP);
};
pub const OP_BP_UI4_SCENE_KEYBOARD_STATE: u32 = 0xDB; // arg0 window -> rc + focused held-key state
pub const OP_BP_UI4_SCENE_FRAME_OPEN_IMMUTABLE: u32 = 0xDC; // arg0 x/y,arg1 width/height -> window
pub const OP_BP_UI4_SCENE_SPRITE_UPLOAD_BEGIN: u32 = 0xDD; // arg0 window,arg1 sprite,payload width/height -> rc
pub const OP_BP_UI4_SCENE_SPRITE_UPLOAD_CHUNK: u32 = 0xDE; // arg0 window,arg1 sprite:offset,payload RGBA8 -> rc
pub const OP_BP_UI4_SCENE_SPRITE_UPLOAD_FINISH: u32 = 0xDF; // arg0 window,arg1 sprite -> rc
pub const OP_BP_UI4_SCENE_SPRITE_FRAME_BEGIN: u32 = 0xE0; // arg0 window,arg1 clear RGBA -> rc
pub const OP_BP_UI4_SCENE_SPRITE_DRAW_BEGIN: u32 = 0xE1; // arg0 window,arg1 quad count -> rc
pub const OP_BP_UI4_SCENE_SPRITE_DRAW_CHUNK: u32 = 0xE2; // arg0 window,arg1 quad offset,payload records -> rc
pub const OP_BP_UI4_SCENE_SPRITE_DRAW_FINISH: u32 = 0xE3; // arg0 window -> rc
pub const OP_BP_VRAM_SNAPSHOT_READ: u32 = 0xE4; // arg0 offset, arg1 cap -> cached vGPU memory snapshot text
pub const OP_BP_UI4_SCENE_RESIZE_EVENT_TAKE: u32 = 0xE5; // arg0 window -> rc + ResizeEvent payload
pub const OP_BP_UI4_SCENE_SET_CUSTOM_CURSOR: u32 = 0xE6; // arg0 window,arg1 enabled -> rc
pub const OP_BP_UI4_SCENE_SET_CURSOR_ICON: u32 = 0xE7; // arg0 window,arg1 icon,optional cursor-source payload -> rc
pub const OP_BP_UI4_SCENE_POINTER_EVENT_TAKE: u32 = 0xE8; // arg0 window -> rc + PointerEvent payload
pub const OP_BP_UI4_SCENE_PARTICLE_CRAFT_RENDER: u32 = 0xE9; // arg0 window,payload ParticleCraftParamsV1 -> rc
pub const OP_BP_MOUSE_MOTION_CURSOR_REQUEST: u32 = 0xEA; // payload label -> rc + MouseMotionCursorInfo
pub const OP_BP_MOUSE_MOTION_CURSOR_RELEASE: u32 = 0xEB; // arg0 handle -> rc
pub const OP_BP_MOUSE_MOTION_SUBMIT: u32 = 0xEC; // arg0 handle,payload MouseMotionCommand -> rc
pub const OP_BP_MOUSE_MOTION_SUBMIT_JSON: u32 = 0xED; // arg0 handle,payload JSON -> command count/rc
pub const OP_BP_MOUSE_MOTION_CURSOR_IDLE: u32 = 0xEE; // arg0 handle -> bool/rc
pub const OP_BP_KEYBOARD_CONTROL_REQUEST: u32 = 0xEF; // payload label -> rc + KeyboardControlDeviceInfo
pub const OP_BP_KEYBOARD_CONTROL_RELEASE: u32 = 0xF0; // arg0 handle -> rc
pub const OP_BP_KEYBOARD_CONTROL_SUBMIT: u32 = 0xF1; // arg0 handle,payload KeyboardControlCommand -> rc
pub const OP_BP_KEYBOARD_CONTROL_SUBMIT_TEXT: u32 = 0xF2; // arg0 handle,arg1 interval:flags,payload UTF-8 -> count/rc
pub const OP_BP_KEYBOARD_CONTROL_SUBMIT_JSON: u32 = 0xF3; // arg0 handle,payload JSON -> command count/rc
pub const OP_BP_KEYBOARD_CONTROL_IDLE: u32 = 0xF4; // arg0 handle -> bool/rc
pub const OP_BP_UI4_SCENE_FIRST_PRESENTATION_TAKE: u32 = 0xF5; // arg0 window -> first SURFLIVE event/empty/rc
pub const OP_BP_UI4_SCENE_OUTPUT_DIMENSIONS: u32 = 0xF6; // -> packed output width:height
pub const OP_BP_USB_SNAPSHOT_READ: u32 = 0xF7; // arg0 offset, arg1 cap -> USB inventory snapshot
pub const OP_BP_UI4_SCENE_INPUT_ROUTES: u32 = 0xF8; // arg0 window,arg1 cap -> selected combo/keyboard routes
pub const OP_BP_GRIDPAPER_SNAPSHOT_CHECKPOINT: u32 = 0xF9; // arg0 instance -> page image + release
pub const OP_BP_ARCHIVE_PACK_START: u32 = 0xFA; // arg0 source len,payload source+archive paths -> operation id/rc
pub const OP_BP_ARCHIVE_UNPACK_START: u32 = 0xFB; // arg0 archive len,payload archive+destination paths -> operation id/rc
pub const OP_BP_ARCHIVE_STATUS: u32 = 0xFC; // arg0 operation id -> pending/ready/rc
pub const OP_BP_ARCHIVE_REPORT: u32 = 0xFD; // arg0 operation id -> completion report payload/rc
pub const OP_BP_ARCHIVE_DISCARD: u32 = 0xFE; // arg0 operation id -> rc
pub const OP_BP_UI4_FONT_CANVAS: u32 = 0xFF; // arg0 window,arg1 font,payload colored rows -> rc
pub const OP_BP_LUMEN_TEMPLATE_OPEN: u32 = 0x100;
pub const OP_BP_LUMEN_PROMPT_SUBMIT: u32 = 0x101;
pub const OP_BP_LUMEN_STATUS: u32 = 0x102;
pub const OP_BP_LUMEN_REPLY_READ: u32 = 0x103;
pub const OP_BP_LUMEN_CHECKPOINT_REQUEST: u32 = 0x104;
pub const OP_BP_LUMEN_CHECKPOINT_READ: u32 = 0x105;
pub const OP_BP_LUMEN_RESTORE_BEGIN: u32 = 0x106;
pub const OP_BP_LUMEN_RESTORE_WRITE: u32 = 0x107;
pub const OP_BP_LUMEN_RESTORE_COMMIT: u32 = 0x108;
pub const OP_BP_LUMEN_CLOSE: u32 = 0x109;
pub const OP_BP_SPIRIT_EMOTION_PLAY: u32 = 0x10A;
pub const OP_BP_SPIRIT_RESPONSE_PRESENT: u32 = 0x10B;
pub const OP_BP_SPIRIT_MOVE: u32 = 0x10C;
pub const OP_BP_SHELL2_FRONTEND_ATTACH_V1: u32 = 0x10D;
pub const OP_BP_SHELL2_FRONTEND_READ_V1: u32 = 0x10E;
pub const OP_BP_SHELL2_FRONTEND_SUBMIT_INPUT_V1: u32 = 0x10F;
pub const OP_BP_SHELL2_FRONTEND_DETACH_V1: u32 = 0x110;
pub const OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE: u32 = 0x111; // arg0 window -> rc + routed KeyboardOutputEvent payload
pub const OP_BP_SPIRIT_TEXT_PRESENT_SILENT: u32 = 0x112; // arg0 turn,payload display-safe UTF-8 -> rc
pub const OP_BP_FETCH_POST_JSON_BYTES_START: u32 = 0x113; // arg0 timeout,arg1 hi32 bearer/lo32 URL,payload URL||bearer||JSON
pub const OP_BP_DOBBY_UI4_WINDOWS: u32 = 0x114; // arg0 cap -> compact live-window JSON
pub const OP_BP_DOBBY_UI4_FOCUS: u32 = 0x115; // arg0 generation-safe window id -> rc
pub const OP_BP_DOBBY_UI4_OBSERVE_PREPARE: u32 = 0x116; // selected Lilly frame -> cached PNG length/rc
pub const OP_BP_DOBBY_UI4_OBSERVE_METADATA: u32 = 0x117; // arg0 cap -> cached compact JSON
pub const OP_BP_DOBBY_UI4_OBSERVE_READ: u32 = 0x118; // arg0 offset,arg1 cap -> cached PNG bytes
pub const OP_BP_DOBBY_UI4_POINTER: u32 = 0x119; // arg0 x:u16|y:u16,arg1 action -> rc
pub const OP_BP_DOBBY_UI4_TYPE: u32 = 0x11A; // payload UTF-8 -> rc
pub const OP_BP_DOBBY_UI4_KEY: u32 = 0x11B; // arg0 named key -> rc
pub const OP_BP_UI4_SCENE_FRAME_OPEN_VISUAL: u32 = 0x11C; // arg0 x/y,arg1 width/height,payload target_hz -> window
pub const OP_BP_UI4_SCENE_SHADERTOY_RENDER: u32 = 0x11D; // arg0 window,payload ShadertoyParamsV1 -> rc
pub const OP_BP_UI4_SCENE_VISUAL_FRAME_BEGIN: u32 = 0x11E; // arg0 window -> kernel-deadline wait + acquired back buffer
pub const OP_BP_UI4_CONTEXT_MENU_REGISTER: u32 = 0x11F; // arg0 window,payload labelled entries -> rc
pub const OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE: u32 = 0x120; // arg0 window -> rc + TrueosUi4ContextMenuEvent payload
pub const OP_BP_IMAGE_SOURCE_INFO: u32 = 0x121; // payload source name -> ImageSourceInfo
pub const OP_BP_IMAGE_SOURCE_READ: u32 = 0x122; // arg0 offset,arg1 cap,payload source name -> bytes
pub const OP_BP_UI4_SCENE_FRAME_SET_HIT_TESTABLE: u32 = 0x123; // arg0 window,arg1 enabled -> rc
pub const OP_BP_VMEDIA_IMAGE_DECODE_BEGIN: u32 = 0x142; // arg0 format,arg1 encoded bytes -> operation id/rc
pub const OP_BP_VMEDIA_IMAGE_DECODE_WRITE: u32 = 0x143; // arg0 operation,arg1 offset,payload encoded chunk -> rc
pub const OP_BP_VMEDIA_IMAGE_DECODE_COMMIT: u32 = 0x144; // arg0 operation -> rc
pub const OP_BP_VMEDIA_IMAGE_DECODE_STATUS: u32 = 0x145; // arg0 operation -> pending/ready/rc
pub const OP_BP_VMEDIA_IMAGE_DECODE_INFO: u32 = 0x146; // arg0 operation -> ImageInfo payload/rc
pub const OP_BP_VMEDIA_IMAGE_DECODE_READ: u32 = 0x147; // arg0 operation,arg1 hi32 offset/lo32 cap -> RGBA bytes
pub const OP_BP_VMEDIA_IMAGE_DECODE_DISCARD: u32 = 0x148; // arg0 operation -> rc
pub const OP_BP_TERMINAL_LEASE_CURRENT_V1: u32 = 0x134; // arg0 ready epoch or 0 -> active epoch/error
pub const OP_BP_TERMINAL_LEASE_RELEASE_V1: u32 = 0x135; // arg0 expected active epoch -> parking ticket/error
pub const OP_BP_TERMINAL_LEASE_POLL_REENTRY_V1: u32 = 0x136; // arg0 parking ticket -> pending/active epoch/error
pub const OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1: u32 = 0x137; // active terminal surface generation + geometry record/error
pub const OP_BP_LOG_RECORD_V1: u32 = 0x138; // arg0 level,arg1 target bytes,payload target || message -> host LogOs
pub const OP_NET_TCP_WRITE: u32 = 0x10; // request payload -> net tcp shell tx
pub const OP_NET_TCP_READ: u32 = 0x11; // net tcp shell rx -> response payload
pub const OP_BP_NET_OPEN: u32 = 0x20; // host-owned blueprint vnet session
pub const OP_BP_NET_SUBMIT: u32 = 0x21; // request payload is wire Command
pub const OP_BP_NET_POLL: u32 = 0x22; // response payload is optional wire Event
pub const OP_BP_FETCH_BYTES_START: u32 = 0x23; // request payload is URL, response is op id
pub const OP_BP_FETCH_BYTES_RESULT_LEN: u32 = 0x24; // arg0 is op id, response is signed len/rc
pub const OP_BP_FETCH_BYTES_READ: u32 = 0x25; // arg0 op id,arg1 offset:cap,response payload bytes
pub const OP_BP_FETCH_BYTES_DISCARD: u32 = 0x26; // arg0 is op id
pub const OP_BP_FETCH_FILE_START: u32 = 0x27; // arg0 url len, payload is URL || cache path
pub const OP_BP_FETCH_FILE_RESULT: u32 = 0x28; // arg0 is op id, response is signed rc/pending
pub const OP_BP_FETCH_FILE_DISCARD: u32 = 0x29; // arg0 is op id
pub const OP_BP_ENV_ARGS_COUNT: u32 = 0x2A; // response is argc
pub const OP_BP_ENV_ARG: u32 = 0x2B; // arg0 is index, response payload is arg bytes
pub const OP_BP_ENV_VAR: u32 = 0x2C; // request payload is key, response payload is value bytes
pub const OP_BP_FS_READ_FILE: u32 = 0x2D; // arg0 offset, arg1 cap; payload path -> payload bytes
pub const OP_BP_FS_WRITE_BEGIN: u32 = 0x2E; // arg0 total len, payload path -> response handle/rc
pub const OP_BP_FS_WRITE_CHUNK: u32 = 0x2F; // arg0 handle, payload chunk -> rc
pub const OP_BP_FS_WRITE_FINISH: u32 = 0x30; // arg0 handle -> rc
pub const OP_BP_FS_WRITE_ABORT: u32 = 0x31; // arg0 handle -> rc
pub const OP_BP_FS_EXISTS: u32 = 0x33; // payload path -> 0/1/rc
pub const OP_BP_FS_REMOVE: u32 = 0x34; // payload path -> rc
pub const OP_BP_FS_STAT: u32 = 0x60; // payload path -> rc + kind in response_data[63:32], optional payload kind:u32 len:u64
pub const OP_BP_THREAD_CURRENT_ID: u32 = 0x61; // response is current TRUEOS vthread id
pub const OP_BP_SERVICE_LANE_SUBMIT: u32 = 0x62; // arg0/arg1 boxed service-lane job raw parts
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub const OP_BP_TOKIO_BLOCKING_SPAWN: u32 = OP_BP_SERVICE_LANE_SUBMIT; // compatibility alias
pub const OP_BP_PLATFORM_WAKE_ONE: u32 = 0x63; // arg0 VM-local wait key -> woke bool
pub const OP_BP_PLATFORM_WAKE_ALL: u32 = 0x64; // arg0 VM-local wait key -> wake count
pub const OP_BP_INPUT_CURSOR_POS: u32 = 0x68; // arg0 cursor id -> packed x/y
pub const OP_BP_INPUT_CURSOR_BUTTONS: u32 = 0x69; // arg0 cursor id -> buttons
pub const OP_BP_INPUT_CURSOR_EVENTS: u32 = 0x6A; // arg0 read seq, arg1 cap -> payload events
pub const OP_BP_DNS_RESOLVE_IPV4: u32 = 0x6B; // payload host -> response payload IPv4 bytes
pub const OP_BP_SHELL_ATTACHED_WRITE: u32 = 0x6C; // payload bytes -> attached shell
pub const OP_BP_SHELL_ATTACHED_READ_BYTE: u32 = 0x6D; // response is byte or u64::MAX
pub const OP_BP_ENV_ALL: u32 = 0x6E; // response payload is newline-separated key=value text
pub const OP_BP_FS_LIST_TREE: u32 = 0x6F; // payload path -> response payload tree text
pub const OP_BP_SHELL_ATTACHED_READABLE_LEN: u32 = 0x70; // response is pending attached-shell input bytes
pub const OP_BP_FS_LIST_DIR: u32 = 0x81; // arg0 offset, arg1 cap; payload path -> newline children
pub const OP_BP_SHELL_RAW_WRITE: u32 = 0x99; // payload bytes -> shell2 raw surface, no log mirror
pub const OP_BP_SHELL_KONSOLE_SIZE: u32 = 0x9F; // response data packs cols:rows for attached shell
pub const OP_BP_EXIT_REASON: u32 = 0xA0; // payload utf8-ish reason string for lifecycle logs
pub const OP_BP_SHELL_KONSOLE_BEGIN_FRAME: u32 = 0xA1; // arg cols/rows+flags -> resize shell terminal
pub const OP_BP_SHUTDOWN: u32 = 0xA2; // payload utf8-ish reason, stop current blueprint VM
pub const OP_BP_RETURN_TO_CLI: u32 = 0xA3; // release rich terminal back to the launching shell2
pub const OP_BP_AUDIO_WRITE_I16_STEREO_48K: u32 = 0x9A; // payload i16 stereo 48k bytes -> frames/rc
pub const OP_BP_AUDIO_STOP: u32 = 0x9B; // stop host overlay lane
pub const OP_BP_AUDIO_PENDING_FRAMES: u32 = 0x9C; // response is host overlay pending frames
pub const OP_BP_AUDIO_SET_VOLUME_PERCENT: u32 = 0x9D; // arg0 percent -> applied percent
pub const OP_BP_AUDIO_VOLUME_PERCENT: u32 = 0x9E; // response is host overlay volume percent
pub const OP_BP_SOCKET_TCP_OPEN: u32 = 0x35; // arg0 domain/type, arg1 protocol -> socket/rc
pub const OP_BP_SOCKET_TCP_CLOSE: u32 = 0x36; // arg0 socket -> rc
pub const OP_BP_SOCKET_TCP_SET_NONBLOCKING: u32 = 0x37; // arg0 socket, arg1 bool -> rc
pub const OP_BP_SOCKET_TCP_BIND_V4: u32 = 0x38; // arg0 socket, arg1 addr/port -> rc
pub const OP_BP_SOCKET_TCP_BIND_V6: u32 = 0x39; // arg0 socket, arg1 port, payload addr -> rc
pub const OP_BP_SOCKET_TCP_CONNECT_V4: u32 = 0x3A; // arg0 socket, arg1 addr/port/nb -> rc
pub const OP_BP_SOCKET_TCP_CONNECT_V6: u32 = 0x3B; // arg0 socket, arg1 port/nb, payload addr -> rc
pub const OP_BP_SOCKET_TCP_POLL_CONNECT: u32 = 0x3C; // arg0 socket, arg1 timeout -> rc
pub const OP_BP_SOCKET_TCP_SEND: u32 = 0x3D; // arg0 socket, payload data -> signed count/rc
pub const OP_BP_SOCKET_TCP_RECV: u32 = 0x3E; // arg0 socket, arg1 cap, payload recv opts -> data
pub const OP_BP_SOCKET_TCP_SHUTDOWN: u32 = 0x3F; // arg0 socket, arg1 how -> rc
pub const OP_BP_SOCKET_TCP_TAKE_ERROR: u32 = 0x40; // arg0 socket -> rc
pub const OP_BP_SOCKET_TCP_PEER_V4: u32 = 0x41; // arg0 socket -> rc + addr/port payload
pub const OP_BP_SOCKET_TCP_PEER_V6: u32 = 0x42; // arg0 socket -> rc + addr/port payload
pub const OP_BP_MIO_TCP_LISTENER_BIND: u32 = 0x50; // payload addr -> socket id/status
pub const OP_BP_MIO_TCP_STREAM_CONNECT: u32 = 0x51; // payload addr -> socket id/status
pub const OP_BP_MIO_UDP_SOCKET_BIND: u32 = 0x52; // payload addr -> socket id/status
pub const OP_BP_MIO_SOCKET_CLOSE: u32 = 0x53; // arg0 socket -> status
pub const OP_BP_MIO_SOCKET_LOCAL_ADDR: u32 = 0x54; // arg0 socket -> addr/status
pub const OP_BP_MIO_SOCKET_PEER_ADDR: u32 = 0x55; // arg0 socket -> addr/status
pub const OP_BP_MIO_SOCKET_TAKE_ERROR: u32 = 0x56; // arg0 socket -> status
pub const OP_BP_MIO_TCP_STREAM_READ: u32 = 0x57; // arg0 socket, arg1 cap -> bytes/status
pub const OP_BP_MIO_TCP_STREAM_WRITE: u32 = 0x58; // arg0 socket, payload bytes -> signed rc
pub const OP_BP_MIO_UDP_SOCKET_CONNECT: u32 = 0x59; // arg0 socket, payload addr -> status
pub const OP_BP_MIO_UDP_SOCKET_SEND_TO: u32 = 0x5A; // arg0 socket, payload addr+bytes -> rc
pub const OP_BP_MIO_UDP_SOCKET_RECV_FROM: u32 = 0x5B; // arg0 socket, arg1 cap -> addr+bytes
pub const OP_BP_MIO_TCP_LISTENER_ACCEPT: u32 = 0x5C; // arg0 socket -> child+addr/status
pub const OP_BP_MIO_SELECTOR_REGISTER_SOCKET: u32 = 0x5D; // selector/socket/token/interests
pub const OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET: u32 = 0x5E; // selector/socket
pub const OP_BP_MIO_SELECTOR_POLL: u32 = 0x5F; // selector/cap/timeout -> ready events
pub const OP_BP_MIO_SELECTOR_WAKE: u32 = 0x80; // selector -> wake parked pollers

// ── response status codes (u32, written by host) ────────────────────────────
pub const STATUS_OK: u32 = 0;
pub const STATUS_UNKNOWN_OP: u32 = 1;
pub const STATUS_BAD_ARG: u32 = 2;
const MAX_GUEST_SLEEP_MS: u64 = 10_000;
pub const COMM_PAGE_VM_ID_MAGIC: u32 = 0x4856_0000;

// ── shared page ─────────────────────────────────────────────────────────────

/// Guest virtual address of the comm page.
/// Fixed above the maximum supported guest stack span so guest-side code can
/// use a stable address independent of the runtime-selected stack size.
pub fn comm_page_guest_va() -> u64 {
    crate::hv::memory::GUEST_COMM_PAGE_VA
}
pub const COMM_PAGE_BYTES: usize = 160 * 1024;
pub const COMM_PAGE_PAGES: usize = COMM_PAGE_BYTES / 4096;
pub const PAYLOAD_CAP: usize = COMM_PAGE_BYTES - 56;

/// Layout of the communication page shared between guest and host.
/// Guest writes request_* fields then vmcall.
/// Host writes response_* fields then vmresumes.
#[repr(C)]
pub struct CommPage {
    // guest fills before vmcall
    pub request_op: u32,
    pub request_seq: u32,
    pub request_arg0: u64,
    pub request_arg1: u64,
    pub request_len: u32,
    pub request_pad: u32,
    // host fills before vmresume
    pub response_seq: u32,
    pub response_status: u32,
    pub response_data: u64,
    pub response_len: u32,
    pub response_pad: u32,
    pub payload: [u8; PAYLOAD_CAP],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    Resume,
    Stop,
    Pause,
    Preserve,
    Yield,
    SleepMs(u64),
    /// Park this Hull vthread until its attached terminal receives input, its
    /// typed surface changes, or the supplied timeout expires.
    WaitConsoleInput {
        seq: u32,
        timeout_ms: u64,
    },
    /// Keep the current VMCALL pending, sleep in the host, then dispatch the
    /// unchanged request again before the guest resumes.
    RetryAfterMs(u64),
}

static GUEST_CABI_SEQ: AtomicU32 = AtomicU32::new(1);

/// Static backing pages for CommPage.
#[repr(C, align(4096))]
pub struct CommPageBacking([u8; COMM_PAGE_BYTES]);

pub static mut COMM_PAGES: [CommPageBacking; crate::hv::TRUEOS_VM_ID_LIMIT] =
    [const { CommPageBacking([0u8; COMM_PAGE_BYTES]) }; crate::hv::TRUEOS_VM_ID_LIMIT];

#[inline]
fn host_ptr(vm_id: u8) -> Option<*mut CommPage> {
    if (vm_id as usize) >= crate::hv::TRUEOS_VM_ID_LIMIT {
        return None;
    }
    Some(unsafe { core::ptr::addr_of_mut!(COMM_PAGES[vm_id as usize].0) as *mut CommPage })
}

pub fn prepare_for_vm(vm_id: u8, reset_transport: bool) -> bool {
    let Some(p) = host_ptr(vm_id) else {
        return false;
    };
    unsafe {
        if reset_transport {
            core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<CommPage>());
        }
        core::ptr::write_volatile(
            &mut (*p).response_pad,
            COMM_PAGE_VM_ID_MAGIC | vm_id.saturating_add(1) as u32,
        );
    }
    true
}

pub(crate) fn guest_comm_page_vm_id_tag() -> Option<u32> {
    let p = comm_page_guest_va() as *const CommPage;
    unsafe {
        let tag = core::ptr::read_volatile(&(*p).response_pad);
        if (tag & 0xFFFF_0000) != COMM_PAGE_VM_ID_MAGIC {
            return None;
        }
        Some(tag & 0xFF)
    }
}

pub fn pa_for_vm(vm_id: u8) -> Option<u64> {
    pa_for_vm_page(vm_id, 0)
}

pub fn pa_for_vm_page(vm_id: u8, page_index: usize) -> Option<u64> {
    if (vm_id as usize) >= crate::hv::TRUEOS_VM_ID_LIMIT {
        return None;
    }
    if page_index >= COMM_PAGE_PAGES {
        return None;
    }
    let va = unsafe { core::ptr::addr_of!(COMM_PAGES[vm_id as usize].0) as u64 }
        .saturating_add((page_index * 4096) as u64);
    kernel_va_to_pa(va)
}

// ── transport helpers ────────────────────────────────────────────────────────

fn read_request(vm_id: u8) -> Option<(u32, u32, u64, u64, u32)> {
    let p = host_ptr(vm_id)?;
    unsafe {
        Some((
            core::ptr::read_volatile(&(*p).request_op),
            core::ptr::read_volatile(&(*p).request_seq),
            core::ptr::read_volatile(&(*p).request_arg0),
            core::ptr::read_volatile(&(*p).request_arg1),
            core::ptr::read_volatile(&(*p).request_len),
        ))
    }
}

fn write_response(vm_id: u8, seq: u32, status: u32, data: u64, len: u32) {
    let Some(p) = host_ptr(vm_id) else {
        return;
    };
    unsafe {
        core::ptr::write_volatile(&mut (*p).response_status, status);
        core::ptr::write_volatile(&mut (*p).response_data, data);
        core::ptr::write_volatile(&mut (*p).response_len, len);
        // seq written last — guest may poll this as a completion flag
        core::ptr::write_volatile(&mut (*p).response_seq, seq);
    }
}

pub(crate) fn complete_console_input_wait(vm_id: u8, seq: u32, woke: bool) {
    write_response(vm_id, seq, STATUS_OK, u64::from(woke), 0);
}

fn release_guest_comm_page(vm_id: u8) {
    let Some(p) = host_ptr(vm_id) else {
        return;
    };
    let lock = unsafe { AtomicU32::from_ptr(core::ptr::addr_of_mut!((*p).request_pad)) };
    lock.store(0, Ordering::Release);
}

fn write_record_response<T: Copy>(vm_id: u8, seq: u32, data: u64, value: &T) {
    let len = core::mem::size_of::<T>();
    if len > PAYLOAD_CAP {
        write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
        return;
    }
    let Some(page) = host_ptr(vm_id) else {
        return;
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            (value as *const T).cast::<u8>(),
            (*page).payload.as_mut_ptr(),
            len,
        );
    }
    write_response(vm_id, seq, STATUS_OK, data, len as u32);
}

fn write_record_slice_response<T: Copy>(vm_id: u8, seq: u32, data: u64, values: &[T]) {
    let Some(len) = core::mem::size_of::<T>().checked_mul(values.len()) else {
        write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
        return;
    };
    if len > PAYLOAD_CAP {
        write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
        return;
    }
    let Some(page) = host_ptr(vm_id) else {
        return;
    };
    if len != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                values.as_ptr().cast::<u8>(),
                (*page).payload.as_mut_ptr(),
                len,
            );
        }
    }
    write_response(vm_id, seq, STATUS_OK, data, len as u32);
}

fn request_payload(vm_id: u8, req_len: u32) -> Option<&'static [u8]> {
    if req_len as usize > PAYLOAD_CAP {
        return None;
    }
    let p = host_ptr(vm_id)?;
    Some(unsafe { &(&(*p).payload)[..req_len as usize] })
}

fn handle_vlayer_text_read_vmcall(
    vm_id: u8,
    seq: u32,
    offset: u64,
    cap: u64,
    len_fn: fn() -> usize,
    read_fn: fn(usize, &mut [u8]) -> usize,
) {
    if cap == 0 {
        write_response(vm_id, seq, STATUS_OK, len_fn() as u64, 0);
        return;
    }

    let Some(p) = host_ptr(vm_id) else {
        write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
        return;
    };

    let want = core::cmp::min(cap as usize, PAYLOAD_CAP);
    let copied = unsafe { read_fn(offset as usize, &mut (&mut (*p).payload)[..want]) };
    write_response(vm_id, seq, STATUS_OK, copied as u64, copied as u32);
}

pub fn guest_call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) {
    let p = comm_page_guest_va() as *mut CommPage;
    // The Hull has one comm page per VM, shared by all of its vthreads and by
    // both guest-side call paths. Use the transport-private request padding as
    // the common lock word so payloads and responses cannot cross-contaminate.
    let lock = unsafe {
        AtomicU32::from_ptr(core::ptr::addr_of_mut!((*p).request_pad)) as *const AtomicU32
    };
    while unsafe { &*lock }
        .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    struct Unlock(*const AtomicU32);
    impl Drop for Unlock {
        fn drop(&mut self) {
            unsafe { &*self.0 }.store(0, Ordering::Release);
        }
    }
    let unlock = Unlock(lock);
    let seq = GUEST_CABI_SEQ.fetch_add(1, Ordering::Relaxed);
    unsafe {
        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, 0);
        core::ptr::write_volatile(&mut (*p).request_seq, seq);
        core::ptr::write_volatile(&mut (*p).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
        if matches!(op, OP_YIELD | OP_SLEEP_MS) {
            core::mem::forget(unlock);
            return (STATUS_OK, 0);
        }
        let status = core::ptr::read_volatile(&(*p).response_status);
        let data = core::ptr::read_volatile(&(*p).response_data);
        (status, data)
    }
}

pub fn guest_yield() {
    let _ = guest_call(OP_YIELD, 0, 0);
}

pub fn guest_sleep_ms(ms: u64) {
    let _ = guest_call(OP_SLEEP_MS, ms, 0);
}

pub fn guest_cpu_count() -> Option<usize> {
    let (status, count) = guest_call(OP_BP_CPU_COUNT, 0, 0);
    if status == STATUS_OK {
        Some(count.max(1) as usize)
    } else {
        None
    }
}

pub fn guest_monotonic_nanos() -> u64 {
    let (status, nanos) = guest_call(OP_MONOTONIC_NANOS, 0, 0);
    if status == STATUS_OK { nanos } else { 0 }
}

pub fn guest_unix_seconds() -> u64 {
    let (status, seconds) = guest_call(OP_UNIX_TIME, 0, 0);
    if status == STATUS_OK { seconds } else { 0 }
}

#[inline]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn pack_i32_pair(a: i32, b: i32) -> u64 {
    ((a as u32 as u64) << 32) | (b as u32 as u64)
}

#[inline]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn pack_u32_pair(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | (b as u64)
}

#[inline]
fn unpack_i32_pair(raw: u64) -> (i32, i32) {
    ((raw >> 32) as u32 as i32, raw as u32 as i32)
}

#[inline]
fn unpack_u32_pair(raw: u64) -> (u32, u32) {
    ((raw >> 32) as u32, raw as u32)
}

const MIO_ADDR_BYTES: usize = core::mem::size_of::<crate::mio_compat::TrueosMioSocketAddr>();
const MIO_READY_EVENT_BYTES: usize = core::mem::size_of::<crate::mio_compat::TrueosMioReadyEvent>();

fn read_mio_addr(bytes: &[u8]) -> Option<crate::mio_compat::TrueosMioSocketAddr> {
    if bytes.len() < MIO_ADDR_BYTES {
        return None;
    }
    Some(unsafe {
        core::ptr::read_unaligned(bytes.as_ptr() as *const crate::mio_compat::TrueosMioSocketAddr)
    })
}

fn write_mio_addr(out: &mut [u8], addr: crate::mio_compat::TrueosMioSocketAddr) -> bool {
    if out.len() < MIO_ADDR_BYTES {
        return false;
    }
    let bytes =
        unsafe { core::slice::from_raw_parts(&addr as *const _ as *const u8, MIO_ADDR_BYTES) };
    out[..MIO_ADDR_BYTES].copy_from_slice(bytes);
    true
}

// ── exec dispatch ────────────────────────────────────────────────────────────

/// Called from the vmexit loop on every VMCALL exit.
pub fn dispatch(vm_id: u8) -> DispatchOutcome {
    crate::allocators::with_host_alloc_domain(|| {
        crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::VmBroker,
            Some(vm_id),
            || crate::hv::with_guest_broker_context(vm_id, || dispatch_inner(vm_id)),
        )
    })
}

fn dispatch_inner(vm_id: u8) -> DispatchOutcome {
    let Some((op, seq, arg0, arg1, req_len)) = read_request(vm_id) else {
        hvwarnf(format_args!("hv: vm{} reporting: vmcall bad vm id", vm_id));
        return DispatchOutcome::Stop;
    };
    match op {
        OP_PRESERVE => {
            match crate::hv::prepare_preserve_mode(vm_id, crate::hv::PreserveMode::Stop) {
                Ok(true) => {
                    write_response(vm_id, seq, STATUS_OK, 0, 0);
                    DispatchOutcome::Preserve
                }
                Ok(false) | Err(_) => {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    DispatchOutcome::Resume
                }
            }
        }
        OP_LIFECYCLE_PAUSE => {
            match crate::hv::request_blueprint_prepare_pause(
                vm_id,
                crate::hv::BlueprintPauseReason::Pause,
            ) {
                Ok(true) => {
                    write_response(vm_id, seq, STATUS_OK, 0, 0);
                    DispatchOutcome::Resume
                }
                Ok(false) | Err(_) => {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    DispatchOutcome::Resume
                }
            }
        }
        OP_LIFECYCLE_SNAPSHOT => {
            match crate::hv::request_blueprint_prepare_pause(
                vm_id,
                crate::hv::BlueprintPauseReason::Replicate,
            ) {
                Ok(true) => {
                    write_response(vm_id, seq, STATUS_OK, 0, 0);
                    DispatchOutcome::Resume
                }
                Ok(false) | Err(_) => {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    DispatchOutcome::Resume
                }
            }
        }
        OP_BP_LIFECYCLE_POLL => {
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct PrepareWire {
                deadline_ms: u64,
                reason: u32,
                reserved: u32,
            }

            if let Some(prepare) = crate::hv::blueprint_prepare_pause(vm_id) {
                let wire = PrepareWire {
                    deadline_ms: prepare.deadline_ms,
                    reason: prepare.reason as u32,
                    reserved: 0,
                };
                write_record_response(vm_id, seq, prepare.operation, &wire);
            } else {
                write_response(vm_id, seq, STATUS_OK, 0, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_LIFECYCLE_READY => match crate::hv::acknowledge_blueprint_ready(vm_id, arg0, arg1) {
            Some(crate::hv::BlueprintReadyDisposition::Pause) => {
                write_response(vm_id, seq, STATUS_OK, arg0, 0);
                DispatchOutcome::Pause
            }
            Some(crate::hv::BlueprintReadyDisposition::Snapshot) => {
                write_response(vm_id, seq, STATUS_OK, arg0, 0);
                DispatchOutcome::Preserve
            }
            None => {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                DispatchOutcome::Resume
            }
        },
        OP_BP_LIFECYCLE_IDENTITY => {
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct IdentityWire {
                instance: [u8; 16],
                lineage: [u8; 16],
                flags: u32,
                reserved: u32,
            }

            if let Some(identity) = crate::hv::blueprint_instance_identity(vm_id) {
                let wire = IdentityWire {
                    instance: identity.instance,
                    lineage: identity.lineage,
                    flags: u32::from(identity.clone),
                    reserved: 0,
                };
                write_record_response(vm_id, seq, identity.generation, &wire);
            } else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_TEMPLATE_OPEN => {
            let rc = request_payload(vm_id, req_len)
                .map(|system| crate::r::lumen_service::template_open(vm_id, system))
                .unwrap_or(-3);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_PROMPT_SUBMIT => {
            let rc = request_payload(vm_id, req_len)
                .and_then(|payload| {
                    let tail_len = u32::from_le_bytes(payload.get(..4)?.try_into().ok()?) as usize;
                    if tail_len > 2 {
                        return None;
                    }
                    let tail_end = 4usize.checked_add(tail_len.checked_mul(4)?)?;
                    let tail_bytes = payload.get(4..tail_end)?;
                    let mut tail = [0u32; 2];
                    for (index, chunk) in tail_bytes.chunks_exact(4).enumerate() {
                        tail[index] = u32::from_le_bytes(chunk.try_into().ok()?);
                    }
                    let prompt = payload.get(tail_end..)?;
                    Some(crate::r::lumen_service::prompt_submit(
                        vm_id,
                        arg0,
                        &tail[..tail_len],
                        prompt,
                    ))
                })
                .unwrap_or(-3);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_STATUS => {
            if let Some(status) = crate::r::lumen_service::status(vm_id) {
                write_record_response(vm_id, seq, 0, &status);
            } else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_REPLY_READ => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let cap = (arg0 as usize).min(PAYLOAD_CAP);
            let rc = unsafe {
                crate::r::lumen_service::reply_read(vm_id, &mut (&mut (*page).payload)[..cap])
            };
            let len = usize::try_from(rc).unwrap_or(0).min(cap);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_CHECKPOINT_REQUEST => {
            let rc = crate::r::lumen_service::checkpoint_request(vm_id);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_CHECKPOINT_READ => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let cap = (arg1 as usize).min(PAYLOAD_CAP);
            let rc = unsafe {
                crate::r::lumen_service::checkpoint_read(
                    vm_id,
                    arg0 as usize,
                    &mut (&mut (*page).payload)[..cap],
                )
            };
            let len = usize::try_from(rc).unwrap_or(0).min(cap);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_RESTORE_BEGIN => {
            let rc = crate::r::lumen_service::restore_begin(vm_id, arg0 as usize);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_RESTORE_WRITE => {
            let rc = request_payload(vm_id, req_len)
                .map(|data| crate::r::lumen_service::restore_write(vm_id, arg0 as usize, data))
                .unwrap_or(-3);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_RESTORE_COMMIT => {
            let rc = crate::r::lumen_service::restore_commit(vm_id);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LUMEN_CLOSE => {
            let rc = crate::r::lumen_service::close(vm_id);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SPIRIT_EMOTION_PLAY => {
            let rc = request_payload(vm_id, req_len)
                .and_then(|idea| core::str::from_utf8(idea).ok())
                .map(|idea| {
                    crate::spirit::enqueue_emotion_words(&[idea])
                        .map(|_| 0)
                        .unwrap_or(-5)
                })
                .unwrap_or(-3);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SPIRIT_RESPONSE_PRESENT => {
            let rc = request_payload(vm_id, req_len)
                .map(|text| crate::r::lumen_service::spirit_response_present(vm_id, arg0, text))
                .unwrap_or(-3);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SPIRIT_TEXT_PRESENT_SILENT => {
            let rc = request_payload(vm_id, req_len)
                .map(|text| crate::r::lumen_service::spirit_text_present_silent(arg0, text))
                .unwrap_or(-3);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SPIRIT_MOVE => {
            let x = f32::from_bits(arg0 as u32);
            let y = f32::from_bits(arg1 as u32);
            let rc = crate::r::lumen_service::spirit_move(x, y);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_WINDOWS => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let cap = (arg0 as usize).min(PAYLOAD_CAP);
            let rc = unsafe {
                crate::spirit::dobby_ui::windows(vm_id, &mut (&mut (*page).payload)[..cap])
            };
            let response_len = usize::try_from(rc)
                .ok()
                .filter(|len| *len <= cap)
                .unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_FOCUS => {
            let rc = crate::spirit::dobby_ui::focus(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_OBSERVE_PREPARE => {
            let rc = crate::spirit::dobby_ui::observe_prepare(vm_id);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_OBSERVE_METADATA => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let cap = (arg0 as usize).min(PAYLOAD_CAP);
            let rc = unsafe {
                crate::spirit::dobby_ui::observe_metadata(vm_id, &mut (&mut (*page).payload)[..cap])
            };
            let response_len = usize::try_from(rc)
                .ok()
                .filter(|len| *len <= cap)
                .unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_OBSERVE_READ => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let cap = (arg1 as usize).min(PAYLOAD_CAP);
            let rc = unsafe {
                crate::spirit::dobby_ui::observe_read(
                    vm_id,
                    arg0 as usize,
                    &mut (&mut (*page).payload)[..cap],
                )
            };
            let response_len = usize::try_from(rc)
                .ok()
                .filter(|len| *len <= cap)
                .unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_POINTER => {
            let rc = if arg0 >> 32 != 0 || arg1 > u64::from(u32::MAX) {
                crate::spirit::dobby_ui::ERROR_BAD_INPUT
            } else {
                let x = (arg0 & 0xFFFF) as u16;
                let y = ((arg0 >> 16) & 0xFFFF) as u16;
                crate::spirit::dobby_ui::pointer(vm_id, x, y, arg1 as u32)
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_TYPE => {
            let rc = request_payload(vm_id, req_len)
                .map(|text| crate::spirit::dobby_ui::type_text(vm_id, text))
                .unwrap_or(crate::spirit::dobby_ui::ERROR_BAD_INPUT);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_DOBBY_UI4_KEY => {
            let rc = u32::try_from(arg0)
                .map(|key| crate::spirit::dobby_ui::key(vm_id, key))
                .unwrap_or(crate::spirit::dobby_ui::ERROR_BAD_INPUT);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL2_FRONTEND_ATTACH_V1 => {
            let rc =
                crate::shell2::backends::session_pool::attach(vm_id, arg0 as usize, arg1 as usize);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL2_FRONTEND_READ_V1 => {
            const HEADER_LEN: usize = 24;
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let cap = (arg1 as usize).min(PAYLOAD_CAP.saturating_sub(HEADER_LEN));
            let payload = unsafe { &mut (*page).payload };
            match crate::shell2::backends::session_pool::read(
                vm_id,
                arg0,
                &mut payload[HEADER_LEN..HEADER_LEN + cap],
            ) {
                Ok(read) => {
                    payload[0..8].copy_from_slice(&read.next_seq.to_le_bytes());
                    payload[8..16].copy_from_slice(&read.epoch.to_le_bytes());
                    payload[16..20].copy_from_slice(&read.flags.to_le_bytes());
                    payload[20..24].fill(0);
                    write_response(
                        vm_id,
                        seq,
                        STATUS_OK,
                        read.len as u64,
                        (HEADER_LEN + read.len) as u32,
                    );
                }
                Err(rc) => {
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                }
            }
            DispatchOutcome::Resume
        }
        OP_BP_SHELL2_FRONTEND_SUBMIT_INPUT_V1 => {
            let rc = request_payload(vm_id, req_len)
                .map(|bytes| {
                    crate::shell2::backends::session_pool::submit_input(vm_id, bytes)
                        .map(|written| written as isize)
                        .unwrap_or_else(|error| error as isize)
                })
                .unwrap_or(-1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL2_FRONTEND_DETACH_V1 => {
            let rc = crate::shell2::backends::session_pool::detach(vm_id);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_PING => {
            write_response(vm_id, seq, STATUS_OK, 0xCAFE_BABE, 0);
            DispatchOutcome::Resume
        }
        OP_BP_CPU_COUNT => {
            let count = crate::hv::blueprint_exposed_cpu_count(vm_id);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
        OP_UNIX_TIME => {
            let t = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, t, 0);
            DispatchOutcome::Resume
        }
        OP_MONOTONIC_NANOS => {
            let t = crate::chronos::monotonic_nanos();
            write_response(vm_id, seq, STATUS_OK, t, 0);
            DispatchOutcome::Resume
        }
        OP_BP_REL_IMAGE_EXEC_ENABLE | OP_BP_REL_IMAGE_EXEC_DISABLE => {
            let executable = op == OP_BP_REL_IMAGE_EXEC_ENABLE;
            match crate::hv::memory::set_guest_rel_image_exec(
                vm_id,
                arg0,
                arg1 as usize,
                executable,
            ) {
                Ok((start, end)) => {
                    hvlogf(format_args!(
                        "blueprint-rel: vm={} stage={} status=ok pages={} gva=0x{:016X}..0x{:016X}",
                        vm_id,
                        if executable {
                            "exec-enable"
                        } else {
                            "exec-disable"
                        },
                        end.saturating_sub(start).div_ceil(4096),
                        start,
                        end,
                    ));
                    write_response(vm_id, seq, STATUS_OK, 0, 0);
                }
                Err(error) => {
                    hvwarnf(format_args!(
                        "blueprint-rel: vm={} stage={} status=error reason={}",
                        vm_id,
                        if executable {
                            "exec-enable"
                        } else {
                            "exec-disable"
                        },
                        error,
                    ));
                    write_response(vm_id, seq, STATUS_OK, (-13i64) as u64, 0);
                }
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_OPEN => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let data = match crate::r::io::vgpu_cabi::broker_open(principal, arg0) {
                Ok(device) => {
                    hvlogf(format_args!(
                        "vgpu-vvideo: vm={} stage=open status=ok capabilities=0x{:X}",
                        vm_id, arg0
                    ));
                    device
                }
                Err(rc) => {
                    hvwarnf(format_args!(
                        "vgpu-vvideo: vm={} stage=open status=error rc={} capabilities=0x{:X}",
                        vm_id, rc, arg0
                    ));
                    (rc as i64) as u64
                }
            };
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_CLOSE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc = crate::r::io::vgpu_cabi::broker_close(principal, arg0);
            if rc == 0 {
                hvlogf(format_args!("vgpu-vvideo: vm={} stage=close status=ok", vm_id));
            } else {
                hvwarnf(format_args!(
                    "vgpu-vvideo: vm={} stage=close status=error rc={}",
                    vm_id, rc
                ));
            }
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_DEVICE_INFO => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            match crate::r::io::vgpu_cabi::broker_device_info(principal, arg0) {
                Ok(info) => write_record_response(vm_id, seq, 0, &info),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_DEVICE_DIAGNOSTICS => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            match crate::r::io::vgpu_cabi::broker_device_diagnostics(principal, arg0) {
                Ok(diagnostics) => {
                    hvlogf(format_args!(
                        "vgpu-vvideo: vm={} stage=diagnostics status=ok copied_upload_bytes={} flushed_vvideo_bytes={} buffers={} mapping_identity={} mapping_digest=0x{:016X}",
                        vm_id,
                        diagnostics.copied_upload_bytes,
                        diagnostics.flushed_vvideo_bytes,
                        diagnostics.vvideo_buffers,
                        u8::from(
                            diagnostics.flags & v::vgpu::DeviceDiagnostics::FLAG_MAPPING_IDENTITY
                                != 0
                        ),
                        diagnostics.mapping_digest,
                    ));
                    write_record_response(vm_id, seq, 0, &diagnostics)
                }
                Err(rc) => {
                    hvwarnf(format_args!(
                        "vgpu-vvideo: vm={} stage=diagnostics status=error rc={}",
                        vm_id, rc
                    ));
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0)
                }
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_BUFFER_CREATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let usage = request_payload(vm_id, req_len)
                .and_then(|payload| payload.get(..4))
                .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            let data = match usage {
                Some(usage) => crate::r::io::vgpu_cabi::broker_buffer_create(
                    principal,
                    arg0,
                    arg1 as usize,
                    usage,
                )
                .unwrap_or_else(|rc| (rc as i64) as u64),
                None => (-22i64) as u64,
            };
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_BUFFER_DESTROY => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc = crate::r::io::vgpu_cabi::broker_buffer_destroy(principal, arg0, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_BUFFER_WRITE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let request = request_payload(vm_id, req_len);
            let result = request
                .filter(|payload| payload.len() >= 8)
                .map(|payload| {
                    let offset = u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
                        payload[6], payload[7],
                    ]) as usize;
                    crate::r::io::vgpu_cabi::broker_buffer_write(
                        principal,
                        arg0,
                        arg1,
                        offset,
                        &payload[8..],
                    )
                })
                .unwrap_or(Err(-22));
            let data = result
                .map(|count| count as u64)
                .unwrap_or_else(|rc| (rc as i64) as u64);
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_BUFFER_READ => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let request = request_payload(vm_id, req_len);
            let parsed = request
                .filter(|payload| payload.len() >= 16)
                .map(|payload| {
                    let offset = u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
                        payload[6], payload[7],
                    ]) as usize;
                    let count = u64::from_le_bytes([
                        payload[8],
                        payload[9],
                        payload[10],
                        payload[11],
                        payload[12],
                        payload[13],
                        payload[14],
                        payload[15],
                    ]) as usize;
                    (offset, count.min(PAYLOAD_CAP))
                });
            let Some((offset, count)) = parsed else {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*page).payload)[..count] };
            match crate::r::io::vgpu_cabi::broker_buffer_read(principal, arg0, arg1, offset, out) {
                Ok(got) => write_response(vm_id, seq, STATUS_OK, got as u64, got as u32),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_BUFFER_INFO => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            match crate::r::io::vgpu_cabi::broker_buffer_info(principal, arg0, arg1) {
                Ok(info) => write_record_response(vm_id, seq, 0, &info),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_UI4_SURFACE_ACQUIRE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            match crate::r::io::vgpu_cabi::broker_ui4_surface_acquire(principal, arg0, arg1 as u32)
            {
                Ok(info) => write_record_response(vm_id, seq, 0, &info),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_UI4_SURFACE_DISCARD => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc = crate::r::io::vgpu_cabi::broker_ui4_surface_discard(principal, arg0, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_UI4_SURFACE_CLEAR_SUBMIT => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let parsed = request_payload(vm_id, req_len)
                .filter(|payload| payload.len() >= 12)
                .map(|payload| {
                    let surface = u64::from_le_bytes(payload[..8].try_into().unwrap());
                    let rgba = u32::from_le_bytes(payload[8..12].try_into().unwrap());
                    (surface, rgba)
                });
            let result = parsed.ok_or(-22).and_then(|(surface, rgba)| {
                crate::r::io::vgpu_cabi::broker_ui4_surface_clear_submit(
                    principal, arg0, arg1, surface, rgba,
                )
            });
            match result {
                Ok(point) => write_record_response(vm_id, seq, 0, &point),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_CLOUD_WORK_GRAPH_CREATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let descriptor = request_payload(vm_id, req_len)
                .filter(|p| p.len() == core::mem::size_of::<v::vgpu::CloudWorkGraphDescriptor>())
                .map(|p| unsafe {
                    core::ptr::read_unaligned(
                        p.as_ptr().cast::<v::vgpu::CloudWorkGraphDescriptor>(),
                    )
                })
                .filter(|d| {
                    d.profile == v::vgpu::CLOUD_PROFILE_HELIO_ENGINE_V1
                        && d.flags == 0
                        && d.reserved == [0; 2]
                });
            let result = descriptor.ok_or(-22).and_then(|d| {
                crate::r::io::vgpu_cabi::broker_cloud_work_graph_create(principal, arg0, d)
            });
            write_response(
                vm_id,
                seq,
                STATUS_OK,
                result.unwrap_or_else(|rc| (rc as i64) as u64),
                0,
            );
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_CLOUD_WORK_GRAPH_DESTROY => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc =
                crate::r::io::vgpu_cabi::broker_cloud_work_graph_destroy(principal, arg0, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_CLOUD_FRAME_SUBMIT => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let submit = request_payload(vm_id, req_len)
                .filter(|p| p.len() == core::mem::size_of::<v::vgpu::CloudFrameSubmit>())
                .map(|p| unsafe {
                    core::ptr::read_unaligned(p.as_ptr().cast::<v::vgpu::CloudFrameSubmit>())
                })
                .filter(|s| {
                    s.flags == 0
                        && s.reserved == [0; 2]
                        && s.simulation_steps <= v::vgpu::CLOUD_FRAME_MAX_SIMULATION_STEPS
                });
            let result = submit.ok_or(-22).and_then(|s| {
                crate::r::io::vgpu_cabi::broker_cloud_frame_submit(
                    principal,
                    arg0,
                    arg1,
                    s.graph,
                    s.surface,
                    s.simulation_steps,
                )
            });
            match result {
                Ok(t) => write_record_response(vm_id, seq, 0, &t),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_SHADER_MODULE_CREATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let data = crate::r::io::vgpu_cabi::broker_shader_module_create(principal, arg0, arg1)
                .unwrap_or_else(|rc| (rc as i64) as u64);
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_SHADER_MODULE_DESTROY => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc = crate::r::io::vgpu_cabi::broker_shader_module_destroy(principal, arg0, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_RENDER_PIPELINE_CREATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let parsed = request_payload(vm_id, req_len)
                .filter(|payload| payload.len() == 8)
                .map(|payload| {
                    (
                        u32::from_le_bytes(payload[..4].try_into().unwrap()),
                        u32::from_le_bytes(payload[4..].try_into().unwrap()),
                    )
                });
            let data = parsed
                .ok_or(-22)
                .and_then(|(stride, position)| {
                    crate::r::io::vgpu_cabi::broker_render_pipeline_create(
                        principal, arg0, arg1, stride, position,
                    )
                })
                .unwrap_or_else(|rc| (rc as i64) as u64);
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_RENDER_PIPELINE_DESTROY => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc = crate::r::io::vgpu_cabi::broker_render_pipeline_destroy(principal, arg0, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_UI4_INDEXED_SUBMIT => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let draw = request_payload(vm_id, req_len)
                .filter(|payload| payload.len() == core::mem::size_of::<v::vgpu::IndexedDraw>())
                .map(|payload| unsafe {
                    core::ptr::read_unaligned(payload.as_ptr().cast::<v::vgpu::IndexedDraw>())
                });
            let result = draw.ok_or(-22).and_then(|draw| {
                crate::r::io::vgpu_cabi::broker_ui4_indexed_submit(principal, arg0, arg1, draw)
            });
            match result {
                Ok(point) => write_record_response(vm_id, seq, 0, &point),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_UI4_INDEXED_BATCH_SUBMIT => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let batch = request_payload(vm_id, req_len)
                .filter(|payload| {
                    payload.len() == core::mem::size_of::<v::vgpu::IndexedDrawBatch>()
                })
                .map(|payload| unsafe {
                    core::ptr::read_unaligned(payload.as_ptr().cast::<v::vgpu::IndexedDrawBatch>())
                });
            let result = batch.ok_or(-22).and_then(|batch| {
                crate::r::io::vgpu_cabi::broker_ui4_indexed_batch_submit(
                    principal, arg0, arg1, batch,
                )
            });
            match result {
                Ok(point) => write_record_response(vm_id, seq, 0, &point),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_QUEUE_CREATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let data =
                match crate::r::io::vgpu_cabi::broker_queue_create(principal, arg0, arg1 as u32) {
                    Ok(queue) => {
                        hvlogf(format_args!(
                            "vgpu-vvideo: vm={} stage=queue-create status=ok class={}",
                            vm_id, arg1
                        ));
                        queue
                    }
                    Err(rc) => {
                        hvwarnf(format_args!(
                            "vgpu-vvideo: vm={} stage=queue-create status=error rc={} class={}",
                            vm_id, rc, arg1
                        ));
                        (rc as i64) as u64
                    }
                };
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_QUEUE_DESTROY => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let rc = crate::r::io::vgpu_cabi::broker_queue_destroy(principal, arg0, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_SUBMIT_CONTROL_NOP => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            match crate::r::io::vgpu_cabi::broker_submit_control_nop(principal, arg0, arg1) {
                Ok(point) => write_record_response(vm_id, seq, 0, &point),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_TIMELINE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            match crate::r::io::vgpu_cabi::broker_timeline(principal, arg0, arg1) {
                Ok(status) => write_record_response(vm_id, seq, 0, &status),
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_WAIT => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let value = request_payload(vm_id, req_len)
                .and_then(|payload| payload.get(..8))
                .map(|bytes| {
                    u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ])
                });
            let rc = value
                .map(|value| crate::r::io::vgpu_cabi::broker_wait(principal, arg0, arg1, value))
                .unwrap_or(-22);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_VVIDEO_CREATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let parsed = request_payload(vm_id, req_len)
                .filter(|payload| payload.len() >= 12)
                .and_then(|payload| {
                    let bytes = u64::from_le_bytes(payload[..8].try_into().ok()?);
                    let bytes = usize::try_from(bytes).ok()?;
                    let usage = u32::from_le_bytes(payload[8..12].try_into().ok()?);
                    Some((bytes, usage))
                });
            let data = match parsed {
                Some((bytes, usage)) => match crate::r::io::vgpu_cabi::broker_vvideo_create(
                    principal, arg0, arg1, bytes, usage,
                ) {
                    Ok(buffer) => {
                        hvlogf(format_args!(
                            "vgpu-vvideo: vm={} stage=map status=ok bytes={} pages={} usage=0x{:X}",
                            vm_id,
                            bytes,
                            bytes.div_ceil(4096),
                            usage
                        ));
                        buffer
                    }
                    Err(rc) => {
                        hvwarnf(format_args!(
                            "vgpu-vvideo: vm={} stage=map status=error rc={} bytes={} usage=0x{:X}",
                            vm_id, rc, bytes, usage
                        ));
                        (rc as i64) as u64
                    }
                },
                None => {
                    hvwarnf(format_args!(
                        "vgpu-vvideo: vm={} stage=map status=error rc=-22 reason=bad-request",
                        vm_id
                    ));
                    (-22i64) as u64
                }
            };
            write_response(vm_id, seq, STATUS_OK, data, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VGPU_VVIDEO_FLUSH | OP_BP_VGPU_VVIDEO_INVALIDATE => {
            let principal = crate::gpu::vgpu::Principal::HullGuest(vm_id as u16);
            let parsed = request_payload(vm_id, req_len)
                .filter(|payload| payload.len() >= 16)
                .and_then(|payload| {
                    let offset =
                        usize::try_from(u64::from_le_bytes(payload[..8].try_into().ok()?)).ok()?;
                    let bytes =
                        usize::try_from(u64::from_le_bytes(payload[8..16].try_into().ok()?))
                            .ok()?;
                    Some((offset, bytes))
                });
            let rc = parsed
                .map(|(offset, bytes)| {
                    if op == OP_BP_VGPU_VVIDEO_FLUSH {
                        crate::r::io::vgpu_cabi::broker_vvideo_flush(
                            principal, arg0, arg1, offset, bytes,
                        )
                    } else {
                        crate::r::io::vgpu_cabi::broker_vvideo_invalidate(
                            principal, arg0, arg1, offset, bytes,
                        )
                    }
                })
                .unwrap_or(-22);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_FONT_SIZES => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let entry_bytes =
                core::mem::size_of::<crate::ui4::blueprint_text::TrueosUi4SolaraFontSize>();
            let cap = (arg0 as usize).min(PAYLOAD_CAP / entry_bytes);
            let result = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_solara_font_sizes(
                    (*page).payload.as_mut_ptr().cast(),
                    cap,
                )
            };
            let response_len = if result > 0 {
                (result as usize).min(cap).saturating_mul(entry_bytes)
            } else {
                0
            };
            write_response(vm_id, seq, STATUS_OK, (result as i64) as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_FRAME_OPEN => {
            let (x, y) = unpack_i32_pair(arg0);
            let (width, height) = unpack_u32_pair(arg1);
            let window =
                crate::ui4::blueprint_text::trueos_cabi_ui4_solara_frame_open(x, y, width, height);
            write_response(vm_id, seq, STATUS_OK, window as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_FRAME_BEGIN => {
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_solara_frame_begin(
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_TEXT_ROWS => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(header) = payload.get(..16) else {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let dst_x = i32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let dst_y = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            let rgba = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let row_count =
                u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
            if row_count == 0 || row_count > 64 {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let mut offset = 16usize;
            let mut rows = alloc::vec::Vec::with_capacity(row_count);
            for _ in 0..row_count {
                let Some(row_header) = payload.get(offset..offset.saturating_add(12)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                let x = f32::from_bits(u32::from_le_bytes([
                    row_header[0],
                    row_header[1],
                    row_header[2],
                    row_header[3],
                ]));
                let y = f32::from_bits(u32::from_le_bytes([
                    row_header[4],
                    row_header[5],
                    row_header[6],
                    row_header[7],
                ]));
                let text_len = u32::from_le_bytes([
                    row_header[8],
                    row_header[9],
                    row_header[10],
                    row_header[11],
                ]) as usize;
                offset = offset.saturating_add(12);
                let Some(text) = payload.get(offset..offset.saturating_add(text_len)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                rows.push(crate::ui4::blueprint_text::TrueosUi4SolaraTextRow {
                    text_ptr: text.as_ptr(),
                    text_len,
                    x,
                    y,
                });
                offset = offset.saturating_add(text_len);
            }
            if offset != payload.len() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let (font_id, native_scale) = unpack_u32_pair(arg1);
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_solara_text_rows(
                    arg0 as u32,
                    font_id,
                    native_scale,
                    dst_x,
                    dst_y,
                    rgba,
                    rows.as_ptr(),
                    rows.len(),
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_TEXT_SCENE => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(header) = payload.get(..16) else {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let viewport_width = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let viewport_height = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            let rgba = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let row_count =
                u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
            if row_count == 0 || row_count > 64 {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let mut offset = 16usize;
            let mut rows = alloc::vec::Vec::with_capacity(row_count);
            for _ in 0..row_count {
                let Some(row_header) = payload.get(offset..offset.saturating_add(16)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                let x = f32::from_bits(u32::from_le_bytes([
                    row_header[0],
                    row_header[1],
                    row_header[2],
                    row_header[3],
                ]));
                let y = f32::from_bits(u32::from_le_bytes([
                    row_header[4],
                    row_header[5],
                    row_header[6],
                    row_header[7],
                ]));
                let font_pixels = f32::from_bits(u32::from_le_bytes([
                    row_header[8],
                    row_header[9],
                    row_header[10],
                    row_header[11],
                ]));
                let text_len = u32::from_le_bytes([
                    row_header[12],
                    row_header[13],
                    row_header[14],
                    row_header[15],
                ]) as usize;
                offset = offset.saturating_add(16);
                let Some(text) = payload.get(offset..offset.saturating_add(text_len)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                rows.push(crate::ui4::blueprint_text::TrueosUi4SolaraSceneTextRow {
                    text_ptr: text.as_ptr(),
                    text_len,
                    x,
                    y,
                    font_pixels,
                });
                offset = offset.saturating_add(text_len);
            }
            if offset != payload.len() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_solara_text_scene(
                    arg0 as u32,
                    arg1 as u32,
                    viewport_width,
                    viewport_height,
                    rgba,
                    rows.as_ptr(),
                    rows.len(),
                )
            };
            if rc == crate::ui4::blueprint_text::ERROR_BUSY {
                if sampled_text_scene_busy_log() {
                    crate::log_info!(target: "ui4/solara-text"; "scene vmcall pending vm={} window={} rows={} rc={} sample_every={}\n", vm_id, arg0 as u32, row_count, rc, BLUEPRINT_TEXT_SCENE_BUSY_LOG_SAMPLE_EVERY);
                }
            } else if rc != 0 {
                crate::log_warn!(target: "ui4/solara-text"; "scene vmcall failed vm={} window={} rows={} rc={}\n", vm_id, arg0 as u32, row_count, rc);
            }
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_FONT_CANVAS => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(header) = payload.get(..12) else {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let canvas_width = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let canvas_height = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            let row_count =
                u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
            if row_count == 0 || row_count > 256 {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let mut offset = 12usize;
            let mut rows = alloc::vec::Vec::with_capacity(row_count);
            for _ in 0..row_count {
                let Some(row_header) = payload.get(offset..offset.saturating_add(20)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                let x = f32::from_bits(u32::from_le_bytes([
                    row_header[0],
                    row_header[1],
                    row_header[2],
                    row_header[3],
                ]));
                let y = f32::from_bits(u32::from_le_bytes([
                    row_header[4],
                    row_header[5],
                    row_header[6],
                    row_header[7],
                ]));
                let font_pixels = f32::from_bits(u32::from_le_bytes([
                    row_header[8],
                    row_header[9],
                    row_header[10],
                    row_header[11],
                ]));
                let color_rgba = u32::from_le_bytes([
                    row_header[12],
                    row_header[13],
                    row_header[14],
                    row_header[15],
                ]);
                let text_len = u32::from_le_bytes([
                    row_header[16],
                    row_header[17],
                    row_header[18],
                    row_header[19],
                ]) as usize;
                offset = offset.saturating_add(20);
                let Some(text) = payload.get(offset..offset.saturating_add(text_len)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                rows.push(crate::ui4::blueprint_text::TrueosUi4FontCanvasRow {
                    text_ptr: text.as_ptr(),
                    text_len,
                    x,
                    y,
                    font_pixels,
                    color_rgba,
                });
                offset = offset.saturating_add(text_len);
            }
            if offset != payload.len() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_font_canvas(
                    arg0 as u32,
                    arg1 as u32,
                    canvas_width,
                    canvas_height,
                    rows.as_ptr(),
                    rows.len(),
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_CONTEXT_MENU_REGISTER => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(header) = payload.get(..4) else {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let entry_count =
                u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
            if entry_count > 16 {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let mut offset = 4usize;
            let mut entries = alloc::vec::Vec::with_capacity(entry_count);
            for _ in 0..entry_count {
                let Some(entry_header) = payload.get(offset..offset.saturating_add(12)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                let action_id = u32::from_le_bytes([
                    entry_header[0],
                    entry_header[1],
                    entry_header[2],
                    entry_header[3],
                ]);
                let enabled = u32::from_le_bytes([
                    entry_header[4],
                    entry_header[5],
                    entry_header[6],
                    entry_header[7],
                ]);
                let label_len = u32::from_le_bytes([
                    entry_header[8],
                    entry_header[9],
                    entry_header[10],
                    entry_header[11],
                ]) as usize;
                offset = offset.saturating_add(12);
                let Some(label) = payload.get(offset..offset.saturating_add(label_len)) else {
                    write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                    return DispatchOutcome::Resume;
                };
                entries.push(crate::ui4::blueprint_text::TrueosUi4ContextMenuEntry {
                    label_ptr: label.as_ptr(),
                    label_len,
                    action_id,
                    enabled,
                });
                offset = offset.saturating_add(label_len);
            }
            if offset != payload.len() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_context_menu_register(
                    arg0 as u32,
                    entries.as_ptr(),
                    entries.len(),
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE => {
            let mut event = crate::ui4::blueprint_text::TrueosUi4ContextMenuEvent::default();
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_context_menu_event_take(
                    arg0 as u32,
                    &mut event,
                )
            };
            if rc == 0 {
                write_record_response(vm_id, seq, 0, &event);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_FRAME_PUBLISH => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(extent) = payload.get(..8) else {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let width = u32::from_le_bytes([extent[0], extent[1], extent[2], extent[3]]);
            let height = u32::from_le_bytes([extent[4], extent[5], extent[6], extent[7]]);
            let (x, y) = unpack_u32_pair(arg1);
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_solara_frame_publish(
                arg0 as u32,
                x,
                y,
                width,
                height,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_COMPUTE_FRAME_PUBLISH => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(extent) = payload.get(..8) else {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let width = u32::from_le_bytes([extent[0], extent[1], extent[2], extent[3]]);
            let height = u32::from_le_bytes([extent[4], extent[5], extent[6], extent[7]]);
            let (x, y) = unpack_u32_pair(arg1);
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_compute_frame_publish(
                arg0 as u32,
                x,
                y,
                width,
                height,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SOLARA_FRAME_CLOSE => {
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_solara_frame_close_requested(
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SKYBOX_UPLOAD_BEGIN => {
            let (width, height) = unpack_u32_pair(arg1);
            let rc = crate::ui4::blueprint_text::begin_skybox_rgb565_upload(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                width,
                height,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SKYBOX_UPLOAD_CHUNK => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let rc = crate::ui4::blueprint_text::write_skybox_rgb565_upload_chunk(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as usize,
                payload,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SKYBOX_UPLOAD_FINISH => {
            let rc = crate::ui4::blueprint_text::finish_skybox_rgb565_upload(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SKYBOX_RENDER => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut words = [0u32; 15];
            if payload.len() != words.len() * core::mem::size_of::<u32>() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            for (word, bytes) in words.iter_mut().zip(payload.chunks_exact(4)) {
                *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }
            let params = crate::ui4::blueprint_text::TrueosUi4SkyboxRenderParams {
                right_x: f32::from_bits(words[0]),
                right_y: f32::from_bits(words[1]),
                right_z: f32::from_bits(words[2]),
                up_x: f32::from_bits(words[3]),
                up_y: f32::from_bits(words[4]),
                up_z: f32::from_bits(words[5]),
                forward_x: f32::from_bits(words[6]),
                forward_y: f32::from_bits(words[7]),
                forward_z: f32::from_bits(words[8]),
                aspect_tan_half_fov_y: f32::from_bits(words[9]),
                tan_half_fov_y: f32::from_bits(words[10]),
                rect_x: words[11],
                rect_y: words[12],
                rect_width: words[13],
                rect_height: words[14],
            };
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_skybox_render_rgb565(
                    arg0 as u32,
                    &params,
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_PARTICLE_CRAFT_RENDER => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut words = [0u32; 16];
            if payload.len() != words.len() * core::mem::size_of::<u32>() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            for (word, bytes) in words.iter_mut().zip(payload.chunks_exact(4)) {
                *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }
            let params = crate::ui4::blueprint_text::TrueosUi4ParticleCraftParamsV1 {
                version: words[0],
                flags: words[1],
                seed: words[2],
                active_count: words[3],
                dt_seconds: f32::from_bits(words[4]),
                time_seconds: f32::from_bits(words[5]),
                emitter_x: f32::from_bits(words[6]),
                emitter_y: f32::from_bits(words[7]),
                attractor_x: f32::from_bits(words[8]),
                attractor_y: f32::from_bits(words[9]),
                attraction: f32::from_bits(words[10]),
                swirl: f32::from_bits(words[11]),
                gravity_x: f32::from_bits(words[12]),
                gravity_y: f32::from_bits(words[13]),
                drag: f32::from_bits(words[14]),
                intensity: f32::from_bits(words[15]),
            };
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_particle_craft_render(
                    arg0 as u32,
                    &params,
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SHADERTOY_RENDER => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut words = [0u32; 16];
            if payload.len() != words.len() * core::mem::size_of::<u32>() {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            for (word, bytes) in words.iter_mut().zip(payload.chunks_exact(4)) {
                *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }
            let params = crate::ui4::blueprint_text::TrueosUi4ShadertoyParamsV1 {
                version: words[0],
                shader_id: words[1],
                frame: words[2],
                flags: words[3],
                time_seconds: f32::from_bits(words[4]),
                delta_seconds: f32::from_bits(words[5]),
                frame_rate: f32::from_bits(words[6]),
                sample_rate: f32::from_bits(words[7]),
                mouse_x: f32::from_bits(words[8]),
                mouse_y: f32::from_bits(words[9]),
                click_x: f32::from_bits(words[10]),
                click_y: f32::from_bits(words[11]),
                date_year: f32::from_bits(words[12]),
                date_month: f32::from_bits(words[13]),
                date_day: f32::from_bits(words[14]),
                date_seconds: f32::from_bits(words[15]),
            };
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_shadertoy_render(
                    arg0 as u32,
                    &params,
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_WRITE_OPAQUE_RGBA8 => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let rc = crate::ui4::blueprint_text::write_opaque_rgba8_chunk(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as usize,
                payload,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FRAME_SET_POSITION => {
            let (x, y) = unpack_i32_pair(arg1);
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_frame_set_position(
                arg0 as u32,
                x,
                y,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FRAME_SET_HIT_TESTABLE => {
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_frame_set_hit_testable(
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FRAME_RESIZE => {
            let (width, height) = unpack_u32_pair(arg1);
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_frame_resize(
                arg0 as u32,
                width,
                height,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FRAME_OPEN_STREAMING => {
            let (x, y) = unpack_i32_pair(arg0);
            let (width, height) = unpack_u32_pair(arg1);
            let window = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_frame_open_streaming(
                x, y, width, height,
            );
            write_response(vm_id, seq, STATUS_OK, window as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FRAME_OPEN_VISUAL => {
            let (x, y) = unpack_i32_pair(arg0);
            let (width, height) = unpack_u32_pair(arg1);
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            if payload.len() != core::mem::size_of::<u32>() {
                write_response(vm_id, seq, STATUS_OK, 0, 0);
                return DispatchOutcome::Resume;
            }
            let target_hz = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let window = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_frame_open_visual(
                x, y, width, height, target_hz,
            );
            write_response(vm_id, seq, STATUS_OK, window as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FRAME_OPEN_IMMUTABLE => {
            let (x, y) = unpack_i32_pair(arg0);
            let (width, height) = unpack_u32_pair(arg1);
            let window = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_frame_open_immutable(
                x, y, width, height,
            );
            write_response(vm_id, seq, STATUS_OK, window as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_UPLOAD_BEGIN => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            if payload.len() != 8 {
                write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let width = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let height = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let rc = crate::ui4::blueprint_text::begin_sprite_rgba8_upload(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as u32,
                width,
                height,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_UPLOAD_CHUNK => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let sprite_id = (arg1 >> 32) as u32;
            let offset = arg1 as u32 as usize;
            let rc = crate::ui4::blueprint_text::write_sprite_rgba8_upload_chunk(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                sprite_id,
                offset,
                payload,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_UPLOAD_FINISH => {
            let rc = crate::ui4::blueprint_text::finish_sprite_rgba8_upload(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_FRAME_BEGIN => {
            let rc = crate::ui4::blueprint_text::begin_blueprint_frame(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as u32,
                false,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_VISUAL_FRAME_BEGIN => {
            let owner = crate::ui4::WindowOwner::Vm(vm_id);
            let rc =
                crate::ui4::blueprint_text::begin_blueprint_frame(owner, arg0 as u32, 0, false);
            if rc == crate::ui4::blueprint_text::ERROR_BUSY
                && let Some(wait_ms) =
                    crate::ui4::blueprint_text::visual_frame_retry_ms(owner, arg0 as u32)
            {
                return DispatchOutcome::RetryAfterMs(wait_ms);
            }
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_DRAW_BEGIN => {
            let rc = crate::ui4::blueprint_text::begin_sprite_scene(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as usize,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_DRAW_CHUNK => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let rc = crate::ui4::blueprint_text::append_sprite_scene_bytes(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
                arg1 as usize,
                payload,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SPRITE_DRAW_FINISH => {
            let rc = crate::ui4::blueprint_text::finish_sprite_scene(
                crate::ui4::WindowOwner::Vm(vm_id),
                arg0 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_PAN_EVENT_TAKE => {
            let mut event = crate::ui4::blueprint_text::TrueosUi4PanEvent::default();
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_pan_event_take(
                    arg0 as u32,
                    &mut event,
                )
            };
            if rc == 0 {
                write_record_response(vm_id, seq, 0, &event);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_RESIZE_EVENT_TAKE => {
            let mut event = crate::ui4::blueprint_text::TrueosUi4ResizeEvent::default();
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_resize_event_take(
                    arg0 as u32,
                    &mut event,
                )
            };
            if rc == 0 {
                write_record_response(vm_id, seq, 0, &event);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_FIRST_PRESENTATION_TAKE => {
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_first_presentation_take(
                arg0 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_OUTPUT_DIMENSIONS => {
            let dimensions = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_output_dimensions();
            write_response(vm_id, seq, STATUS_OK, dimensions, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SET_CUSTOM_CURSOR => {
            let rc = crate::ui4::blueprint_text::trueos_cabi_ui4_scene_set_custom_cursor(
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_SET_CURSOR_ICON => {
            let source = match req_len as usize {
                0 => None,
                len if len
                    == core::mem::size_of::<crate::ui4::blueprint_text::TrueosUi4CursorSource>(
                    ) =>
                {
                    let Some(payload) = request_payload(vm_id, req_len) else {
                        write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                        return DispatchOutcome::Resume;
                    };
                    Some(unsafe {
                        core::ptr::read_unaligned(
                            payload
                                .as_ptr()
                                .cast::<crate::ui4::blueprint_text::TrueosUi4CursorSource>(),
                        )
                    })
                }
                _ => {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    return DispatchOutcome::Resume;
                }
            };
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_set_cursor_icon(
                    arg0 as u32,
                    source
                        .as_ref()
                        .map_or(core::ptr::null(), |source| source as *const _),
                    arg1 as u32,
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_POINTER_EVENT_TAKE => {
            let mut event = crate::ui4::blueprint_text::TrueosUi4PointerEvent::default();
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_pointer_event_take(
                    arg0 as u32,
                    &mut event,
                )
            };
            if rc == 0 {
                write_record_response(vm_id, seq, 0, &event);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE => {
            let mut event = crate::r::keyboard::TrueosKeyboardOutputEvent::default();
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_keyboard_event_take(
                    arg0 as u32,
                    &mut event,
                )
            };
            if rc == 0 {
                write_record_response(vm_id, seq, 0, &event);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_KEYBOARD_STATE => {
            let mut state = crate::ui4::blueprint_text::TrueosUi4KeyboardState::default();
            let rc = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_keyboard_state(
                    arg0 as u32,
                    &mut state,
                )
            };
            if rc == 0 {
                write_record_response(vm_id, seq, 0, &state);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_UI4_SCENE_INPUT_ROUTES => {
            const MAX_ROUTES: usize = 32;
            let cap = (arg1 as usize).min(MAX_ROUTES);
            let mut routes =
                [crate::ui4::blueprint_text::TrueosUi4InputRouteState::default(); MAX_ROUTES];
            let count = unsafe {
                crate::ui4::blueprint_text::trueos_cabi_ui4_scene_input_routes(
                    arg0 as u32,
                    routes.as_mut_ptr(),
                    cap as u32,
                )
            };
            if count < 0 {
                write_response(vm_id, seq, STATUS_OK, (count as i64) as u64, 0);
            } else {
                let copied = (count as usize).min(cap);
                write_record_slice_response(vm_id, seq, count as u64, &routes[..copied]);
            }
            DispatchOutcome::Resume
        }
        OP_BP_IMAGE_SOURCE_INFO => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Ok(name) = core::str::from_utf8(payload) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            match crate::ui4::blueprint_text::blueprint_image_source_info(name) {
                Ok(info) => write_record_response(vm_id, seq, 0, &info),
                Err(error) => write_response(vm_id, seq, STATUS_OK, (error as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_IMAGE_SOURCE_READ => {
            const MAX_READ_BYTES: usize = 16 * 1024;
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Ok(name) = core::str::from_utf8(payload) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let capacity = (arg1 as usize).min(MAX_READ_BYTES);
            if capacity == 0 {
                write_response(vm_id, seq, STATUS_OK, (-3i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let mut bytes = alloc::vec![0u8; capacity];
            match crate::ui4::blueprint_text::copy_blueprint_image_source(
                name,
                arg0 as usize,
                &mut bytes,
            ) {
                Ok(copied) => {
                    write_record_slice_response(vm_id, seq, copied as u64, &bytes[..copied])
                }
                Err(error) => write_response(vm_id, seq, STATUS_OK, (error as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_BEGIN => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::media_service::begin(owner, arg0 as u32, arg1 as usize);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_WRITE => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::media_service::write(owner, arg0 as u32, arg1 as usize, payload);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_COMMIT => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::media_service::commit(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_STATUS => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::media_service::status(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_INFO => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            match crate::r::media_service::info(owner, arg0 as u32) {
                Ok(info) => write_record_response(vm_id, seq, 0, &info),
                Err(error) => write_response(vm_id, seq, STATUS_OK, (error as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_READ => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let offset = (arg1 >> 32) as usize;
            let capacity = (arg1 as u32 as usize).min(PAYLOAD_CAP);
            if capacity == 0 {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::media_service::ERR_INVALID as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..capacity] };
            match crate::r::media_service::read(owner, arg0 as u32, offset, out) {
                Ok(copied) => write_response(vm_id, seq, STATUS_OK, copied as u64, copied as u32),
                Err(error) => write_response(vm_id, seq, STATUS_OK, (error as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_VMEDIA_IMAGE_DECODE_DISCARD => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::media_service::discard(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_CHILD_SPAWN_V1 => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let result = crate::hv::blueprint_child_spawn(vm_id, payload);
            write_response(
                vm_id,
                seq,
                STATUS_OK,
                result.unwrap_or_else(|error| (error as i64) as u64),
                0,
            );
            DispatchOutcome::Resume
        }
        OP_BP_CHILD_SEND_V1 => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let result = crate::hv::blueprint_child_send(vm_id, arg0, payload)
                .map(|written| written as i64)
                .unwrap_or_else(i64::from);
            write_response(vm_id, seq, STATUS_OK, result as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_CHILD_RECEIVE_V1 => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (*page).payload };
            match crate::hv::blueprint_child_receive(vm_id, arg0, out) {
                Ok(length) => write_response(vm_id, seq, STATUS_OK, length as u64, length as u32),
                Err(error) => write_response(vm_id, seq, STATUS_OK, (error as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_CHILD_STATUS_V1 => {
            let result = crate::hv::blueprint_child_status(vm_id, arg0)
                .map(i64::from)
                .unwrap_or_else(i64::from);
            write_response(vm_id, seq, STATUS_OK, result as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_CHILD_TERMINATE_V1 => {
            let result = crate::hv::blueprint_child_terminate(vm_id, arg0)
                .map(|()| 0i64)
                .unwrap_or_else(i64::from);
            write_response(vm_id, seq, STATUS_OK, result as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_GRIDPAPER_SNAPSHOT_SUBMIT => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let rc = if arg1 & crate::r::gridpaper_service::SIZED_SNAPSHOT_VMCALL_MARKER != 0 {
                let instance_id = ((arg1 >> 32) & 0x7fff_ffff) as u32;
                let rows = ((arg1 >> 24) & 0xff) as u32;
                let columns = ((arg1 >> 16) & 0xff) as u32;
                let scale_percent = (arg1 & 0xffff) as u32;
                crate::r::gridpaper_service::submit_sized_snapshot_for_owner(
                    vm_id,
                    instance_id,
                    arg0,
                    scale_percent,
                    columns,
                    rows,
                    payload,
                )
            } else {
                let instance_id = (arg1 >> 32) as u32;
                let scale_percent = arg1 as u32;
                crate::r::gridpaper_service::submit_snapshot_for_owner(
                    vm_id,
                    instance_id,
                    arg0,
                    scale_percent,
                    payload,
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_GRIDPAPER_SNAPSHOT_CHECKPOINT => {
            let Some(page) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out =
                unsafe { &mut (&mut (*page).payload)[..crate::r::gridpaper_service::PAGE_BYTES] };
            let rc =
                crate::r::gridpaper_service::checkpoint_snapshot_for_owner(vm_id, arg0 as u32, out);
            let len = if rc == 0 {
                crate::r::gridpaper_service::PAGE_BYTES as u32
            } else {
                0
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, len);
            DispatchOutcome::Resume
        }
        OP_BP_GRIDPAPER_TEXT_ANIMATIONS_SUBMIT => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::gridpaper_service::submit_text_animations_for_owner(
                vm_id,
                arg0 as u32,
                payload,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_GRIDPAPER_CLOSE => {
            let rc = crate::r::gridpaper_service::close_owner(vm_id, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_GRIDPAPER_PRINT_REQUEST_TAKE => {
            let packed =
                crate::r::gridpaper_service::take_print_request_for_owner(vm_id, arg0 as u32)
                    .map(|(token, _generation)| u64::from(token))
                    .unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, packed, 0);
            DispatchOutcome::Resume
        }
        OP_BP_PRINT2D_SUBMIT => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let result = crate::r::print2d::submit_for_owner(vm_id, arg0 as u32, arg1, payload);
            write_response(vm_id, seq, STATUS_OK, result as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_PRINT2D_STATUS => {
            let state = crate::r::print2d::status_for_owner(vm_id, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (state as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_YIELD => {
            write_response(vm_id, seq, STATUS_OK, 0, 0);
            release_guest_comm_page(vm_id);
            DispatchOutcome::Yield
        }
        OP_SLEEP_MS => {
            let sleep_ms = arg0.min(MAX_GUEST_SLEEP_MS);
            write_response(vm_id, seq, STATUS_OK, sleep_ms, 0);
            release_guest_comm_page(vm_id);
            DispatchOutcome::SleepMs(sleep_ms)
        }
        OP_RAND_BYTES => {
            let want = core::cmp::min(arg0 as usize, PAYLOAD_CAP);
            if want == 0 {
                write_response(vm_id, seq, STATUS_OK, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            if crate::tyche::fill_bytes(out) {
                write_response(vm_id, seq, STATUS_OK, want as u64, want as u32);
            } else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_THREAD_CURRENT_ID => {
            let vtid = 0x8000u32.saturating_add(vm_id as u32);
            write_response(vm_id, seq, STATUS_OK, vtid as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SERVICE_LANE_SUBMIT => {
            let rc = unsafe {
                crate::r::blocking::submit_guest_service_lane_job_from_raw(
                    vm_id,
                    arg0 as usize,
                    arg1 as usize,
                    "vmx-service-lane",
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_PLATFORM_WAKE_ONE => {
            let woke = crate::wait::platform_wake_one_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, u64::from(woke), 0);
            DispatchOutcome::Resume
        }
        OP_BP_PLATFORM_WAKE_ALL => {
            let count = crate::wait::platform_wake_all_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MOUSE_MOTION_CURSOR_REQUEST => {
            let rc = request_payload(vm_id, req_len)
                .and_then(|payload| core::str::from_utf8(payload).ok())
                .filter(|label| !label.is_empty())
                .map(|label| {
                    let mut cursor = v::vinput::MouseMotionCursorInfo::default();
                    let rc = unsafe {
                        crate::r::io::cabi::trueos_cabi_mouse_motion_cursor_request(
                            label.as_ptr(),
                            label.len(),
                            &mut cursor,
                        )
                    };
                    (rc, cursor)
                });
            match rc {
                Some((0, cursor)) => write_record_response(vm_id, seq, 0, &cursor),
                Some((rc, _)) => {
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                }
                None => write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_MOUSE_MOTION_CURSOR_RELEASE => {
            let rc = crate::r::io::cabi::trueos_cabi_mouse_motion_cursor_release(arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MOUSE_MOTION_SUBMIT => {
            let command = request_payload(vm_id, req_len)
                .filter(|payload| {
                    payload.len() == core::mem::size_of::<v::vinput::MouseMotionCommand>()
                })
                .map(|payload| unsafe {
                    core::ptr::read_unaligned(
                        payload.as_ptr().cast::<v::vinput::MouseMotionCommand>(),
                    )
                });
            let rc = command
                .map(|command| unsafe {
                    crate::r::io::cabi::trueos_cabi_mouse_motion_submit(arg0, &command)
                })
                .unwrap_or(-1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MOUSE_MOTION_SUBMIT_JSON => {
            let rc = request_payload(vm_id, req_len)
                .filter(|payload| !payload.is_empty())
                .map(|payload| unsafe {
                    crate::r::io::cabi::trueos_cabi_mouse_motion_submit_json(
                        arg0,
                        payload.as_ptr(),
                        payload.len(),
                    )
                })
                .unwrap_or(-1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MOUSE_MOTION_CURSOR_IDLE => {
            let rc = crate::r::io::cabi::trueos_cabi_mouse_motion_cursor_idle(arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_KEYBOARD_CONTROL_REQUEST => {
            let rc = request_payload(vm_id, req_len)
                .and_then(|payload| core::str::from_utf8(payload).ok())
                .filter(|label| !label.is_empty())
                .map(|label| {
                    let mut keyboard = v::vinput::KeyboardControlDeviceInfo::default();
                    let rc = unsafe {
                        crate::r::io::cabi::trueos_cabi_keyboard_control_request(
                            label.as_ptr(),
                            label.len(),
                            &mut keyboard,
                        )
                    };
                    (rc, keyboard)
                });
            match rc {
                Some((0, keyboard)) => write_record_response(vm_id, seq, 0, &keyboard),
                Some((rc, _)) => {
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                }
                None => write_response(vm_id, seq, STATUS_OK, (-1i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_KEYBOARD_CONTROL_RELEASE => {
            let rc = crate::r::io::cabi::trueos_cabi_keyboard_control_release(arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_KEYBOARD_CONTROL_SUBMIT => {
            let command = request_payload(vm_id, req_len)
                .filter(|payload| {
                    payload.len() == core::mem::size_of::<v::vinput::KeyboardControlCommand>()
                })
                .map(|payload| unsafe {
                    core::ptr::read_unaligned(
                        payload.as_ptr().cast::<v::vinput::KeyboardControlCommand>(),
                    )
                });
            let rc = command
                .map(|command| unsafe {
                    crate::r::io::cabi::trueos_cabi_keyboard_control_submit(arg0, &command)
                })
                .unwrap_or(-1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_KEYBOARD_CONTROL_SUBMIT_TEXT => {
            let interval_ms = (arg1 >> 32) as u32;
            let flags = arg1 as u32;
            let rc = request_payload(vm_id, req_len)
                .and_then(|payload| core::str::from_utf8(payload).ok())
                .filter(|text| !text.is_empty())
                .map(|text| unsafe {
                    crate::r::io::cabi::trueos_cabi_keyboard_control_submit_text(
                        arg0,
                        text.as_ptr(),
                        text.len(),
                        interval_ms,
                        flags,
                    )
                })
                .unwrap_or(-1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_KEYBOARD_CONTROL_SUBMIT_JSON => {
            let rc = request_payload(vm_id, req_len)
                .filter(|payload| !payload.is_empty())
                .map(|payload| unsafe {
                    crate::r::io::cabi::trueos_cabi_keyboard_control_submit_json(
                        arg0,
                        payload.as_ptr(),
                        payload.len(),
                    )
                })
                .unwrap_or(-1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_KEYBOARD_CONTROL_IDLE => {
            let rc = crate::r::io::cabi::trueos_cabi_keyboard_control_idle(arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_CURSOR_POS => {
            let mut x = 0i32;
            let mut y = 0i32;
            let rc = crate::r::io::cabi::host_input_cursor_pos(arg0 as u32, &mut x, &mut y);
            let packed = ((x as u32 as u64) << 32) | (y as u32 as u64);
            if rc == 0 {
                write_response(vm_id, seq, STATUS_OK, packed, 0);
            } else {
                write_response(vm_id, seq, STATUS_BAD_ARG, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_CURSOR_BUTTONS => {
            let mut buttons = 0u32;
            let rc = crate::r::io::cabi::host_input_cursor_buttons(arg0 as u32, &mut buttons);
            write_response(vm_id, seq, STATUS_OK, ((rc as i64 as u64) << 32) | (buttons as u64), 0);
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_CURSOR_EVENTS => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let (wrote, response_len) =
                crate::r::io::cabi::host_input_cursor_events_since(arg0, arg1 as u32, unsafe {
                    &mut (*p).payload
                });
            write_response(vm_id, seq, STATUS_OK, wrote as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_KEYBOARD_OUTPUT_POP => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, (-1i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let (rc, response_len) =
                crate::r::io::cabi::host_input_pop_keyboard_output(unsafe { &mut (*p).payload });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_KEYBOARD_OUTPUT_SINCE => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let (wrote, response_len) =
                crate::r::io::cabi::host_input_keyboard_output_since(arg0, arg1 as u32, unsafe {
                    &mut (*p).payload
                });
            write_response(vm_id, seq, STATUS_OK, wrote as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_DNS_RESOLVE_IPV4 => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(host) = core::str::from_utf8(bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    crate::r::net::vlayer::dns_resolve_error_code(
                        crate::r::net::vlayer::DnsResolveError::BadName,
                    ),
                    0,
                );
                return DispatchOutcome::Resume;
            };
            match crate::r::net::vlayer::resolve_ipv4_for_sync_abi_host(host) {
                Ok(ip) => {
                    unsafe {
                        (&mut (*p).payload)[..4].copy_from_slice(&ip);
                    }
                    write_response(vm_id, seq, STATUS_OK, 0, 4);
                }
                Err(err) => {
                    write_response(
                        vm_id,
                        seq,
                        STATUS_OK,
                        crate::r::net::vlayer::dns_resolve_error_code(err),
                        0,
                    );
                }
            }
            DispatchOutcome::Resume
        }
        OP_BP_RAPL_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::rapl_snapshot_len_host,
                crate::r::net::vlayer::rapl_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_RAPL_HISTORY_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::rapl_history_len_host,
                crate::r::net::vlayer::rapl_history_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_PCI_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::pci_snapshot_len_host,
                crate::r::net::vlayer::pci_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_USB_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::usb_snapshot_len_host,
                crate::r::net::vlayer::usb_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_THERMAL_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::thermal_snapshot_len_host,
                crate::r::net::vlayer::thermal_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_VRAM_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::vram_snapshot_len_host,
                crate::r::net::vlayer::vram_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_SYSTEM_SERVICES_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::system_services_snapshot_len_host,
                crate::r::net::vlayer::system_services_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_PRINTER_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::printer_snapshot_len_host,
                crate::r::net::vlayer::printer_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_NET_TCP_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            match crate::hv::vnet::tcp_write(vm_id, bytes) {
                Ok(written) => write_response(vm_id, seq, STATUS_OK, written as u64, 0),
                Err(_) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        OP_NET_TCP_READ => {
            let want = core::cmp::min(arg0 as usize, PAYLOAD_CAP);
            if want == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }

            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            match crate::hv::vnet::tcp_read(vm_id, out) {
                Ok(got) => write_response(vm_id, seq, STATUS_OK, got as u64, got as u32),
                Err(_) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        OP_BP_NET_OPEN => {
            match crate::hv::blueprint_net::open_primary(vm_id) {
                Some(session_id) => write_response(vm_id, seq, STATUS_OK, session_id as u64, 0),
                None => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        OP_BP_NET_SUBMIT => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            match crate::hv::blueprint_net::submit(vm_id, arg0 as u32, bytes) {
                Ok(()) => write_response(vm_id, seq, STATUS_OK, 0, 0),
                Err(()) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        OP_BP_NET_POLL => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
            match crate::hv::blueprint_net::poll_event(vm_id, arg0 as u32, out) {
                Ok(Some(len)) => write_response(vm_id, seq, STATUS_OK, 1, len as u32),
                Ok(None) => write_response(vm_id, seq, STATUS_OK, 0, 0),
                Err(()) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_START => {
            let n = req_len as usize;
            if n == 0 || n > PAYLOAD_CAP {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(url) = core::str::from_utf8(bytes) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            const TIMEOUT_MS: u32 = 45_000;
            const MAX_BYTES: usize = 8 * 1024 * 1024;
            let op_id =
                crate::r::net::https::cabi_net_fetch_bytes_start_host(url, TIMEOUT_MS, MAX_BYTES);
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, op_id as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_POST_JSON_BYTES_START => {
            let n = req_len as usize;
            if arg0 > u64::from(u32::MAX) || n == 0 || n > PAYLOAD_CAP {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let payload = unsafe { &(&(*p).payload)[..n] };
            let Some(request) =
                crate::r::net::https::decode_post_json_bytes_vm_request(payload, arg1)
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            const MAX_BYTES: usize = 4 * 1024 * 1024;
            let op_id = crate::r::net::https::cabi_net_fetch_post_json_bytes_start_host(
                request.url,
                request.body,
                request.bearer,
                arg0 as u32,
                MAX_BYTES,
            );
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, u64::from(op_id), 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_RESULT_LEN => {
            if arg0 > u64::from(u32::MAX) {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let rc = crate::r::net::https::cabi_net_fetch_bytes_result_len_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_READ => {
            if arg0 > u64::from(u32::MAX) {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some((offset, want)) = crate::r::net::https::decode_fetch_bytes_vm_read(arg1)
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let rc = crate::r::net::https::cabi_net_fetch_bytes_read_chunk_host(
                arg0 as u32,
                offset,
                out,
            );
            if rc > want as isize {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let out_len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_DISCARD => {
            if arg0 > u64::from(u32::MAX) {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let rc = crate::r::net::https::cabi_net_fetch_bytes_discard_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let url_len = arg0 as usize;
            if url_len == 0 || url_len >= n {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(url) = core::str::from_utf8(&bytes[..url_len]) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Ok(path) = core::str::from_utf8(&bytes[url_len..]) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            const TIMEOUT_MS: u32 = 45_000;
            const MAX_BYTES: usize = 8 * 1024 * 1024;
            let op_id =
                crate::r::net::https::cabi_net_fetch_start_host(url, path, TIMEOUT_MS, MAX_BYTES);
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, op_id as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_RESULT => {
            let rc = crate::r::net::https::cabi_net_fetch_result_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_DISCARD => {
            let rc = crate::r::net::https::cabi_net_fetch_discard_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ARGS_COUNT => {
            let count = crate::hv::blueprint_process_arg_count(vm_id)
                .unwrap_or_else(crate::r::io::env::arg_count);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ARG => {
            let Some(arg) = crate::hv::blueprint_process_arg(vm_id, arg0 as usize)
                .or_else(|| crate::r::io::env::arg(arg0 as usize))
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = arg.as_bytes();
            let n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..n]).copy_from_slice(&bytes[..n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_VAR => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let key_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(key) = core::str::from_utf8(key_bytes) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(value) = crate::hv::blueprint_process_env_var(vm_id, key)
                .or_else(|| crate::r::io::env::var(key))
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = value.as_bytes();
            let out_n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[..out_n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ALL => {
            let Some(text) = crate::hv::blueprint_process_env_text(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = text.as_bytes();
            let out_n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[..out_n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let data = unsafe { &(&(*p).payload)[..n] };
            let written = crate::hv::blueprint_console_write(vm_id, data);
            write_response(vm_id, seq, STATUS_OK, written as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_RAW_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let data = unsafe { &(&(*p).payload)[..n] };
            let written = crate::hv::blueprint_console_raw_write(vm_id, data);
            write_response(vm_id, seq, STATUS_OK, written as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_KONSOLE_SIZE => {
            let (cols, rows) = crate::hv::blueprint_console_konsole_size(vm_id);
            let packed = (u64::from(cols) << 32) | u64::from(rows);
            write_response(vm_id, seq, STATUS_OK, packed, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_KONSOLE_BEGIN_FRAME => {
            let cols = arg0 as usize;
            let rows = (arg1 & 0xFFFF_FFFF) as usize;
            let terminal_handoff = (arg1 >> 63) != 0;
            let (actual_cols, actual_rows) = crate::hv::blueprint_console_konsole_begin_frame(
                vm_id,
                cols,
                rows,
                terminal_handoff,
            );
            let packed = (u64::from(actual_cols) << 32) | u64::from(actual_rows);
            write_response(vm_id, seq, STATUS_OK, packed, 0);
            DispatchOutcome::Resume
        }
        OP_BP_LOG_RECORD_V1 => {
            const TARGET_MAX: usize = 256;
            let Some(data) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let target_len = arg1 as usize;
            let level = match u32::try_from(arg0).ok() {
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_ERROR) => LogLevel::Error,
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_WARN) => LogLevel::Warn,
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_INFO) => LogLevel::Info,
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_DEBUG) => LogLevel::Debug,
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_TRACE) => LogLevel::Trace,
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_IMPORTANT) => LogLevel::Important,
                Some(trueos_vm::vmcall::BP_LOG_LEVEL_ONCE) => LogLevel::Once,
                _ => {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    return DispatchOutcome::Resume;
                }
            };
            if target_len == 0 || target_len > TARGET_MAX || target_len > data.len() {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Ok(target) = core::str::from_utf8(&data[..target_len]) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Ok(message) = core::str::from_utf8(&data[target_len..]) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let message = message.trim_end_matches(&['\r', '\n'][..]);
            if target == "texplo-startup-probe" {
                // This sparse, enumerated startup channel exists specifically
                // to distinguish pre-lease app initialization from terminal
                // transport and first-frame readiness. Reject control bytes so
                // an application cannot forge adjacent LogOs records.
                if !message.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'-' | b'_' | b':' | b' ')
                }) {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    return DispatchOutcome::Resume;
                }
                crate::log_os::blueprint_important_line(format_args!(
                    "texplo-startup-probe: vm={} {}\n",
                    vm_id, message
                ));
            } else {
                crate::log_os::log_with_area_purpose(
                    crate::log_os::flags::LogArea::Apps,
                    level,
                    Some(crate::log_os::purpose_for_level(level)),
                    format_args!("vm{} {}: {}\n", vm_id, target, message),
                );
            }
            write_response(vm_id, seq, STATUS_OK, data.len() as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_EXIT_REASON => {
            let Some(data) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            if data.is_empty() {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let reason = core::str::from_utf8(data).unwrap_or("non-utf8-exit-reason");
            crate::hv::blueprint_console_set_exit_reason(vm_id, reason);
            write_response(vm_id, seq, STATUS_OK, data.len() as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHUTDOWN => {
            let Some(data) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let reason = if data.is_empty() {
                "blueprint shutdown requested"
            } else {
                core::str::from_utf8(data).unwrap_or("non-utf8-shutdown-reason")
            };
            // VM teardown is the authoritative terminal-owner revocation
            // boundary.  Do this before accepting the stop request so an
            // application that exits before its own RAII guard is built (or
            // whose Rust entry point terminates through `_exit`) cannot leave
            // the launching shell's terminal attached to a stopped guest.
            // `clear_blueprint_process_context` remains the final backstop if
            // an exact backend refuses this first release.
            crate::log_os::blueprint_important_line(format_args!(
                "terminal-lifecycle: vm={} phase=shutdown-enter state=stop-requested\n",
                vm_id
            ));
            let terminal_returned = crate::hv::blueprint_console_return_to_cli(vm_id);
            crate::hv::blueprint_console_set_exit_reason(vm_id, reason);
            crate::hv::mark_blueprint_clean_exit(vm_id);
            write_response(vm_id, seq, STATUS_OK, data.len() as u64, 0);
            crate::log_os::blueprint_important_line(format_args!(
                "terminal-lifecycle: vm={} phase=shutdown-ack state=stopping terminal_returned={}\n",
                vm_id, terminal_returned as u8
            ));
            DispatchOutcome::Stop
        }
        OP_BP_RETURN_TO_CLI => {
            crate::log_os::blueprint_important_line(format_args!(
                "terminal-lifecycle: vm={} phase=return-to-cli-enter state=release-requested\n",
                vm_id
            ));
            let changed = crate::hv::blueprint_console_return_to_cli(vm_id);
            if changed {
                write_response(vm_id, seq, STATUS_OK, 1, 0);
                crate::log_os::blueprint_important_line(format_args!(
                    "terminal-lifecycle: vm={} phase=return-to-cli-ack state=returned\n",
                    vm_id
                ));
            } else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_BAD_ARG,
                    crate::hv::BlueprintTerminalLeaseError::NotActive.code(),
                    0,
                );
                crate::log_os::blueprint_important_line(format_args!(
                    "terminal-lifecycle: vm={} phase=return-to-cli-failed state=not-active\n",
                    vm_id
                ));
            }
            DispatchOutcome::Resume
        }
        OP_BP_TERMINAL_LEASE_CURRENT_V1 => {
            match crate::hv::blueprint_terminal_lease_current(vm_id, arg0) {
                Ok(epoch) => write_response(vm_id, seq, STATUS_OK, epoch, 0),
                Err(error) => write_response(vm_id, seq, STATUS_BAD_ARG, error.code(), 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_TERMINAL_LEASE_RELEASE_V1 => {
            match crate::hv::blueprint_terminal_lease_release(vm_id, arg0) {
                Ok(ticket) => write_response(vm_id, seq, STATUS_OK, ticket, 0),
                Err(error) => write_response(vm_id, seq, STATUS_BAD_ARG, error.code(), 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_TERMINAL_LEASE_POLL_REENTRY_V1 => {
            match crate::hv::blueprint_terminal_lease_poll_reentry(vm_id, arg0) {
                Ok(crate::hv::BlueprintTerminalReentryPoll::Pending) => {
                    write_response(vm_id, seq, STATUS_OK, 0, 0);
                }
                Ok(crate::hv::BlueprintTerminalReentryPoll::Ready(epoch)) => {
                    write_response(vm_id, seq, STATUS_OK, epoch, 0);
                }
                Err(error) => write_response(vm_id, seq, STATUS_BAD_ARG, error.code(), 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1 => {
            match crate::hv::blueprint_terminal_surface_snapshot(vm_id) {
                Ok(snapshot) => write_record_response(vm_id, seq, 0, &snapshot),
                Err(error) => write_response(vm_id, seq, STATUS_BAD_ARG, error.code(), 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_AUDIO_WRITE_I16_STEREO_48K => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n == 0 || (n & 1) != 0 {
                crate::log_warn!(
                    target: "audio";
                    "blueprint-audio-vmcall: vm={} bad-len bytes={} req_len={}\n",
                    vm_id,
                    n,
                    req_len
                );
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                crate::log_error!(
                    target: "audio";
                    "blueprint-audio-vmcall: vm={} missing-host-ptr bytes={}\n",
                    vm_id,
                    n
                );
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let data = unsafe { &(&(*p).payload)[..n] };
            let mut samples = Vec::with_capacity(n / 2);
            for bytes in data.chunks_exact(2) {
                samples.push(i16::from_le_bytes([bytes[0], bytes[1]]));
            }
            let log_sample = sampled_log(&BLUEPRINT_AUDIO_WRITE_LOG_SEQ);
            if log_sample {
                crate::log_trace!(
                    target: "audio";
                    "blueprint-audio-vmcall: vm={} write bytes={} samples={} frames={} pending_before={}\n",
                    vm_id,
                    n,
                    samples.len(),
                    samples.len() / crate::hda::PCM_CHANNELS,
                    crate::aud::pcm_lane::pending_frames()
                );
                crate::audio_probe!(
                    "blueprint-audio-vmcall: vm={} write bytes={} samples={} frames={} pending_before={}\n",
                    vm_id,
                    n,
                    samples.len(),
                    samples.len() / crate::hda::PCM_CHANNELS,
                    crate::aud::pcm_lane::pending_frames()
                );
            }
            let rc = match crate::aud::pcm_lane::submit_i16_stereo_48k(
                "blueprint-audio-vmcall",
                samples,
            ) {
                Ok(frames) => frames as i64,
                Err(crate::aud::pcm_lane::PcmLaneError::QueueFull) => -16,
                Err(crate::aud::pcm_lane::PcmLaneError::BadShape) => -22,
                Err(crate::aud::pcm_lane::PcmLaneError::EmptyBuffer) => -5,
            };
            if log_sample {
                crate::log_trace!(
                    target: "audio";
                    "blueprint-audio-vmcall: vm={} write-rc={} pending_after={}\n",
                    vm_id,
                    rc,
                    crate::aud::pcm_lane::pending_frames()
                );
                crate::audio_probe!(
                    "blueprint-audio-vmcall: vm={} write-rc={} pending_after={}\n",
                    vm_id,
                    rc,
                    crate::aud::pcm_lane::pending_frames()
                );
            }
            write_response(vm_id, seq, STATUS_OK, rc as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_AUDIO_STOP => {
            let generation = crate::aud::pcm_lane::request_stop();
            crate::log_info!(
                target: "audio";
                "blueprint-audio-vmcall: vm={} stop generation={}\n",
                vm_id,
                generation
            );
            crate::audio_probe!(
                "blueprint-audio-vmcall: vm={} stop generation={}\n",
                vm_id,
                generation
            );
            write_response(vm_id, seq, STATUS_OK, generation as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_AUDIO_PENDING_FRAMES => {
            let frames = crate::aud::pcm_lane::pending_frames();
            if sampled_log(&BLUEPRINT_AUDIO_POLL_LOG_SEQ) {
                crate::log_trace!(
                    target: "audio";
                    "blueprint-audio-vmcall: vm={} pending frames={}\n",
                    vm_id,
                    frames
                );
            }
            write_response(vm_id, seq, STATUS_OK, frames as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_AUDIO_SET_VOLUME_PERCENT => {
            let requested = (arg0 as u32).min(100);
            let applied = crate::aud::pcm_lane::set_volume_percent(requested as u16);
            crate::log_info!(
                target: "audio";
                "blueprint-audio-vmcall: vm={} set-volume requested={} applied={}\n",
                vm_id,
                requested,
                applied
            );
            write_response(vm_id, seq, STATUS_OK, applied as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_AUDIO_VOLUME_PERCENT => {
            let percent = crate::aud::pcm_lane::volume_percent();
            if sampled_log(&BLUEPRINT_AUDIO_POLL_LOG_SEQ) {
                crate::log_trace!(
                    target: "audio";
                    "blueprint-audio-vmcall: vm={} volume percent={}\n",
                    vm_id,
                    percent
                );
            }
            write_response(vm_id, seq, STATUS_OK, percent as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_READ_BYTE => {
            let byte = crate::hv::blueprint_console_read_byte(vm_id)
                .map(u64::from)
                .unwrap_or(u64::MAX);
            write_response(vm_id, seq, STATUS_OK, byte, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_READ => {
            let want = core::cmp::min(arg0 as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let read = crate::hv::blueprint_console_read(vm_id, out);
            write_response(vm_id, seq, STATUS_OK, read as u64, read as u32);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_READABLE_LEN => {
            let len = crate::hv::blueprint_console_readable_len(vm_id);
            write_response(vm_id, seq, STATUS_OK, len as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_WAIT_READABLE => {
            let len = crate::hv::blueprint_console_readable_len(vm_id);
            if len != 0 || arg0 == 0 {
                write_response(vm_id, seq, STATUS_OK, u64::from(len != 0), 0);
                DispatchOutcome::Resume
            } else {
                DispatchOutcome::WaitConsoleInput {
                    seq,
                    timeout_ms: arg0.min(MAX_GUEST_SLEEP_MS),
                }
            }
        }
        OP_BP_ARCHIVE_PACK_START | OP_BP_ARCHIVE_UNPACK_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let split = arg0 as usize;
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            if split == 0 || split >= n {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PARAM as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let (first_bytes, second_bytes) = bytes.split_at(split);
            let (Ok(first), Ok(second)) =
                (core::str::from_utf8(first_bytes), core::str::from_utf8(second_bytes))
            else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let (Some(first), Some(second)) = (
                crate::r::io::env::resolve_fs_path(first, false),
                crate::r::io::env::resolve_fs_path(second, false),
            ) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = if op == OP_BP_ARCHIVE_PACK_START {
                crate::r::archive_cabi::start_pack(owner, first, second)
            } else {
                crate::r::archive_cabi::start_unpack(owner, first, second)
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ARCHIVE_STATUS => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::archive_cabi::status(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ARCHIVE_REPORT => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            match crate::r::archive_cabi::report(owner, arg0 as u32) {
                Ok(report) => {
                    let out = unsafe { &mut (&mut (*p).payload)[..24] };
                    out[0..8].copy_from_slice(&report.input_bytes.to_le_bytes());
                    out[8..16].copy_from_slice(&report.output_bytes.to_le_bytes());
                    out[16..20].copy_from_slice(&report.file_count.to_le_bytes());
                    out[20..24].fill(0);
                    write_response(vm_id, seq, STATUS_OK, 0, 24);
                }
                Err(rc) => {
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                }
            }
            DispatchOutcome::Resume
        }
        OP_BP_ARCHIVE_DISCARD => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::archive_cabi::discard(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_RENAME_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let payload = unsafe { &(&(*p).payload)[..n] };
            if payload.len() < 4 {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PARAM as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let source_len = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
            let Some(destination_offset) = 4usize.checked_add(source_len) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PARAM as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            if source_len == 0 || destination_offset >= payload.len() {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PARAM as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let Ok(source) = core::str::from_utf8(&payload[4..destination_offset]) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let Ok(destination) = core::str::from_utf8(&payload[destination_offset..]) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let Ok(source) = crate::r::path::FsPath::parse(source, false) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let Ok(destination) = crate::r::path::FsPath::parse(destination, false) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let source = source.to_relative_string();
            let destination = destination.to_relative_string();
            if !vm_mount_selector_allowed(vm_id, source.as_str())
                || !vm_mount_selector_allowed(vm_id, destination.as_str())
            {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let rc = crate::r::io::async_fs_cabi::start_rename(
                crate::r::io::async_fs_cabi::owner_for_vm(vm_id),
                source,
                destination,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_LIST_MOUNTS_START => {
            let rc = if vm_has_trueosfs_scope(vm_id) {
                crate::r::io::async_fs_cabi::start_list_mounts(
                    crate::r::io::async_fs_cabi::owner_for_vm(vm_id),
                )
            } else {
                crate::r::io::cabi::FS_ERR_BAD_PATH
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_READ_START | OP_BP_ASYNC_FS_REMOVE_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            // `vFile:` names kernel-provided virtual data, not TrueOSFS
            // paths. Preserve the identifier verbatim so the async-FS
            // provider can dispatch it before filesystem normalization.
            let path = if op == OP_BP_ASYNC_FS_READ_START && path == "vFile:launch" {
                path.into()
            } else {
                let Ok(path) = crate::r::path::FsPath::parse(path, false) else {
                    write_response(
                        vm_id,
                        seq,
                        STATUS_OK,
                        (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                        0,
                    );
                    return DispatchOutcome::Resume;
                };
                path.to_relative_string()
            };
            if !vm_mount_selector_allowed(vm_id, path.as_str()) {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = if op == OP_BP_ASYNC_FS_READ_START {
                crate::r::io::async_fs_cabi::start_read(owner, path)
            } else {
                crate::r::io::async_fs_cabi::start_remove(owner, path)
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_WRITE_BEGIN => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let Ok(path) = crate::r::path::FsPath::parse(path, false) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let path = path.to_relative_string();
            if !vm_mount_selector_allowed(vm_id, path.as_str()) {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::io::async_fs_cabi::start_write(owner, path, arg0 as usize);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_WRITE_CHUNK => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc =
                crate::r::io::async_fs_cabi::write_chunk(owner, arg0 as u32, arg1 as usize, bytes);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_WRITE_COMMIT => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::io::async_fs_cabi::write_commit(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_CREATE_DIR_ALL_START
        | OP_BP_ASYNC_FS_STAT_START
        | OP_BP_ASYNC_FS_LIST_DIR_START
        | OP_BP_ASYNC_FS_RECORD_KEY_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let Ok(path) = crate::r::path::FsPath::parse(path, true) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let path = path.to_relative_string();
            if !vm_mount_selector_allowed(vm_id, path.as_str()) {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_PATH as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            }
            let rc = match op {
                OP_BP_ASYNC_FS_CREATE_DIR_ALL_START => {
                    crate::r::io::async_fs_cabi::start_create_dir_all(owner, path)
                }
                OP_BP_ASYNC_FS_STAT_START => crate::r::io::async_fs_cabi::start_stat(owner, path),
                OP_BP_ASYNC_FS_LIST_DIR_START => {
                    crate::r::io::async_fs_cabi::start_list_dir(owner, path)
                }
                OP_BP_ASYNC_FS_RECORD_KEY_START => {
                    crate::r::io::async_fs_cabi::start_record_key(owner, path)
                }
                _ => unreachable!(),
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_STATUS => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::io::async_fs_cabi::status(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_RESULT_LEN => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::io::async_fs_cabi::result_len(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_RESULT_READ => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let offset = (arg1 >> 32) as usize;
            let want = core::cmp::min(arg1 as u32 as usize, PAYLOAD_CAP);
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::io::async_fs_cabi::result_read(owner, arg0 as u32, offset, out);
            let out_len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_ASYNC_FS_DISCARD => {
            let owner = crate::r::io::async_fs_cabi::owner_for_vm(vm_id);
            let rc = crate::r::io::async_fs_cabi::discard(owner, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_LIST_TREE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(text) = crate::hv::blueprint_process_file_tree_text(vm_id, path) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = text.as_bytes();
            let out_n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[..out_n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_FS_LIST_DIR => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let text = match crate::r::io::cabi::fs_list_dir_host_text(path) {
                Ok(text) => text,
                Err(rc) => {
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                    return DispatchOutcome::Resume;
                }
            };
            if arg1 == 0 {
                write_response(vm_id, seq, STATUS_OK, text.len() as u64, 0);
                return DispatchOutcome::Resume;
            }
            let bytes = text.as_bytes();
            let offset = core::cmp::min(arg0 as usize, bytes.len());
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let end = core::cmp::min(offset.saturating_add(want), bytes.len());
            let out_n = end.saturating_sub(offset);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[offset..end]);
            }
            write_response(vm_id, seq, STATUS_OK, out_n as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_FS_READ_FILE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            if arg1 == 0 {
                let rc = crate::r::io::cabi::fs_read_file_len_host(path);
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let rc = crate::r::io::cabi::fs_read_file_chunk_host(path, arg0 as usize, out);
            let out_len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_BEGIN => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_write_begin_host(path, arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_CHUNK => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let rc = crate::r::io::cabi::fs_write_chunk_host(arg0 as u32, bytes);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_FINISH => {
            let rc = crate::r::io::cabi::fs_write_finish_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_ABORT => {
            let rc = crate::r::io::cabi::fs_write_abort_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_EXISTS => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_exists_host(path);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_STAT => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let mut kind = 0u32;
            let mut len = 0u64;
            let rc = crate::r::io::cabi::fs_stat_host(path, &mut kind, &mut len);
            let data = (rc as u32 as u64) | ((kind as u64) << 32);
            if path.contains("ggml-tiny") {
                crate::log!(
                    "vmcall: bp-fs-stat path={} rc={} kind={} len={}\n",
                    path,
                    rc,
                    kind,
                    len
                );
            }
            let out_len = if rc == 0 {
                let payload = unsafe { &mut (&mut (*p).payload)[..12] };
                payload[..4].copy_from_slice(&kind.to_le_bytes());
                payload[4..12].copy_from_slice(&len.to_le_bytes());
                12
            } else {
                0
            };
            write_response(vm_id, seq, STATUS_OK, data, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_FS_REMOVE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_remove_host(path);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_OPEN => {
            let domain = arg0 as u32 as i32;
            let socket_type = (arg0 >> 32) as u32 as i32;
            let protocol = arg1 as u32 as i32;
            let rc =
                crate::r::net::socket_cabi::socket_tcp_open_host(domain, socket_type, protocol);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_CLOSE => {
            let rc = crate::r::net::socket_cabi::socket_tcp_close_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_SET_NONBLOCKING => {
            let rc = crate::r::net::socket_cabi::socket_tcp_set_nonblocking_host(
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_BIND_V4 => {
            let addr_be = arg1 as u32;
            let port_be = ((arg1 >> 32) & 0xFFFF) as u16;
            let rc =
                crate::r::net::socket_cabi::socket_tcp_bind_v4_host(arg0 as u32, addr_be, port_be);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_BIND_V6 => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 16 {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut addr = [0u8; 16];
            unsafe {
                addr.copy_from_slice(&(&(*p).payload)[..16]);
            }
            let rc =
                crate::r::net::socket_cabi::socket_tcp_bind_v6_host(arg0 as u32, addr, arg1 as u16);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_CONNECT_V4 => {
            let addr_be = arg1 as u32;
            let port_be = ((arg1 >> 32) & 0xFFFF) as u16;
            let nonblocking = ((arg1 >> 48) & 1) as u32;
            let rc = crate::r::net::socket_cabi::socket_tcp_connect_v4_host(
                arg0 as u32,
                addr_be,
                port_be,
                nonblocking,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_CONNECT_V6 => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 16 {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut addr = [0u8; 16];
            unsafe {
                addr.copy_from_slice(&(&(*p).payload)[..16]);
            }
            let port_be = arg1 as u16;
            let nonblocking = ((arg1 >> 16) & 1) as u32;
            let rc = crate::r::net::socket_cabi::socket_tcp_connect_v6_host(
                arg0 as u32,
                addr,
                port_be,
                nonblocking,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_POLL_CONNECT => {
            let rc = crate::r::net::socket_cabi::socket_tcp_poll_connect_host(arg0 as u32, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_SEND => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let rc = crate::r::net::socket_cabi::socket_tcp_send_host(arg0 as u32, bytes);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_RECV => {
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 16 {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let payload = unsafe { &mut (*p).payload };
            let flags = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let nonblocking = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let timeout_ms = u64::from_le_bytes([
                payload[8],
                payload[9],
                payload[10],
                payload[11],
                payload[12],
                payload[13],
                payload[14],
                payload[15],
            ]);
            let rc = crate::r::net::socket_cabi::socket_tcp_recv_host(
                arg0 as u32,
                &mut payload[..want],
                flags,
                nonblocking,
                timeout_ms,
            );
            let out_len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_SHUTDOWN => {
            let rc = crate::r::net::socket_cabi::socket_tcp_shutdown_host(arg0 as u32, arg1 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_TAKE_ERROR => {
            let rc = crate::r::net::socket_cabi::socket_tcp_take_error_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_PEER_V4 => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            match crate::r::net::socket_cabi::socket_tcp_peer_v4_host(arg0 as u32) {
                Ok((addr, port)) => {
                    unsafe {
                        (&mut (*p).payload)[..4].copy_from_slice(&addr.to_le_bytes());
                        (&mut (*p).payload)[4..6].copy_from_slice(&port.to_le_bytes());
                    }
                    write_response(vm_id, seq, STATUS_OK, 0, 6);
                }
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_PEER_V6 => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            match crate::r::net::socket_cabi::socket_tcp_peer_v6_host(arg0 as u32) {
                Ok((addr, port)) => {
                    unsafe {
                        (&mut (*p).payload)[..16].copy_from_slice(&addr);
                        (&mut (*p).payload)[16..18].copy_from_slice(&port.to_le_bytes());
                    }
                    write_response(vm_id, seq, STATUS_OK, 0, 18);
                }
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_LISTENER_BIND | OP_BP_MIO_TCP_STREAM_CONNECT | OP_BP_MIO_UDP_SOCKET_BIND => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Some(addr) = read_mio_addr(bytes) else {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let mut socket_id = 0u32;
            let status = crate::hv::with_guest_broker_context(vm_id, || match op {
                OP_BP_MIO_TCP_LISTENER_BIND => unsafe {
                    crate::mio_compat::mio_tcp_listener_bind_host(addr, &mut socket_id)
                },
                OP_BP_MIO_TCP_STREAM_CONNECT => unsafe {
                    crate::mio_compat::mio_tcp_stream_connect_host(addr, &mut socket_id)
                },
                _ => unsafe { crate::mio_compat::mio_udp_socket_bind_host(addr, &mut socket_id) },
            });
            if status == 0 {
                write_response(vm_id, seq, STATUS_OK, socket_id as u64, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, (status as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SOCKET_CLOSE => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_socket_close_host(arg0 as u32)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SOCKET_LOCAL_ADDR | OP_BP_MIO_SOCKET_PEER_ADDR => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut addr = crate::mio_compat::TrueosMioSocketAddr::default();
            let rc = crate::hv::with_guest_broker_context(vm_id, || {
                if op == OP_BP_MIO_SOCKET_LOCAL_ADDR {
                    unsafe { crate::mio_compat::mio_socket_local_addr_host(arg0 as u32, &mut addr) }
                } else {
                    unsafe { crate::mio_compat::mio_socket_peer_addr_host(arg0 as u32, &mut addr) }
                }
            });
            if rc == 0 {
                let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
                let len = if write_mio_addr(out, addr) {
                    MIO_ADDR_BYTES as u32
                } else {
                    0
                };
                write_response(vm_id, seq, STATUS_OK, 0, len);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SOCKET_TAKE_ERROR => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_socket_take_error_host(arg0 as u32)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_STREAM_READ => {
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_tcp_stream_read_host(arg0 as u32, out.as_mut_ptr(), want)
            });
            let len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, len);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_STREAM_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_tcp_stream_write_host(
                    arg0 as u32,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_UDP_SOCKET_CONNECT => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Some(addr) = read_mio_addr(bytes) else {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_udp_socket_connect_host(arg0 as u32, addr)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_UDP_SOCKET_SEND_TO => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Some(addr) = read_mio_addr(bytes) else {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let data_len = core::cmp::min(arg1 as usize, n.saturating_sub(MIO_ADDR_BYTES));
            let data = &bytes[MIO_ADDR_BYTES..MIO_ADDR_BYTES + data_len];
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_udp_socket_send_to_host(
                    arg0 as u32,
                    addr,
                    data.as_ptr(),
                    data.len(),
                )
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_UDP_SOCKET_RECV_FROM => {
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP.saturating_sub(MIO_ADDR_BYTES));
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let payload = unsafe { &mut (*p).payload };
            let mut addr = crate::mio_compat::TrueosMioSocketAddr::default();
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_udp_socket_recv_from_host(
                    arg0 as u32,
                    &mut addr,
                    payload[MIO_ADDR_BYTES..].as_mut_ptr(),
                    want,
                )
            });
            if rc > 0 {
                let _ = write_mio_addr(&mut payload[..MIO_ADDR_BYTES], addr);
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    rc as u64,
                    (MIO_ADDR_BYTES + rc as usize) as u32,
                );
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_LISTENER_ACCEPT => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut socket_id = 0u32;
            let mut addr = crate::mio_compat::TrueosMioSocketAddr::default();
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_tcp_listener_accept_host(
                    arg0 as u32,
                    &mut socket_id,
                    &mut addr,
                )
            });
            if rc == 0 {
                let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
                let len = if write_mio_addr(out, addr) {
                    MIO_ADDR_BYTES as u32
                } else {
                    0
                };
                write_response(vm_id, seq, STATUS_OK, socket_id as u64, len);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_REGISTER_SOCKET => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 8 {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let token = u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]) as usize;
            let socket_id = arg1 as u32;
            let interests = ((arg1 >> 32) & 0xFF) as u8;
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_register_socket_host(
                    arg0 as usize,
                    socket_id,
                    token,
                    interests,
                )
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_deregister_socket_host(arg0 as usize, arg1 as u32)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_WAKE => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_wake_host(arg0 as usize)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_POLL => {
            let max_events = core::cmp::min(
                arg1 as usize,
                PAYLOAD_CAP / core::cmp::max(MIO_READY_EVENT_BYTES, 1),
            );
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let timeout_nanos = if n >= 8 {
                let Some(p) = host_ptr(vm_id) else {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    return DispatchOutcome::Resume;
                };
                let bytes = unsafe { &(&(*p).payload)[..n] };
                u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            } else {
                u64::MAX
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
            let count = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_poll_host(
                    arg0 as usize,
                    out.as_mut_ptr() as *mut crate::mio_compat::TrueosMioReadyEvent,
                    max_events,
                    timeout_nanos,
                )
            });
            let count = core::cmp::min(count, max_events);
            write_response(
                vm_id,
                seq,
                STATUS_OK,
                count as u64,
                (count * MIO_READY_EVENT_BYTES) as u32,
            );
            DispatchOutcome::Resume
        }
        _ => {
            hvlogf(format_args!(
                "hv: vm{} reporting: vmcall unknown op=0x{:02X} seq={}",
                vm_id, op, seq
            ));
            write_response(vm_id, seq, STATUS_UNKNOWN_OP, 0, 0);
            DispatchOutcome::Resume
        }
    }
}
