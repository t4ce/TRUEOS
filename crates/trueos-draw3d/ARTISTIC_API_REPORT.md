# TRUEOS draw3d artistic API report

Date: 2026-07-14

This report describes the API as it felt while authoring and iterating on the
`celestial-signal-garden` scene. It is deliberately written from a scene author's point of view,
not as a protocol implementation review. A behavior can be correctly documented and still be a
serious usability or rendering-correctness problem.

## Executive assessment

The protocol is pleasantly small, deterministic, and easy to drive. Scene replacement, camera
setup, mesh reuse, lifecycle control, statistics, and exact frame capture all worked on the first
attempt. A ten-mesh scene containing 1,306 vertices and 1,468 polygon faces remained live after
the client disconnected, and a later connection returned the same 512x512 PNG byte-for-byte.

The main problem is that the API looks like a small retained-mode 3D API, but overlapping geometry
currently behaves more like whole-object 2D painter layers. Every instance is assigned one average
depth and its entire triangle job is drawn at that position in the order. Consequently, changing
the contents of a mesh can change the apparent occlusion of otherwise unrelated geometry. This is
the largest gap between what the API shape suggests and how it feels to make art with it.

The later `null-meridian` iteration reproduced this in a particularly clear way: adding a small
high relay to an existing cyan mesh changed that mesh's average depth and collapsed most of the
previously stable frame. Moving the same relay into its own mesh restored the composition. The
scene is now left live with that split-mesh workaround.

A follow-up attempt to add two very small, high relay braces as another isolated mesh also
collapsed the frame at 12 instances. Removing that mesh restored the frame. The exact trigger is
not yet isolated between job ordering and the renderer's current scene complexity, but the result
is enough to classify tiny high-depth additions as unsafe for iterative art authoring without a
transactional update or explicit per-instance ordering.

The nested-lattice experiment found the same class of problem with a segmented loop: one mesh made
from many thin bars that crossed the core depth caused the cage and backdrop to disappear. The
stable lattice without that loop rendered correctly at ten meshes. This makes depth-spanning
procedural details particularly risky even when they are geometrically small.

Finally, changing only the opaque RGBA color of the core mesh—without changing its vertices,
faces, transform, instance count, or camera—also caused the surrounding cage to disappear. The
dark-core color restored the frame. This suggests that the current retained draw path can let
material updates disturb job ordering or presentation state; `set color` is therefore not safe to
treat as a purely visual, order-preserving edit.

The layered `signal-atlas` composition rendered reliably with eight constant-depth meshes. Editing
only the vertex outline of its deepest background plate then hid all seven foreground layers, even
though the edited mesh stayed at the same depth and within all limits. Reverting that outline
restored the image. This is further evidence that scene edits need an atomic rebuild/commit path
before the API can support iterative generative artwork safely.

The follow-up `flux-portrait` used 5,670 vertices across eleven material layers. Its first upload
was rejected with the compact vertex-limit status because one contour layer contained 2,304
vertices; splitting that same material into three resident meshes stayed within the per-mesh limit
and rendered successfully. The artistic workaround is straightforward, but the error response did
not identify which mesh exceeded the limit.

No framing, state-management, connection, lifecycle, or capture bug was found during this work.
The rendering-order issue below is a confirmed visual correctness problem for general 3D scenes,
even though the experimental limitation is already mentioned in `PROTOCOL.md`.

## What was exercised

The scene used:

- ten stored meshes and ten instances;
- disconnected components grouped by material color;
- boxes, octahedra, cylinders, frustums, toruses, prisms, and a custom crescent;
- opaque RGBA colors and a non-white opaque clear color;
- an oblique perspective camera;
- stop, clear, camera, put mesh, put instance, start, statistics, and render-image commands;
- 1,306 vertices, zero declared edges, 1,468 faces, and 39,736 estimated mesh bytes.

The final frame was captured twice through separate TCP connections. Both captures were PNG,
512x512, 9,038 bytes, with SHA-256:

```text
49f88253586461953727e359a5dabedfb9f68b60323ee34dd9834bb6c0abfa0a
```

This was a useful proof that the cached presented-frame behavior is deterministic and that the
live scene is not tied to the connection which created it.

## Findings

### 1. Whole-instance depth ordering makes 3D composition unstable

Classification: **rendering correctness defect / highest artistic friction**

Expected:

Geometry that is behind another surface should stay behind it. Adding vertices to a disconnected
part of a mesh, regrouping objects by material, or moving a separate background object should not
change the visibility of unrelated foreground surfaces.

Observed:

The renderer computes one depth for an entire instance by averaging the camera-space depth of its
visible vertices. It then sorts complete instance jobs back-to-front. There is no per-pixel depth
resolution between those jobs.

During the scene work, the first moon treatment used one gold moon mesh and one background-colored
occluder mesh. Moving the occluder nearer to guarantee that it covered the moon caused substantial
parts of the mountains and tower to disappear in the captured frame as well. Removing that
occluder and constructing the crescent from gold geometry restored those unrelated layers.

The workaround was artistically expensive: model every intended silhouette directly, avoid broad
occluders, split geometry according to expected depth as well as color, and repeatedly inspect a
hardware capture after small geometry changes.

Why it feels wrong:

- Mesh membership becomes a visual property even when the mesh's geometry does not overlap.
- Reusing one mesh for disconnected same-color details is efficient but changes its average depth.
- Splitting those details into separate meshes improves ordering but spends scarce mesh, instance,
  and draw-job budget.
- Authors cannot supply an explicit order when the automatic average is wrong.
- Normal 3D intuition is unreliable precisely in the complex scenes where a retained 3D API is
  most valuable.

Recommended resolution, in order of preference:

1. Use a depth buffer for opaque geometry and reserve sorting for blended geometry.
2. Until depth testing exists, expose an explicit signed `sort_key` or `layer` on each instance.
3. Document the exact current depth calculation, not only that jobs are ordered back-to-front.
4. Consider bounds-center depth or per-triangle sorting only as temporary mitigations; both still
   fail for intersecting or depth-spanning geometry.

### 2. One color per mesh couples material grouping to draw ordering

Classification: **API design problem**

Expected:

An instance should be able to reuse geometry with a different color, and a mesh should be able to
contain a modest number of differently colored parts without becoming many independently managed
objects.

Observed:

Color belongs to the mesh. `set color` changes every instance of that mesh. To draw the same shape
in two colors, the author must copy or recreate the mesh. To keep job count low, the demo grouped
many disconnected components into one mesh per color. That grouping then interacted directly with
the average-depth problem above.

Recommended resolution:

- Add an optional instance color override.
- Longer term, add material or color indices per face.
- Keep `set mesh color` as a useful default/fallback operation.

An instance color override would provide much of the immediate value without enlarging every
vertex or face.

### 3. `request render` does not request a render

Classification: **misnamed command**

Expected:

A command named `request render` normally means “render the current scene now and return or present
the result.”

Observed:

Opcode `0x23` returns the most recently presented cached target. It does not force the current scene
state through rendering, and before any presentation it can return the embedded fallback logo.

The actual behavior is useful and worked reliably, but its name encourages incorrect synchronization
assumptions in clients.

Recommended names:

- `capture presented frame`, or
- `get latest frame`.

If a synchronous render operation is later added, reserve `request render` or `render now` for that
operation. A reply should also identify whether its source is `live_scene_cache` or `fallback_logo`;
today the service log has that information, but the wire response does not.

### 4. Polygon `faces` accept more shapes than the renderer can represent correctly

Classification: **semantic footgun / partially misnamed data model**

Expected:

A face expressed as an ordered polygon is commonly expected to preserve its polygonal area, at
least for a simple planar polygon. If only convex or triangle-fan-compatible polygons are valid,
the API should say so at the validation boundary.

