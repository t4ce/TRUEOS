use alloc::vec::Vec;
use core::f32::consts::PI;

use lyon_geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use spin::Once;
use trueos_math::{cos_f32, sin_f32};

#[derive(Copy, Clone, Debug)]
struct MyVertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct CachedIcon {
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
        (4.0, 9.0),
        (16.0, 9.0),
        (16.0, 5.0),
        (28.0, 16.0),
        (16.0, 27.0),
        (16.0, 23.0),
        (4.0, 23.0),
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
    let mut builder = Path::builder();
    builder.begin(point(16.0, 5.0));
    builder.line_to(point(16.0, 27.0));
    builder.end(false);

    builder.begin(point(5.0, 16.0));
    builder.line_to(point(27.0, 16.0));
    builder.end(false);

    builder.build()
}

fn build_minus_path() -> Path {
    let mut builder = Path::builder();
    builder.begin(point(5.0, 16.0));
    builder.line_to(point(27.0, 16.0));
    builder.end(false);
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

fn build_cell_inner_border_path() -> Path {
    let mut builder = Path::builder();
    // 1px stroke centered on this contour stays inside the 32x32 cell.
    builder.begin(point(0.5, 0.5));
    builder.line_to(point(31.5, 0.5));
    builder.line_to(point(31.5, 31.5));
    builder.line_to(point(0.5, 31.5));
    builder.end(true);
    builder.build()
}

fn build_cached_icons() -> Vec<CachedIcon> {
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
    let cell_border = build_cell_inner_border_path();
    let flat_color = [0.0, 0.0, 0.0, 1.0];

    let mut out: Vec<CachedIcon> = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        let mut geometry: VertexBuffers<MyVertex, u16> = VertexBuffers::new();
        let mut tessellator = StrokeTessellator::new();

        let icon_res = tessellator.tessellate_path(
            path,
            &StrokeOptions::default().with_line_width(3.0),
            &mut BuffersBuilder::new(&mut geometry, |vertex: StrokeVertex| {
                let p = vertex.position().to_array();
                MyVertex {
                    position: [p[0], p[1]],
                    color: flat_color,
                }
            }),
        );

        if icon_res.is_err() {
            crate::log!("lyon-demo: tessellation failed at icon={}\n", i);
            out.push(CachedIcon {
                vertices: Vec::new(),
                indices: Vec::new(),
            });
            continue;
        }

        let border_res = tessellator.tessellate_path(
            &cell_border,
            &StrokeOptions::default().with_line_width(1.0),
            &mut BuffersBuilder::new(&mut geometry, |vertex: StrokeVertex| {
                let p = vertex.position().to_array();
                MyVertex {
                    position: [p[0], p[1]],
                    color: flat_color,
                }
            }),
        );

        if border_res.is_err() {
            crate::log!("lyon-demo: border tessellation failed at icon={}\n", i);
        }

        out.push(CachedIcon {
            vertices: geometry.vertices,
            indices: geometry.indices,
        });
    }
    out
}

fn cached_icons() -> &'static [CachedIcon] {
    ICON_CACHE.call_once(build_cached_icons).as_slice()
}

pub fn lyon_geom_api_demo_no_present(view_w: u32, view_h: u32) -> bool {
    let fb_w = view_w.max(1) as f32;
    let fb_h = view_h.max(1) as f32;
    const CELL_PX: f32 = 32.0;
    let icons = cached_icons();
    let cols = core::cmp::max(1usize, (view_w as usize) / (CELL_PX as usize));
    let mut rgb_blob: Vec<u8> = Vec::new();
    let mut total_vertices = 0usize;
    let mut total_indices = 0usize;

    for (i, icon) in icons.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let ox = (col as f32) * CELL_PX;
        let oy = (row as f32) * CELL_PX;

        total_vertices = total_vertices.saturating_add(icon.vertices.len());
        total_indices = total_indices.saturating_add(icon.indices.len());

        for &idx in &icon.indices {
            if let Some(v) = icon.vertices.get(idx as usize) {
                let vv = MyVertex {
                    position: [v.position[0] + ox, v.position[1] + oy],
                    color: v.color,
                };
                push_rgb_vtx(&mut rgb_blob, &vv, fb_w, fb_h);
            }
        }
    }

    crate::log!(
        "lyon-demo: icons={} outlines=3px vertices={} indices={} (cached)\n",
        icons.len(),
        total_vertices,
        total_indices
    );

    let draw_rc = unsafe {
        crate::surface::io::cabi::trueos_cabi_gfx_draw_rgb_triangles_no_present(
            rgb_blob.as_ptr(),
            rgb_blob.len(),
        )
    };
    if draw_rc != 0 {
        crate::log!(
            "lyon-demo: submit rc draw={} bytes={}\n",
            draw_rc,
            rgb_blob.len()
        );
    }
    draw_rc == 0
}
