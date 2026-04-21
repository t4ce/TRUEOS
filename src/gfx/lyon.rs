use alloc::vec::Vec;
use core::f32::consts::PI;

use lyon_geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};
use spin::Once;
use trueos_gfx_core::{
    RGB_VERTEX_SIZE, RgbVertexPx, Rgba8, ViewTransform, push_indexed_rgb_mesh_px, push_rgb_quad_px,
};
use trueos_math::{cos_f32, sin_f32};

#[derive(Copy, Clone, Debug)]
struct MyVertex {
    position: [f32; 2],
    color: Rgba8,
}

struct CachedIcon {
    cell_px: f32,
    vertices: Vec<MyVertex>,
    indices: Vec<u16>,
}

static ICON_CACHE: Once<Vec<CachedIcon>> = Once::new();
const ICON_SHAPE_COUNT: usize = 12;
const ICON_PALETTE_COUNT: usize = 5;

#[inline]
fn rgb_from_f32(r: f32, g: f32, b: f32, a: f32) -> Rgba8 {
    Rgba8::new(
        (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
    )
}

#[inline]
fn to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
fn icon_vertices_px(icon: &CachedIcon, x: f32, y: f32, scale: f32, alpha: u8) -> Vec<RgbVertexPx> {
    let mut out = Vec::with_capacity(icon.vertices.len());
    for v in &icon.vertices {
        out.push(RgbVertexPx {
            x: v.position[0] * scale + x,
            y: v.position[1] * scale + y,
            color: v.color.scale_alpha(alpha),
        });
    }
    out
}

#[inline]
fn submit_rgb_blob_no_present(blob: &[u8]) -> bool {
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(blob.as_ptr(), blob.len())
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc == 0
}

pub fn draw_solid_rect_no_present(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rgba: (u8, u8, u8, u8),
    view_w: u32,
    view_h: u32,
) -> bool {
    if w <= 0.0 || h <= 0.0 {
        return true;
    }

    let mut blob: Vec<u8> = Vec::with_capacity(6 * RGB_VERTEX_SIZE);
    push_rgb_quad_px(
        &mut blob,
        ViewTransform::from_extent(view_w, view_h),
        x,
        y,
        x + w,
        y + h,
        Rgba8::new(rgba.0, rgba.1, rgba.2, rgba.3),
    );

    submit_rgb_blob_no_present(blob.as_slice())
}

pub fn draw_horizontal_three_stop_rect_no_present(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    left_rgba: (u8, u8, u8, u8),
    mid_rgba: (u8, u8, u8, u8),
    right_rgba: (u8, u8, u8, u8),
    mid_offset: f32,
    view_w: u32,
    view_h: u32,
) -> bool {
    if w <= 0.0 || h <= 0.0 {
        return true;
    }

    let x0 = x;
    let xm = x + w * mid_offset.clamp(0.0, 1.0);
    let x1 = x + w;
    let y0 = y;
    let y1 = y + h;
    let transform = ViewTransform::from_extent(view_w, view_h);
    let mid_color = Rgba8::new(mid_rgba.0, mid_rgba.1, mid_rgba.2, mid_rgba.3);
    let left_color = Rgba8::new(left_rgba.0, left_rgba.1, left_rgba.2, left_rgba.3);
    let right_color = Rgba8::new(right_rgba.0, right_rgba.1, right_rgba.2, right_rgba.3);
    let vertices = [
        RgbVertexPx {
            x: x0,
            y: y0,
            color: left_color,
        },
        RgbVertexPx {
            x: xm,
            y: y0,
            color: mid_color,
        },
        RgbVertexPx {
            x: x1,
            y: y0,
            color: right_color,
        },
        RgbVertexPx {
            x: x0,
            y: y1,
            color: left_color,
        },
        RgbVertexPx {
            x: xm,
            y: y1,
            color: mid_color,
        },
        RgbVertexPx {
            x: x1,
            y: y1,
            color: right_color,
        },
    ];
    let indices: [u16; 12] = [0, 1, 4, 0, 4, 3, 1, 2, 5, 1, 5, 4];
    let mut blob: Vec<u8> = Vec::with_capacity(indices.len() * RGB_VERTEX_SIZE);
    push_indexed_rgb_mesh_px(&mut blob, transform, &vertices, &indices);

    submit_rgb_blob_no_present(blob.as_slice())
}

pub fn draw_horizontal_four_stop_rect_no_present(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    left_left_rgba: (u8, u8, u8, u8),
    left_rgba: (u8, u8, u8, u8),
    mid_rgba: (u8, u8, u8, u8),
    right_rgba: (u8, u8, u8, u8),
    view_w: u32,
    view_h: u32,
) -> bool {
    if w <= 0.0 || h <= 0.0 {
        return true;
    }

    let x0 = x;
    let x1 = x + (w / 3.0);
    let x2 = x + ((w * 2.0) / 3.0);
    let x3 = x + w;
    let y0 = y;
    let y1 = y + h;
    let transform = ViewTransform::from_extent(view_w, view_h);
    let left_left_color =
        Rgba8::new(left_left_rgba.0, left_left_rgba.1, left_left_rgba.2, left_left_rgba.3);
    let left_color = Rgba8::new(left_rgba.0, left_rgba.1, left_rgba.2, left_rgba.3);
    let mid_color = Rgba8::new(mid_rgba.0, mid_rgba.1, mid_rgba.2, mid_rgba.3);
    let right_color = Rgba8::new(right_rgba.0, right_rgba.1, right_rgba.2, right_rgba.3);
    let vertices = [
        RgbVertexPx {
            x: x0,
            y: y0,
            color: left_left_color,
        },
        RgbVertexPx {
            x: x1,
            y: y0,
            color: left_color,
        },
        RgbVertexPx {
            x: x2,
            y: y0,
            color: mid_color,
        },
        RgbVertexPx {
            x: x3,
            y: y0,
            color: right_color,
        },
        RgbVertexPx {
            x: x0,
            y: y1,
            color: left_left_color,
        },
        RgbVertexPx {
            x: x1,
            y: y1,
            color: left_color,
        },
        RgbVertexPx {
            x: x2,
            y: y1,
            color: mid_color,
        },
        RgbVertexPx {
            x: x3,
            y: y1,
            color: right_color,
        },
    ];
    let indices: [u16; 18] = [0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6];
    let mut blob: Vec<u8> = Vec::with_capacity(indices.len() * RGB_VERTEX_SIZE);
    push_indexed_rgb_mesh_px(&mut blob, transform, &vertices, &indices);

    submit_rgb_blob_no_present(blob.as_slice())
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

fn build_circle_polygon_path(r: f32) -> Path {
    let mut builder = Path::builder();
    let n = 24usize;
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

fn build_circle_path() -> Path {
    build_circle_polygon_path(10.0)
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
                    color: rgb_from_f32(0.0, 0.0, 0.0, 1.0),
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
                    color: rgb_from_f32(0.0, 0.0, 0.0, 1.0),
                }
            }),
        );

        if i == 6 {
            let mut fill_geometry: VertexBuffers<MyVertex, u16> = VertexBuffers::new();
            let fill_res = FillTessellator::new().tessellate_path(
                &build_circle_polygon_path(5.0),
                &FillOptions::default(),
                &mut BuffersBuilder::new(&mut fill_geometry, |vertex: FillVertex| {
                    let p = vertex.position().to_array();
                    MyVertex {
                        position: [p[0], p[1]],
                        color: rgb_from_f32(0.0, 0.0, 0.0, 1.0),
                    }
                }),
            );

            if fill_res.is_ok() {
                let fill_offset = main_geometry.vertices.len() as u16;
                main_geometry.vertices.extend(fill_geometry.vertices);
                for idx in fill_geometry.indices {
                    main_geometry.indices.push(idx + fill_offset);
                }
            }
        }

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
    for (icon_px, scale, include_aa) in [(32.0f32, 1.0f32, true), (16.0f32, 0.5f32, true)] {
        for color in palette {
            let aa_color = rgb_from_f32(color[0], color[1], color[2], 0.26);
            for geom in &base_geometries {
                let aa_count = if include_aa {
                    geom.aa_positions.len()
                } else {
                    0
                };
                let mut baked_vertices: Vec<MyVertex> =
                    Vec::with_capacity(geom.main_positions.len() * 2 + aa_count);

                if include_aa {
                    for &p in &geom.aa_positions {
                        baked_vertices.push(MyVertex {
                            position: [p[0] * scale, p[1] * scale],
                            color: aa_color,
                        });
                    }
                }

                for &p in &geom.main_positions {
                    baked_vertices.push(MyVertex {
                        position: [
                            p[0] * scale + SHADOW_DX * scale,
                            p[1] * scale + SHADOW_DY * scale,
                        ],
                        color: rgb_from_f32(
                            SHADOW_COLOR[0],
                            SHADOW_COLOR[1],
                            SHADOW_COLOR[2],
                            SHADOW_COLOR[3],
                        ),
                    });
                }

                for &p in &geom.main_positions {
                    baked_vertices.push(MyVertex {
                        color: rgb_from_f32(color[0], color[1], color[2], color[3]),
                        position: [p[0] * scale, p[1] * scale],
                    });
                }

                let aa_len = if include_aa {
                    geom.aa_positions.len()
                } else {
                    0
                };
                let shadow_offset = aa_len as u16;
                let main_offset = (aa_len + geom.main_positions.len()) as u16;
                let aa_idx_len = if include_aa { geom.aa_indices.len() } else { 0 };
                let mut baked_indices: Vec<u16> =
                    Vec::with_capacity(aa_idx_len + geom.main_indices.len() * 2);
                if include_aa {
                    for &idx in &geom.aa_indices {
                        baked_indices.push(idx);
                    }
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

#[inline]
fn cached_icon_by_id(icon_id: u32, color_id: u32, small_set: bool) -> Option<&'static CachedIcon> {
    let shape = (icon_id as usize) % ICON_SHAPE_COUNT;
    let color = (color_id as usize) % ICON_PALETTE_COUNT;
    let size_block = ICON_SHAPE_COUNT.saturating_mul(ICON_PALETTE_COUNT);
    let base = if small_set { size_block } else { 0 };
    let idx = base
        .saturating_add(color.saturating_mul(ICON_SHAPE_COUNT))
        .saturating_add(shape);
    cached_icons().get(idx)
}

#[inline]
fn edge_fn(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_bake_lyon_icon_rgba(
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    out_ptr: *mut u8,
    out_len: usize,
) -> i32 {
    let Some(icon) = cached_icon_by_id(icon_id, color_id, small_set != 0) else {
        return -1;
    };

    let side = icon.cell_px.max(1.0) as usize;
    let need = side.saturating_mul(side).saturating_mul(4);
    if out_ptr.is_null() || out_len == 0 {
        return need as i32;
    }
    if out_len < need {
        return -2;
    }

    let mut accum: Vec<[f32; 4]> = vec![[0.0; 4]; side.saturating_mul(side)];

    let mut tri = 0usize;
    while tri + 2 < icon.indices.len() {
        let i0 = icon.indices[tri] as usize;
        let i1 = icon.indices[tri + 1] as usize;
        let i2 = icon.indices[tri + 2] as usize;
        tri += 3;

        let (Some(v0), Some(v1), Some(v2)) =
            (icon.vertices.get(i0), icon.vertices.get(i1), icon.vertices.get(i2))
        else {
            continue;
        };

        let x0 = v0.position[0];
        let y0 = v0.position[1];
        let x1 = v1.position[0];
        let y1 = v1.position[1];
        let x2 = v2.position[0];
        let y2 = v2.position[1];

        let area = edge_fn(x0, y0, x1, y1, x2, y2);
        if area.abs() < 1e-6 {
            continue;
        }

        let min_x =
            (libm::floorf(x0.min(x1).min(x2)) as isize).clamp(0, (side as isize) - 1) as usize;
        let max_x =
            (libm::ceilf(x0.max(x1).max(x2)) as isize).clamp(0, (side as isize) - 1) as usize;
        let min_y =
            (libm::floorf(y0.min(y1).min(y2)) as isize).clamp(0, (side as isize) - 1) as usize;
        let max_y =
            (libm::ceilf(y0.max(y1).max(y2)) as isize).clamp(0, (side as isize) - 1) as usize;

        for py in min_y..=max_y {
            let sy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let sx = px as f32 + 0.5;

                let w0 = edge_fn(x1, y1, x2, y2, sx, sy) / area;
                let w1 = edge_fn(x2, y2, x0, y0, sx, sy) / area;
                let w2 = edge_fn(x0, y0, x1, y1, sx, sy) / area;
                if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                    continue;
                }

                let mut sr = w0 * (v0.color.r as f32 / 255.0)
                    + w1 * (v1.color.r as f32 / 255.0)
                    + w2 * (v2.color.r as f32 / 255.0);
                let mut sg = w0 * (v0.color.g as f32 / 255.0)
                    + w1 * (v1.color.g as f32 / 255.0)
                    + w2 * (v2.color.g as f32 / 255.0);
                let mut sb = w0 * (v0.color.b as f32 / 255.0)
                    + w1 * (v1.color.b as f32 / 255.0)
                    + w2 * (v2.color.b as f32 / 255.0);
                let sa = (w0 * (v0.color.a as f32 / 255.0)
                    + w1 * (v1.color.a as f32 / 255.0)
                    + w2 * (v2.color.a as f32 / 255.0))
                    .clamp(0.0, 1.0);
                if sa <= 0.0 {
                    continue;
                }

                sr = sr.clamp(0.0, 1.0);
                sg = sg.clamp(0.0, 1.0);
                sb = sb.clamp(0.0, 1.0);

                let dst = &mut accum[py.saturating_mul(side).saturating_add(px)];
                let inv = 1.0 - sa;
                dst[0] = sr * sa + dst[0] * inv;
                dst[1] = sg * sa + dst[1] * inv;
                dst[2] = sb * sa + dst[2] * inv;
                dst[3] = sa + dst[3] * inv;
            }
        }
    }

    let out = core::slice::from_raw_parts_mut(out_ptr, need);
    let mut o = 0usize;
    for px in accum {
        let a = px[3].clamp(0.0, 1.0);
        // Match the existing textured shader contract used by cmd-stream text:
        // keep coverage in red and set alpha to fully-on. Final icon color is
        // supplied via per-vertex tint in the quad draw helper.
        out[o] = to_u8(a);
        out[o + 1] = 0;
        out[o + 2] = 0;
        out[o + 3] = 255;
        o += 4;
    }

    need as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_draw_lyon_icon_no_present(
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
) -> i32 {
    draw_lyon_icon_alpha_no_present(icon_id, color_id, small_set, x, y, view_w, view_h, 255)
}

pub fn draw_lyon_icon_alpha_no_present(
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> i32 {
    let Some(icon) = cached_icon_by_id(icon_id, color_id, small_set != 0) else {
        return -1;
    };

    let mut icon_blob: Vec<u8> =
        Vec::with_capacity(icon.indices.len().saturating_mul(RGB_VERTEX_SIZE));
    let vertices = icon_vertices_px(icon, x, y, 1.0, alpha);
    push_indexed_rgb_mesh_px(
        &mut icon_blob,
        ViewTransform::from_extent(view_w, view_h),
        vertices.as_slice(),
        icon.indices.as_slice(),
    );

    if icon_blob.is_empty() {
        return -2;
    }

    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
            icon_blob.as_ptr(),
            icon_blob.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc
}

pub fn draw_lyon_icon_alpha_scaled_no_present(
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    x: f32,
    y: f32,
    size_px: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> i32 {
    if !size_px.is_finite() || size_px <= 0.0 {
        return 0;
    }

    let Some(icon) = cached_icon_by_id(icon_id, color_id, small_set != 0) else {
        return -1;
    };

    let scale = size_px / icon.cell_px.max(1.0);
    let mut icon_blob: Vec<u8> =
        Vec::with_capacity(icon.indices.len().saturating_mul(RGB_VERTEX_SIZE));
    let vertices = icon_vertices_px(icon, x, y, scale, alpha);
    push_indexed_rgb_mesh_px(
        &mut icon_blob,
        ViewTransform::from_extent(view_w, view_h),
        vertices.as_slice(),
        icon.indices.as_slice(),
    );

    if icon_blob.is_empty() {
        return -2;
    }

    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0)
    };
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
            icon_blob.as_ptr(),
            icon_blob.len(),
        )
    };
    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    rc
}

pub fn lyon_geom_api_demo_no_present(view_w: u32, view_h: u32) -> bool {
    let fb_w = view_w.max(1) as f32;
    let fb_h = view_h.max(1) as f32;
    let transform = ViewTransform::from_extent(view_w, view_h);
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
        let mut icon_blob: Vec<u8> =
            Vec::with_capacity(icon.indices.len().saturating_mul(RGB_VERTEX_SIZE));

        total_vertices = total_vertices.saturating_add(icon.vertices.len());
        total_indices = total_indices.saturating_add(icon.indices.len());

        let vertices = icon_vertices_px(icon, ox, oy, 1.0, 255);
        push_indexed_rgb_mesh_px(
            &mut icon_blob,
            transform,
            vertices.as_slice(),
            icon.indices.as_slice(),
        );

        let draw_rc = unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
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
