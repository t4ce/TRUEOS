//! Guest-side vmcall protocol.
//!
//! Mirrors CommPage layout from src/hv/vmcall.rs.
//! Guest writes request fields, issues vmcall (synchronous — host handles
//! and vmresumes before this call returns), then reads back the response.

use core::sync::atomic::{AtomicU32, Ordering};

// ── op codes ─────────────────────────────────────────────────────────────────
pub const OP_PRESERVE: u32 = 0x01;
pub const OP_PING: u32 = 0x02;
pub const OP_UNIX_TIME: u32 = 0x03;
pub const OP_YIELD: u32 = 0x04;
pub const OP_SLEEP_MS: u32 = 0x05;
pub const OP_RAND_BYTES: u32 = 0x06;
pub const OP_BP_CPU_COUNT: u32 = 0x07;
pub const OP_MONOTONIC_NANOS: u32 = 0x08;
pub const OP_LIFECYCLE_PAUSE: u32 = 0x09;
pub const OP_BP_REL_IMAGE_EXEC_ENABLE: u32 = 0x0A;
pub const OP_BP_REL_IMAGE_EXEC_DISABLE: u32 = 0x0B;
pub const OP_BP_LIFECYCLE_POLL: u32 = 0x0C;
pub const OP_BP_LIFECYCLE_READY: u32 = 0x0D;
pub const OP_BP_LIFECYCLE_IDENTITY: u32 = 0x0E;
pub const OP_LIFECYCLE_SNAPSHOT: u32 = 0x0F;
pub const OP_BP_RAPL_SNAPSHOT_READ: u32 = 0x91;
pub const OP_BP_RAPL_HISTORY_READ: u32 = 0x92;
pub const OP_BP_PCI_SNAPSHOT_READ: u32 = 0x93;
pub const OP_BP_THERMAL_SNAPSHOT_READ: u32 = 0x94;
pub const OP_BP_VGPU_VVIDEO_CREATE: u32 = 0x95;
pub const OP_BP_VGPU_VVIDEO_FLUSH: u32 = 0x96;
pub const OP_BP_VGPU_VVIDEO_INVALIDATE: u32 = 0x97;
pub const OP_BP_VGPU_DEVICE_DIAGNOSTICS: u32 = 0x90;
pub const OP_BP_SYSTEM_SERVICES_SNAPSHOT_READ: u32 = 0xA4;
pub const OP_BP_VGPU_OPEN: u32 = 0xA5;
pub const OP_BP_VGPU_CLOSE: u32 = 0xA6;
pub const OP_BP_VGPU_DEVICE_INFO: u32 = 0xA7;
pub const OP_BP_VGPU_BUFFER_CREATE: u32 = 0xA8;
pub const OP_BP_VGPU_BUFFER_DESTROY: u32 = 0xA9;
pub const OP_BP_VGPU_BUFFER_INFO: u32 = 0xAA;
pub const OP_BP_VGPU_QUEUE_CREATE: u32 = 0xAB;
pub const OP_BP_VGPU_QUEUE_DESTROY: u32 = 0xAC;
pub const OP_BP_VGPU_SUBMIT_CONTROL_NOP: u32 = 0xAD;
pub const OP_BP_VGPU_TIMELINE: u32 = 0xAE;
pub const OP_BP_VGPU_WAIT: u32 = 0xAF;
pub const OP_BP_VGPU_BUFFER_WRITE: u32 = 0xB0;
pub const OP_BP_VGPU_BUFFER_READ: u32 = 0xB1;
pub const OP_BP_VGPU_UI4_SURFACE_ACQUIRE: u32 = 0x124;
pub const OP_BP_VGPU_UI4_SURFACE_DISCARD: u32 = 0x125;
pub const OP_BP_VGPU_UI4_SURFACE_CLEAR_SUBMIT: u32 = 0x126;
pub const OP_BP_VGPU_SHADER_MODULE_CREATE: u32 = 0x127;
pub const OP_BP_VGPU_SHADER_MODULE_DESTROY: u32 = 0x128;
pub const OP_BP_VGPU_RENDER_PIPELINE_CREATE: u32 = 0x129;
pub const OP_BP_VGPU_RENDER_PIPELINE_DESTROY: u32 = 0x12A;
pub const OP_BP_VGPU_UI4_INDEXED_SUBMIT: u32 = 0x12B;
pub const OP_BP_ASYNC_FS_RECORD_KEY_START: u32 = 0x12C;
/// Publish an active BlueprintScene lease after its kernel-owned compute
/// producer has supplied the exact release proof. This opcode deliberately
/// names neither the producer nor a UI toolkit.
pub const OP_BP_UI4_SCENE_COMPUTE_FRAME_PUBLISH: u32 = 0x12D;
pub const OP_BP_UI4_SOLARA_FONT_SIZES: u32 = 0xB2;
pub const OP_BP_UI4_SOLARA_FRAME_OPEN: u32 = 0xB3;
pub const OP_BP_UI4_SOLARA_FRAME_BEGIN: u32 = 0xB4;
pub const OP_BP_UI4_SOLARA_TEXT_ROWS: u32 = 0xB5;
pub const OP_BP_UI4_SOLARA_FRAME_PUBLISH: u32 = 0xB6;
/// `arg0=window`, `arg1=close flags` (zero preserves legacy/default teardown).
pub const OP_BP_UI4_SOLARA_FRAME_CLOSE: u32 = 0xB7;
pub const OP_BP_UI4_SOLARA_TEXT_SCENE: u32 = 0xB8;
pub const OP_BP_GRIDPAPER_SNAPSHOT_SUBMIT: u32 = 0xB9;
pub const OP_BP_GRIDPAPER_SNAPSHOT_CHECKPOINT: u32 = 0xF9;
pub const OP_BP_GRIDPAPER_CLOSE: u32 = 0xBA;
pub const OP_BP_GRIDPAPER_TEXT_ANIMATIONS_SUBMIT: u32 = 0xBB;
pub const OP_BP_PRINTER_SNAPSHOT_READ: u32 = 0xBC;
pub const OP_BP_PRINT2D_SUBMIT: u32 = 0xBD;
pub const OP_BP_PRINT2D_STATUS: u32 = 0xBE;
pub const OP_BP_GRIDPAPER_PRINT_REQUEST_TAKE: u32 = 0xBF;
pub const OP_BP_UI4_SCENE_SKYBOX_UPLOAD_BEGIN: u32 = 0xC0;
pub const OP_BP_UI4_SCENE_SKYBOX_UPLOAD_CHUNK: u32 = 0xC1;
pub const OP_BP_UI4_SCENE_SKYBOX_UPLOAD_FINISH: u32 = 0xC2;
pub const OP_BP_UI4_SCENE_SKYBOX_RENDER: u32 = 0xC3;
pub const OP_BP_UI4_SCENE_WRITE_OPAQUE_RGBA8: u32 = 0xC4;
pub const OP_BP_UI4_SCENE_FRAME_SET_POSITION: u32 = 0xC5;
pub const OP_BP_UI4_SCENE_FRAME_RESIZE: u32 = 0xC6;
pub const OP_BP_UI4_SCENE_FRAME_OPEN_STREAMING: u32 = 0xC7;
pub const OP_BP_SHELL_ATTACHED_READ: u32 = 0xCB;
pub const OP_BP_INPUT_KEYBOARD_OUTPUT_POP: u32 = 0xCC;
pub const OP_BP_INPUT_KEYBOARD_OUTPUT_SINCE: u32 = 0xCD;
pub const OP_BP_ASYNC_FS_READ_START: u32 = 0xCE;
pub const OP_BP_ASYNC_FS_REMOVE_START: u32 = 0xCF;
pub const OP_BP_ASYNC_FS_STATUS: u32 = 0xD0;
pub const OP_BP_ASYNC_FS_RESULT_LEN: u32 = 0xD1;
pub const OP_BP_ASYNC_FS_RESULT_READ: u32 = 0xD2;
pub const OP_BP_ASYNC_FS_DISCARD: u32 = 0xD3;
pub const OP_BP_UI4_SCENE_PAN_EVENT_TAKE: u32 = 0xD4;
pub const OP_BP_ASYNC_FS_WRITE_BEGIN: u32 = 0xD5;
pub const OP_BP_ASYNC_FS_WRITE_CHUNK: u32 = 0xD6;
pub const OP_BP_ASYNC_FS_WRITE_COMMIT: u32 = 0xD7;
pub const OP_BP_ASYNC_FS_CREATE_DIR_ALL_START: u32 = 0xD8;
pub const OP_BP_ASYNC_FS_STAT_START: u32 = 0xD9;
pub const OP_BP_ASYNC_FS_LIST_DIR_START: u32 = 0xDA;
pub const OP_BP_ASYNC_FS_LIST_MOUNTS_START: u32 = 0x139;
pub const OP_BP_ASYNC_FS_RENAME_START: u32 = 0x13A;
pub const OP_BP_SHELL_ATTACHED_WAIT_READABLE: u32 = 0x13B;
pub const OP_BP_CHILD_SPAWN_V1: u32 = 0x13C;
pub const OP_BP_CHILD_SEND_V1: u32 = 0x13D;
pub const OP_BP_CHILD_RECEIVE_V1: u32 = 0x13E;
pub const OP_BP_CHILD_STATUS_V1: u32 = 0x13F;
pub const OP_BP_CHILD_TERMINATE_V1: u32 = 0x140;
pub const OP_BP_VGPU_UI4_INDEXED_BATCH_SUBMIT: u32 = 0x141;
pub const OP_BP_VGPU_CLOUD_WORK_GRAPH_CREATE: u32 = 0x149;
pub const OP_BP_VGPU_CLOUD_WORK_GRAPH_DESTROY: u32 = 0x14A;
pub const OP_BP_VGPU_CLOUD_FRAME_SUBMIT: u32 = 0x14B;
const _: () = {
    assert!(OP_BP_VGPU_CLOUD_WORK_GRAPH_CREATE == 0x149);
    assert!(OP_BP_VGPU_CLOUD_WORK_GRAPH_DESTROY == 0x14A);
    assert!(OP_BP_VGPU_CLOUD_FRAME_SUBMIT == 0x14B);
};
pub const OP_BP_UI4_SCENE_KEYBOARD_STATE: u32 = 0xDB;
pub const OP_BP_UI4_SCENE_FRAME_SET_HIT_TESTABLE: u32 = 0x123;
pub const OP_BP_VMEDIA_IMAGE_DECODE_BEGIN: u32 = 0x142;
pub const OP_BP_VMEDIA_IMAGE_DECODE_WRITE: u32 = 0x143;
pub const OP_BP_VMEDIA_IMAGE_DECODE_COMMIT: u32 = 0x144;
pub const OP_BP_VMEDIA_IMAGE_DECODE_STATUS: u32 = 0x145;
pub const OP_BP_VMEDIA_IMAGE_DECODE_INFO: u32 = 0x146;
pub const OP_BP_VMEDIA_IMAGE_DECODE_READ: u32 = 0x147;
pub const OP_BP_VMEDIA_IMAGE_DECODE_DISCARD: u32 = 0x148;
pub const OP_BP_TERMINAL_LEASE_CURRENT_V1: u32 = 0x134;
pub const OP_BP_TERMINAL_LEASE_RELEASE_V1: u32 = 0x135;
pub const OP_BP_TERMINAL_LEASE_POLL_REENTRY_V1: u32 = 0x136;
pub const OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1: u32 = 0x137;
pub const OP_BP_LOG_RECORD_V1: u32 = 0x138;
/// Structured Blueprint log levels carried in `OP_BP_LOG_RECORD_V1.arg0`.
///
/// Values 1 through 5 are the original wire ABI and must not be renumbered.
/// V1 carries only the level, target, and message, so `ONCE` does not include
/// a stable guest callsite identifier; a future protocol revision is required
/// for host-side once tracking across distinct guest callsites.
pub const BP_LOG_LEVEL_ERROR: u32 = v::vsys::LOG_LEVEL_ERROR;
pub const BP_LOG_LEVEL_WARN: u32 = v::vsys::LOG_LEVEL_WARN;
pub const BP_LOG_LEVEL_INFO: u32 = v::vsys::LOG_LEVEL_INFO;
pub const BP_LOG_LEVEL_DEBUG: u32 = v::vsys::LOG_LEVEL_DEBUG;
pub const BP_LOG_LEVEL_TRACE: u32 = v::vsys::LOG_LEVEL_TRACE;
pub const BP_LOG_LEVEL_IMPORTANT: u32 = v::vsys::LOG_LEVEL_IMPORTANT;
pub const BP_LOG_LEVEL_ONCE: u32 = v::vsys::LOG_LEVEL_ONCE;
pub const OP_BP_UI4_SCENE_FRAME_OPEN_IMMUTABLE: u32 = 0xDC;
pub const OP_BP_UI4_SCENE_SPRITE_UPLOAD_BEGIN: u32 = 0xDD;
pub const OP_BP_UI4_SCENE_SPRITE_UPLOAD_CHUNK: u32 = 0xDE;
pub const OP_BP_UI4_SCENE_SPRITE_UPLOAD_FINISH: u32 = 0xDF;
pub const OP_BP_UI4_SCENE_SPRITE_FRAME_BEGIN: u32 = 0xE0;
pub const OP_BP_UI4_SCENE_SPRITE_DRAW_BEGIN: u32 = 0xE1;
pub const OP_BP_UI4_SCENE_SPRITE_DRAW_CHUNK: u32 = 0xE2;
pub const OP_BP_UI4_SCENE_SPRITE_DRAW_FINISH: u32 = 0xE3;
pub const OP_BP_VRAM_SNAPSHOT_READ: u32 = 0xE4;
pub const OP_BP_UI4_SCENE_RESIZE_EVENT_TAKE: u32 = 0xE5;
pub const OP_BP_UI4_SCENE_SET_CUSTOM_CURSOR: u32 = 0xE6;
pub const OP_BP_UI4_SCENE_SET_CURSOR_ICON: u32 = 0xE7;
pub const OP_BP_UI4_SCENE_POINTER_EVENT_TAKE: u32 = 0xE8;
pub const OP_BP_UI4_SCENE_PARTICLE_CRAFT_RENDER: u32 = 0xE9;
pub const OP_BP_MOUSE_MOTION_CURSOR_REQUEST: u32 = 0xEA;
pub const OP_BP_MOUSE_MOTION_CURSOR_RELEASE: u32 = 0xEB;
pub const OP_BP_MOUSE_MOTION_SUBMIT: u32 = 0xEC;
pub const OP_BP_MOUSE_MOTION_SUBMIT_JSON: u32 = 0xED;
pub const OP_BP_MOUSE_MOTION_CURSOR_IDLE: u32 = 0xEE;
pub const OP_BP_KEYBOARD_CONTROL_REQUEST: u32 = 0xEF;
pub const OP_BP_KEYBOARD_CONTROL_RELEASE: u32 = 0xF0;
pub const OP_BP_KEYBOARD_CONTROL_SUBMIT: u32 = 0xF1;
pub const OP_BP_KEYBOARD_CONTROL_SUBMIT_TEXT: u32 = 0xF2;
pub const OP_BP_KEYBOARD_CONTROL_SUBMIT_JSON: u32 = 0xF3;
pub const OP_BP_KEYBOARD_CONTROL_IDLE: u32 = 0xF4;
pub const OP_BP_UI4_SCENE_FIRST_PRESENTATION_TAKE: u32 = 0xF5;
pub const OP_BP_UI4_SCENE_OUTPUT_DIMENSIONS: u32 = 0xF6;
pub const OP_BP_USB_SNAPSHOT_READ: u32 = 0xF7;
pub const OP_BP_UI4_SCENE_INPUT_ROUTES: u32 = 0xF8;
pub const OP_BP_ARCHIVE_PACK_START: u32 = 0xFA;
pub const OP_BP_ARCHIVE_UNPACK_START: u32 = 0xFB;
pub const OP_BP_ARCHIVE_STATUS: u32 = 0xFC;
pub const OP_BP_ARCHIVE_REPORT: u32 = 0xFD;
pub const OP_BP_ARCHIVE_DISCARD: u32 = 0xFE;
pub const OP_BP_UI4_FONT_CANVAS: u32 = 0xFF;
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
pub const OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE: u32 = 0x111;
pub const OP_BP_SPIRIT_TEXT_PRESENT_SILENT: u32 = 0x112;
/// Start a host-owned JSON POST returning bytes.
///
/// `arg0` is the timeout in milliseconds. `arg1` packs the URL length in the
/// low 32 bits and bearer length in the high 32 bits. The request payload is
/// exactly `URL || bearer || JSON body`; the body is the non-empty remainder.
pub const OP_BP_FETCH_POST_JSON_BYTES_START: u32 = 0x113;
pub const OP_BP_DOBBY_UI4_WINDOWS: u32 = 0x114;
pub const OP_BP_DOBBY_UI4_FOCUS: u32 = 0x115;
pub const OP_BP_DOBBY_UI4_OBSERVE_PREPARE: u32 = 0x116;
pub const OP_BP_DOBBY_UI4_OBSERVE_METADATA: u32 = 0x117;
pub const OP_BP_DOBBY_UI4_OBSERVE_READ: u32 = 0x118;
pub const OP_BP_DOBBY_UI4_POINTER: u32 = 0x119;
pub const OP_BP_DOBBY_UI4_TYPE: u32 = 0x11A;
pub const OP_BP_DOBBY_UI4_KEY: u32 = 0x11B;
pub const OP_BP_UI4_SCENE_FRAME_OPEN_VISUAL: u32 = 0x11C;
pub const OP_BP_UI4_SCENE_SHADERTOY_RENDER: u32 = 0x11D;
pub const OP_BP_UI4_SCENE_VISUAL_FRAME_BEGIN: u32 = 0x11E;
pub const OP_BP_UI4_CONTEXT_MENU_REGISTER: u32 = 0x11F;
pub const OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE: u32 = 0x120;
pub const OP_BP_IMAGE_SOURCE_INFO: u32 = 0x121;
pub const OP_BP_IMAGE_SOURCE_READ: u32 = 0x122;
pub const OP_NET_TCP_WRITE: u32 = 0x10;
pub const OP_NET_TCP_READ: u32 = 0x11;
pub const OP_BP_NET_OPEN: u32 = 0x20;
pub const OP_BP_NET_SUBMIT: u32 = 0x21;
pub const OP_BP_NET_POLL: u32 = 0x22;
pub const OP_BP_FETCH_BYTES_START: u32 = 0x23;
pub const OP_BP_FETCH_BYTES_RESULT_LEN: u32 = 0x24;
pub const OP_BP_FETCH_BYTES_READ: u32 = 0x25;
pub const OP_BP_FETCH_BYTES_DISCARD: u32 = 0x26;
pub const OP_BP_FETCH_FILE_START: u32 = 0x27;
pub const OP_BP_FETCH_FILE_RESULT: u32 = 0x28;
pub const OP_BP_FETCH_FILE_DISCARD: u32 = 0x29;
pub const OP_BP_ENV_ARGS_COUNT: u32 = 0x2A;
pub const OP_BP_ENV_ARG: u32 = 0x2B;
pub const OP_BP_ENV_VAR: u32 = 0x2C;
pub const OP_BP_FS_READ_FILE: u32 = 0x2D;
pub const OP_BP_FS_WRITE_BEGIN: u32 = 0x2E;
pub const OP_BP_FS_WRITE_CHUNK: u32 = 0x2F;
pub const OP_BP_FS_WRITE_FINISH: u32 = 0x30;
pub const OP_BP_FS_WRITE_ABORT: u32 = 0x31;
pub const OP_BP_FS_EXISTS: u32 = 0x33;
pub const OP_BP_FS_REMOVE: u32 = 0x34;
pub const OP_BP_FS_STAT: u32 = 0x60;
pub const OP_BP_THREAD_CURRENT_ID: u32 = 0x61;
pub const OP_BP_TOKIO_BLOCKING_SPAWN: u32 = 0x62;
pub const OP_BP_LEGACY_FRAME_CREATE: u32 = 0x63;
pub const OP_BP_LEGACY_FRAME_OP: u32 = 0x64;
pub const OP_BP_GFX_TEXTURE_UPLOAD_BEGIN: u32 = 0x65;
pub const OP_BP_GFX_TEXTURE_UPLOAD_CHUNK: u32 = 0x66;
pub const OP_BP_GFX_TEXTURE_UPLOAD_FINISH: u32 = 0x67;
pub const OP_BP_GFX_TEXTURE_DIMENSIONS: u32 = 0x70;
pub const OP_BP_GFX_QUEUE_RENDER_RGB: u32 = 0x71;
pub const OP_BP_GFX_QUEUE_RENDER_TEX: u32 = 0x72;
pub const OP_BP_GFX_QUEUE_RENDER_MANDELBROT: u32 = 0x73;
pub const OP_BP_GFX_QUEUE_RENDER_BEGIN: u32 = 0x74;
pub const OP_BP_GFX_QUEUE_RENDER_CHUNK: u32 = 0x75;
pub const OP_BP_GFX_QUEUE_RENDER_FINISH: u32 = 0x76;
pub const OP_BP_GFX_TEXTURE_STATUS: u32 = 0x77;
pub const OP_BP_GFX_FRAME_BEGIN: u32 = 0x78;
pub const OP_BP_GFX_FRAME_SET_TARGET: u32 = 0x79;
pub const OP_BP_GFX_FRAME_STATE: u32 = 0x7A;
pub const OP_BP_GFX_FRAME_DRAW_BEGIN: u32 = 0x7B;
pub const OP_BP_GFX_FRAME_DRAW_CHUNK: u32 = 0x7C;
pub const OP_BP_GFX_FRAME_DRAW_FINISH: u32 = 0x7D;
pub const OP_BP_GFX_FRAME_END: u32 = 0x7E;
pub const OP_BP_LEGACY_FRAME_CURSOR_EVENTS: u32 = 0x7F;
pub const OP_BP_INPUT_CURSOR_POS: u32 = 0x68;
pub const OP_BP_INPUT_CURSOR_BUTTONS: u32 = 0x69;
pub const OP_BP_INPUT_CURSOR_EVENTS: u32 = 0x6A;
pub const OP_BP_DNS_RESOLVE_IPV4: u32 = 0x6B;
pub const OP_BP_SHELL_ATTACHED_WRITE: u32 = 0x6C;
pub const OP_BP_SHELL_ATTACHED_READ_BYTE: u32 = 0x6D;
pub const OP_BP_ENV_ALL: u32 = 0x6E;
pub const OP_BP_FS_LIST_TREE: u32 = 0x6F;
pub const OP_BP_SHELL_ATTACHED_READABLE_LEN: u32 = 0x70;
pub const OP_BP_FS_LIST_DIR: u32 = 0x81;
pub const OP_BP_SHELL_RAW_WRITE: u32 = 0x99;
pub const OP_BP_SHELL_KONSOLE_SIZE: u32 = 0x9F;
pub const OP_BP_EXIT_REASON: u32 = 0xA0;
pub const OP_BP_SHELL_KONSOLE_BEGIN_FRAME: u32 = 0xA1;
pub const OP_BP_SHUTDOWN: u32 = 0xA2;
pub const OP_BP_RETURN_TO_CLI: u32 = 0xA3;
pub const OP_BP_AUDIO_WRITE_I16_STEREO_48K: u32 = 0x9A;
pub const OP_BP_AUDIO_STOP: u32 = 0x9B;
pub const OP_BP_AUDIO_PENDING_FRAMES: u32 = 0x9C;
pub const OP_BP_AUDIO_SET_VOLUME_PERCENT: u32 = 0x9D;
pub const OP_BP_AUDIO_VOLUME_PERCENT: u32 = 0x9E;
pub const OP_BP_SOCKET_TCP_OPEN: u32 = 0x35;
pub const OP_BP_SOCKET_TCP_CLOSE: u32 = 0x36;
pub const OP_BP_SOCKET_TCP_SET_NONBLOCKING: u32 = 0x37;
pub const OP_BP_SOCKET_TCP_BIND_V4: u32 = 0x38;
pub const OP_BP_SOCKET_TCP_BIND_V6: u32 = 0x39;
pub const OP_BP_SOCKET_TCP_CONNECT_V4: u32 = 0x3A;
pub const OP_BP_SOCKET_TCP_CONNECT_V6: u32 = 0x3B;
pub const OP_BP_SOCKET_TCP_POLL_CONNECT: u32 = 0x3C;
pub const OP_BP_SOCKET_TCP_SEND: u32 = 0x3D;
pub const OP_BP_SOCKET_TCP_RECV: u32 = 0x3E;
pub const OP_BP_SOCKET_TCP_SHUTDOWN: u32 = 0x3F;
pub const OP_BP_SOCKET_TCP_TAKE_ERROR: u32 = 0x40;
pub const OP_BP_SOCKET_TCP_PEER_V4: u32 = 0x41;
pub const OP_BP_SOCKET_TCP_PEER_V6: u32 = 0x42;
pub const OP_BP_MIO_TCP_LISTENER_BIND: u32 = 0x50;
pub const OP_BP_MIO_TCP_STREAM_CONNECT: u32 = 0x51;
pub const OP_BP_MIO_UDP_SOCKET_BIND: u32 = 0x52;
pub const OP_BP_MIO_SOCKET_CLOSE: u32 = 0x53;
pub const OP_BP_MIO_SOCKET_LOCAL_ADDR: u32 = 0x54;
pub const OP_BP_MIO_SOCKET_PEER_ADDR: u32 = 0x55;
pub const OP_BP_MIO_SOCKET_TAKE_ERROR: u32 = 0x56;
pub const OP_BP_MIO_TCP_STREAM_READ: u32 = 0x57;
pub const OP_BP_MIO_TCP_STREAM_WRITE: u32 = 0x58;
pub const OP_BP_MIO_UDP_SOCKET_CONNECT: u32 = 0x59;
pub const OP_BP_MIO_UDP_SOCKET_SEND_TO: u32 = 0x5A;
pub const OP_BP_MIO_UDP_SOCKET_RECV_FROM: u32 = 0x5B;
pub const OP_BP_MIO_TCP_LISTENER_ACCEPT: u32 = 0x5C;
pub const OP_BP_MIO_SELECTOR_REGISTER_SOCKET: u32 = 0x5D;
pub const OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET: u32 = 0x5E;
pub const OP_BP_MIO_SELECTOR_POLL: u32 = 0x5F;
pub const OP_BP_MIO_SELECTOR_WAKE: u32 = 0x80;

