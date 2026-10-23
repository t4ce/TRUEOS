//! Native line/point/quad coverage from Microfont's CPU-decoded raster seeds.
//! The guest never tessellates strokes: Gallium rasterizes lines and points,
//! and the fragment shader supplies their one-pixel inward coverage fringe.
use super::*;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeMaskVertex {
    pub(crate) position: [f32; 4],
    pub(crate) geometry: [f32; 4],
    pub(crate) style: [f32; 4],
}

pub(crate) struct NativeMaskGeometry {
    pub(crate) lines: Vec<NativeMaskVertex>,
    pub(crate) points: Vec<NativeMaskVertex>,
    pub(crate) quads: Vec<NativeMaskVertex>,
    pub(crate) line_width_px: f32,
}

const VS_MASK: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL IN[2]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], TEXCOORD[0]\n\
DCL OUT[2], TEXCOORD[1]\n\
0: MOV OUT[0], IN[0]\n\
1: MOV OUT[1], IN[1]\n\
2: MOV OUT[2], IN[2]\n\
3: END\n";

// style.y: 0=solid quad, 1=line body, 2=outer endpoint half-disc,
// 3=bevel join, 4=circular join sector. Join offsets are exact CPU-decoded
// line normals, not generated vertex triangles. All join clipping is here.
const FS_MASK: &str = "FRAG\n\
DCL IN[0], TEXCOORD[0], LINEAR\n\
DCL IN[1], TEXCOORD[1], LINEAR\n\
DCL IN[2], POSITION, LINEAR\n\
DCL OUT[0], COLOR\n\
DCL TEMP[0..6]\n\
IMM[0] FLT32 {0.0, 1.0, 2.0, 3.0}\n\
IMM[1] FLT32 {4.0, 0.0, 0.0, 0.0}\n\
0: MOV TEMP[0].w, IMM[0].yyyy\n\
1: SUB TEMP[0].xy, IN[2].xyyy, IN[0].xyyy\n\
2: SEQ TEMP[1].x, IN[1].yyyy, IMM[0].yyyy\n\
3: IF TEMP[1].xxxx\n\
4: MUL TEMP[1].x, TEMP[0].xxxx, IN[0].wwww\n\
5: MAD TEMP[1].x, -TEMP[0].yyyy, IN[0].zzzz, TEMP[1].xxxx\n\
6: ABS TEMP[1].x, TEMP[1].xxxx\n\
7: SUB_SAT TEMP[0].w, IN[1].xxxx, TEMP[1].xxxx\n\
8: ENDIF\n\
9: SEQ TEMP[1].x, IN[1].yyyy, IMM[0].zzzz\n\
10: SEQ TEMP[1].y, IN[1].yyyy, IMM[1].xxxx\n\
11: MAX TEMP[1].z, TEMP[1].xxxx, TEMP[1].yyyy\n\
12: IF TEMP[1].zzzz\n\
13: DP2 TEMP[2].x, TEMP[0].xyyy, TEMP[0].xyyy\n\
14: SQRT TEMP[2].x, TEMP[2].xxxx\n\
15: SUB_SAT TEMP[0].w, IN[1].xxxx, TEMP[2].xxxx\n\
16: IF TEMP[1].xxxx\n\
17: DP2 TEMP[2].x, TEMP[0].xyyy, IN[0].zwww\n\
18: SLT TEMP[2].x, TEMP[2].xxxx, IMM[0].xxxx\n\
19: MUL TEMP[0].w, TEMP[0].wwww, TEMP[2].xxxx\n\
20: ENDIF\n\
21: ENDIF\n\
22: SEQ TEMP[1].x, IN[1].yyyy, IMM[0].wwww\n\
23: SEQ TEMP[1].y, IN[1].yyyy, IMM[1].xxxx\n\
24: MAX TEMP[1].z, TEMP[1].xxxx, TEMP[1].yyyy\n\
25: IF TEMP[1].zzzz\n\
26: MUL TEMP[2].x, IN[0].zzzz, TEMP[0].yyyy\n\
27: MAD TEMP[2].x, -IN[0].wwww, TEMP[0].xxxx, TEMP[2].xxxx\n\
28: MUL TEMP[2].y, TEMP[0].xxxx, IN[1].wwww\n\
29: MAD TEMP[2].y, -TEMP[0].yyyy, IN[1].zzzz, TEMP[2].yyyy\n\
30: SLE TEMP[2].xy, TEMP[2].xyyy, IMM[0].xxxx\n\
31: MUL TEMP[0].w, TEMP[0].wwww, TEMP[2].xxxx\n\
32: MUL TEMP[0].w, TEMP[0].wwww, TEMP[2].yyyy\n\
33: IF TEMP[1].xxxx\n\
34: SUB TEMP[3].xy, IN[1].zwww, IN[0].zwww\n\
35: SUB TEMP[4].xy, TEMP[0].xyyy, IN[0].zwww\n\
36: MUL TEMP[4].z, TEMP[3].yyyy, TEMP[4].xxxx\n\
37: MAD TEMP[4].z, -TEMP[3].xxxx, TEMP[4].yyyy, TEMP[4].zzzz\n\
38: DP2 TEMP[3].z, TEMP[3].xyyy, TEMP[3].xyyy\n\
39: RSQ TEMP[3].z, TEMP[3].zzzz\n\
40: MUL_SAT TEMP[4].z, TEMP[4].zzzz, TEMP[3].zzzz\n\
41: MUL TEMP[0].w, TEMP[0].wwww, TEMP[4].zzzz\n\
42: ENDIF\n\
43: ENDIF\n\
44: MOV OUT[0].xyz, IMM[0].yyyy\n\
45: MOV OUT[0].w, TEMP[0].wwww\n\
46: END\n";

