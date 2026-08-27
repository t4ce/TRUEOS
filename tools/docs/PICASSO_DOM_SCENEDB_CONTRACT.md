# Solara–Picasso First-Stage Display Contract

Status: first-stage architecture contract

Audience: Solara, Picasso, UI4, SURFLIVE, and scanout implementers

This document freezes the durable first-stage path from a live Solara document
to TRUEOS scanout. Solara is the Rust web browser with its QuickJS runtime; it
presents through Picasso, the data-driven renderer, which is backed by the
custom TRUEOS driver architecture. The contract deliberately freezes ownership,
identity, publication, ordering, trust, and frame-lifetime rules before it
freezes a packed byte ABI or a particular Xe-LP kernel schedule.

The product-level chain is intentionally short:

~~~text
Solara (Rust browser + QuickJS)
        |
        | browser-owned DOM, cascade, layout, and paint data
        v
Picasso (data-driven rendering)
        |
        | exact released UI4 frame
        v
UI4
        |
        v
SURFLIVE
        |
        v
scanout
~~~

SceneDB and Helio/HelioV are retained implementation vocabulary inside the
Solara-to-Picasso handoff; they are not additional product-level stages in this
first-stage chain. The DOM owns the scene semantically. Its SceneDB shadow is
the committed visual input to Picasso, not a second DOM, a widget hierarchy, a
CSS object model, or a renderer command stream.

## 1. Normative language

MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY describe the intended permanent
contract. A section explicitly marked first wave or implementation note is
allowed to describe a temporary implementation without weakening the permanent
contract.

## 2. The permanent ownership split

| Part | Owns | Must not own |
| --- | --- | --- |
| Solara (Rust and QuickJS) | nodes, attributes, JS objects, event listeners, document state, CSS inputs, browser semantics | frame buffers, GPU addresses, residency slots |
| Solara scene compiler | cascade, layout, fragmentation, CSS paint-order compilation, DOM-to-fragment mapping, the only authored SceneDB transaction | presentation leases, GPU scheduling |
| Picasso scene input (currently the SceneDB shadow) | the last atomically committed visual facts and hit-test facts | DOM semantics, layout constraints, backend worklists |
| Picasso renderer backend (currently Helio/HelioV and Bakery) | read-only derivation of world transforms, visibility, clips, damage, tile bins, effect passes, indirect work | mutation of authored scene rows |
| Picasso | validated resource resolution and deterministic rendering into a leased UI4 target | DOM traversal, CSS decisions, public GPU pointers |
| UI4 | viewport and input brokerage, write/read leases, publication cadence, replacement ownership | document semantics |
| SURFLIVE and scanout | the final display/scanout lifetime | scene mutation or rendering policy |

There is one semantic writer for a document: its CPU scene compiler. JavaScript
may cause arbitrary mutations, and async Rust producers may finish work in any
order, but neither fact creates another SceneDB writer.

The renderer is free to cache and derive aggressively. Those derived products
are disposable and can always be rebuilt from a committed snapshot plus its
referenced resources.

## 3. What the SceneDB shadow means

A committed shadow answers only the visual questions needed below Solara:

- what visual fragments exist;
- how those fragments are spatially related;
- how clipping and compositing groups are nested;
- the exact painter order;
- which immutable or revisioned resources they reference;
- which regions can participate in hit testing;
- which authored rows changed at this commit.

It does not answer why a fragment exists. Tags, attributes, selector text,
computed-style objects, JS wrappers, event listeners, form state, accessibility
objects, layout constraints, and the DOM parent tree stay above this seam.

The shadow is retained because stable rows make incremental changes cheap. It is
a snapshot because consumers observe it only at coherent commit boundaries.
Both descriptions are true at once.

Placement does not change ownership. A native Solara process may publish the
logical database directly. A VM-contained Solara may build it on its side of
the trust boundary and send the pointer-free transaction for host validation
and copying. The accepted host copy represents the same logical commit; it is
not a second semantic scene model.

An optional CPU-side abstraction may call a collection of fragments a widget,
but widget identity and behavior are not part of the Picasso contract. The
permanent ABI consists of visual facts, not a toolkit object model.

Spatial, clip, and paint-group rows are created only for independently
render-relevant coordinate, clipping, ordering, or effect facts. There is no
one-row-per-DOM-node rule or expectation. These compact visual graphs must not
be allowed to grow into a mirrored semantic tree.

## 4. Identity and lifetime

Six identity domains must remain distinct.

~~~text
PicassoSceneId {
    slot
    generation
}

DomRef {
    realm
    slot
    generation
}

SceneRef<TableKind> {
    slot
    generation
}

SpatialRef   = SceneRef<Spatial>
ClipRef      = SceneRef<Clip>
GroupRef     = SceneRef<PaintGroup>
PrimitiveRef = SceneRef<Primitive>
HitRef       = SceneRef<Hit>
ChildBindingRef = SceneRef<ChildBinding>

FragmentKey {
    anchor: DomRef
    formatting_scope
    stable_fragment_id
    fragment_generation
}

FragmentAttributes {
    pseudo_kind
    formatting_box_kind
    paint_role
    continuation_index
}

ResourceRef {
    owner_scope
    slot
    generation
}

ChildAttachmentRef {
    host_capability_slot
    generation
}

PayloadRef {
    arena_kind
    slot
    generation
}
~~~

PicassoSceneId identifies one document scene within an authenticated VM or
process owner. All DomRef, typed SceneRef, ResourceRef, commit_epoch, and
transport state is scoped to it. Its generation changes after teardown or
reset, so a delayed message cannot address a newly created scene that reused the
same slot.

DomRef identifies a live semantic node. One DOM node may produce zero, one, or
many paint fragments.

FragmentKey is a compiler-maintained identity, not a tuple of current tree
position or continuation ordinal. The compiler preserves stable_fragment_id
while it can establish that a formatting object or continuation survived a
reflow. If a formatting scope is rebuilt without a valid correspondence, it
retires the old fragment generation and creates new keys atomically.
pseudo kind, paint role, and continuation index are mutable attributes rather
than identity.

Pseudo-elements and anonymous formatting boxes anchor to the nearest semantic
DomRef but receive distinct compiler-issued fragment IDs. Compiler-only
formatting records may represent anonymous ancestry; they are never fake
DomRefs or semantic nodes.

