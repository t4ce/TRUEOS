# Read. Build. Repeat.

A buildless HTML/CSS interpretation of the Hull concept artwork, intended to run
inside native TRUEOS Hull. Text, cards, borders and side panes are real HTML/CSS;
the concept image is not used as the page background.

Open `index.html` in an ordinary browser, or copy this entire directory into an
Edit HTML-preview project's `public/` directory and open its preview URL in Hull.
Keep `assets/` beside the HTML. All CSS is embedded in `index.html`. A host-served copy needs a URL and
scheme that the target Hull build accepts; the normal resource loader uses HTTPS,
while the TRUEOS IDE preview route can provide local project resources.

For a host-only browser preview:

```sh
cd tools/aiupsell/page
python3 -m http.server 8080
```

Then open `http://localhost:8080/`. There is no install, build step, JavaScript,
web font, tracker or external image dependency.

The composition is designed for 1280×720 and 2560×1440 content viewports. It uses
solid background colors, viewport lengths, absolute panel placement, simple flex
rows, borders and radii supported by Hull's current CSS/Taffy implementation.
Native rendering uses approximate text metrics and may draw rounded borders as
chords; it does not yet reproduce the clipped rounded background fills of a
mainstream browser. After resizing or switching Hull to fullscreen, reload the
page so its viewport-based sizes are recomputed. Small phone widths are outside
this desktop demo's scope.

Validated using Hull's production parser/CSS/Taffy host sandbox at both target
viewports: zero HTML parse errors, zero unsupported CSS rules, two image boxes,
and a document extent equal to the viewport. The sandbox does not decode images
or exercise native GPU painting; a real Hull capture checks those separately.

The Edit source and Codex conversation are illustrative page content. Article
cards contain short local summaries and link to the actual Wikipedia articles,
[Artificial intelligence](https://en.wikipedia.org/wiki/Artificial_intelligence)
and [Sam Altman](https://en.wikipedia.org/wiki/Sam_Altman). The biography card is
typographic artwork, not a photograph. This is an independent TRUEOS page.

`assets/lilly.png` is copied from the existing TRUEOS README mascot asset.
`assets/neural-network.svg` is original, code-authored diagram artwork;
`assets/neural-network.png` is its locally drawn raster counterpart for native
image-loader compatibility. The page uses the PNG.

## Live rig copy

The page is uploaded in its own `aiupsell/` subdirectory of the existing preview
server. In Hull on the rig, open:

```text
http://127.0.0.1:50000/aiupsell/index.html
```

Its current TRUEOS filesystem location is:

```text
/apps/common/ide/previews/1793689247000000000-1/public/aiupsell/index.html
```

`rig-location.json` records this instance-specific location. The preview server
must remain running; a new preview session may use a different port/root.
The existing project's source files were not replaced.

Verified on the native GPU/display at 2560×1440, including both local images.
[Real display capture](../screenshots/hull-live-page.png) is included below the
concept artwork in the repository README. Native borders use clipped corners
and the page uses a typographic Sam Altman card; it is a working interpretation,
not a pixel-for-pixel reproduction of the generated illustration.
