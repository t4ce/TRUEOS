// Stable local entrypoint for parse5.
//
// This mirrors the Pixi vendor module pattern: consumers import a local
// /qjs/vendor path while the version pin stays centralized here.
//
// Source URL (pinned):
//   https://esm.sh/parse5@7.2.1?bundle

export * from 'https://esm.sh/parse5@7.2.1?bundle';
import * as parse5 from 'https://esm.sh/parse5@7.2.1?bundle';
export default parse5;
