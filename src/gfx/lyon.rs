use alloc::vec::Vec;
use core::f32::consts::PI;

use lyon_geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use spin::Once;
use trueos_math::{cos_f32, sin_f32};

#[derive(Copy, Clone, Debug)]
struct MyVertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct CachedIcon {
    cell_px: f32,
    vertices: Vec<MyVertex>,
    indices: Vec<u16>,
}

static ICON_CACHE: Once<Vec<CachedIcon>> = Once::new();

#[inline]
fn to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
fn push_rgb_vtx(out: &mut Vec<u8>, v: &MyVertex, view_w: f32, view_h: f32) {
    let nx = (2.0 * (v.position[0] / view_w)) - 1.0;
    let ny = 1.0 - (2.0 * (v.position[1] / view_h));
    out.extend_from_slice(&nx.to_le_bytes());
    out.extend_from_slice(&ny.to_le_bytes());
    out.push(to_u8(v.color[0]));
    out.push(to_u8(v.color[1]));
    out.push(to_u8(v.color[2]));
    out.push(to_u8(v.color[3]));
}

#[inline]
fn rotate_quadrant(p: (f32, f32), q: u32) -> (f32, f32) {
    let x = p.0 - 16.0;
    let y = p.1 - 16.0;
    let (rx, ry) = match q % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    };
    (rx + 16.0, ry + 16.0)
}

fn build_arrow_path(rotation_quadrants: u32) -> Path {
    let base = [
        (8.0, 14.0),
        (18.0, 14.0),
        (18.0, 8.0),
        (26.0, 16.0),
        (18.0, 24.0),
        (18.0, 18.0),
        (8.0, 18.0),
    ];
    let mut builder = Path::builder();
    let p0 = rotate_quadrant(base[0], rotation_quadrants);
    builder.begin(point(p0.0, p0.1));
    for p in &base[1..] {
        let pr = rotate_quadrant(*p, rotation_quadrants);
        builder.line_to(point(pr.0, pr.1));
    }
    builder.end(true);
    builder.build()
}

fn build_plus_path() -> Path {
    let base = [
        (0.0, 0.0),
        (0.0, 1.0),
        (1.0, 1.0),
        (1.0, 0.0),
        (2.0, 0.0),
        (2.0, -1.0),
        (1.0, -1.0),
        (1.0, -2.0),
        (0.0, -2.0),
        (0.0, -1.0),
        (-1.0, -1.0),
        (-1.0, 0.0),
    ];

    let scale = 5.0f32;
    let cx = 16.0f32;
    let cy = 16.0f32;
    let ux = 0.5f32;
    let uy = -0.5f32;

    let mut builder = Path::builder();
    let p0 = base[0];
    builder.begin(point(cx + (p0.0 - ux) * scale, cy + (p0.1 - uy) * scale));
    for p in &base[1..] {
        builder.line_to(point(cx + (p.0 - ux) * scale, cy + (p.1 - uy) * scale));
    }
    builder.end(true);
    builder.build()
}

fn build_minus_path() -> Path {
    let base = [(2.0, 0.0), (2.0, -1.0), (-1.0, -1.0), (-1.0, 0.0)];

    let scale = 5.0f32;
    let cx = 16.0f32;
    let cy = 16.0f32;
    let ux = 0.5f32;
    let uy = -0.5f32;

    let mut builder = Path::builder();
    let p0 = base[0];
    builder.begin(point(cx + (p0.0 - ux) * scale, cy + (p0.1 - uy) * scale));
    for p in &base[1..] {
        builder.line_to(point(cx + (p.0 - ux) * scale, cy + (p.1 - uy) * scale));
    }
    builder.end(true);
    builder.build()
}

fn build_rect_path() -> Path {
    let mut builder = Path::builder();
    // Match the circle diameter exactly: r=10 => d=20.
    builder.begin(point(6.0, 6.0));
    builder.line_to(point(26.0, 6.0));
    builder.line_to(point(26.0, 26.0));
    builder.line_to(point(6.0, 26.0));
    builder.end(true);
    builder.build()
}

