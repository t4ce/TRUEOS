# Blueprint vendor dependency usage — 2026-10-22

Inventoried **87 packages** in Blueprint vendor folders. Resolved **58/59** app compilation-unit graphs; protected the remaining app with its lockfile closure. Scanned 531 local manifests for dependency declarations and patch registrations.

| Result | Packages |
| --- | ---: |
| required | 70 |
| stale vendor candidate | 4 |
| optional terminal backend | 3 |
| example package | 1 |
| other platform backend | 9 |

## Vendor packages absent from the configured app builds

| Package | Version | Source KiB | Assessment |
| --- | --- | ---: | --- |
| `crossfont` | 0.9.0 | 126 | No selected graph or local dependency declaration; old font integration candidate. |
| `hickory-proto` | 0.25.2 | 1760 | Voxy selects registry 0.26.3; vendor 0.25.2 patch is unused. |
| `hickory-resolver` | 0.25.2 | 338 | Voxy selects registry 0.26.3; vendor 0.25.2 patch is unused. |
| `ratatui-termina` | 0.1.0 | 57 | optional terminal backend |
| `ratatui-termion` | 0.1.2 | 43 | optional terminal backend |
| `ratatui-termwiz` | 0.1.2 | 66 | optional terminal backend |
| `rayon-demo` | 0.0.0 | 156 | example package |
| `taffy` | 0.15.0 | 1328 | Hull selects registry 0.7.7; vendor 0.15.0 is unused. |
| `winit-android` | 0.31.0-beta.3 | 75 | other platform backend |
| `winit-appkit` | 0.31.0-beta.3 | 329 | other platform backend |
| `winit-common` | 0.31.0-beta.3 | 117 | Shared support for non-TRUEOS backends, absent from the configured target graphs. |
| `winit-orbital` | 0.31.0-beta.3 | 55 | other platform backend |
| `winit-uikit` | 0.31.0-beta.3 | 133 | other platform backend |
| `winit-wayland` | 0.31.0-beta.3 | 314 | other platform backend |
| `winit-web` | 0.31.0-beta.3 | 265 | other platform backend |
| `winit-win32` | 0.31.0-beta.3 | 475 | other platform backend |
| `winit-x11` | 0.31.0-beta.3 | 372 | other platform backend |

## Dependencies which must not be mistaken for unused vendors

- `tokio-macros` and `tracing-attributes` are host-side proc macros used to build apps.
- `tower` is consumed through packer-generated source-overlay copies. Its absence as a direct vendor path is not non-use.
- `libloading` has lockfile dependency evidence from `rustc-min`; it is retained as unresolved target/build usage.

The Winit rows and three alternate Ratatui backends are not selected by the current TRUEOS app builds. They remain optional/cross-platform support, rather than unconditional dead-code findings. `rayon-demo` is an example package.

## Coverage and reproduction

Cargo compilation unit graphs reconstructed from existing staged manifests, packer production feature planning, and source-overlay configs; offline/locked, no compilation. Catalog defaults and source feature directives, not all optional features or all OS targets. A failed unit graph is protected by a conservative lockfile dependency closure including host/optional/other-target edges. Existing staging can lag original sources; this is dependency selection, not final linked-code reachability. No code deletion.

`rustc-min` cannot resolve its unit graph offline because `thin-vec 0.2.19` is missing from the local registry cache. Its lockfile closure is used conservatively; no package reachable there is called unused.

Patch registrations and `[workspace.dependencies]` entries are recorded separately from consumer dependency declarations in JSON. A patch entry alone is not use. Direct declarations can still be optional, dev-only, or target-specific; version requirements are preserved for review. Source sizes exclude target/build/.git directories and nested package roots; they include docs/tests shipped with each package.

Full package/app mappings and the exact query commands are in the adjacent JSON and TSV. Raw graph diagnostics are saved under `/tmp/trueos-blueprint-vendor-graphs/`.

```sh
python3 tools/audit/blueprint_vendor_usage.py --out tools/docs/blueprint-vendor-usage-2026-10-22
```
