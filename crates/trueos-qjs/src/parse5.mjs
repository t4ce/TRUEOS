// Stable local entrypoint for parse5.
//
// This mirrors the Pixi vendor module pattern: consumers import a local
// /qjs/vendor path while the implementation is embedded under /qjs/cdn.
//
// Source URL that was seeded (for reference):
//   https://esm.sh/parse5@7.2.1/es2022/parse5.bundle.mjs

export * from '/qjs/cdn/39d8f7d0658e6d2e.mjs';
import * as parse5 from '/qjs/cdn/39d8f7d0658e6d2e.mjs';
export default parse5;