fn build_circle_path() -> Path {
    let mut builder = Path::builder();
    let n = 24usize;
    let r = 10.0f32;
    let cx = 16.0f32;
    let cy = 16.0f32;

    let p0 = point(cx + r, cy);
    builder.begin(p0);
    for i in 1..n {
        let t = (i as f32) * 2.0 * PI / (n as f32);
        builder.line_to(point(cx + r * cos_f32(t), cy + r * sin_f32(t)));
    }
    builder.end(true);
    builder.build()
}

fn build_regular_polygon_path(sides: usize) -> Path {
    let mut builder = Path::builder();
    let r = 11.0f32;
    let cx = 16.0f32;
    let cy = 16.0f32;
    let start = -PI * 0.5;

    let p0 = point(cx + r * cos_f32(start), cy + r * sin_f32(start));
    builder.begin(p0);
    for i in 1..sides {
        let t = start + (i as f32) * 2.0 * PI / (sides as f32);
        builder.line_to(point(cx + r * cos_f32(t), cy + r * sin_f32(t)));
    }
    builder.end(true);
    builder.build()
}

fn build_cached_icons() -> Vec<CachedIcon> {
    const SHADOW_DX: f32 = 1.0;
    const SHADOW_DY: f32 = 1.0;
    const SHADOW_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.22];

    struct BaseGeom {
        main_positions: Vec<[f32; 2]>,
        main_indices: Vec<u16>,
        aa_positions: Vec<[f32; 2]>,
        aa_indices: Vec<u16>,
    }

    let paths = [
        build_arrow_path(0),
        build_arrow_path(1),
        build_arrow_path(2),
        build_arrow_path(3),
        build_plus_path(),
        build_minus_path(),
        build_circle_path(),
        build_rect_path(),
        build_regular_polygon_path(3),
        build_regular_polygon_path(5),
        build_regular_polygon_path(6),
        build_regular_polygon_path(8),
    ];
    let palette = [
        [0.0, 0.0, 0.0, 1.0],
        [0.85, 0.18, 0.18, 1.0],
        [0.12, 0.62, 0.22, 1.0],
        [0.12, 0.32, 0.85, 1.0],
        [0.95, 0.55, 0.12, 1.0],
    ];

    let mut base_geometries: Vec<BaseGeom> = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        let mut main_geometry: VertexBuffers<MyVertex, u16> = VertexBuffers::new();
        let mut aa_geometry: VertexBuffers<MyVertex, u16> = VertexBuffers::new();
        let mut tessellator = StrokeTessellator::new();

        let main_res = tessellator.tessellate_path(
            path,
            &StrokeOptions::default()
                .with_line_width(2.0)
                .with_line_join(LineJoin::Round),
            &mut BuffersBuilder::new(&mut main_geometry, |vertex: StrokeVertex| {
                let p = vertex.position().to_array();
                MyVertex {
                    position: [p[0], p[1]],
                    color: [0.0, 0.0, 0.0, 1.0],
                }
            }),
        );
        let aa_res = tessellator.tessellate_path(
            path,
            &StrokeOptions::default()
                .with_line_width(3.2)
                .with_line_join(LineJoin::Round),
            &mut BuffersBuilder::new(&mut aa_geometry, |vertex: StrokeVertex| {
                let p = vertex.position().to_array();
                MyVertex {
                    position: [p[0], p[1]],
                    color: [0.0, 0.0, 0.0, 1.0],
                }
            }),
        );

        if main_res.is_err() || aa_res.is_err() {
            crate::log!("lyon-demo: tessellation failed at icon={}\n", i);
            base_geometries.push(BaseGeom {
                main_positions: Vec::new(),
                main_indices: Vec::new(),
                aa_positions: Vec::new(),
                aa_indices: Vec::new(),
            });
            continue;
        }

        let mut main_positions: Vec<[f32; 2]> = Vec::with_capacity(main_geometry.vertices.len());
        for v in &main_geometry.vertices {
            main_positions.push(v.position);
        }

        let mut aa_positions: Vec<[f32; 2]> = Vec::with_capacity(aa_geometry.vertices.len());
        for v in &aa_geometry.vertices {
            aa_positions.push(v.position);
        }

        base_geometries.push(BaseGeom {
            main_positions,
            main_indices: main_geometry.indices,
            aa_positions,
            aa_indices: aa_geometry.indices,
        });
    }

    let mut out: Vec<CachedIcon> = Vec::with_capacity(paths.len() * palette.len() * 2);
    for (icon_px, scale) in [(32.0f32, 1.0f32), (16.0f32, 0.5f32)] {
        for color in palette {
            let aa_color = [color[0], color[1], color[2], 0.26];
            for geom in &base_geometries {
                let mut baked_vertices: Vec<MyVertex> =
                    Vec::with_capacity(geom.main_positions.len() * 2 + geom.aa_positions.len());

                for &p in &geom.aa_positions {
                    baked_vertices.push(MyVertex {
                        position: [p[0] * scale, p[1] * scale],
                        color: aa_color,
                    });
                }

                for &p in &geom.main_positions {
                    baked_vertices.push(MyVertex {
                        position: [
                            p[0] * scale + SHADOW_DX * scale,
                            p[1] * scale + SHADOW_DY * scale,
                        ],
                        color: SHADOW_COLOR,
                    });
                }

                for &p in &geom.main_positions {
                    baked_vertices.push(MyVertex {
                        color,
                        position: [p[0] * scale, p[1] * scale],
                    });
                }

                let aa_offset = 0u16;
                let shadow_offset = geom.aa_positions.len() as u16;
                let main_offset = (geom.aa_positions.len() + geom.main_positions.len()) as u16;
                let mut baked_indices: Vec<u16> =
                    Vec::with_capacity(geom.aa_indices.len() + geom.main_indices.len() * 2);
                for &idx in &geom.aa_indices {
                    baked_indices.push(idx + aa_offset);
                }
                for &idx in &geom.main_indices {
                    baked_indices.push(idx + shadow_offset);
                }
                for &idx in &geom.main_indices {
                    baked_indices.push(idx + main_offset);
                }

                out.push(CachedIcon {
                    cell_px: icon_px,
                    vertices: baked_vertices,
                    indices: baked_indices,
                });
            }
        }
    }
    out
}

