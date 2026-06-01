SVG.js VM notes
===============

The runtime-visible SVG.js modules live in `src/app/svgjs/` because
`crates/trueos-qjs/build.rs` embeds `src/app/**/*.mjs` as `/qjs/**/*.mjs`.

Keep this directory for Rust/native SVG.js host glue if that grows later.
