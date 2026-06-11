# wid

`wid` is the renderer-agnostic widget description layer.

It does not know about Pixi, Yoga, retained scenes, pointer harnesses, popup state, or capture handoff. It only turns an already-parsed DOM into a small tree of widget descriptors that another runtime can lay out and render.

## Files

- `index.mjs`: public exports
- `fromDom.mjs`: parsed DOM to widget descriptor tree
- `registry.mjs`: generic registry engine
- `widgets/forms.mjs`: form/control widget definitions and helpers
- `widgets/values.mjs`: value/rich-control widget definitions and helpers
- `widgets/structure.mjs`: structure/media widget definitions and helpers
- `widgets/index.mjs`: widget definition barrel and default definition list
- `dom.mjs`: parse5 DOM helpers
- `tags.mjs`: block/inline/control tag sets
- `tree.mjs`: descriptor tree utilities

## Current To Do Upfront

- Keep this library as plain `.mjs` with no Pixi/Yoga imports.
- Move only DOM-to-widget classification here first; leave rendering and layout in the current app.
- Treat complex controls as descriptors before rebuilding their UI: `color`, `dialog`, `iframe`, `search`, and temporal inputs.
- Stop remapping temporal inputs into custom picker widgets for the portable layer. Keep them as `input` with `meta.currentStatus = "defer-special-ui"`.
- Keep composite expansion optional. A renderer can later decide whether `<search>` becomes button plus text field, or remains one host widget.
- Preserve stable keys from traversal paths so later state stores can map descriptor nodes to runtime state.
- Add tests or snapshots once the expected descriptor tree settles.

## Calls And Flow

```js
import {
  collectWidgetStats,
  defaultRegistry,
  domToWidgets,
  flattenWidgetTree,
  walkWidgets,
} from './src/wid/index.mjs';

const treeFromDom = domToWidgets(parse5Document, {
  registry: defaultRegistry,
});
const stats = collectWidgetStats(treeFromDom);
```

Flow:

1. `domToWidgets(dom, options)` finds the document body and starts traversal.
2. `nodeToWidgets(node, path, options)` normalizes each DOM node.
3. `attrsToObject(node)` converts DOM attributes into plain objects.
4. Inline text is folded into `{ kind: "text", text }`.
5. Block/control elements become `{ kind: "widget", tag, attrs, props, meta, children }`.
6. The registry classifies widget tags into renderer-neutral categories.
7. `walkWidgets`, `flattenWidgetTree`, and `collectWidgetStats` inspect the descriptor tree.
8. A renderer/layout runtime consumes the descriptor tree.

Public calls:

- `domToWidgets(parse5DocumentOrNode, options)`
- `nodeToWidgets(parse5Node, path, options)`
- `createWidgetRegistry(definitions)`
- `defaultRegistry.get(tag, attrs)`
- `defaultRegistry.register(tag, definition)`
- `defaultRegistry.entries()`
- `DEFAULT_WIDGET_DEFINITIONS`
- `FORM_WIDGET_DEFINITIONS`
- `VALUE_WIDGET_DEFINITIONS`
- `STRUCTURE_WIDGET_DEFINITIONS`
- `walkWidgets(root, visitor)`
- `flattenWidgetTree(root)`
- `collectWidgetStats(root)`

## Descriptor Shape

```js
{
  kind: 'widget',
  key: 'root.0:button',
  tag: 'button',
  widget: 'button',
  role: 'block',
  category: 'form-control',
  attrs: { type: 'button' },
  props: {},
  children: [{ kind: 'text', text: 'Go' }],
  meta: {
    source: 'author',
    kind: 'container',
    complexity: 'basic',
    leaf: false,
    interactive: true,
    complex: false,
    currentStatus: 'basic',
    notes: '',
    layoutDefaults: { minWidth: 100, minHeight: 36, paddingY: 6 },
    attrs: ['type', 'disabled'],
    state: [],
    interactions: ['press'],
    overlays: [],
    expandsTo: []
  }
}
```

Special cases currently represented in `props`:

- `textarea`: `{ value }`
- `select`: `{ options, selectedIndex }`
- `svg`: `{ markup }`
- `progress` / `meter`: `{ fallbackText }`
- `details`: `{ open }`
- `summary`: `{ detailsKey }`
- `iframe`: `{ srcdoc }`

## Registry

The registry answers: “What kind of thing is this tag?”

```js
import { createWidgetRegistry } from './src/wid/index.mjs';

const registry = createWidgetRegistry().register('my-widget', {
  tag: 'my-widget',
  role: 'block',
  category: 'host-control',
  leaf: true,
  interactive: true,
  complex: false,
  currentStatus: 'host-defined',
});
```

The registry deliberately does not render. It only provides portable metadata:

- `id`
- `source`
- `kind`
- `complexity`
- `role`
- `category`
- `leaf`
- `interactive`
- `complex`
- `currentStatus`
- `notes`
- `layoutDefaults`
- `attrs`
- `state`
- `interactions`
- `overlays`
- `expandsTo`

## Current Widget Inventory

Basic first-pass widgets:

- Structure: `p`, `div`, `form`, `label`, `fieldset`, `section`, `article`, `header`, `footer`, `main`, `nav`, `aside`
- Text/chrome: `h1`-`h6`, `hr`, `button`, `progress`, `meter`, `table`, `tr`, `td`, `th`, `canvas`
- Direct form descriptors: `input`, `textarea`, `select`

Complex or represent-only widgets:

- `color`: keep value semantics; rebuild picker later.
- `search`: represent as one composite; optional expansion later.
- `details` / `summary`: keep open/toggle semantics; runtime decides collapse behavior.
- `dialog`: descriptor only; dragging, z-order, and focus trapping are runtime work.
- `img` / `svg`: descriptor plus source/markup; async texture/image loading is renderer work.
- `iframe`: descriptor plus `srcdoc`; nested parsing/root scene ownership stays outside this layer.
- Temporal inputs: stay as `input[type=date|time|month|week|datetime-local]` with `currentStatus: "defer-special-ui"`.

Legacy synthetic entries are registered so the existing app can be mapped later:

- `barrow`
- `sliderlabel`
- `searchrow`
- `searchbutton`
- `timeinput`
- `dateinput`
- `monthinput`
- `weekinput`
- `datetimelocalinput`

## Ported Widget Modules

The old Pixi files in `src/widgets` are still the renderer implementation. The portable port lives under `src/wid/widgets` and is split like this:

- `widgets/forms.mjs`: `button`, `input`, `textarea`, `select`, `search`, `searchrow`, `searchbutton`
- `widgets/values.mjs`: `progress`, `meter`, `slider`, `sliderlabel`, `barrow`, `number`, `color`, temporal descriptors
- `widgets/structure.mjs`: root/text, structural tags, headings, `hr`, `details`, `summary`, `dialog`, tables, `img`, `svg`, `canvas`, `iframe`

These files export descriptor arrays plus pure helpers such as `classifyInput`, `normalizeSelectState`, `normalizeSliderValue`, `normalizeNumberValue`, `normalizeColorRgba`, `parseTemporalValue`, `replacedDimensionsFromAttrs`, and `iframeSrcdocProps`.

## Current Boundary

Things that stay outside `wid` for now:

- Pixi drawing
- Yoga layout defaults
- Hover and pointer events
- Keyboard editing
- Selection/caret rendering
- Popup rendering
- Multi-cursor harness
- Dialog dragging and z-order
- Iframe scroll state
- Capture-only texture resolution
