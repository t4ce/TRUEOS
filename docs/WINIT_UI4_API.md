# Winit and the UI4 application API

The vendored `TRUEOS-Blueprints/vendor/winit` workspace now has a
`winit-trueos` backend for the 0.31 API. Windows are owned UI4 visual frames;
the host derives the owner from the calling Blueprint. A caller cannot supply
another VM's identity. `WindowExtTrueOS::trueos_window_id()` exposes the frame
identifier for a renderer using the existing UI4/vGPU APIs.

## Additive ABI

The following symbols extend `crates/trueos-v/src/bp_abi.rs`. Existing function
signatures and the legacy keyboard stream remain unchanged. The matching
Blueprint SDK declarations must travel with the host change. The portal CABI
lock was updated after checking that all previous canonical signatures remain
identical.

| Symbol suffix after `trueos_cabi_ui4_scene_` | Contract |
| --- | --- |
| `keyboard_event_take_v1(window, out)` | Returns 0 with one keyboard record, 1 when empty, a negative error on failure. |
| `window_state_get_v1(window, out)` | Returns the broker's 32-byte versioned state. |
| `window_state_set_v1(window, state)` | Atomically applies visibility, hit testing, and opacity. |
| `window_title_get_v1(window, out, capacity)` | Copies UTF-8 metadata and returns its byte length; capacity must be at least 120. |
| `window_title_set_v1(window, bytes, length)` | Stores up to 120 valid UTF-8 bytes, rejecting longer or invalid input. |

State fields are `version`, `visible`, `hit_testable`, `opacity`, `focused`,
and three reserved `u32`s. Setters require version 1, Boolean visibility and
hit-test values, opacity 0–255, and zero reserved fields. `focused` is a query
result, not a request to take another application's focus. Title is broker
metadata; this does not add a title-bar renderer.

VMCALL 0x209 polls the keyboard stream. 0x20A gets/sets state and 0x20B
gets/sets title; for the latter two, arg0 is the window ID and arg1 is 0 for
get or 1 for set. Set requests carry the record/title as a bounded payload.
The host validates ownership again after decoding the payload.

## Native display and window handles

The vendored `raw-window-handle` crate exposes `TrueosDisplayHandle` with a
nonzero `u64` connection token and `TrueosWindowHandle` with a nonzero `u32`
UI4 scene frame ID. These identify UI4 resources; neither is a framebuffer
address or an OpenGL context.

| Symbol suffix after `trueos_cabi_ui4_display_` | Contract |
| --- | --- |
| `open_v1()` | Opens or retains the caller's connection; returns its token, or zero on failure. |
| `retain_v1(connection)` | Adds a reference to a live connection owned by the caller. |
| `close_v1(connection)` | Drops one reference; the last reference invalidates the token. |
| `validate_window_v1(connection, window)` | Checks that both the connection and scene frame are live and owned by the caller. |

The last three operations return zero on success and a negative error on
failure. VMCALLs 0x20C–0x20F implement these operations in table order, with
the connection in arg0 and the window in arg1 for validation. Requests have
no payload; unused arguments must be zero.

Tokens contain a registry slot and generation. Closing the final reference
or tearing down the owner revokes the token; reusing a slot does not revive
an old token. Winit shares a connection through `Arc` between the event loop,
windows, and owned display handles, and closes it on the final drop. It
validates the scene frame before exposing a raw window handle. A renderer
that needs an independently owned connection must retain and later close it.

## Keyboard transitions

The new stream reuses the 44-byte `TrueosKeyboardOutputEvent` record. Kind 4
means a physical transition, and `key_code` is a USB HID keyboard-page usage,
including modifier usages 0xE0–0xE7. Flag bit 0 denotes press; clearing it
denotes release. Modifier bits retain the HID report layout. Kind 1/2 synthetic
committed input retains the existing text/named-key semantics; these codes
must never be mistaken for physical HID usages. Kind 3 resets held state.

Physical events use a separate source ring and owner queue, so they do not
double the event volume in the legacy text path. Presses capture their routed
window. Releases use that captured destination even after selection changes
or the window becomes hidden. Owner teardown retires captures. Device removal
emits synthetic releases, and HID rollover/error reports do not manufacture
releases for keys that may still be down.

