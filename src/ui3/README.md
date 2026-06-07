# UI3 Pixi Host

UI3 starts as a Pixi-style 2D scene command host.

It is not a widget toolkit, not a gfx backend abstraction, and not a UI2
compatibility layer. Widgets, HTML-ish controls, Yoga layout, and app semantics
sit above this layer. UI3 receives the already-lowered scene workload:
containers, graphics paths, text objects, listener metadata, and render
boundaries.

## First Command Vocabulary

The current Parse5/Pixi trace says the useful first vocabulary is:

- `Container.addChild`
- `Container.addChildAt`
- `Container.setChildIndex`
- `Container.removeChildren`
- `Container.on`
- `Container.removeAllListeners`
- `Graphics.clear`
- `Graphics.rect`
- `Graphics.fill`
- `Graphics.stroke`
- `Graphics.moveTo`
- `Graphics.lineTo`
- `Graphics.circle`
- `Text`
- `Render`

Most commands are structural bookkeeping. The first renderer only needs enough
state to produce the final retained scene at each render boundary.

## Deliberate No-Ops

These can be mostly no-op initially:

- `removeAllListeners`: clear listener metadata only.
- `on`: record event names and hit capability, not JavaScript handler bodies.
- exact Pixi stroke joins, caps, alignment, miter, and style details.
- most Pixi text style fields.
- unsupported transforms beyond position, visibility, and z order.

## First Visible Renderer

The visible subset is:

- solid filled rectangles
- stroked rectangles and simple line paths
- circles for cursor/controls
- one-tier text
- retained child ordering
- render boundary as "present scene"

That is enough for the traced demo page to become recognizable without
building buttons, inputs, labels, or HTML widgets inside UI3.

## Text Tier

Text is intentionally tiny at first:

- one default font tier
- Palatino-like face when available
- size tier `1x`
- fill color
- plain text payload
- position

Pixi may send rich font params. UI3 accepts the shape of that API but ignores
nearly all of it until the rest of the scene host is real.

## GPGPU Direction

The first fast path should target batches of simple 2D draw descriptors:

- fill rect descriptors
- stroke rect descriptors lowered to thin fill rects
- line descriptors or CPU-lowered line spans
- circle descriptors only if they show up enough to matter

UI3 should not expose those kernels to apps. The renderer decides which command
groups become GPGPU work and which remain no-op or CPU fallback.