SceneRef<TableKind> identifies one logical authored record in one typed SceneDB
component table. The table type is part of the identity domain; a spatial row
and a primitive row with the same numeric slot are unrelated. It must not be
assumed to be an entity ID, a Helio component-row index, or an index into a
GPU-private projection. An adapter resolves it through the relevant
entity/component or per-table allocator.

While one logical component remains present, ordinary value changes and
reordering retain its typed SceneRef and authored slot. Removing that component
retires the row. Only renderer-private projections may compact or reorder rows;
they cannot silently change the authored handle.

ResourceRef identifies authored resource metadata. It never denotes a physical
atlas slot, a GPU virtual address, a UI4 surface, or a frame-pool handle.

ChildAttachmentRef is a separate host-issued authorization capability. It is
not a ResourceRef, has no ResourceRow or content_revision semantics, and is
never dereferenced by guest code. A parent transaction may retain the stable
capability while each host composition receipt pins one exact child frame and
FrameSceneSet.

PayloadRef identifies an entry in one typed variable-size arena. The arena kind
is part of the identity, so path data cannot be reinterpreted as glyph instances
or filter data.

All Picasso wire handles use slot plus generation and reserve generation zero
as invalid. This is a wire rule, not a bit-compatible claim about existing
hosted SceneDB entities, whose initial generation may be zero. Adapters
translate and validate explicitly. A freed Picasso slot increments its
generation before reuse. If a generation would exhaust its representable
range, that slot is retired permanently rather than wrapping and making an
ancient handle valid again.

DomRef realm values are scoped beneath an authenticated VM or process owner.
Wire ownership is derived from that context. A guest cannot claim another owner
merely by placing an owner or realm number in a record.

CPU-side DomRef-to-FragmentKey and FragmentKey-to-SceneRef maps belong to the
scene compiler. They are not uploaded unless a renderer operation explicitly
needs the corresponding visual or hit-test fact.

## 5. The logical snapshot

The following is a logical schema. It freezes meaning and relationships, not
field offsets, AoS versus SoA layout, or the eventual public enum numbers.

### 5.1 Snapshot header

~~~text
PicassoSnapshot {
    format_version
    scene_id: PicassoSceneId
    commit_epoch
    dom_revision
    layout_viewport_css
    visual_viewport_css_offset
    visual_viewport_scale
    visual_viewport_clip
    target_pixel_width
    target_pixel_height
    device_scale
    working_color_space
    root_spatial: SpatialRef
    document_root_group: GroupRef
    top_layer_group: optional GroupRef
    component_publications
    resource_publication
    damage_hint
}
~~~

commit_epoch is monotonic and nonwrapping within one PicassoSceneId and
identifies one coherent publication across every component table. dom_revision
identifies the semantic document revision from which it was compiled. They are
not interchangeable. Epoch exhaustion requires a new scene generation rather
than wraparound.

Each component publication identifies its logical content epoch,
allocation_epoch, alive/generation state, dirty_base_commit_epoch, and zero or
more dirty row spans or dirty pages. Dirty data is applicable only if the
consumer has exactly dirty_base_commit_epoch; otherwise it performs a full
rebuild. A first implementation may use one conservative min-to-max dirty span,
but the logical contract permits multiple disjoint changes.

A component allocation_epoch may identify replacement or relocation of its
backing storage. It invalidates cached storage mappings and normally forces a
full upload, but it has no DOM or paint meaning. commit_epoch,
allocation_epoch, resource generation, content_revision, and renderer residency
epoch are separate clocks.

### 5.1.1 Coordinate convention

Authored box, path, clip, and hit geometry is expressed in the local coordinate
space named by its spatial row. A spatial local transform maps that space into
its parent spatial space. The root spatial space is the layout viewport in CSS
pixels. Visual-viewport offset and scale, visual-viewport clipping,
device_scale, and the target pixel extent together define the root-to-target
mapping. Pinch zoom is therefore not confused with device pixel ratio.

Primitive and clip conservative_local_bounds use their referenced spatial
space. Paint-group conservative_bounds use composite_spatial. Hit geometry uses
the HitRow spatial space. HelioV-derived device bounds and tile bins are in
physical target pixels.

The packed V1 ABI must additionally freeze rectangle edge, pixel-center,
sampling, antialiasing, and rounding conventions. No producer may depend on an
unstated host-language cast or rounding mode.

Each nested browsing context owns a separate realm and document snapshot. Its
parent inserts it at the replaced-content paint position through a versioned
child-snapshot or composited-surface binding and records the transform and clip
between realm roots:

~~~text
ChildSceneBindingRow {
    self_ref: ChildBindingRef
    authorized_attachment: ChildAttachmentRef
    mode: EXACT_CHILD_SNAPSHOT or COMPOSITOR_ATTACHMENT
    exact_child_scene_id_and_epoch: optional
    flags
}
~~~

ChildSceneBindingRow is authored pointer-free data. It never contains a frame
handle, lease, surface, or raw child authority. ChildAttachmentRef is a
generation-bearing, owner-scoped capability issued by the host after it
authorizes the parent-to-child relationship. The host verifies that capability
and atomically resolves and pins the binding into host-private state.

EXACT_CHILD_SNAPSHOT selects one child scene and commit and requires a parent
transaction to select a different child epoch. COMPOSITOR_ATTACHMENT names the
stable authorized attachment but not whichever child frame is newest; the final
host composition selects one exact already-released child FrameSceneSet and
records that selection in its composition receipt. Thus an independently
composed iframe can animate without a parent DOM commit while presentation and
hit testing still agree exactly.

The binding row carries authority and epoch policy, not placement. One or more
ordered child-scene portal PrimitiveRows reference it and supply group,
order_in_group, spatial, clip, geometry, and opacity. Corresponding HitRows
place the portal in global hit order. This keeps child content inside normal
parent occlusion and effect semantics.

Input targeting can descend only through a winning child-portal hit. DOM
ancestry and commit epochs are never merged across realm boundaries, and DOM
events do not bubble across the browsing-context boundary unless a separate
browser semantic explicitly says so. Child bindings form an acyclic,
depth-bounded tree.

### 5.2 Spatial rows

~~~text
SpatialRow {
    parent: optional SpatialRef
    local_transform
    transform_kind
    local_reference_origin
    flags
}
~~~

Spatial rows form an acyclic, depth-bounded tree. They carry authored local
state. HelioV derives world transforms, inverse transforms, device bounds, and
visibility.

The logical transform can represent browser transform semantics, including a
general matrix. A compact physical implementation may place the common affine
2D case inline and uncommon perspective data in a typed payload arena.