pub const STATUS_OK: u32 = 0;
pub const STATUS_BAD_ARG: u32 = 2;
pub const COMM_PAGE_BYTES: usize = 160 * 1024;
pub const PAYLOAD_CAP: usize = COMM_PAGE_BYTES - 56;

/// Guest virtual address of the shared comm page.
const COMM_PAGE_VA: u64 = 0x0000_0000_2040_0000;

#[repr(C)]
struct CommPage {
    request_op: u32,
    request_seq: u32,
    request_arg0: u64,
    request_arg1: u64,
    request_len: u32,
    request_pad: u32,
    response_seq: u32,
    response_status: u32,
    response_data: u64,
    response_len: u32,
    response_pad: u32,
    payload: [u8; PAYLOAD_CAP],
}

static SEQ: AtomicU32 = AtomicU32::new(1);

#[inline(always)]
fn page() -> *mut CommPage {
    COMM_PAGE_VA as *mut CommPage
}

/// Serializes access to the single communication page shared by every Hull
/// vthread in a VM. `request_pad` is transport-private and starts at zero when
/// the host prepares the page.
struct CommPageGuard {
    lock: *const AtomicU32,
}

impl CommPageGuard {
    #[inline]
    fn acquire(page: *mut CommPage) -> Self {
        let lock = unsafe {
            AtomicU32::from_ptr(core::ptr::addr_of_mut!((*page).request_pad)) as *const AtomicU32
        };
        while unsafe { &*lock }
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Self { lock }
    }
}

