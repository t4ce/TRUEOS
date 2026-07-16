# TRUEOS 3D draw protocol v1

The 3D draw service listens on TCP port 4246. Ip of the testrig is:
192.168.178.94

The wire format is little-endian and intentionally contains no names, JSON, alignment padding,
or checksum. TCP supplies ordering and integrity. A client may pipeline requests; `request_id`
correlates each reply.

## Frame

| Offset | Size | Value |
| ---: | ---: | --- |
| 0 | 2 | ASCII `D3` |
| 2 | 1 | protocol version (`1`) |
| 3 | 1 | opcode; replies set bit `0x80` |
| 4 | 4 | request ID (`u32`) |
| 8 | 4 | payload byte length (`u32`) |
| 12 | N | opcode payload |

IDs are `u64`, counts and vertex indices are `u32`, vector components are IEEE-754 `f32`,
and RGBA is four bytes. A `Vec3` is 12 bytes. A transform is location, Euler rotation in
radians, then scale (36 bytes total). The renderer applies scale, then intrinsic X/Y/Z rotation,
then translation. Faces preserve polygons: `u16 vertex_count` followed by that many `u32` indices.

## Commands

| Opcode | Command | Payload |
| ---: | --- | --- |
| `01` | put/replace mesh | mesh ID, RGBA, vertices, edges, faces |
| `02` | delete mesh | mesh ID, cascade byte |
| `03` | copy mesh | source ID, target ID |
| `04` | set vertices | mesh ID, vertex array |
| `05` | set edges | mesh ID, edge array |
| `06` | set faces | mesh ID, face array |
| `07` | set color | mesh ID, RGBA |
| `10` | put/replace instance | instance ID, mesh ID, transform |
| `11` | delete instance | instance ID |
| `12` | copy instance | source ID, target ID |
| `13` | retarget instance | instance ID, mesh ID |
| `14` | set transform | instance ID, transform |
| `15` | set location | instance ID, Vec3 |
| `16` | set rotation | instance ID, Vec3 |
| `17` | set scale | instance ID, Vec3 |
| `18` | clear scene and mesh store | empty |
| `19` | start live scene rendering | empty, or RGBA clear color |
| `1a` | stop live scene rendering | empty, or permanent boolean |
| `20` | get statistics | empty |
| `21` | ping | nonce (`u64`) |
| `22` | set view camera | legacy camera, optionally followed by look-at orbit |
| `23` | request render | empty |

Arrays start with a `u32` count. An edge is two `u32` vertex indices. Mesh deletion fails while
instances reference it unless `cascade` is `1`; cascading also deletes those instances. Put
operations replace an existing object with the same ID. Copy requires an unused target ID.

The original static camera payload remains 48 bytes: three `Vec3` values (position, view direction,
and up axis) followed by near plane, far plane, and vertical FOV as `f32`. FOV is in radians. Near
must be positive, far must be greater than near, FOV must be between zero and pi, and the view/up
vectors must be nonzero and nonparallel. Sending this original payload also disables any previously
configured orbit, so existing clients retain exactly the static behavior they had before.

An optional 36-byte tail turns the command into a look-at orbit. The tail is look-at `Vec3`, XYZ
Euler rotation `Vec3`, X radius `f32`, Z radius `f32`, then angular speed `f32`. Its unrotated
ellipse is:

```text
look_at + (cos(angle) * x_radius, 0, sin(angle) * z_radius)
angle = angular_speed * seconds_since_camera_command
```

The Euler rotation tilts the ellipse and its up axis with the same intrinsic X/Y/Z convention used
by instance transforms. Rotation is in radians and speed is in radians per second; negative speed
reverses direction. Both radii must be finite and greater than zero. A speed of zero evaluates the
phase-zero position once and remains static, while retaining the explicit protocol-level look-at.
The legacy camera's clipping planes and FOV are retained for an orbit; its position, direction, and
up axis are replaced by the evaluated look-at camera.

The start-scene payload is either empty for a transparent scene background or exactly four RGBA
bytes. The four-byte form is backward-compatible with the original empty command and avoids a
separate option tag. Repeating start while running is idempotent unless it supplies a different
clear color; stopping and then starting empty restores the transparent background.

The stop-scene payload is empty for the original resumable pause, or one boolean byte. Omitting the
byte or sending `0` records the stopped state while retaining the complete scene. Sending `1`
permanently stops and discards meshes and instances, resets camera/orbit/clear
state, and invalidates the cached scene screenshot. A later start creates a fresh empty run; the
permanent option does not lock out future scenes.

## Replies

The first payload byte is status (`0` for success; nonzero values are compact error codes).
The second success byte selects the body: applied=`0`, stats=`1`, pong=`2`, render image=`3`.
Applied replies contain affected count and current scene statistics. Stats contain mesh and
instance counts, vertex/edge/face totals, and estimated mesh bytes. Pong contains the original
nonce. A render image contains format (`1` JPEG, `2` PNG), width (`u32`), height (`u32`), then
the encoded image bytes through the end of the frame. The service provides a straight-alpha RGBA8
capture of the Draw3D scene camera. A
render request produces a fresh off-screen capture, including while an ordinarily stopped scene is
retained, or returns the last complete capture if a fresh attempt fails. It is deliberately not a
readback of a composed UI output or the physical scanout. Before any frame has been captured,
after permanent reset, or if no PNG has been
captured successfully, the response falls back to the kernel's embedded 3840x2160 `logo.jpg`.

## Off-screen renderer

The service retains the TCP scene model but owns no UI plane and never changes scanout. A render
request projects the current camera, creates temporary resident Intel triangle jobs, captures the
result into a straight-alpha RGBA8 surface, and releases those jobs after readback. UI4 therefore
has no implicit Draw3D producer.

`start scene` and ordinary `stop scene` keep their wire-compatible scene-state semantics; neither
starts or stops a presentation loop. A permanent stop discards meshes, instances, camera/orbit
state, clear color, and the cached capture. Orbit is sampled when a render request is handled.

The capture target's width divided by height supplies the default camera aspect ratio. The
experimental target does not yet blend overlapping transparent meshes with each other, and
triangles crossing the near or far plane are suppressed; X/Y clipping remains GPU-owned.

The active scene budget is 100 stored meshes, 100 placed instances, 1,000 vertices per mesh,
3,000 edges per mesh, and 2,000 triangles per mesh after polygon fan triangulation. These are
service-level limits; the larger wire decoder ceilings below only protect framing and allocation.

The service accepts payloads up to 128 MiB, with per-command safety ceilings of 16,777,216
vertices, 16,777,216 edges, and 4,194,304 faces. These limits are checked before allocation.

Error status codes `1..14` are framing/schema errors in this order: buffer limit, bad magic,
unsupported version, unknown opcode, payload too large, truncated payload, trailing bytes,
invalid boolean, count overflow, unexpected response, unexpected request, invalid response,
unknown error code, and collection limit. Codes `32..44` are state errors: missing mesh,
missing instance, target exists, mesh in use, mesh limit, instance limit, vertex limit, edge
limit, face limit, per-face vertex limit, face too small, vertex index out of range, and a
non-finite vector.
Camera state errors continue at `45..49`: invalid clipping planes, invalid FOV, zero view
direction, zero up axis, and parallel view/up axes. Code `50` is an invalid orbit scale.