Ordinary scrolled-content translation changes one spatial row and does not
require rewriting every descendant primitive. Fixed and sticky descendants use
the spatial ancestry required by their semantics. A scroll can additionally
patch sticky rows at thresholds, scroll clips, scrollbars, hit rows, or layout
when browser semantics require it.

### 5.3 Clip rows

~~~text
ClipRow {
    parent_clip: optional ClipRef
    spatial: SpatialRef
    clip_kind
    geometry_or_payload
    conservative_local_bounds
    flags
}
~~~

Clips form a retained chain. Required logical kinds are rectangle, rounded
rectangle, path or coverage mask, and empty. A clip is not destructively folded
into every primitive because retained chains allow scroll and ancestor clip
changes to remain local.

HelioV may pre-intersect simple axis-aligned clips and may rasterize complex
chains to coverage masks, provided the result is visually equivalent.

### 5.4 Paint-group rows

~~~text
PaintGroupRow {
    parent_group: optional GroupRef
    order_in_parent
    composite_spatial: SpatialRef
    composite_clip: optional ClipRef
    conservative_bounds
    opacity
    blend_mode
    isolation_flags
    backdrop_input_mode
    filter_chain: optional PayloadRef
    mask_binding: optional PayloadRef
}
~~~

A paint group represents a stacking or compositing boundary. It is the retained
place for group opacity, blend isolation, filters, masks, and intermediate
surface requirements. A child group occupies exactly one ordered item in its
parent. The ordered item namespace is the union of the parent's direct
PrimitiveRows and child PaintGroupRows; order values must be unique across that
union.

Trivial groups may be flattened as a renderer optimization. A group whose
children overlap under group opacity, filtering, masking, or nontrivial
blending must preserve offscreen group semantics. Flattening may never change
the picture.

backdrop_input_mode declares whether an effect consumes already-composited
pixels behind the group at its exact paint position. HelioV models that as an
explicit pass dependency; an ordinary isolated child surface is insufficient
for backdrop-filter.

A mask binding names the ResourceRef, its required revision, mask mode, mask
spatial row, geometry, and CSS reference box. Mask coordinates are therefore
not inferred from the resource extent.

document_root_group contains normal document painting. top_layer_group, when
present, is composited after it and contains the CPU-resolved ordering of
top-layer entries and their backdrops. The two roots form one defined top-level
paint sequence.

### 5.5 Primitive rows

~~~text
PrimitiveRow {
    group: GroupRef
    order_in_group
    spatial: SpatialRef
    clip: optional ClipRef
    conservative_local_bounds
    primitive_kind
    primitive_flags
    opacity
    payload: optional PayloadRef
    resource: optional ResourceRef
    child_binding: optional ChildBindingRef
}
~~~

order_in_group is a unique unsigned integer within the group. Row slot, table
iteration order, floating-point depth, primitive kind, and batching choice have
no paint-order meaning.

The foundational primitive vocabulary is intentionally small:

- CSS backgrounds as ordered solid or rounded primitives;
- other solid rectangles and rounded rectangles;
- border;
- linear gradient;
- sampled affine image parallelogram;
- R8 coverage or mask with a color;
- glyph run expressed as positioned coverage instances;
- child-scene portal referencing one ChildBindingRef;
- a typed fallback image for a feature rasterized above Picasso.

Additional kinds are versioned extensions. An unsupported CSS feature can be
rendered into an immutable or revisioned image resource and inserted at its
exact normal paint position. That corner cut preserves the permanent
architecture.

A child group is a logical ordered item, not an authored image primitive.
Picasso may turn the output of an effect pass into a GPU-private sampled
primitive while scheduling the parent pass.

child_binding is present only for the child-scene portal kind and absent for all
other kinds. The portal's geometry, spatial row, clip, opacity, group, and exact
order govern the child just like any other ordered visual item. Its binding row
governs attachment authority and epoch selection.

Primitive opacity is legal only when applying it directly is equivalent to CSS
group semantics. Otherwise the compiler emits or retains a PaintGroupRow and
places opacity there.

### 5.6 Hit rows

~~~text
HitRow {
    target: HitTarget
    group: GroupRef
    global_hit_order
    spatial: SpatialRef
    clip: optional ClipRef
    hit_shape_or_payload
    pointer_flags
    cursor
}

HitTarget =
    DOM(DomRef)
    or CHILD_PORTAL {
        portal_primitive: PrimitiveRef
        binding: ChildBindingRef
        fallback_owner: DomRef
    }
~~~

Hit rows are separate from draw primitives. Invisible hit regions, decorative
primitives, pointer-events rules, and one node producing several fragments make
a draw-row-as-hit-row shortcut incorrect.

Hit rows use the same spatial, clip, group, and ordering model as painting. A
successful lookup returns a generation-checked DomRef, not a typed SceneRef.
global_hit_order is a unique CPU-compiled total order from the final flattened
hit-test traversal, including atomic nested groups and the top layer. It avoids
trying to compare unrelated local group orders. Decorative visual items simply
have no hit row.

When a CHILD_PORTAL row wins, targeting resolves the exact binding_ref in the
visible FrameSceneSet and validates that portal_primitive is a live
child-scene PrimitiveRow referencing that same binding. That primitive's
geometry, spatial, and clip state supplies the exact parent-to-child-root
mapping for this portal instance; the resolved binding supplies authorization,
attachment, and child FrameSceneSet only. If the binding is unavailable or
policy says not to descend, fallback_owner receives normal parent-document
targeting. A child cannot be entered merely because its surface overlaps the
pointer; its ordered portal hit must win.

Hit geometry comes from CSS and element hit-test boxes, not from paint coverage.
Compilation accounts for unpainted boxes, visibility, display: contents,
HTML and SVG pointer-events rules, SVG fill and stroke geometry, replaced
content, pseudo-element retargeting to the originating element, scrollbars, UA
boxes, and top-layer boxes. cursor is a resolved style hint returned after a hit;
it is not targeting authority.

### 5.7 Resource rows and payload arenas

~~~text
ResourceRow {
    resource_kind
    required_content_revision
    logical_extent
    sampling_and_color_metadata
    immutable_asset_key_or_provider
    explicit_fallback_resource_and_revision
    flags
}
~~~

Variable-size data lives in typed, generation-checked arenas. Examples include
glyph instances, path verbs, gradient stops, filter chains, uncommon transform
matrices, and polygon hit shapes.

