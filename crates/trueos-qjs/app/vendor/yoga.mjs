// Stable local entrypoint for Yoga (yoga-layout).
//
// Mirrors the Pixi/Parse5 vendor module pattern: consumers import a local
// /qjs/vendor path while the implementation remains embedded under /qjs/cdn.
//
// Source URL that was seeded (for reference):
//   https://esm.sh/yoga-layout@3.2.1/node/yoga-layout.bundle.mjs

export * from '/qjs/cdn/fde6b9a19a6f2d59.mjs';
import * as yoga from '/qjs/cdn/fde6b9a19a6f2d59.mjs';
export default yoga;