#[repr(C)]
struct QuadPointVertex { position: [f32; 4], planes: [[f32; 4]; 4] }

const VS_QUAD_POINT: &str = "VERT\n\
DCL IN[0..4]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], TEXCOORD[0]\n\
DCL OUT[2], TEXCOORD[1]\n\
DCL OUT[3], TEXCOORD[2]\n\
DCL OUT[4], TEXCOORD[3]\n\
0: MOV OUT[0], IN[0]\n\
1: MOV OUT[1], IN[1]\n\
2: MOV OUT[2], IN[2]\n\
3: MOV OUT[3], IN[3]\n\
4: MOV OUT[4], IN[4]\n\
5: END\n";
const FS_QUAD_POINT: &str = "FRAG\n\
DCL IN[0], TEXCOORD[0], LINEAR\n\
DCL IN[1], TEXCOORD[1], LINEAR\n\
DCL IN[2], TEXCOORD[2], LINEAR\n\
DCL IN[3], TEXCOORD[3], LINEAR\n\
DCL IN[4], POSITION, LINEAR\n\
DCL TEMP[0..1]\n\
DCL OUT[0], COLOR\n\
IMM[0] FLT32 {0.0, 1.0, 0.0, 0.0}\n\
0: DP2 TEMP[0].x, IN[4], IN[0]\n\
1: DP2 TEMP[0].y, IN[4], IN[1]\n\
2: DP2 TEMP[0].z, IN[4], IN[2]\n\
3: DP2 TEMP[0].w, IN[4], IN[3]\n\
4: ADD TEMP[0].x, TEMP[0].xxxx, IN[0].zzzz\n\
5: ADD TEMP[0].y, TEMP[0].yyyy, IN[1].zzzz\n\
6: ADD TEMP[0].z, TEMP[0].zzzz, IN[2].zzzz\n\
7: ADD TEMP[0].w, TEMP[0].wwww, IN[3].zzzz\n\
8: SGE TEMP[0], TEMP[0], IMM[0].xxxx\n\
9: MUL TEMP[1].x, TEMP[0].xxxx, TEMP[0].yyyy\n\
10: MUL TEMP[1].x, TEMP[1].xxxx, TEMP[0].zzzz\n\
11: MUL OUT[0].w, TEMP[1].xxxx, TEMP[0].wwww\n\
12: MOV OUT[0].xyz, IMM[0].yyyy\n\
13: END\n";