Observed:

Every face is triangulated as a fan from its first vertex. Concave polygons are accepted but can
produce triangles outside the intended polygon. The crescent therefore could not be sent as one
concave face; it had to be manually decomposed into twenty convex quad strips.

`PROTOCOL.md` does state that fan triangulation is used, so this is not an undocumented implementation
surprise. The problem is that the state API accepts a broader polygon vocabulary than the renderer
can faithfully interpret.

Recommended resolution:

- For the current renderer, call these `fan_faces` and explicitly require fan-compatible convex
  polygons, or reject unsupported concave input.
- Alternatively, make triangles the wire-level primitive and leave polygon tessellation to client
  libraries.
- If general polygons are desired, triangulate them with a real polygon tessellator and define how
  non-planar faces are treated.

### 5. `edges` and `set edges` imply a rendered primitive, but edges are state only

Classification: **misleading API surface / missing capability disclosure**

Expected:

Because edges are first-class mesh data, have their own replacement command, consume limits, and
appear in statistics, an author reasonably expects them to affect the image, for example as lines
or wireframes.

Observed:

The live renderer projects and submits faces only. Edges are validated, stored, counted, and
replaceable, but they do not produce visible geometry. The scene client therefore sent zero edges.

Recommended resolution:

- Explicitly label edges as non-rendered topology metadata in the protocol document, or
- advertise an `edge_rendering = false` capability, or
- add a line-rendering mode and define width, clipping, depth, and color behavior.

Until lines exist, a client-facing helper should not suggest that `set edges` is a drawing command.

### 6. RGBA implies compositing that the renderer does not yet provide

Classification: **capability mismatch, already documented**

Expected:

RGBA mesh colors usually imply that partially transparent surfaces can overlap opaque scene
geometry and be composited in a defined way.

Observed:

Alpha is preserved through the target, capture, and final display composition, but overlapping
scene meshes are not blended with each other. This is clearly documented and was avoided in the
final scene. An earlier bridge demo's translucent river is only safe because it is kept behind the
other geometry.

Recommended resolution:

- Until blending exists, describe the field as `output_rgba` and state near the command table that
  mesh-to-mesh alpha compositing is unsupported.
- Expose a compositing capability enum rather than making every client rediscover the experimental
  limitation from prose.
- When blending is added, define straight versus premultiplied input and how transparent jobs are
  sorted relative to opaque jobs.

### 7. There is no capability query

Classification: **API omission**

Expected:

A client should be able to discover the live target size, active scene limits, supported primitive
types, polygon behavior, compositing mode, and capture formats from the service it connected to.

Observed:

Protocol version and statistics are available, but operational capabilities are only in the
document. Important facts include the fixed 512x512 target, mesh and instance limits, per-mesh
triangle limits, face fan behavior, no rendered edges, no inter-mesh blending, and near/far triangle
suppression.

Recommended resolution:

Add `get capabilities`, returning at least:

- target width and height;
- mesh, instance, vertex, edge, face, and post-triangulation limits;
- supported capture formats;
- primitive rendering flags for faces and edges;
- face tessellation mode;
- depth/compositing mode;
- clipping mode;
- supported optional commands or feature bits.

This would let a client adapt instead of depending on an exactly matching external document.

### 8. Scene replacement is safe, but live multi-command edits are not transactional

Classification: **workflow limitation**

Expected:

An artist should be able to submit a related set of mesh, instance, transform, and camera changes
as one visual revision.

Observed:

For complete replacement, `stop -> clear -> upload -> start` worked well and avoided exposing a
partially constructed scene. While a scene is running, commands are coalesced on the render cadence,
but there is no explicit transaction boundary. A large logical update can therefore be presented
in intermediate states depending on timing.

Recommended resolution:

- Add `begin update` and `commit update`, or a batch command containing nested operations.
- Apply and validate the batch atomically, then advance one scene revision.
- Keep the current individual commands for low-latency interactive edits.

