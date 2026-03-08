extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cmp::min;
use core::ptr;
use spin::Once;

use lyon_tessellation::geometry_builder::simple_builder;
use lyon_tessellation::math::point as tess_point;
use lyon_tessellation::path::Path;
use lyon_tessellation::path::builder::BorderRadii;
use lyon_tessellation::{StrokeOptions, StrokeTessellator, VertexBuffers};

struct DemoMeshSpec {
    name: &'static str,
    width: f32,
    height: f32,
    radius: f32,
    line_width: f32,
}

struct KernelDemoMesh {
    name: &'static str,
    vertices: Box<[f32]>,
    indices: Box<[u16]>,
}

static DEMO_MESHES: Once<Vec<KernelDemoMesh>> = Once::new();

fn tessellate_round_rect_stroke_mesh(
    width: f32,
    height: f32,
    radius: f32,
    line_width: f32,
) -> Result<(Vec<f32>, Vec<u16>), &'static str> {
    let w = width.max(1.0);
    let h = height.max(1.0);
    let r = radius.clamp(0.0, 0.5 * w.min(h));
    let lw = line_width.max(0.1);

    let mut builder = Path::builder();
    builder.add_rounded_rectangle(
        &lyon_tessellation::path::math::Box2D::new(tess_point(0.0, 0.0), tess_point(w, h)),
        &BorderRadii::new(r),
        lyon_tessellation::path::Winding::Positive,
    );
    let path = builder.build();

    let mut geometry: VertexBuffers<lyon_tessellation::math::Point, u16> = VertexBuffers::new();
    let mut tess = StrokeTessellator::new();
    let opts = StrokeOptions::default().with_line_width(lw);
    if tess
        .tessellate_path(&path, &opts, &mut simple_builder(&mut geometry))
        .is_err()
    {
        return Err("round-rect stroke tessellation failed");
    }

    let mut verts_xy = Vec::with_capacity(geometry.vertices.len() * 2);
    for p in geometry.vertices.iter() {
        verts_xy.push(p.x);
        verts_xy.push(p.y);
    }
    Ok((verts_xy, geometry.indices))
}

fn build_demo_meshes() -> Vec<KernelDemoMesh> {
    const SPECS: &[DemoMeshSpec] = &[
        DemoMeshSpec {
            name: "roundRectThin",
            width: 44.0,
            height: 28.0,
            radius: 6.0,
            line_width: 1.0,
        },
        DemoMeshSpec {
            name: "roundRectBold",
            width: 72.0,
            height: 40.0,
            radius: 12.0,
            line_width: 3.0,
        },
        DemoMeshSpec {
            name: "pillFrame",
            width: 96.0,
            height: 34.0,
            radius: 17.0,
            line_width: 2.5,
        },
    ];

    let mut out = Vec::with_capacity(SPECS.len());
    for spec in SPECS.iter() {
        if let Ok((verts, idx)) =
            tessellate_round_rect_stroke_mesh(spec.width, spec.height, spec.radius, spec.line_width)
        {
            out.push(KernelDemoMesh {
                name: spec.name,
                vertices: verts.into_boxed_slice(),
                indices: idx.into_boxed_slice(),
            });
        }
    }
    out
}

fn meshes() -> &'static [KernelDemoMesh] {
    DEMO_MESHES.call_once(build_demo_meshes).as_slice()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lyon_is_available() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lyon_demo_mesh_count() -> u32 {
    meshes().len() as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lyon_demo_mesh_name_len(index: u32) -> usize {
    let Some(mesh) = meshes().get(index as usize) else {
        return 0;
    };
    mesh.name.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lyon_demo_mesh_name_copy(
    index: u32,
    out_ptr: *mut u8,
    out_len: usize,
) -> usize {
    let Some(mesh) = meshes().get(index as usize) else {
        return 0;
    };
    if out_ptr.is_null() || out_len == 0 {
        return 0;
    }
    let n = min(mesh.name.len(), out_len);
    ptr::copy_nonoverlapping(mesh.name.as_ptr(), out_ptr, n);
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lyon_demo_mesh_vertex_count(index: u32) -> u32 {
    let Some(mesh) = meshes().get(index as usize) else {
        return 0;
    };
    (mesh.vertices.len() / 2) as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lyon_demo_mesh_index_count(index: u32) -> u32 {
    let Some(mesh) = meshes().get(index as usize) else {
        return 0;
    };
    mesh.indices.len() as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lyon_demo_mesh_copy_vertices(
    index: u32,
    out_ptr: *mut f32,
    out_len: usize,
) -> usize {
    let Some(mesh) = meshes().get(index as usize) else {
        return 0;
    };
    if out_ptr.is_null() || out_len == 0 {
        return 0;
    }
    let n = min(mesh.vertices.len(), out_len);
    ptr::copy_nonoverlapping(mesh.vertices.as_ptr(), out_ptr, n);
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lyon_demo_mesh_copy_indices(
    index: u32,
    out_ptr: *mut u16,
    out_len: usize,
) -> usize {
    let Some(mesh) = meshes().get(index as usize) else {
        return 0;
    };
    if out_ptr.is_null() || out_len == 0 {
        return 0;
    }
    let n = min(mesh.indices.len(), out_len);
    ptr::copy_nonoverlapping(mesh.indices.as_ptr(), out_ptr, n);
    n
}