impl Drop for CommPageGuard {
    #[inline]
    fn drop(&mut self) {
        unsafe { &*self.lock }.store(0, Ordering::Release);
    }
}

pub fn hull_bss_anchor() -> u64 {
    core::ptr::addr_of!(SEQ) as u64
}

pub fn hull_bss_anchor_range() -> (u64, u64) {
    let start = hull_bss_anchor();
    let end = start.saturating_add(core::mem::size_of::<AtomicU32>() as u64);
    (start, end)
}

/// Issue a vmcall and return (response_status, response_data).
/// Synchronous: host writes response before vmresume, so data is ready
/// on return.
pub fn call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) {
    let p = page();
    let guard = CommPageGuard::acquire(p);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    unsafe {
        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, 0);
        core::ptr::write_volatile(&mut (*p).request_seq, seq);
        // op written last — host treats this as the trigger
        core::ptr::write_volatile(&mut (*p).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
        // Yield/sleep requests are complete once the host has captured them.
        // The host releases the page before parking this vthread, and these
        // calls intentionally have no response value to preserve.
        if matches!(op, OP_YIELD | OP_SLEEP_MS) {
            core::mem::forget(guard);
            return (STATUS_OK, 0);
        }
        // vmcall is synchronous; response is ready on return
        let status = core::ptr::read_volatile(&(*p).response_status);
        let data = core::ptr::read_volatile(&(*p).response_data);
        (status, data)
    }
}

