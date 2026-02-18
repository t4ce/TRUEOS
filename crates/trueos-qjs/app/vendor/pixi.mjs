// Stable local entrypoint for Pixi.
//
// This avoids runtime URL fetching/DNS by importing from the embedded cdn cache
// modules under /qjs/cdn/.
//
// Source URL that was seeded (for reference):
//   https://esm.sh/pixi.js@7.4.3?bundle

export * from '/qjs/cdn/2232fcf00ce9d149.mjs';
import * as PIXI from '/qjs/cdn/2232fcf00ce9d149.mjs';
export default PIXI;
