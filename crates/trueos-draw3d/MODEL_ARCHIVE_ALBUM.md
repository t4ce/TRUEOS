# Personal Model Archive — Object Album

Album date: 2026-07-15

Source archive: `/home/t4ce/Downloads/OneDrive_1_15.7.2026`

This is the object-focused companion to `MODEL_ARCHIVE_REVIEW.md`. Each selected project has an
isolated presentation stage and three 2560×1440 TCP-rendered views. The source archive remained
read-only; all converted geometry and images live in the TRUEOS repository.

![Five-object album contact sheet](../../bld/model-archive-review/archive-album-contact.jpg)

## View set

| Object | Source project | Three-quarter | Opposite turn | Elevated detail |
|---|---|---|---|---|
| Floating Island | `GameModels/FloatingIsland.blend` | [view 1](../../bld/draw3d-captures/archive-album/floating-island/floating-island-01-three-quarter.png) | [view 2](../../bld/draw3d-captures/archive-album/floating-island/floating-island-02-opposite-turn.png) | [view 3](../../bld/draw3d-captures/archive-album/floating-island/floating-island-03-elevated-detail.png) |
| Guy Sword | `guy sword/guy_sword.blend` | [view 1](../../bld/draw3d-captures/archive-album/guy-sword/guy-sword-01-three-quarter.png) | [view 2](../../bld/draw3d-captures/archive-album/guy-sword/guy-sword-02-opposite-turn.png) | [view 3](../../bld/draw3d-captures/archive-album/guy-sword/guy-sword-03-elevated-detail.png) |
| Magnetic Stirrer | `MagStirr/MagnetHolder.blend` | [view 1](../../bld/draw3d-captures/archive-album/magnetic-stirrer/magnetic-stirrer-01-three-quarter.png) | [view 2](../../bld/draw3d-captures/archive-album/magnetic-stirrer/magnetic-stirrer-02-opposite-turn.png) | [view 3](../../bld/draw3d-captures/archive-album/magnetic-stirrer/magnetic-stirrer-03-elevated-detail.png) |
| Carved Sphere | `GameModels/CarvedSphere/CarvedSphere.blend` | [view 1](../../bld/draw3d-captures/archive-album/carved-sphere/carved-sphere-01-three-quarter.png) | [view 2](../../bld/draw3d-captures/archive-album/carved-sphere/carved-sphere-02-opposite-turn.png) | [view 3](../../bld/draw3d-captures/archive-album/carved-sphere/carved-sphere-03-elevated-detail.png) |
| Remote Control | `RemotecontrolCase/RCON.blend` | [view 1](../../bld/draw3d-captures/archive-album/remote-control/remote-control-01-three-quarter.png) | [view 2](../../bld/draw3d-captures/archive-album/remote-control/remote-control-02-opposite-turn.png) | [view 3](../../bld/draw3d-captures/archive-album/remote-control/remote-control-03-elevated-detail.png) |

The first view is the album portrait, the second reveals the opposite silhouette and rear structure,
and the third looks down into top surfaces and interior parts. Camera distance is derived from each
object's measured bounds, so tiny form studies and wide assemblies receive comparable framing.

## Isolated scene sizes

| Object | Scene meshes | Faces |
|---|---:|---:|
| Floating Island | 20 | 10,161 |
| Guy Sword | 16 | 6,071 |
| Magnetic Stirrer | 11 | 4,920 |
| Carved Sphere | 8 | 2,782 |
| Remote Control | 26 | 10,785 |

The counts include the reusable pedestal, rings, arches, pylons, and motes. Those stage elements are
deliberately consistent across the album; only their scale and accent color adapt to the subject.

## Album artifacts

- `bld/model-archive-review/archive-album-contact.jpg` — all fifteen views on one sheet;
- `bld/draw3d-captures/archive-album/*-contact.jpg` — one three-view sheet per object;
- `bld/draw3d-captures/archive-album/<object>/*.png` — full-resolution individual views;
- `bld/draw3d-captures/archive-album/archive-album-final-live.png` — persisted live portrait;
- `tools/draw3d_archive_album.py` — reproducible TCP album generator.

## API note

During the first framing pass, a few invalid camera positions yielded the previously presented frame.
This is the already-documented cached-frame behavior in `ARTISTIC_API_REPORT.md` section 3, not a new
API issue. The final camera set stays outside each model's measured bounds, and all fifteen final PNGs
have distinct content hashes.
