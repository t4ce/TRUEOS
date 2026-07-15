# Personal Model Archive Review

Review date: 2026-07-15

Source archive: `/home/t4ce/Downloads/OneDrive_1_15.7.2026`

The source archive was treated as read-only. Generated previews, compact exports, and draw3d
captures were written below `bld/model-archive-review` and `bld/draw3d-captures` in the TRUEOS
repository.

## Executive summary

The archive contains 541 MB across 900 files and 35 top-level folders. It is more varied than a
collection of Blender files: it combines authored Blender studies, printable exports, slicer
projects, source packages downloaded from model sites, a fluid simulation cache, and a small
Blender 3MF add-on checkout.

The strongest coherent personal-work thread is:

- low-poly game/environment studies (`GameModels`, `Graph`, `Icosido`, `HumanRage`);
- character/rig experimentation (`guy sword`);
- increasingly complex functional assemblies (`MagStirr`, `RemotecontrolCase`, `VorhangHalter`);
- personalized/product forms (`alissa ring`, `BottleCap`, lamp, pump, brackets, and holders).

Five projects were selected for the first live retrospective scene:

1. `GameModels/FloatingIsland.blend` — the strongest environment composition;
2. `guy sword/guy_sword.blend` — the clearest character/rig experiment;
3. `MagStirr/MagnetHolder.blend` — a dense mechanical assembly with 37 mesh objects;
4. `RemotecontrolCase/RCON.blend` — a PCB/case assembly with 19 material groups;
5. `GameModels/CarvedSphere/CarvedSphere.blend` — a compact early low-poly form study.

## File-format inventory

| Count | Extension | Interpretation |
|---:|---|---|
| 603 | `.gz` | Blender fluid surface/velocity cache frames (`.bobj.gz` and `.bvel.gz`) |
| 84 | `.stl` | Printable triangle meshes, both binary and ASCII |
| 44 | `.jpg` | Product/reference renders and photographs |
| 37 | `.blend` | Primary Blender projects |
| 31 | `.blend1` | Blender automatic previous-save backups |
| 24 | `.png` | Renders, screenshots, and project previews |
| 22 | `.gcode` | Printer-specific sliced toolpaths, not reusable source geometry |
| 13 | `.txt` | Package notes, licenses, or slicer metadata |
| 10 | `.3mf` | Printable meshes and/or slicer project configuration |
| 8 | `.zip` | Downloaded source packages |
| 7 | `.py` | Blender 3MF add-on source |
| 4 | `.html` | Download-package landing pages |
| 2 each | `.x3d`, `.obj`, `.mtl`, `.hdr` | Portable scene/mesh and environment assets |
| 1 each | `.scad`, `.rar`, `.pdf`, `.glb`, `.fbx` | Parametric source, archive, documentation, and exchange formats |

The 603 `.gz` files are not unexplained duplicate archives. They form the `Water/cache_fluid`
simulation sequence and should remain beside `water.blend` if that simulation is reopened.

## Blender compatibility and scene probe

The primary Blender files span versions 2.80 through 4.05. All 37 primary `.blend` files opened
successfully, without resaving, in the already-installed Blender 5.1.2 build. The probe collected
object names, object types, vertex/polygon counts, materials, image references, and world bounds.

Notable results:

| Project | Mesh objects | Vertices | Polygons | What the data revealed |
|---|---:|---:|---:|---|
| Floating Island | 14 | 5,615 | 10,658 | Land, water, clouds, two tree families, cameras, light, probe |
| Carved Sphere | 3 | 414 | 325 | Deliberately compact low-poly sphere/form study |
| HumanRage | 11 | 2,564 | 2,411 | Colored named pieces (`Blacky`, `Bluey`, `Greeny`, etc.) and board |
| Icosido | 11 | 332 | 273 | Sparse geometric scene with repeated `icosido` objects |
| Guy Sword | 9 | 2,876 | 2,772 | Nine meshes plus two armatures |
| Magnetic Stirrer | 37 | 26,354 | 22,765 | Multi-part enclosure/mechanism, not a single printable shell |
| Remote Control | 9 | 10,162 | 19,875 | PCB assembly, case planes, and 19 named materials |
| Curtain stepper model | 23 | 47,789 | 46,625 | Detailed motor, connector, wiring, cable, and material library |
| Alissa Ring | 11 | 89,275 | 93,482 | Several torus/text variants and personalized lettering |
| Water | 3 | 4,050 | 4,108 | Fluid-domain study backed by the 603 cached frames |