/// One native point carries four exact seed-edge half planes. This preserves
/// even the non-parallelogram diagonal cuts without GL_QUADS or tessellation.
fn quad_points(vertices: &[NativeMaskVertex], width: u32, height: u32) -> Result<(Vec<QuadPointVertex>, f32), &'static str> {
    let mut points = Vec::with_capacity(vertices.len() / 4);
    let mut envelope = 1.0f32;
    for quad in vertices.chunks_exact(4) {
        let corners: [[f32; 2]; 4] = core::array::from_fn(|i| [
            (quad[i].position[0] + 1.0) * width as f32 * 0.5,
            (quad[i].position[1] + 1.0) * height as f32 * 0.5]);
        let min = [corners.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
            corners.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min)];
        let max = [corners.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
            corners.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max)];
        let area: f32 = (0..4).map(|i| corners[i][0]*corners[(i+1)%4][1]-corners[i][1]*corners[(i+1)%4][0]).sum();
        if area.abs() < 1e-6 { return Err("virgl-mask-quad-degenerate"); }
        let sign = if area > 0.0 { 1.0 } else { -1.0 };
        let planes = core::array::from_fn(|i| {
            let a = corners[i]; let b = corners[(i+1)%4];
            let normal = [(a[1]-b[1])*sign, (b[0]-a[0])*sign];
            [normal[0], normal[1], -normal[0]*a[0]-normal[1]*a[1], 0.0]
        });
        envelope = envelope.max(libm::ceilf((max[0]-min[0]).max(max[1]-min[1])) + 2.0);
        points.push(QuadPointVertex { position: [
            (min[0]+max[0])/width as f32-1.0,
            (min[1]+max[1])/height as f32-1.0, 0.0, 1.0], planes });
    }
    Ok((points, envelope))
}

fn quad_elements(buf: &mut VirglCmdBuf, handle: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_VERTEX_ELEMENTS, 21));
    buf.push(handle);
    for i in 0..5 { for word in [i*16, 0, 0, VIRGL_FORMAT_R32G32B32A32_FLOAT] { buf.push(word); } }
}

fn draw_native(buf: &mut VirglCmdBuf, count: u32, topology: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_DRAW_VBO, 0, 12));
    for word in [0, count, topology, 0, 1, 0, 0, 0, 0, 0, 0, 0] { buf.push(word); }
}

fn destroy_mask_objects(buf: &mut VirglCmdBuf, objects: &[(u8, u32)]) {
    const VIRGL_CCMD_DESTROY_OBJECT: u8 = 3;
    for &(kind, handle) in objects {
        buf.push(virgl_cmd0(VIRGL_CCMD_DESTROY_OBJECT, kind, 1));
        buf.push(handle);
    }
}

fn rasterizer(buf: &mut VirglCmdBuf, handle: u32, width: f32, point_size: f32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJ_RS_SIZE));
    buf.push(handle);
    // Same pixel-centre / edge convention as the compositor, with native
    // Smooth native lines use perpendicular width; aliased GL lines measure
    // their width on the major axis and pinch diagonal strokes. The wider
    // envelope keeps native AA outside the shader's exact coverage boundary.
    buf.push((1 << 1) | (1 << 14) | (1 << 26) | (1 << 29) | (1 << 30));
    buf.push(fui(point_size));
    buf.push(0);
    buf.push(0);
    buf.push(fui(width + 2.0));
    for _ in 0..3 { buf.push(0); }
}