fn cached_icons() -> &'static [CachedIcon] {
    ICON_CACHE.call_once(build_cached_icons).as_slice()
}

pub fn lyon_geom_api_demo_no_present(view_w: u32, view_h: u32) -> bool {
    let fb_w = view_w.max(1) as f32;
    let fb_h = view_h.max(1) as f32;
    let icons = cached_icons();
    let mut total_vertices = 0usize;
    let mut total_indices = 0usize;
    let mut first_draw_err: i32 = 0;
    let mut cursor_x = 0.0f32;
    let mut cursor_y = 0.0f32;
    let mut row_h = 0.0f32;

    for (i, icon) in icons.iter().enumerate() {
        let cell_px = icon.cell_px;
        if cursor_x + cell_px > fb_w && cursor_x > 0.0 {
            cursor_x = 0.0;
            cursor_y += row_h;
            row_h = 0.0;
        }
        let ox = cursor_x;
        let oy = cursor_y;
        let mut icon_blob: Vec<u8> = Vec::with_capacity(icon.indices.len().saturating_mul(12));

        total_vertices = total_vertices.saturating_add(icon.vertices.len());
        total_indices = total_indices.saturating_add(icon.indices.len());

        for &idx in &icon.indices {
            if let Some(v) = icon.vertices.get(idx as usize) {
                let vv = MyVertex {
                    position: [v.position[0] + ox, v.position[1] + oy],
                    color: v.color,
                };
                push_rgb_vtx(&mut icon_blob, &vv, fb_w, fb_h);
            }
        }

        let draw_rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
                icon_blob.as_ptr(),
                icon_blob.len(),
            )
        };
        if draw_rc != 0 {
            if first_draw_err == 0 {
                first_draw_err = draw_rc;
            }
            crate::log!(
                "lyon-demo: submit rc draw={} icon={} bytes={}\n",
                draw_rc,
                i,
                icon_blob.len()
            );
        }

        cursor_x += cell_px;
        row_h = row_h.max(cell_px);

        if cursor_y > fb_h {
            break;
        }
    }

    crate::log!(
        "lyon-demo: icons={} outlines=3px vertices={} indices={} (cached)\n",
        icons.len(),
        total_vertices,
        total_indices
    );

    first_draw_err == 0
}