## Folder-by-folder interpretation

| Folder | Interpretation | Presentation/use status |
|---|---|---|
| `Blender3mfFormat-1.0.2` | Seven-file Blender 3MF import/export add-on | Tooling, not artwork |
| `BottleCap` | PET bottle-cap Blender source, STL, and G-code | Functional, printable |
| `Cute+Mini+Octopus+` | Empty directory | No recoverable asset |
| `Futti` | Forty-two-part Blender assembly with holder/screw/ring names and STL | Functional assembly; identity needs owner context |
| `GameModels` | Floating island, carved sphere, ball, and shader/form experiments | Best artistic folder; two pieces selected |
| `Gear_Bearing` | OpenSCAD source and two large ASCII STLs | Parametric/printable; likely packaged reference |
| `Graph` | Icosphere/cylinder/plane composition | Geometric study; visually sparse |
| `HumanRage` | Colored board/piece scene | Game-study candidate for a later grouped scene |
| `Icosido` | Repeated low-poly geometric objects | Early abstraction/form study |
| `LampeDecke` | Ceiling-lamp mount in Blender, STL, and GLB | Clean reusable functional model |
| `MagStirr` | Magnetic stirrer enclosure/mechanism and printable subparts | Selected mechanical exhibit |
| `Magnethalter` | Several magnet-holder iterations and STL | Functional iteration history |
| `Minimalist contemporary chess set v2` | Complete curved chess family, STLs, slicer project, renders | Presentation-ready package; likely downloaded/reference structure |
| `Minimalist+contemporary+chess+set+v2` | Empty alias directory | No additional asset |
| `Modern Medieval Low-Poly Chess Set` | Complete faceted chess family with polished renders | Strong package; likely downloaded/reference structure |
| `Modern+Medieval+Low-Poly+Chess+Set` | Empty alias directory | No additional asset |
| `Neuer Ordner` | Bottle-cap experiments plus several downloaded ZIP packages | Mixed scratch/reference folder |
| `NewRap` | Default cube-only Blender scene | Placeholder, not a presentation candidate |
| `RemotecontrolCase` | Remote PCB, case, lid, slate, snaps, and printable shells | Selected product/system exhibit |
| `Schach` | Pawn 3MF and slicer screenshots | Printing/configuration fragment |
| `Schrankhalter` | Cabinet bracket in Blender/STL/3MF | Functional printable |
| `Trichter` | High-resolution funnel in Blender, OBJ, STL, and MTL | Reusable but mechanically simple; very dense exports |
| `V29` | Whistle STLs, photos, notes, and package HTML | Downloaded/reference package structure |
| `VorhangHalter` | Curtain automation: stepper motor, PCB mounts, clips, holders, FBX/STLs/G-code | Deepest engineering iteration history |
| `Water` | Blender fluid test plus 603 cached surface/velocity frames | Simulation study; cache is meaningful |
| `alissa ring` | Personalized torus/text ring variants, STL and 3MF | Personal/product form; high polygon count |
| `chess` | High-density pawn Blender file plus pawn/rook STLs | Separate chess modeling experiment |
| `cube` | Minimal X3D cube | Format test |
| `grinder` | Complete turbine grinder download with many previews and variants | Downloaded/reference package structure |
| `guy sword` | Rigged low-poly humanoid/sword scene | Selected character exhibit |
| `laufrad` | Multi-part wheel assembly and STL | Mechanical motion/form study |
| `ocarina` | Twelve-hole ocarina STL, ZIP, previews, and package HTML | Downloaded/reference package structure |
| `pump` | Two Blender pump studies and X3D export | Compact functional study |
| `solis-planter-model_files` | Planter STL/3MF and documentation PDF | Downloaded/reference package structure |
| `spinner` | Air-spinner STLs, ZIPs, previews, and package HTML | Downloaded/reference package structure |

