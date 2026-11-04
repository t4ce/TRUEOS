# README visual bundle

Prepared on 2026-11-03. The root README uses this folder's checked-in assets;
[readme.md](readme.md) is a locally linked copy of the same presentation.

## Real captures

- `screenshots/hull-live-page.png`: follow-up capture of the actual HTML/CSS
  implementation in native Hull, maximized at 2560×1440. Source is in `page/`;
  it is not the generated concept image displayed as a webpage. Local mascot
  and neural-network PNGs loaded successfully. Colors, text, cards, and borders
  were inspected on the native display. Header dots use CSS shapes so missing
  Unicode glyphs do not appear. The hardware Lilly overlay remains visible.
  The page was left open in Hull as requested; the floating shell and delayed
  editor were collapsed to reveal it. The original project's files were preserved.


- `screenshots/codex-edit-lilly.png`: unedited native `pic` screenshot fetched
  over HTTP from the rig's TRUEOSFS service. Original remote filename:
  `screenshots/wd-postblend-xyuv709-1793689889-1455594-wd000001-capture000001.png`.
  Shows an already signed-in Codex session, Microsoft Edit, and Lilly. This is
  not a capture of the authentication process; no sign-in credentials or device
  codes were staged for publication.
- `screenshots/cubes-fullscreen.png`: one decoded frame from TRUEOS-RDP's actual
  display feed, converted from RGBA to PNG. Cubes was maximized and its world
  scene selected. A temporary solid-color image was used while staging the
  scene. No video file was created. Display content has not been reconstructed
  or replaced in post-production.
- `screenshots/cubes-rubik-cube.png`: unchanged copy of a local capture (original
  filename `wd-postblend-xyuv709-1793789466-256473-wd000001-capture000003.png`,
  2560x1440 RGBA). Shows the Cubes scene with a 3x3 cube on the neon grid and
  Lilly beside it. It replaces `cubes-fullscreen.png` on the root README; the
  earlier capture stays because this folder's `readme.md` still uses it.

The first shell `pic` capture succeeded. Subsequent requests remained pending
and another request reported `WD screenshot slot is busy`; the RDP still-frame
path supplied the fullscreen capture. This task made no capture-service changes.

## Illustration and local demos

- `assets/hull-wikipedia-concept.png`: AI-generated concept illustration, with
  a visible concept label. It is not evidence of the browser's current layout,
  Wikipedia compatibility, or Microsoft/OpenAI endorsement. Lilly's existing
  sprite was supplied as the character reference.
- `assets/lilly.png`: original frame extracted from the repository's embedded
  `tools/Lilly.tar.lz4`, at `Lilly/Idea/finger-up_frames/frame_07.png`.
- `assets/get-started.svg`: authored download/boot/sign-in/build diagram,
  not an authentication screenshot.
- `assets/backdrop.png`: plain staging background used with the image viewer.
- `wiki/artificial-intelligence.html` and `wiki/sam-altman.html`: authored,
  locally bundled summaries with links to Wikipedia. The source articles were
  fetched over HTTPS for reference; these files do not reproduce Wikipedia's UI.
- `wiki/reader.js`: optional DOM-based Hull console demo of those topics. Its
  live rendering was not verified, so it is not advertised as a successful demo.

The full AI Wikipedia page downloaded in Hull, but a usable rendered capture
was not obtained. Attempts to open a separate Edit HTML project timed out;
no project source was written by those attempts. A delayed extra editor instance
appeared during cleanup; it was left open to preserve project state. The browser illustration is
explicitly labeled rather than presented as a successful live capture.

The user requested fullscreen visual scenes after noting a concurrent remote
session. Existing project source was preserved. Extra scene/browser/image
instances were used for staging. The Cubes, image-viewer, and extra Hull staging
instances were stopped afterward; no kernel rebuild, deployment, or reboot was
performed by this task.

## References

- [Artificial intelligence — Wikipedia](https://en.wikipedia.org/wiki/Artificial_intelligence)
- [Sam Altman — Wikipedia](https://en.wikipedia.org/wiki/Sam_Altman)
- [Microsoft Edit](https://github.com/microsoft/edit)
- [Blueprint catalog](https://github.com/t4ce/TRUEOS-Blueprints/blob/main/apps.json)
- [Built-in catalog](https://github.com/t4ce/TRUEOS-Buildins/blob/main/buildins.json)

`manifest.json` records the local assets' dimensions, sizes, and SHA-256 hashes.

## Follow-up native page

The earlier standalone reading-script attempt was superseded by `page/index.html`.
Sol at high effort implemented the real HTML/CSS page, checked with Hull's
production parser/layout sandbox at 1280×720 and 2560×1440: no parse errors,
unsupported CSS rules, or document overflow. The completed native display
capture appears directly below the concept illustration in both READMEs.
The page lives in an isolated folder under the existing local HTML preview root;
see `page/rig-location.json`. No new public or cloud hosting was created.