Large pixels, decoded video frames, font atlases, and path coverage do not need
to live inside SceneDB. SceneDB retains their logical ResourceRef, required
content revision, and explicit fallback. The host resource system owns bytes,
residency, atlas placement, views, pins, eviction, and physical addresses.

Font family/face selection, size, variation, italic synthesis or face choice,
shaping, and coverage production remain in the existing accelerated font stack.
The shadow contains resolved positioned glyph instances and generation-checked
coverage resources. Picasso does not recreate a second font or text-layout
engine.

An authored resource generation describes identity. content_revision describes
new content for that same identity. A renderer residency epoch describes a
private cache decision. These three values must not be conflated.

A committed snapshot names an exact resource content revision. Providers must
not mutate bytes in place while any accepted snapshot or GPU submission can
read them. The host pins that revision or resolves an equivalent immutable
view. If it is unavailable, the snapshot's exact fallback ResourceRef and
revision is used, or the previous coherent frame is preserved when the declared
policy says not to render. The host never samples an arbitrary older revision.

At commit acceptance or its first render attempt, the host freezes one exact
required-versus-fallback choice for that commit and records it in persistent
commit state. Every RenderTicket copies that choice. Re-rendering the same
commit cannot choose differently because a provider became ready later. If no
declared choice can be pinned, the commit is marked non-renderable and the prior
frame is preserved; it is not retried later with a different picture. Readiness
of new image, video, canvas, font, or mask content produces a new content
revision and a new SceneTxn before it may change visible pixels.

## 6. Exact paint order

CSS cascade and layout are CPU work. After layout, the scene compiler emits one
exact display order that has already resolved stacking contexts, pseudo
elements, positioned descendants, outlines, text decoration, and fragment
ordering.

The order contract is:

1. Every item in a group has one unique integer order.
2. A child group has one order in its parent and its own independent child
   order.
3. Picasso produces the same result regardless of authored row allocation,
   renderer-private projection compaction, primitive kind, batching, worker
   count, or tile assignment.
4. Equal-order ties are invalid input, not an invitation to use an unstable
   sort.

The CPU compiler should use sparse order-maintenance labels so insertion usually
changes only nearby rows. It may relabel a group transactionally when space
runs out.

Separating shapes, images, and text into independent batches is not a compliant
permanent representation because it loses cross-kind order. A temporary adapter
may accept such batches only after it reconstructs one exact ordered stream.

## 7. Transactions and publication

One browser task may cause hundreds of DOM and style changes. They become one
SceneTxn, not hundreds of partially visible frames.

~~~text
run one JS or browser task
    -> apply DOM mutations
    -> resolve style and cascade
    -> perform required layout
    -> compile and diff paint fragments
    -> validate the complete SceneTxn
    -> atomically publish all component tables at one commit_epoch
~~~

Normal publication occurs at a stable browser checkpoint such as the rendering
update associated with requestAnimationFrame. A synchronous JS layout query may
force style and layout immediately for correct browser semantics; it does not
force the renderer to observe a half-built scene.

All table changes, resource-reference changes, liveness changes, hit rows, and
snapshot metadata commit together. Consumers see the entire new epoch or the
entire preceding epoch. There is no state in which a primitive points to a clip
or resource row from another partial transaction.

The existing per-store publication model needs a coordinator plus immutable
published versions for this rule. The coordinator validates every staged store,
assigns the common commit_epoch, and swaps the root publication only after all
staged data is ready. The root publication is the visibility boundary.

Dirty tracking is an upload hint, not authority. A consumer that misses epochs
may upload or rebuild the whole live state. Removal dirties both liveness and
generation information.

### 7.1 Snapshot immutability and read leases

A root swap alone is not snapshot isolation. Once published, every table page,
alive/generation column, payload arena segment, and snapshot header reachable
from that root is immutable. Later transactions use copy-on-write pages,
double-buffered allocations, or equivalent versioning.

~~~text
SnapshotReadLease {
    scene_id
    commit_epoch
    root_publication_version
    table_allocation_versions
    payload_versions
}
~~~

A consumer acquires a SnapshotReadLease before derivation. The lease pins the
exact root, tables, and payloads through CPU derivation, upload, and every GPU
read that may reference them. Resource bytes are separately pinned at exact
revisions when the render ticket resolves them. Old versions are reclaimed only
after the last applicable CPU, GPU, and presentation lease retires.

An implementation may retain a compact, equivalent hit-test publication for a
displayed frame rather than pinning unrelated paint data through scanout. The
retained state must still be the exact commit_epoch associated with that frame.

### 7.2 Async producers

QJS remains single-owner even when the surrounding Rust system is highly
asynchronous. Async fetch, decode, font, image, canvas, and application workers
return immutable completions tagged with:

~~~text
owner scope
DomRef or ResourceRef
request generation
expected content revision
result
~~~

The owner validates those tags against current state. A stale completion is
dropped. A valid completion is incorporated into a later SceneTxn. Workers do
not patch SceneDB directly.

### 7.3 Scene cadence, render tickets, and presentation cadence

Scene commits and displayed frames are not one-to-one. The compiler may commit
faster than Picasso or the display can present, and Picasso may coalesce
intermediate commits.

One document produces a singleton epoch set. A frame containing nested browsing
contexts or independently composed child surfaces carries an immutable recursive
set:

~~~text
FrameSceneSet {
    scene_id: PicassoSceneId
    commit_epoch
    retained_render_or_hit_lease
    child_bindings: [ {
        binding_ref: ChildBindingRef
        child_frame_or_attachment_generation
        retained_resolved_binding_and_attachment_lease
        child: FrameSceneSet
    } ]
}
~~~

Every rendered or composed frame is tagged with the exact FrameSceneSet it
represents. For COMPOSITOR_ATTACHMENT, this is the final composition receipt
that freezes the selected child epoch without changing the parent commit. UI4
retains the frame together with the matching scene, resolved-binding,
attachment, or compact hit-test leases. That association remains alive until
the frame is genuinely replaced at SURFLIVE. The set is cycle-free and bounded
by scene, depth, and total-entry capabilities.

Input targeting for a visible surface uses the snapshot associated with its
SURFLIVE frame, not merely the newest SceneDB commit. This prevents a click on
old visible pixels from being routed through a newer, not-yet-visible layout.
Recursive targeting descends only through the exact ChildSceneBinding entries in
that frame's FrameSceneSet. binding_ref locates the authored authorization
binding in the retained parent epoch; the winning portal_primitive supplies the
portal-specific transform, geometry, and clip; the host-resolved binding
supplies the exact attachment generation and child FrameSceneSet.