The provenance labels above are intentionally cautious. A `files/images/*.html` package layout is
strong evidence of a model-site download, but it does not prove who authored the underlying design.
The review therefore distinguishes package structure from personal authorship instead of claiming
ownership either way.

## Conversion and draw3d presentation

The selected Blender scenes were converted with the following read-only pipeline:

1. Blender 5.1.2 opens the original `.blend` without saving it.
2. Evaluated mesh objects are triangulated, transformed into world space, and grouped by material.
3. Coordinates are converted from Blender Z-up to draw3d Y-up.
4. Exact duplicates are removed; oversized groups use deterministic vertex clustering until each
   remains below 950 vertices and 1,800 triangles.
5. Compact gzip JSON exports are loaded by the TCP gallery generator.

The final scene contains 57 meshes/instances, 19,414 vertices, 27,839 faces, and 632,798 bytes of
stored mesh data. Forty-six meshes are converted archive geometry; eleven are gallery floor,
pedestal, trim, arch, and mote geometry.

The island required one presentational adaptation. Its entire land mass used a white material and
derived most depth from Blender lighting. Since draw3d currently has flat per-mesh color and no
lighting, the same triangles were divided into four non-overlapping normal/height strata: snow,
sunlit rock, ochre mid-rock, and dark underside. Geometry and silhouette were not redesigned.

## Generated artifacts

- `bld/model-archive-review/blender-previews-contact.jpg` — neutral Blender shortlist sheet;
- `bld/model-archive-review/archive-album-contact.jpg` — fifteen-view isolated object album;
- `bld/model-archive-review/*-contact.jpg` — archived-image contact sheets;
- `bld/model-archive-review/exports/*.json.gz` — compact draw3d-safe geometry exports;
- `bld/draw3d-captures/archive-gallery-collection.png` — full retrospective;
- `bld/draw3d-captures/archive-gallery-island.png` — environment close-up;
- `bld/draw3d-captures/archive-gallery-character.png` — character close-up;
- `bld/draw3d-captures/archive-gallery-mechanical.png` — mechanical close-up;
- `bld/draw3d-captures/archive-gallery-studies.png` — rear/form-study angle.

Reusable scripts:

- `tools/blender_archive_probe.py` — Blender metadata probe;
- `tools/blender_archive_preview.py` — neutral studio preview generator;
- `tools/blender_archive_export.py` — material-aware draw3d export;
- `tools/draw3d_archive_gallery.py` — TCP gallery scene generator.
- `tools/draw3d_archive_album.py` — adaptive three-view TCP album generator.

The individual album view index is in `MODEL_ARCHIVE_ALBUM.md`.

## API observation

An exactly rear-facing camera produced no newly presented geometry, after which opcode `0x23`
returned the previous byte-identical frame. A diagonal rear camera rendered normally. This is not a
new API finding: it is a practical reproduction of `ARTISTIC_API_REPORT.md` section 3, which already
documents that `request render` returns the cached presented frame rather than synchronously forcing
a fresh render. The gallery therefore uses the diagonal rear angle and no duplicate report item was
added.

## Recommended next archive passes

Good focused follow-ups would be:

- a dedicated `VorhangHalter` engineering exploded view;
- a chess retrospective comparing the smooth minimalist and faceted medieval families;
- a game-studies scene combining `HumanRage`, `Icosido`, `Graph`, and `CarvedSphere`;
- a product-form scene for the ring, lamp mount, pump, bottle cap, and magnet holders;
- a Blender-native fluid render from the `Water` cache, which draw3d cannot presently reproduce
  with equivalent surface shading.