pub fn call_with_payload(op: u32, arg0: u64, arg1: u64, req: &[u8], out: &mut [u8]) -> (u32, u64) {
    let p = page();
    let _guard = CommPageGuard::acquire(p);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    unsafe {
        let req_n = core::cmp::min(req.len(), PAYLOAD_CAP);
        if req_n != 0 {
            (&mut (&mut (*p).payload)[..req_n]).copy_from_slice(&req[..req_n]);
        }

        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, req_n as u32);
        core::ptr::write_volatile(&mut (*p).request_seq, seq);
        core::ptr::write_volatile(&mut (*p).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));

        let status = core::ptr::read_volatile(&(*p).response_status);
        let data = core::ptr::read_volatile(&(*p).response_data);
        let resp_n = core::cmp::min(
            core::ptr::read_volatile(&(*p).response_len) as usize,
            core::cmp::min(out.len(), PAYLOAD_CAP),
        );
        if resp_n != 0 {
            out[..resp_n].copy_from_slice(&(&(*p).payload)[..resp_n]);
        }
        (status, data)
    }
}

pub fn ping() -> bool {
    let (s, d) = call(OP_PING, 0, 0);
    s == STATUS_OK && d == 0xCAFE_BABE
}

pub fn unix_time() -> u64 {
    let (_s, d) = call(OP_UNIX_TIME, 0, 0);
    d
}