pub(crate) fn render_coverage_mask(
    width: u32, height: u32, geometry: NativeMaskGeometry,
) -> Result<Vec<u8>, &'static str> {
    if width == 0 || height == 0 || width > 4096 || height > 4096
        || !geometry.line_width_px.is_finite() || !(0.25..=32.0).contains(&geometry.line_width_px)
        || geometry.lines.len() % 2 != 0 || geometry.quads.len() % 4 != 0
    { return Err("virgl-mask-shape"); }
    super::super::requests::dispatch_blocking(move |compositor, gpu| {
        render(compositor, gpu, width, height, geometry)
    })
}

fn render(
    compositor: &mut Compositor, gpu: &mut VirtioGpuLogo,
    width: u32, height: u32, geometry: NativeMaskGeometry,
) -> Result<Vec<u8>, &'static str> {
    let target = compositor.next_id;
    compositor.next_id = target.checked_add(1).ok_or("virgl-mask-resource-id")?;
    const VS: u32 = 0x4d00_0001;
    const FS: u32 = 0x4d00_0002;
    const ELEMENTS: u32 = 0x4d00_0003;
    const BLEND: u32 = 0x4d00_0004;
    const DSA: u32 = 0x4d00_0005;
    const RASTER: u32 = 0x4d00_0006;
    const QUAD_VS: u32 = 0x4d00_0007;
    const QUAD_FS: u32 = 0x4d00_0008;
    const QUAD_ELEMENTS: u32 = 0x4d00_0009;
    const QUAD_RASTER: u32 = 0x4d00_000a;
    let (quads, point_size) = quad_points(&geometry.quads, width, height)?;
    if !gpu.create_3d(target, width, height, TEXTURE_FORMAT_RGBA, 1 << 1, false) {
        return Err("virgl-mask-target");
    }
    let result = (|| {
        let mut init = VirglCmdBuf::new();
        encode_create_surface(&mut init, target, target, TEXTURE_FORMAT_RGBA);
        encode_set_framebuffer(&mut init, target);
        encode_set_viewport(&mut init, width, height);
        encode_set_scissor(&mut init, 0, 0, width, height);
        encode_clear_color(&mut init, 0.0, 0.0, 0.0, 0.0);
        encode_create_vertex_elements(&mut init, ELEMENTS);
        encode_bind_object(&mut init, VIRGL_OBJECT_VERTEX_ELEMENTS, ELEMENTS);
        encode_shader(&mut init, VS, PIPE_SHADER_VERTEX, VS_MASK);
        encode_shader(&mut init, FS, PIPE_SHADER_FRAGMENT, FS_MASK);
        encode_bind_shader(&mut init, VS, PIPE_SHADER_VERTEX);
        encode_bind_shader(&mut init, FS, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, VS, FS);
        encode_create_blend(&mut init, BLEND, true, 3, 0x13); // SRC_ALPHA, INV_SRC_ALPHA.
        encode_bind_object(&mut init, VIRGL_OBJECT_BLEND, BLEND);
        encode_create_dsa(&mut init, DSA);
        encode_bind_object(&mut init, VIRGL_OBJECT_DSA, DSA);
        rasterizer(&mut init, RASTER, geometry.line_width_px, libm::ceilf(geometry.line_width_px + 2.0));
        encode_bind_object(&mut init, VIRGL_OBJECT_RASTERIZER, RASTER);
        if !quads.is_empty() {
            encode_shader(&mut init, QUAD_VS, PIPE_SHADER_VERTEX, VS_QUAD_POINT);
            encode_shader(&mut init, QUAD_FS, PIPE_SHADER_FRAGMENT, FS_QUAD_POINT);
            quad_elements(&mut init, QUAD_ELEMENTS);
            rasterizer(&mut init, QUAD_RASTER, geometry.line_width_px, point_size);
        }
        if !compositor.submit(gpu, &init) { return Err("virgl-mask-initialize"); }
        if !quads.is_empty() {
            let mut bind = VirglCmdBuf::new();
            encode_bind_shader(&mut bind, QUAD_VS, PIPE_SHADER_VERTEX);
            encode_bind_shader(&mut bind, QUAD_FS, PIPE_SHADER_FRAGMENT);
            encode_link_shader(&mut bind, QUAD_VS, QUAD_FS);
            encode_bind_object(&mut bind, VIRGL_OBJECT_VERTEX_ELEMENTS, QUAD_ELEMENTS);
            encode_bind_object(&mut bind, VIRGL_OBJECT_RASTERIZER, QUAD_RASTER);
            if !compositor.submit(gpu, &bind) { return Err("virgl-mask-quad-bind"); }
            for chunk in quads.chunks(2048) {
                let bytes = unsafe { core::slice::from_raw_parts(chunk.as_ptr().cast::<u8>(), core::mem::size_of_val(chunk)) };
                let mut commands = VirglCmdBuf::new();
                encode_inline_write_buffer_at(&mut commands, VBO, 0, bytes);
                encode_set_vertex_buffer(&mut commands, 80, 0, VBO);
                draw_native(&mut commands, chunk.len() as u32, 0);
                if !compositor.submit(gpu, &commands) { return Err("virgl-mask-quad-draw"); }
            }
            let mut bind = VirglCmdBuf::new();
            encode_bind_shader(&mut bind, VS, PIPE_SHADER_VERTEX);
            encode_bind_shader(&mut bind, FS, PIPE_SHADER_FRAGMENT);
            encode_link_shader(&mut bind, VS, FS);
            encode_bind_object(&mut bind, VIRGL_OBJECT_VERTEX_ELEMENTS, ELEMENTS);
            encode_bind_object(&mut bind, VIRGL_OBJECT_RASTERIZER, RASTER);
            if !compositor.submit(gpu, &bind) { return Err("virgl-mask-line-bind"); }
        }
        // Every range is uploaded only after the preceding draw has retired.
        // 4096 * 48 bytes fits the existing VBO and one inline-write command.
        for (vertices, topology) in [(&geometry.lines, 1), (&geometry.points, 0)] {
            for chunk in vertices.chunks(4096) {
                let bytes = unsafe { core::slice::from_raw_parts(chunk.as_ptr().cast::<u8>(), core::mem::size_of_val(chunk)) };
                let mut commands = VirglCmdBuf::new();
                encode_inline_write_buffer_at(&mut commands, VBO, 0, bytes);
                encode_set_vertex_buffer(&mut commands, 48, 0, VBO);
                draw_native(&mut commands, chunk.len() as u32, topology);
                if !compositor.submit(gpu, &commands) { return Err("virgl-mask-draw"); }
            }
        }
        compositor.fence += 1;
        let rgba = gpu.readback_rgba(target, width, height, compositor.fence)?;
        Ok(rgba.chunks_exact(4).map(|pixel| pixel[0]).collect())
    })();
    // Fenced draws/readback have retired before releasing ordinary resources.
    // A failed device is quarantined by the shared backend's DMA policy.
    if !gpu.failed {
        let mut destroy = VirglCmdBuf::new();
        destroy_mask_objects(&mut destroy, &[(VIRGL_OBJECT_SHADER, VS), (VIRGL_OBJECT_SHADER, FS),
            (VIRGL_OBJECT_VERTEX_ELEMENTS, ELEMENTS), (VIRGL_OBJECT_BLEND, BLEND),
            (VIRGL_OBJECT_DSA, DSA), (VIRGL_OBJECT_RASTERIZER, RASTER),
            (VIRGL_OBJECT_SURFACE, target)]);
        if !quads.is_empty() {
            destroy_mask_objects(&mut destroy, &[(VIRGL_OBJECT_SHADER, QUAD_VS),
                (VIRGL_OBJECT_SHADER, QUAD_FS), (VIRGL_OBJECT_VERTEX_ELEMENTS, QUAD_ELEMENTS),
                (VIRGL_OBJECT_RASTERIZER, QUAD_RASTER)]);
        }
        if !compositor.submit(gpu, &destroy) || !gpu.unref_resource(target) {
            return Err("virgl-mask-release");
        }
    }
    result
}