Every GPU attempt is represented by a host-owned ticket:

~~~text
RenderTicket {
    frame_scene_set
    presentation_generation
    target_scene_binding_generation
    target_slot_and_generation
    target_extent_and_format
    snapshot_read_lease
    exact_resource_pin_set
    derived_allocation_and_mapping_set
    execution_slot
}
~~~

The host assigns presentation_generation monotonically and without wrap for one
UI surface or window composition endpoint. It maintains that endpoint's
publication watermark. target_scene_binding_generation identifies the exact
root scene binding currently attached to that endpoint. Navigation, scene
replacement, or teardown invalidates the old binding before accepting its
replacement.

A completed ticket may publish only if its presentation generation is not older
than the watermark, its root PicassoSceneId plus
target_scene_binding_generation is still the endpoint's current binding, and
its target, child-binding, attachment, extent, and format generations still
match. Publication advances the watermark atomically. A late older completion
or a completion from a replaced document retires normally but never replaces a
newer frame.

## 8. Mutation mapping

| Source change | Minimal shadow change |
| --- | --- |
| color or background change | patch the affected primitive payload |
| transform animation | patch one spatial row |
| ordinary scroll | patch the scrolled-content spatial row plus any required sticky, clip, scrollbar, or hit rows |
| simple opacity animation | patch one primitive or group row |
| group opacity, filter, or mask change | patch the paint-group row and effect damage |
| text content or font result | replace the affected glyph payload and bounds |
| layout-affecting style | rerun affected layout and fragment diff |
| same-realm DOM reparent | keep DomRef; patch fragment parents, groups, and orders as required |
| connected-subtree removal | retire descendant fragments, pseudos, anonymous boxes, hits, resource uses, and child-context groups; keep live JS DomRefs |
| cross-document adoption | explicitly remap to the target realm's DomRef domain; never silently reinterpret a realm-scoped ref |
| node destruction or realm teardown | retire DomRef only when semantic identity truly ends |
| pseudo-element creation | add FragmentKey rows owned by the real DomRef |
| image, canvas, or video frame | advance content_revision; topology may remain unchanged |
| selector or stylesheet change | recascade, then emit only resulting visual diffs |

damage_hint is conservative root layout-viewport CSS-space damage. It includes
both the old and new visual bounds of changed or removed content and expands for
filters, antialiasing, shadows, masks, and transformed conservative bounds.
Uncertain bounds fall back to the containing group or full viewport. HelioV
applies the visual-viewport mapping and clipping to derive physical-target
damage.

The scene compiler may publish that conservative result as damage_hint. HelioV
may verify, expand, or replace it while comparing coherent snapshots. The hint
is never authority for omitting pixels: v1 repaints the full target, and a
future partial renderer must prove untouched-pixel coherence independently.

## 9. Input and event routing

UI4 remains the window and input broker. Element targeting is a scene operation:

1. Select one coherent published snapshot and its viewport mapping.
2. Query hit rows by descending global_hit_order.
3. Apply inverse spatial transforms and the complete clip chain.
4. Apply hit shape and pointer-events rules.
5. Return a generation-checked DomRef plus the sampled commit_epoch.
6. Let the live DOM construct capture, target, and bubble paths.

For an on-screen surface, step 1 selects the commit_epoch attached to the
SURFLIVE frame's exact recursive FrameSceneSet as defined in Section 7.3.
Targeting descends child bindings with the corresponding retained child epoch,
transform, clip, and hit state.

Focus, pointer capture, IME state, and event-listener execution remain semantic
state above SceneDB. If the node has been retired before dispatch, generation
validation prevents accidental delivery to a reused slot; browser policy may
drop or retarget the event.

A non-invertible spatial transform cannot produce an accidental hit. Its hit
rows are treated as non-hittable unless a future version defines an explicit
alternative.

CPU hit testing is a valid first wave. A later HelioV spatial index or GPU query
is an optimization over the same HitRow contract.

## 10. The public Picasso trust seam

The public boundary is a versioned, pointer-free scene transaction. It follows
the existing Blueprint principle: callers supply scene facts, never frame
handles, writable surfaces, physical addresses, GPU virtual addresses, batch
buffers, fences, or display aliases.

A suitable transport shape is:

~~~text
PICASSO_SCENE_BEGIN_V1({
    authenticated_scene_capability
    expected_base_commit_epoch
    client_txn_id
    mode: DELTA or FULL_REPLACE
    exact_total_bytes
    operation_counts
})
PICASSO_SCENE_CHUNK_V1(offset, bytes)
PICASSO_SCENE_FINISH_V1()
~~~

V1 is an optimistic retained transaction. DELTA contains typed creates,
updates, and removes. CREATE uses transaction-local typed references. The host
allocates generation-bearing handles and returns the complete temporary-to-host
remap only if FINISH commits. References within the candidate may use those
temporary IDs. UPDATE and REMOVE carry exact host-issued handles and expected
generations.

FULL_REPLACE contains a complete logical snapshot and is the bounded resync
path, but it is not permission to reset identity. Existing records carry their
host-issued live handles; new records use transaction-local references;
omitted live records are removed. The host reconciles all operations against
its retained live, dead, and permanently retired slot ledger. A full replacement
can never lower, reuse, or invent a generation. A producer that intentionally
wants to discard all prior handle identity must destroy the old scene and
obtain a new PicassoSceneId generation.

Both modes name the exact currently accepted base epoch, including when that
base is zero for a new scene. A base mismatch rejects the candidate rather than
rebasing untrusted operations.

The authenticated endpoint binds the transaction to one generation-bearing
PicassoSceneId. The host, never guest data, assigns the next authoritative
commit_epoch after validation. client_txn_id exists only for reply correlation
and replay detection.

There is at most one bounded assembly candidate per authenticated scene
endpoint. BEGIN reserves an exact policy-bounded length. CHUNK bytes are copied
immediately and must arrive at monotonically increasing exact offsets with no
gaps, duplicates, or overlap. FINISH succeeds only at the exact declared end.
Protocol error or assembly deadline expiry discards the candidate before any
frame lease or GPU resource is acquired.

Native in-kernel callers may use a queue rather than vmcalls, but they pass the
same validated logical transaction. Transport choice does not change authority.

The host derives the caller owner, copies untrusted bytes, and validates the
entire candidate before acquiring a UI4 write lease. Validation includes:

- supported version and exact declared byte length;
- matching authenticated scene generation, base epoch, and transaction mode;
- valid transaction-local create references and host generation-ledger
  transitions;
- fixed-width little-endian records and zero reserved fields;
- checked offset, count, alignment, and multiplication arithmetic;
- no illegal overlap between typed regions;
- finite numeric values and valid enum domains;
- viewport, row, byte, graph-depth, and resource budgets;
- live generation-correct references of the expected table type;
- host-authorized ChildAttachmentRefs and legal ChildBindingRef transitions;
- acyclic, depth-bounded spatial, clip, and group graphs;
- acyclic, depth-bounded resolved child-binding graphs;
- unique legal paint orders;
- legal payload ranges and resource metadata;
- conservative bounds clipped to safe arithmetic domains.

Malformed input leaves the previous committed scene and current front frame
intact. Validation failure never turns into a partially published scene.

Resource resolution uses one atomic
resolve_and_pin(owner, slot, generation, content_revision) operation. Metadata
is checked against the returned immutable view, closing the time-of-check versus
time-of-use gap. The host then writes resolved addresses only into host-owned,
immutable GPU tables that the caller cannot modify.

Resource resolution validation covers virtual and physical range overflow,
allocation coverage, pitch/extent/format/byte-length consistency, every table
and resource range, forbidden source/destination aliases, and all payload, clip,
primitive, and index references. Failure releases every newly acquired pin. The
host then either freezes and pins the exact declared fallback or marks the
commit non-renderable and preserves the preceding coherent frame. It never
waits and later renders the same commit with a different resource revision.

ChildAttachmentRef resolution is a separate authorization and composition step.
For COMPOSITOR_ATTACHMENT it does not freeze through ResourceRow rules; the
final host composition receipt records and pins the exact child attachment
generation, frame, and FrameSceneSet used by that presented result.

The exact packed record layout is deliberately not frozen by this document. It
must be generated or asserted in every participating crate, include explicit
size and offset checks, and receive a new version for incompatible changes.

## 11. Picasso renderer-derived state

Given one PicassoSnapshot plus the RenderTicket's exact FrameSceneSet and
host-resolved child-binding pins, the Picasso renderer backend may derive:

- world and inverse transforms;
- accumulated clip descriptions and simple clip intersections;
- conservative device bounds and visibility;
- damage tiles;
- exact per-tile ordered primitive-index lists;
- effect and intermediate-surface passes;
- resource residency requests;
- indirect walker descriptions and batching decisions.

None of those products is authored back into SceneDB. Parent-only geometry,
ordering, clip, and bin caches may be keyed by PicassoSceneId, commit_epoch,
and relevant allocation or resource epochs. Anything containing a child sample,
attachment choice, child-dependent effect input, or presentation dependency is
ticket-scoped or keyed by a collision-resistant recursive FrameSceneSet
fingerprint that includes every binding ref, attachment generation, child scene
ID, and child commit epoch. All derived products may be discarded on pressure
or device reset.

A renderer can skip unchanged rows, but correctness cannot depend on having
observed every earlier dirty notification.

## 12. Picasso execution model

### 12.1 The foundational walker

The permanent painter is pixel-owned, not descriptor-owned. For each damage or
full-frame tile, each destination pixel has exactly one writer. That writer
walks the tile's ordered primitive-index list and composites intersecting
primitives in exact painter order.

~~~text
tile table -> ordered primitive indices -> immutable resolved primitive table
                                             |
                                             v
                                  one writer per destination pixel
~~~

This removes cross-workgroup races between overlapping rectangles and avoids
serializing one GPU submission per DOM primitive. CPU compilation or HelioV
builds ordered tile bins; the walker remains simple enough for the Bakery
artifact contract.

The GPU-private resolved primitive table may contain destination-space bounds,
inverse affine data, pre-intersected clips, resolved resource addresses, pitches,
formats, and sampling data. These are derived records, not the public SceneDB
ABI.

The V1 walker accepts only finite, invertible 2D affine transforms. Perspective,
3D, and other unsupported transform cases are flattened by the compiler into an
ordered fallback image before this boundary. A singular transform produces no
drawable or hittable pixels unless browser semantics require an explicitly
prerasterized fallback.

Even though physical struct packing remains version work, every submitted V1
table obeys these executable invariants:

- tile.first plus tile.count is checked and no greater than total index count;
- every primitive index is less than primitive count;
- every payload, resource, spatial, clip, and group index is in range;
- every independent table byte length is passed to and rechecked by the kernel;
- tables and resolved resource views are immutable for the execution lifetime;
- pass_clear_rgba initializes each pixel accumulator outside the authored paint
  stream; CSS backgrounds remain normal ordered primitives; all primitives
  blend in strict order, and exactly one final store writes each destination
  pixel.

Complex filters and nontrivial paint groups form an effect-pass graph. Each pass
uses the same pixel-owned ordered model for its target. A completed intermediate
surface then appears as one ordered child-group image in the parent pass.

A COMPOSITOR_ATTACHMENT portal is normally sampled or composed at its exact
PrimitiveRow position. Direct plane promotion is legal only if the host proves
equivalence for parent occlusion, overlap, opacity, clipping, masks, filters,
blend/isolation, transforms, color conversion, and painter order. Otherwise the
child frame is recomposed inside the parent effect graph; promotion may never
escape the containing PaintGroup semantics.

### 12.2 First-wave rendering subset

The first native walker should support:

- out-of-stream full-target pass_clear_rgba;
- premultiplied source-over;
- solid and rounded rectangles;
- borders;
- linear gradients;
- sampled affine image parallelograms;
- R8 glyph, outline, and generic coverage masks;
- rectangular and rounded clipping;
- composited intermediate images.

Paths, rare gradients, shadows, and unsupported filters may initially become R8
coverage or a prerasterized image. That fallback changes neither identity,
ordering, transactions, input targeting, nor future ABI direction.

Color space and blending convention are explicit snapshot or format metadata.
V1 may expose only one well-defined premultiplied RGBA8 working format; it must
not silently reinterpret resources with different conventions.

### 12.3 Frame and fence sequence

The only valid render-to-display sequence is:

~~~text
validate and publish a coherent scene
    -> acquire its SnapshotReadLease
    -> atomically resolve and pin exact resource revisions
    -> authorize and resolve child bindings
    -> select and pin the exact recursive FrameSceneSet/composition receipt
    -> acquire a checked, non-front UI4 write lease
    -> create the RenderTicket
    -> build and pin GPU-private derived tables
    -> clear and render the target
    -> prove final-job drain, marker, and saved-head completion
    -> mint an exact GPGPU release for that physical allocation
    -> reject stale ticket or publish frame plus FrameSceneSet to UI4
    -> UI4 hands the published frame to display
    -> SURFLIVE confirms the live surface
    -> scanout
