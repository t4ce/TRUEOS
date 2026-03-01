// Stable local entrypoint for Yoga (yoga-layout).
//
// Mirrors the Pixi/Parse5 vendor module pattern: consumers import a local
// /qjs/vendor path while the version pin stays centralized here.
//
// Source URL (pinned):
//   https://esm.sh/yoga-layout@3.2.1?bundle

export * from 'https://esm.sh/yoga-layout@3.2.1?bundle';
import * as yoga from 'https://esm.sh/yoga-layout@3.2.1?bundle';
export default yoga;