Bounded source-ring overflow releases captured keys. Owner-queue overflow
resets that owner's physical input state, and per-window queue overflow emits
a reset marker before later events. These recovery paths prefer an explicit
loss of held state to leaving a key stuck. Global shortcut dispositions from
the logical stream are reused rather than executing the shortcut twice.

The backend preserves physical identities and remembers the logical key from
the press when delivering its release. It does not guess an unmodified layout
character when the host has not supplied one. Automatic key repetition and
composition/IME are not added by this change.

## Rendering and version boundaries

Winit exposes native TRUEOS handles through the patched `raw-window-handle`
crate. Consumers must use that same crate version/source to share the new
enum variants. Context creation, framebuffer leases, GL procedure resolution,
and presentation belong to the glutin/trueos-gl integration; raw handles alone
do not implement those operations.

Glutin links the local `glutin/trueos-gl` crate. Procedure resolution returns
Blueprint-side code pointers; there is no unresolved `trueos_gl_get_proc_address`
import and no attempt to call a kernel function pointer from the VM. The
library maintains per-context buffer, atlas texture, vertex-array, uniform,
blend, viewport, and clear state for the GLES2Pure entry points.

Glutin retains its display connection, validates window ownership, and binds
the GL context to a leased UI4 surface. Clear-frame swaps consume that lease
through the existing vGPU clear submission, wait for its timeline point, and
acquire the next lease. Unbinding retires unused leases; resize discards the
old lease, resizes the UI4 frame, and reacquires it. Failed submissions remain
errors and do not recycle an ambiguously submitted frame. Context switches
cannot move pending commands into another window.

Native AOT draw execution is still absent. A text or rectangle draw prevents
swap from publishing a misleading clear-only frame: swap reports unsupported.
An empty swap and a flush/finish with queued commands are also unsupported.
This is a clear-frame integration checkpoint, not a complete GLES implementation
or evidence that terminal text renders. The Bakery modules and corresponding
native draw submission remain necessary.

The standalone wgpu workspace is pinned to the GitHub winit fork's compatible
0.30.13 revision, `e9809ef54b18499bb4f2cac945719ecc2a61061b`. That pin does not
include this new 0.31 backend. Alacritty now selects the local 0.31 winit
workspace and patches glutin and raw-window-handle to their local forks.

Alacritty's `res/trueos/bakery-input.json` specifies seven shader stages and
five linked programs for `Gles2Pure`. The checked-in directory contains the
input contract, not the baked native packages. Text draws also use several
blend configurations; five linked shader programs do not imply only five
complete GPU pipeline states.

## Validation

Backend tests run from outside the Blueprint workspace to avoid inheriting
its Cargo target/build-std configuration:

```sh
cd /tmp
cargo test --manifest-path /home/t4ce/Repos/TRUEOS-Blueprints/vendor/winit/Cargo.toml -p winit-trueos
```

The kernel changes are checked with the repository's TRUEOS target and pinned
compiler. The source keyboard module also has host tests covering transitions,
modifier-only input, rollover, and disconnect recovery. Run
`python3 tools/testpy/test_ui4_display_registry.py` for the actual display registry's
ownership, reference lifetime, stale-token, and teardown tests; `RUSTC` can
select the compiler. No hardware deployment or runtime window test is implied
by these compile/unit checks.

Glutin's TRUEOS backend tests run its real public API and GL procedure pointers
against a recording vGPU/UI4 ABI double. From the glutin checkout:

```sh
RUSTFLAGS='--cfg trueos_backend' cargo test -p glutin --lib \
  --no-default-features --features trueos --target x86_64-unknown-linux-gnu \
  --config 'patch.crates-io.raw-window-handle.path="../TRUEOS-Blueprints/vendor/raw-window-handle"'
```

The TRUEOS target check additionally needs the archived compiler, the Blueprint
application target, `build-std`, and the vendored libc overlay. Host tests check
submission ordering and resource ownership; only a hardware run can verify
visible presentation. The Blueprint builder automatically publishes successful
builds to its configured apps share. Set
`TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1` for a local-only Blueprint build.