~~~

Picasso never manufactures its own UI4 frame handle and never publishes before
the exact release. A release is minted only after the final
destination-writing job's HDC/L3 drain, ordered PIPE_CONTROL post-sync marker,
and saved LRC head have reached the published tail. A marker alone is
insufficient, and an intermediate effect pass can never mint the publishable
final-target release.

For the current pinned ADL-S path, the exact UI4 destination is mapped with the
proven PAT3/UC policy from its first destination write through release. Dirty
destination bytes are never written through PAT0/WB and remapped afterward.
Sources normally remain PAT0/WB. Cache/PTE selection is host-only; no guest
field or scene flag chooses it. A future target policy requires its own
hardware-backed coherency proof.

Failure lifetime depends on whether GPU admission occurred:

- Before admission, cancel the write lease and recursively release the complete
  ticket candidate: snapshot, resource, resolved-binding, ChildAttachmentRef,
  selected child-frame, child FrameSceneSet/hit, derived-allocation, mapping,
  and execution-slot pins.
- After an accepted request has uncertain completion, retain the write lease,
  SnapshotReadLease, every source and exact resource revision, every table and
  derived allocation, every resolved child binding and ChildAttachmentRef, each
  selected child frame and recursive FrameSceneSet/hit lease, all PPGTT
  mappings, result/batch storage, and the execution slot until proven retirement
  or a real engine reset. The request is never replayed and none of those
  objects is reused based on timeout alone.
- After proven completion, a stale ticket skips publication but retires all
  resources and the non-front target through the normal completed path.

Publication consumes the producer write lease and UI4 acquires the exact front
read lease. Direct scanout retains that producer buffer until its successor is
observed SURFLIVE. A composed source is retained through composition and
presentation completion; the compositor output, not every source, owns the
scanout lifetime. SURFLIVE is per plane, so a timeout after a multi-plane update
may require retaining exact old and new leases according to each observed plane
state.

Snapshot tables, source-resource pins, and renderer-derived memory may retire
once all GPU reads are proven complete. The coherent hit-test publication tied
to a visible frame remains available until that frame is actually replaced.

### 12.4 Full repaint in v1

Current dirty/double and streaming/triple UI4 write buffers are not guaranteed
to contain the preceding front image. A GPU-full-overwrite lease may begin with
undefined contents.

Therefore Picasso v1 MUST clear and repaint the whole acquired target. Scene
damage is still valuable for upload minimization, tile-bin rebuilding,
presentation metadata, and future work, but it cannot justify partial raster
into an unseeded target.

Partial raster becomes legal only after one of these is explicit:

- copy or seed the preceding coherent front into the acquired backbuffer; or
- track the scene epoch represented by every buffer and replay all intervening
  changes; or
- use another scheme proven to reconstruct every untouched pixel.

This rule is about backing-store coherence, not a limitation of the SceneDB
model.

## 13. Capabilities and bounded behavior

The public contract exposes versioned capabilities rather than inheriting
incidental backend constants. Capabilities include:

- maximum snapshot and payload bytes;
- maximum rows by component type;
- maximum resources and resident bytes;
- maximum graph depth;
- maximum surface extent;
- tile geometry, maximum indices per tile, total tile-index count, and bytes;
- maximum resolved-table bytes and primitive visits per pixel and per frame;
- maximum effect-pass count and intermediate-surface count and bytes;
- maximum pinned bytes per owner and globally;
- maximum assembly chunks, assembled bytes, and assembly deadline;
- supported primitive, clip, filter, blend, color, and resource formats;
- maximum in-flight scene and render submissions.

An initial policy may choose roughly four to eight thousand primitives while
dedicated storage is introduced. That is a deployment limit, not the permanent
semantic ceiling and not the existing generic descriptor-worklist limit.

When authored content exceeds a capability, the scene compiler may flatten a
subtree into a revisioned fallback image, split safe payloads, or report a
bounded failure. It may not silently drop primitives or change painter order.

All loops visible to untrusted input are bounded by validated counts and depth.
Shaders use the same or stricter bounds as the validator. Resource failure uses
the exact fallback frozen for the commit or preserves the previous coherent
frame according to policy.

## 14. Current TRUEOS seams to reuse

The implementation should extend these seams instead of inventing parallel
ownership systems:

- [Helio runtime SceneDB](../../crates/trueos-helio-runtime/src/scene_db.rs)
  already models a writer-owned retained data seam, slot-generation handles,
  dirty rows, and coherent publication.
- [Retained transforms](../../crates/trueos-helio-runtime/src/retained_transform.rs)
  already model renderer-derived hierarchy propagation.
- [Blueprint scene transport](../../src/ui4/blueprint_text.rs) already enforces
  the rule that a scene producer receives no writable UI4 surface or GPU
  address.
- [UI4 frame pool](../../src/ui4/frame_pool.rs) already owns checked write
  leases, exact GPGPU release matching, cadence, and publication.
- [UI4 compositor service](../../src/ui4/compositor_service.rs) already retains
  published ownership through the SURFLIVE transition.
- [The ordered UI4 layer compositor](../../crates/trueos-shader/gpgpu/kernels/ui4_compose_layers_rgba8.clcpp)
  is a working precedent for a per-pixel ordered loop over immutable inputs.
- [The SVG outline operation](../../src/intel/gpgpu/operations/svg_outline.rs)
  is a bounded trusted probe proving path-to-R8-coverage-to-scanout composition
  with exact frame release; it is not yet a general untrusted path renderer.
- [The GPGPU kernel contract](../../crates/trueos-shader/gpgpu/kernels/README.md)
  and [Intel GPU Bakery](../intel-gpu-bakery/README.md) remain the artifact and
  admission boundary for the current pinned ADL-S Xe-LP target, PCI 8086:4680
  revision 0x0c.

Current parallel fill and blend descriptor worklists remain useful specialized
operations. They are not the permanent overlapping DOM painter because parallel
descriptors do not, by themselves, define exact painter order.

The current sprite record is also an implementation bridge, not Picasso's
public DOM ABI. It lacks retained clip chains, paint groups, resource
generations, and a general primitive vocabulary.

## 15. Known gaps to close