### 9. Compact error statuses are machine-friendly but weak for iteration

Classification: **developer-experience limitation**

Expected:

When a scene upload fails, a client should be able to report which mesh, instance, face, index, or
limit caused the rejection.

Observed:

The compact status identifies the category but carries no structured context. This was not a blocker
for the final scene because every upload succeeded, but it would make procedural-mesh debugging
slower than necessary.

Recommended resolution:

Keep the status byte, then optionally append structured details such as object ID, offending index,
received count, and permitted limit. Older clients can ignore the extra error payload if framing
rules permit it in the next protocol version.

### 10. The current 3D camera has no lighting or volumetric cue

Classification: **artistic capability gap**

Expected:

An oblique camera and geometry at different depths should provide enough visual information to
compose a convincing spatial scene, even when the author supplies simple flat materials.

Observed:

The strongest results in this iteration came from treating draw3d as a retained 2.5D compositor:
front-facing layered sheets, deliberate negative space, and manual color bands. Applying an oblique
camera to the same layered artwork produced a tilted card rather than convincing volume, because
there is no lighting, normal-based shading, shadowing, or per-pixel depth cue. The change was
artistically worse despite using a perspective camera.

Recommended resolution:

- Add a minimal opaque depth buffer and a flat-light or normal-based material mode.
- Expose a background/ambient light and one directional light before adding more primitive types.
- Keep the current flat RGBA path as an explicit `unlit_2d` or `unlit_layer` mode; it is useful for
  graphic compositions when named honestly.

## What felt good

Several parts of the API should be preserved:

- The frame format is trivial to implement correctly in a small client.
- Request IDs and explicit response opcodes make pipelining understandable.
- Stable numeric mesh and instance IDs make procedural scene generation convenient.
- Separating meshes from transforms allows geometry reuse.
- Stop, clear, upload, and start provide a reliable full-scene replacement workflow.
- Camera validation and transform order are unusually explicit for such a small protocol.
- The clear color, straight alpha target, and PNG capture agree.
- Statistics are immediately useful for checking artistic budget.
- The presented-frame cache survives client disconnects and produced byte-identical captures.
- Ten persistent draw jobs rendered successfully on the tested rig.

The low-level wire format itself did not feel overly difficult. Most client complexity came from
generating primitives and working around visual ordering, both of which could be hidden by a small
official client library without changing the protocol.

## Suggested priority

1. **P1:** Add real opaque depth testing, or expose an explicit instance sort key as an interim
   control.
2. **P1:** Add a capability query so clients can discover the renderer's actual semantic contract.
3. **P2:** Add instance color overrides to decouple reuse, material grouping, and draw ordering.
4. **P2:** Rename `request render` to reflect cached-frame capture behavior.
5. **P2:** Constrain or rename polygon faces so invalid concave expectations fail early.
6. **P2:** Mark edges as non-rendered, or add an actual line-rendering capability.
7. **P3:** Add atomic update batches and structured error detail.

## Minimal API evolution sketch

A small compatible evolution could provide most of the immediate usability improvement:

```text
get_capabilities -> target, limits, topology flags, tessellation mode,
                    depth/compositing mode, capture formats
set_instance_color(instance_id, RGBA | inherit)
set_instance_sort_key(instance_id, i32 | automatic)
capture_presented_frame
begin_update
commit_update
```

The sort key is not a substitute for depth testing, but it would make the current experimental
renderer intentionally controllable instead of making authors manipulate mesh composition to
influence an implicit average.

## Bottom line

The service already works well as a deterministic geometric display experiment and as a retained
scene store. It does not yet feel dependable as a general 3D art API because object membership,
material grouping, and disconnected geometry can unexpectedly change visual occlusion. Fixing or
exposing control over depth order would improve artistic usability more than adding new primitive
types. After that, capability discovery and clearer names would make the existing limitations feel
intentional rather than surprising.