pub fn monotonic_nanos() -> u64 {
    let (_s, d) = call(OP_MONOTONIC_NANOS, 0, 0);
    d
}

pub fn yield_now() {
    let _ = call(OP_YIELD, 0, 0);
}

pub fn sleep_ms(ms: u64) {
    let _ = call(OP_SLEEP_MS, ms, 0);
}

pub fn cpu_count() -> Option<usize> {
    let (status, count) = call(OP_BP_CPU_COUNT, 0, 0);
    if status == STATUS_OK {
        Some(count.max(1) as usize)
    } else {
        None
    }
}

pub fn net_tcp_write(bytes: &[u8]) -> usize {
    let mut total = 0usize;
    let mut out = [0u8; 1];
    while total < bytes.len() {
        let end = core::cmp::min(total + PAYLOAD_CAP, bytes.len());
        let (s, d) = call_with_payload(OP_NET_TCP_WRITE, 0, 0, &bytes[total..end], &mut out);
        if s != STATUS_OK {
            break;
        }
        let wrote = d as usize;
        if wrote == 0 {
            break;
        }
        total += wrote;
    }
    total
}

pub fn net_tcp_read(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let want = core::cmp::min(out.len(), PAYLOAD_CAP);
    let (s, d) = call_with_payload(OP_NET_TCP_READ, want as u64, 0, &[], &mut out[..want]);
    if s == STATUS_BAD_ARG {
        return 0;
    }
    if s != STATUS_OK {
        return 0;
    }
    let got = core::cmp::min(d as usize, want);
    got
}

/// Signal host to snapshot and stop executing the guest.
/// This is the final call; the guest halts after this.
#[inline(never)]
pub fn preserve() {
    let p = page();
    let _guard = CommPageGuard::acquire(p);
    unsafe {
        core::ptr::write_volatile(&mut (*p).request_len, 0);
        core::ptr::write_volatile(&mut (*p).request_seq, 0xFFFF_FFFF);
        core::ptr::write_volatile(&mut (*p).request_op, OP_PRESERVE);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
    }
}

/// Stop a completed Blueprint hull without creating a preserve snapshot.
///
/// The host consumes this VMCALL without resuming the guest. The fallback loop
/// is therefore only reachable if a host implementation incorrectly resumes a
/// terminal shutdown request.
#[inline(never)]
pub fn shutdown(reason: &[u8]) -> ! {
    let _ = call_with_payload(OP_BP_SHUTDOWN, 0, 0, reason, &mut []);
    loop {
        core::hint::spin_loop();
    }
}