The first implementation work should close these precise gaps:

1. Replace generation wrapping in TRUEOS SceneStore with permanent slot
   retirement on exhaustion; hosted SceneDB already retires exhausted slots.
   Add explicit translation for hosted generation-zero entity IDs.
2. Add a multi-table root publication coordinator, immutable/COW published
   versions, SnapshotReadLease, and common commit_epoch.
3. Add generation-bearing PicassoSceneId and scope handles, transport, and
   nonwrapping epochs to it.
4. Let one DomRef own zero-to-many compiler-stable FragmentKey records.
5. Extend dirty reporting from one broad span to disjoint spans or dirty pages
   when measurements justify it.
6. Give each paint group a unique local item order and hit rows a unique global
   hit order.
7. State every cross-boundary record layout, endianness, size, alignment, and
   reserved field explicitly.
8. Introduce typed resource generations and content revisions without exposing
   physical atlas or surface slots.
9. Build optimistic delta/full-resync assembly, a dedicated Picasso validator,
   and atomic resolve-and-pin into host-private immutable tables.
10. Add RenderTicket, stale-publication watermark, exact rollback, and
    accepted-timeout quarantine for every referenced allocation.
11. Replace cross-kind shape/text batching with one ordered display stream.
12. Add element hit testing that returns stable DomRef values.

## 16. First-stage build map

### Stage A: freeze and test the logical contract

- Land the identity types and logical row schemas behind a versioned module.
- Add PicassoSceneId, the atomic SceneTxn coordinator, immutable publication
  versions, and SnapshotReadLease.
- Add graph, generation, numeric, range, order, and budget validation.
- Test stale handles, generation exhaustion, broken graphs, and failed atomic
  commits.
- Test stable fragment matching and atomic retirement across reflow.
- Test base-epoch mismatch, missed-dirty-epoch full rebuild, and old snapshot
  readers overlapping a new commit.

### Stage B: adapt the working Solara path

- Give live DOM nodes stable DomRef values.
- Compile the current static output into SpatialRow, PaintGroupRow,
  PrimitiveRow, HitRow, and ResourceRow records.
- Preserve exact shape, image, and glyph order.
- Continue using existing GPU primitives or CPU hit testing behind the new
  contract where convenient.
- Add paint-order fixtures covering CSS Appendix E categories: negative,
  auto, and positive z-index; block backgrounds; floats; inline content;
  outlines; generated content; nested opacity/effect groups; top-layer entries
  and backdrops; and interleaved text, images, and shapes.
- Add hit fixtures for unpainted boxes, display: contents, HTML and SVG
  pointer-events, pseudo retargeting, transformed clips, and top-layer content.

### Stage C: make mutation real

- Diff fragments at browser rendering checkpoints.
- Route QJS changes and async completions through the sole owner SceneTxn.
- Exercise reparenting, deletion and slot reuse, pseudo-elements, scrolling,
  font completion, image completion, canvas/video revisions, and animation.

### Stage D: add the native Picasso path

- Add optimistic pointer-free Begin/Chunk/Finish delta/full-resync transport and
  its native queue twin.
- Build HelioV world transforms, clip derivation, and ordered tile bins.
- Bake the full-repaint pixel-owned V1 walker.
- Add atomic host-only resource resolution, RenderTicket, stale-result rejection,
  full accepted-timeout quarantine, and the existing exact UI4
  release/SURFLIVE lifetime.

### Stage E: extend without moving the seam

- Add nontrivial group passes, filters, masks, and more primitive kinds.
- Add accelerated hit indices.
- Add coherent partial repaint only after backing-store reconstruction exists.
- Tune SoA layouts, dirty pages, tile sizes, residency, and in-flight slots from
  measurements.

## 17. Acceptance invariants

The marriage is sound when the following remain true under arbitrary script
mutation:

- A committed frame corresponds to exactly one complete recursive FrameSceneSet.
- Hit testing a visible frame uses that same retained recursive epoch set.
- A late render can never publish behind the endpoint presentation watermark.
- A ticket from a navigated, torn-down, or rebound root scene cannot publish.
- The DOM is the only semantic authority and the scene compiler is the only
  authored SceneDB writer.
- One semantic node can produce any number of visual fragments without losing
  stable identity.
- Reusing a slot never revives a stale DOM, scene, payload, or resource handle.
- Text, shapes, images, masks, and child groups obey one exact painter order.
- Group opacity, clipping, filters, and masks cannot be reordered by batching.
- Renderer caches can be dropped and rebuilt without consulting the DOM.
- A guest-controlled byte can never become an unchecked pointer, frame lease,
  fence, or display alias.
- No GPU result reaches UI4 without the exact release for the leased physical
  target.
- An accepted uncertain request retains every object the GPU may still access
  until proven retirement or engine reset.
- UI4 retains ownership until SURFLIVE proves replacement.
- A failed transaction or render leaves a coherent older scene or frame visible.
- Unsupported visual features degrade through ordered fallback resources, not a
  second semantic architecture.

## 18. Decisions frozen here and decisions deliberately deferred

Frozen now:

- the authority and dataflow split;
- SceneDB as a draw-only retained snapshot;
- slot-generation identity and one-node-to-many-fragment mapping;
- generation-bearing scene identity and monotonic nonwrapping commit epochs;
- spatial, clip, group, primitive, hit, resource, and typed-payload semantics;
- atomic multi-table publication and immutable leased snapshot versions;
- exact integer painter order;
- pointer-free trust boundaries;
- renderer-private derived state;
- optimistic delta/full-resync transport and host-assigned epochs;
- render tickets, stale-result rejection, and exact resource pinning;
- UI4 lease, release, publication, and SURFLIVE lifetime;
- host-only PAT3/UC final-target policy for the current pinned ADL-S path;
- full repaint for the first unseeded backbuffer implementation.

Deliberately deferred:

- exact packed ABI offsets and public enum numbers;
- AoS, SoA, page, and compression choices;
- tile dimensions and bin construction algorithm;
- concrete policy caps;
- the final division between CPU and GPU derivation;
- source-resource cache and residency tuning, but not the proven host-only
  final-target cache policy;
- complete CSS/filter/blend feature coverage;
- partial-repaint strategy;
- how many frames or scenes may be in flight.

Those deferred choices can change as measurements and specification coverage
arrive. They do not require moving the permanent Solara-to-Picasso seam or the
Solara-to-Picasso-to-UI4-to-SURFLIVE-to-scanout chain.
