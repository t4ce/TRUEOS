// Stable local entrypoint for Pixi.
//
// This avoids runtime URL fetching/DNS by importing from the embedded cdn cache
// modules under /qjs/cdn/.
//
// Source URL that was seeded (for reference):
//   https://esm.sh/pixi.js@8.7.2?bundle

export * from '/qjs/cdn/8d2f5f0bba6a6702.mjs';
import * as PIXI from '/qjs/cdn/8d2f5f0bba6a6702.mjs';
export default PIXI;
