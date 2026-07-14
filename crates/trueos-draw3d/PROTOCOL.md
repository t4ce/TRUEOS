# TRUEOS 3D draw protocol v1

The 3D draw service listens on TCP port 4246.

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
| `20` | get statistics | empty |
| `21` | ping | nonce (`u64`) |
| `22` | set view camera | position, view direction, up axis, near, far, vertical FOV |
| `23` | request render | empty |

Arrays start with a `u32` count. An edge is two `u32` vertex indices. Mesh deletion fails while
instances reference it unless `cascade` is `1`; cascading also deletes those instances. Put
operations replace an existing object with the same ID. Copy requires an unused target ID.

The camera payload is 48 bytes: three `Vec3` values followed by near plane, far plane, and
vertical FOV as `f32`. FOV is in radians. Near must be positive, far must be greater than near,
FOV must be between zero and pi, and the view/up vectors must be nonzero and nonparallel.

## Replies

The first payload byte is status (`0` for success; nonzero values are compact error codes).
The second success byte selects the body: applied=`0`, stats=`1`, pong=`2`, render image=`3`.
Applied replies contain affected count and current scene statistics. Stats contain mesh and
instance counts, vertex/edge/face totals, and estimated mesh bytes. Pong contains the original
nonce. A render image contains format (`1` JPEG, `2` PNG), width (`u32`), height (`u32`), then
the encoded image bytes through the end of the frame. The current placeholder response is the
kernel's embedded 3840x2160 `logo.jpg`. This remains intentionally separate from the live scene
preview while render-image transport is being proven.

## Experimental live renderer

The service owns the Intel triangle engine for this experiment. Each placed instance is projected
through the configured camera and fan-triangulated into a persistent indexed GPU job. Geometry is
uploaded on creation, updated in place after geometry/camera/transform changes, and reused on steady
frames. RGBA is volatile shader state, so `set color` does not upload geometry. Jobs are ordered
back-to-front into one 512x512 target and presented on a 33 ms cadence (about 30 FPS). Triangles
which cross the near or far plane are currently suppressed; X/Y clipping remains GPU-owned.
Deleted jobs unmap their pages and return their persistent GPU virtual-address range for reuse.

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
direction, zero up axis, and parallel view/up axes.
