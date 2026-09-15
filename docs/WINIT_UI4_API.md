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

UI4 has no raw native OpenGL window/display handle. Winit returns
`HandleError::NotSupported` for these requests; it never masquerades as an
X11 or Wayland window. A glutin/OpenGL bridge is still required for Alacritty.

The standalone wgpu workspace is pinned to the GitHub winit fork's compatible
0.30.13 revision, `e9809ef54b18499bb4f2cac945719ecc2a61061b`. That pin does not
include this new 0.31 backend. Current Alacritty also uses the 0.30 API; a
backport or application migration is needed before connecting it to this
backend.

## Validation

Backend tests run from outside the Blueprint workspace to avoid inheriting
its Cargo target/build-std configuration:

```sh
cd /tmp
cargo test --manifest-path /home/t4ce/Repos/TRUEOS-Blueprints/vendor/winit/Cargo.toml -p winit-trueos
```

The kernel changes are checked with the repository's TRUEOS target and pinned
compiler. The source keyboard module also has host tests covering transitions,
modifier-only input, rollover, and disconnect recovery. No hardware deployment
or runtime window test is implied by these compile/unit checks.
