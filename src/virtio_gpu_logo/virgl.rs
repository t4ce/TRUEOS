//! Small virgl wire encoder recovered from the former gfx backend.
//! UI4 composition plus triangles from the existing font and sprite APIs.
use super::*;
use alloc::vec::Vec;
const PIPE_PRIM_TRIANGLES: u32 = 4;

const PIPE_CLEAR_COLOR0: u32 = 1 << 2;
const PIPE_MASK_RGBA: u32 = 0xF;

// Virgl format IDs (see virgl_hw.h):
// virgl_hw.h: VIRGL_FORMAT_R8G8B8A8_UNORM = 67
const VIRGL_FORMAT_R32G32B32A32_FLOAT: u32 = 31;

// --- Virgl protocol (see virgl_protocol.h) ---
const VIRGL_OBJECT_BLEND: u8 = 1;
const VIRGL_OBJECT_RASTERIZER: u8 = 2;
const VIRGL_OBJECT_DSA: u8 = 3;
const VIRGL_OBJECT_SHADER: u8 = 4;
const VIRGL_OBJECT_VERTEX_ELEMENTS: u8 = 5;
// From virgl_protocol.h enum virgl_object_type.
const VIRGL_OBJECT_SAMPLER_VIEW: u8 = 6;
const VIRGL_OBJECT_SAMPLER_STATE: u8 = 7;
const VIRGL_OBJECT_SURFACE: u8 = 8;

const VIRGL_CCMD_CREATE_OBJECT: u8 = 1;
const VIRGL_CCMD_BIND_OBJECT: u8 = 2;
const VIRGL_CCMD_SET_VIEWPORT_STATE: u8 = 4;
const VIRGL_CCMD_SET_FRAMEBUFFER_STATE: u8 = 5;
const VIRGL_CCMD_SET_VERTEX_BUFFERS: u8 = 6;
const VIRGL_CCMD_CLEAR: u8 = 7;
const VIRGL_CCMD_DRAW_VBO: u8 = 8;
const VIRGL_CCMD_RESOURCE_INLINE_WRITE: u8 = 9;
const VIRGL_CCMD_SET_SAMPLER_VIEWS: u8 = 10;
const VIRGL_CCMD_SET_SCISSOR_STATE: u8 = 15;
const VIRGL_CCMD_BIND_SAMPLER_STATES: u8 = 18;
// NOTE: Values must match virglrenderer `enum virgl_context_cmd`.
const VIRGL_CCMD_BIND_SHADER: u8 = 31;
const VIRGL_CCMD_LINK_SHADER: u8 = 52;

const VIRGL_LINK_SHADER_SIZE: u32 = 6;

const VIRGL_OBJ_BLEND_SIZE: u32 = 11;
const VIRGL_OBJ_DSA_SIZE: u32 = 5;
const VIRGL_OBJ_RS_SIZE: u32 = 9;
const VIRGL_OBJ_SURFACE_SIZE: u32 = 5;

fn virgl_cmd0(cmd: u8, obj: u8, len_dwords: u32) -> u32 {
    (cmd as u32) | ((obj as u32) << 8) | (len_dwords << 16)
}

fn fui(v: f32) -> u32 {
    u32::from_le_bytes(v.to_le_bytes())
}

struct VirglCmdBuf {
    dwords: Vec<u32>,
}

impl VirglCmdBuf {
    fn new() -> Self {
        Self { dwords: Vec::new() }
    }

    fn push(&mut self, v: u32) {
        self.dwords.push(v);
    }

    fn push_bytes_padded(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            let mut chunk = [0u8; 4];
            let take = (bytes.len() - i).min(4);
            chunk[..take].copy_from_slice(&bytes[i..i + take]);
            self.dwords.push(u32::from_le_bytes(chunk));
            i += take;
        }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.dwords.as_ptr() as *const u8, self.dwords.len() * 4)
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
}

const VS_TEX: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL IN[2]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], TEXCOORD[0]\n\
DCL OUT[2], COLOR\n\
    0: MOV OUT[2], IN[2]\n\
    1: MOV OUT[1], IN[1]\n\
    2: MOV OUT[0], IN[0]\n\
    3: END\n";

const FS_TEX_RGBA: &str = "FRAG\n\
DCL IN[0], TEXCOORD[0], LINEAR\n\
DCL IN[1], COLOR, LINEAR\n\
DCL SAMP[0]\n\
DCL TEMP[0]\n\
DCL OUT[0], COLOR\n\
    0: TEX TEMP[0], IN[0], SAMP[0], 2D\n\
    1: MUL OUT[0], TEMP[0], IN[1]\n\
    2: END\n";

fn encode_draw_vbo_count(buf: &mut VirglCmdBuf, count: u32) {
    // VIRGL_DRAW_VBO_SIZE = 12
    buf.push(virgl_cmd0(VIRGL_CCMD_DRAW_VBO, 0, 12));
    buf.push(0); // start
    buf.push(count); // count
    buf.push(PIPE_PRIM_TRIANGLES);
    buf.push(0); // indexed
    buf.push(1); // instance_count
    buf.push(0); // index_bias
    buf.push(0); // start_instance
    buf.push(0); // primitive_restart
    buf.push(0); // restart_index
    buf.push(0); // min_index
    buf.push(0); // max_index
    buf.push(0); // count_from_so
}
fn encode_shader(buf: &mut VirglCmdBuf, handle: u32, shader_type: u32, text: &str) {
    // For TGSI text shaders virgl still expects a sensible token count in the header.
    // Small fixed shaders tolerated an arbitrary constant, but larger generated shaders do not.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);

    let shader_len = bytes.len() as u32;
    let num_tokens = ((bytes.len() as u32).div_ceil(4)).max(1);
    let offlen = shader_len & 0x7fff_ffff;

    // Base header size=5 dwords: handle, type, offlen, num_tokens, num_outputs.
    let len_dwords = 5 + (bytes.len() as u32).div_ceil(4);
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SHADER, len_dwords));
    buf.push(handle);
    buf.push(shader_type);
    buf.push(offlen);
    buf.push(num_tokens);
    buf.push(0); // num streamout outputs
    buf.push_bytes_padded(&bytes);
}
fn encode_bind_shader(buf: &mut VirglCmdBuf, handle: u32, shader_type: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_BIND_SHADER, 0, 2));
    buf.push(handle);
    buf.push(shader_type);
}
fn encode_link_shader(buf: &mut VirglCmdBuf, vs: u32, fs: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_LINK_SHADER, 0, VIRGL_LINK_SHADER_SIZE));
    buf.push(vs);
    buf.push(fs);
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(0);
}
fn encode_create_surface(buf: &mut VirglCmdBuf, surf_handle: u32, res_handle: u32, format: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SURFACE, VIRGL_OBJ_SURFACE_SIZE));
    buf.push(surf_handle);
    buf.push(res_handle);
    buf.push(format);
    buf.push(0); // level
    buf.push(0); // first_layer | (last_layer<<16)
}
fn encode_set_framebuffer(buf: &mut VirglCmdBuf, surf_handle: u32) {
    // VIRGL_SET_FRAMEBUFFER_STATE_SIZE(nr_cbufs) = nr + 2
    let len = 1 + 2;
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, 0, len));
    buf.push(1); // nr_cbufs
    buf.push(0); // zsbuf
    buf.push(surf_handle);
}
fn encode_clear_color(buf: &mut VirglCmdBuf, r: f32, g: f32, b: f32, a: f32) {
    // VIRGL_OBJ_CLEAR_SIZE = 8 dwords payload:
    // buffers (1) + color[4] (4) + depth(double)=2 dwords + stencil (1).
    buf.push(virgl_cmd0(VIRGL_CCMD_CLEAR, 0, 8));
    buf.push(PIPE_CLEAR_COLOR0);
    buf.push(fui(r));
    buf.push(fui(g));
    buf.push(fui(b));
    buf.push(fui(a));
    // depth is a double in the original encoder; we don't clear depth/stencil.
    buf.push(0);
    buf.push(0);
    buf.push(0); // stencil
}
fn encode_create_vertex_elements(buf: &mut VirglCmdBuf, ve_handle: u32) {
    // VIRGL_OBJ_VERTEX_ELEMENTS_SIZE(num) = num*4 + 1
    let num = 3u32;
    let len = 1 + num * 4;
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_VERTEX_ELEMENTS, len));
    buf.push(ve_handle);

    // element 0: position vec4 at offset 0 from vbo
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(VIRGL_FORMAT_R32G32B32A32_FLOAT);

    // element 1: uv vec4 at offset 16
    buf.push(16);
    buf.push(0);
    buf.push(0);
    buf.push(VIRGL_FORMAT_R32G32B32A32_FLOAT);

    // element 2: color vec4 at offset 32
    buf.push(32);
    buf.push(0);
    buf.push(0);
    buf.push(VIRGL_FORMAT_R32G32B32A32_FLOAT);
}
fn encode_bind_object(buf: &mut VirglCmdBuf, object: u8, handle: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_BIND_OBJECT, object, 1));
    buf.push(handle);
}
fn encode_set_vertex_buffer(buf: &mut VirglCmdBuf, stride: u32, offset: u32, res_handle: u32) {
    // VIRGL_SET_VERTEX_BUFFERS_SIZE(num) = num*3
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_VERTEX_BUFFERS, 0, 3));
    buf.push(stride);
    buf.push(offset);
    buf.push(res_handle);
}
fn encode_create_sampler_view(
    buf: &mut VirglCmdBuf,
    view_handle: u32,
    res_handle: u32,
    format: u32,
) {
    // virgl_protocol.h: VIRGL_OBJ_SAMPLER_VIEW_SIZE = 6
    const VIRGL_OBJ_SAMPLER_VIEW_SIZE: u32 = 6;

    // virgl_protocol.h packs 4 swizzles (3 bits each). Values match Gallium's
    // enum pipe_swizzle: X=0, Y=1, Z=2, W=3, 0=4, 1=5, NONE=6.
    // Identity RGBA swizzle is required for correct sampling.
    const PIPE_SWIZZLE_X: u32 = 0;
    const PIPE_SWIZZLE_Y: u32 = 1;
    const PIPE_SWIZZLE_Z: u32 = 2;
    const PIPE_SWIZZLE_W: u32 = 3;
    let swizzle = ((PIPE_SWIZZLE_X & 0x7) << 0)
        | ((PIPE_SWIZZLE_Y & 0x7) << 3)
        | ((PIPE_SWIZZLE_Z & 0x7) << 6)
        | ((PIPE_SWIZZLE_W & 0x7) << 9);

    buf.push(virgl_cmd0(
        VIRGL_CCMD_CREATE_OBJECT,
        VIRGL_OBJECT_SAMPLER_VIEW,
        VIRGL_OBJ_SAMPLER_VIEW_SIZE,
    ));
    buf.push(view_handle);
    buf.push(res_handle);
    buf.push(format);
    buf.push(0); // texture_layer / first element
    buf.push(0); // texture_level / last element
    buf.push(swizzle);
}
fn encode_set_sampler_views(
    buf: &mut VirglCmdBuf,
    shader_type: u32,
    start_slot: u32,
    views: &[u32],
) {
    let num = views.len().min(32) as u32;
    // virgl expects: shader_type, start_slot, then view handles.
    // Count is encoded in command length (VIRGL_SET_SAMPLER_VIEWS_SIZE).
    let len = num + 2;
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_SAMPLER_VIEWS, 0, len));
    buf.push(shader_type);
    buf.push(start_slot);
    for i in 0..(num as usize) {
        buf.push(views[i]);
    }
}
fn encode_bind_sampler_states(
    buf: &mut VirglCmdBuf,
    shader_type: u32,
    start_slot: u32,
    states: &[u32],
) {
    let num = states.len().min(32) as u32;
    // virgl expects: shader_type, start_slot, then state handles.
    // Count is encoded in command length (VIRGL_BIND_SAMPLER_STATES).
    let len = num + 2;
    buf.push(virgl_cmd0(VIRGL_CCMD_BIND_SAMPLER_STATES, 0, len));
    buf.push(shader_type);
    buf.push(start_slot);
    for i in 0..(num as usize) {
        buf.push(states[i]);
    }
}
fn encode_inline_write_buffer_at(
    buf: &mut VirglCmdBuf,
    res_handle: u32,
    dst_offset: u32,
    data: &[u8],
) {
    // Matches virgl_encoder_inline_send_box for a PIPE_BUFFER upload.
    // cmd length is data_dwords + 11
    let data_dwords = (data.len() as u32).div_ceil(4);
    buf.push(virgl_cmd0(VIRGL_CCMD_RESOURCE_INLINE_WRITE, 0, data_dwords + 11));
    buf.push(res_handle);
    buf.push(0); // level
    buf.push(0); // usage
    buf.push(data.len() as u32); // stride
    buf.push(0); // layer_stride
    buf.push(dst_offset); // box x (byte offset for PIPE_BUFFER)
    buf.push(0); // box y
    buf.push(0); // box z
    buf.push(data.len() as u32); // box width
    buf.push(1); // box height
    buf.push(1); // box depth
    buf.push_bytes_padded(data);
}
fn encode_inline_write_texture_region(
    buf: &mut VirglCmdBuf,
    res_handle: u32,
    dst_x: u32,
    dst_y: u32,
    width: u32,
    height: u32,
    stride_width: u32,
    rgba: &[u8],
) {
    // Matches virgl_encoder_inline_write() with a provided box for a 2D texture.
    // VIRGL_CMD0 stores payload length in 16 bits (dwords), so large textures must
    // be split into multiple RESOURCE_INLINE_WRITE commands.
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let data = if rgba.len() >= expected {
        &rgba[..expected]
    } else {
        rgba
    };
    let stride = stride_width.saturating_mul(4);
    if stride == 0 || height == 0 || data.is_empty() {
        return;
    }

    // VIRGL RESOURCE_INLINE_WRITE command has 11 dwords before inline payload.
    // cmd0 length field is 16-bit dwords, so keep each command at <= 0xFFFF dwords.
    let max_payload_dwords = 0xFFFFu32.saturating_sub(11);
    let max_payload_bytes = (max_payload_dwords as usize).saturating_mul(4);
    let rows_per_chunk = (max_payload_bytes / stride as usize).max(1);
    let total_rows = (data.len() / stride as usize).min(height as usize);

    let mut row = 0usize;
    while row < total_rows {
        let chunk_rows = (total_rows - row).min(rows_per_chunk);
        let chunk_bytes = chunk_rows.saturating_mul(stride as usize);
        let byte_off = row.saturating_mul(stride as usize);
        let chunk = &data[byte_off..byte_off + chunk_bytes];
        let data_dwords = (chunk.len() as u32).div_ceil(4);

        buf.push(virgl_cmd0(VIRGL_CCMD_RESOURCE_INLINE_WRITE, 0, data_dwords + 11));
        buf.push(res_handle);
        buf.push(0); // level
        buf.push(0); // usage
        buf.push(stride); // stride
        buf.push(stride.saturating_mul(chunk_rows as u32)); // layer_stride
        buf.push(dst_x); // box x
        buf.push(dst_y.saturating_add(row as u32)); // box y
        buf.push(0); // box z
        buf.push(width); // box width
        buf.push(chunk_rows as u32); // box height
        buf.push(1); // box depth
        buf.push_bytes_padded(chunk);

        row += chunk_rows;
    }
}
fn encode_set_viewport(buf: &mut VirglCmdBuf, width: u32, height: u32) {
    // VIRGL_SET_VIEWPORT_STATE_SIZE(num) = 6*num + 1
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_VIEWPORT_STATE, 0, 7));
    buf.push(0); // start_slot

    let half_w = width as f32 / 2.0;
    let half_h = height as f32 / 2.0;
    let half_d = 0.5;

    // scale[3]
    buf.push(fui(half_w));
    buf.push(fui(half_h));
    buf.push(fui(half_d));
    // translate[3]
    buf.push(fui(half_w));
    buf.push(fui(half_h));
    buf.push(fui(half_d));
}
fn encode_set_scissor(buf: &mut VirglCmdBuf, min_x: u32, min_y: u32, max_x: u32, max_y: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_SET_SCISSOR_STATE, 0, 3));
    buf.push(0); // start_slot
    buf.push((min_x & 0xffff) | ((min_y & 0xffff) << 16));
    buf.push((max_x & 0xffff) | ((max_y & 0xffff) << 16));
}
fn encode_create_blend(
    buf: &mut VirglCmdBuf,
    blend_handle: u32,
    enabled: bool,
    src_factor: u32,
    dst_factor: u32,
) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_BLEND, VIRGL_OBJ_BLEND_SIZE));
    // Values from Mesa pipe/p_defines.h (virgl uses the Gallium enums).
    // NOTE: We only rely on a tiny subset that we have validated with virglrenderer.
    const PIPE_BLEND_ADD: u32 = 0;

    buf.push(blend_handle);
    buf.push(0); // s0
    buf.push(0); // s1
    for i in 0..8u32 {
        let mut rt = 0u32;
        if i == 0 {
            if enabled {
                // Enable blending on RT0.
                rt |= 1 << 0;
                rt |= (PIPE_BLEND_ADD & 0x7) << 1;
                rt |= (src_factor & 0x1f) << 4;
                rt |= (dst_factor & 0x1f) << 9;
                // Alpha blend: mirror RGB factors.
                rt |= (PIPE_BLEND_ADD & 0x7) << 14;
                rt |= (src_factor & 0x1f) << 17;
                rt |= (dst_factor & 0x1f) << 22;
            }
            // Write all color channels.
            rt |= (PIPE_MASK_RGBA & 0xF) << 27;
        }
        buf.push(rt);
    }
}
fn encode_create_dsa(buf: &mut VirglCmdBuf, dsa_handle: u32) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_DSA, VIRGL_OBJ_DSA_SIZE));
    buf.push(dsa_handle);
    buf.push(0);
    buf.push(0);
    buf.push(0);
    buf.push(0); // alpha_ref
}
fn encode_create_rasterizer(buf: &mut VirglCmdBuf, rs_handle: u32, scissor_enable: bool) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJ_RS_SIZE));
    buf.push(rs_handle);

    let mut s0 = 0u32;
    // depth_clip=1
    s0 |= 1 << 1;
    if scissor_enable {
        s0 |= 1 << 14;
    }
    // cull_face=PIPE_FACE_NONE(0) at bits 8..9 -> 0
    // half_pixel_center=1
    s0 |= 1 << 29;
    // bottom_edge_rule=1
    s0 |= 1 << 30;
    buf.push(s0);
    buf.push(fui(1.0)); // point_size
    buf.push(0); // sprite_coord_enable
    buf.push(0); // s3
    buf.push(fui(1.0)); // line_width
    buf.push(fui(0.0)); // offset_units
    buf.push(fui(0.0)); // offset_scale
    buf.push(fui(0.0)); // offset_clamp
}

fn encode_sampler(buf: &mut VirglCmdBuf) {
    buf.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SAMPLER_STATE, 9));
    buf.push(26);
    buf.push(2 | (2 << 3) | (2 << 6) | (1 << 9) | (2 << 11) | (1 << 13));
    for word in [0, 0, fui(1000.0), 0, 0, 0, 0] {
        buf.push(word);
    }
}

const PIPE_SHADER_VERTEX: u32 = 0;
const PIPE_SHADER_FRAGMENT: u32 = 1;
const CONTEXT: u32 = 1;
const TARGETS: [u32; 2] = [100, 101];
const VBO: u32 = 102;
const BACKGROUND: u32 = 103;
const WHITE: u32 = 104;
const TEXTURE_FORMAT_RGBA: u32 = 67;
const TEXTURE_FORMAT_BGRA: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct ContextCreate {
    hdr: CtrlHdr,
    nlen: u32,
    init: u32,
    name: [u8; 64],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Resource3d {
    hdr: CtrlHdr,
    id: u32,
    target: u32,
    format: u32,
    bind: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    samples: u32,
    flags: u32,
    padding: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ResourceCommand {
    hdr: CtrlHdr,
    id: u32,
    padding: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Submit {
    hdr: CtrlHdr,
    size: u32,
    padding: u32,
}

impl VirtioGpuLogo {
    fn create_3d(
        &mut self,
        id: u32,
        width: u32,
        height: u32,
        format: u32,
        bind: u32,
        buffer: bool,
    ) -> bool {
        let command = Resource3d {
            hdr: CtrlHdr {
                type_: 0x204,
                ctx_id: CONTEXT,
                ..Default::default()
            },
            id,
            target: if buffer { 0 } else { 2 },
            format,
            bind,
            width,
            height,
            depth: 1,
            array_size: 1,
            ..Default::default()
        };
        self.ctrl_submit_bytes(as_bytes(&command))
            && self.ctrl_submit_bytes(as_bytes(&ResourceCommand {
                hdr: CtrlHdr {
                    type_: 0x202,
                    ctx_id: CONTEXT,
                    ..Default::default()
                },
                id,
                padding: 0,
            }))
    }

    // A fenced control response is returned only after the renderer fence has
    // signalled. Never mistake an unfenced used-ring entry for GPU completion.
    fn submit_3d(&mut self, commands: &VirglCmdBuf, serial: u64) -> bool {
        let header = Submit {
            hdr: CtrlHdr {
                type_: 0x207,
                flags: 1,
                fence_id: serial,
                ctx_id: CONTEXT,
                ..Default::default()
            },
            size: commands.as_bytes().len() as u32,
            padding: 0,
        };
        let length = core::mem::size_of::<Submit>() + commands.as_bytes().len();
        if self.failed || length > self.req.len() {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                as_bytes(&header).as_ptr(),
                self.req.virt(),
                core::mem::size_of::<Submit>(),
            );
            core::ptr::copy_nonoverlapping(
                commands.as_bytes().as_ptr(),
                self.req.virt().add(core::mem::size_of::<Submit>()),
                commands.as_bytes().len(),
            );
            core::ptr::write_bytes(self.resp.virt(), 0, self.resp.len());
        }
        if !self.ctrl_submit_desc_chain(length) {
            return false;
        }
        let response = unsafe { core::ptr::read_unaligned(self.resp.virt().cast::<CtrlHdr>()) };
        let ok = response.flags & 1 != 0 && response.fence_id == serial;
        self.failed |= !ok;
        ok
    }
}

struct Texture {
    frame: crate::ui4::FrameHandle,
    serial: u64,
    id: u32,
    width: u32,
    height: u32,
}

pub(super) struct Compositor {
    scanout: u32,
    width: u32,
    height: u32,
    next_target: usize,
    next_id: u32,
    fence: u64,
    revision: Option<u64>,
    background_color: u64,
    textures: Vec<Texture>,
    presents: u64,
    interaction_rects: Vec<crate::intel::LiveOverlayRect>,
}

impl Compositor {
    pub(super) fn new(
        gpu: &mut VirtioGpuLogo,
        scanout: u32,
        width: u32,
        height: u32,
        background: &DmaRegion,
    ) -> Option<Self> {
        let mut name = [0; 64];
        name[..9].copy_from_slice(b"TRUEOSUI4");
        if !gpu.ctrl_submit_bytes(as_bytes(&ContextCreate {
            hdr: CtrlHdr {
                type_: 0x200,
                ctx_id: CONTEXT,
                ..Default::default()
            },
            nlen: 9,
            init: 0,
            name,
        })) {
            return None;
        }
        for id in TARGETS {
            if !gpu.create_3d(id, width, height, TEXTURE_FORMAT_BGRA, (1 << 1) | (1 << 2), false) {
                return None;
            }
        }
        if !gpu.create_3d(VBO, 240 * 1024, 1, 64, 1 << 4, true)
            || !gpu.create_3d(BACKGROUND, width, height, TEXTURE_FORMAT_BGRA, 1 << 3, false)
        {
            return None;
        }
        if !gpu.create_3d(WHITE, 1, 1, TEXTURE_FORMAT_RGBA, 1 << 3, false) {
            return None;
        }
        let mut this = Self {
            scanout,
            width,
            height,
            next_target: 0,
            next_id: 105,
            fence: 0,
            revision: None,
            background_color: 0,
            textures: Vec::new(),
            presents: 0,
            interaction_rects: Vec::new(),
        };
        let mut init = VirglCmdBuf::new();
        for id in TARGETS {
            encode_create_surface(&mut init, id, id, TEXTURE_FORMAT_BGRA);
        }
        encode_create_vertex_elements(&mut init, 11);
        encode_bind_object(&mut init, VIRGL_OBJECT_VERTEX_ELEMENTS, 11);
        encode_set_vertex_buffer(&mut init, 48, 0, VBO);
        encode_shader(&mut init, 20, PIPE_SHADER_VERTEX, VS_TEX);
        encode_shader(&mut init, 21, PIPE_SHADER_FRAGMENT, FS_TEX_RGBA);
        encode_shader(&mut init, 22, PIPE_SHADER_FRAGMENT, FS_STRAIGHT);
        encode_bind_shader(&mut init, 20, PIPE_SHADER_VERTEX);
        encode_bind_shader(&mut init, 21, PIPE_SHADER_FRAGMENT);
        encode_link_shader(&mut init, 20, 21);
        encode_create_blend(&mut init, 30, true, 1, 0x13); // ONE, INV_SRC_ALPHA
        encode_bind_object(&mut init, VIRGL_OBJECT_BLEND, 30);
        encode_create_blend(&mut init, 33, false, 1, 0);
        encode_create_dsa(&mut init, 31);
        encode_bind_object(&mut init, VIRGL_OBJECT_DSA, 31);
        encode_create_rasterizer(&mut init, 32, true);
        encode_bind_object(&mut init, VIRGL_OBJECT_RASTERIZER, 32);
        encode_sampler(&mut init);
        init.push(virgl_cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SAMPLER_STATE, 9));
        init.push(27);
        init.push(2 | (2 << 3) | (2 << 6) | (2 << 11));
        for word in [0, 0, fui(1000.0), 0, 0, 0, 0] {
            init.push(word);
        }
        encode_create_sampler_view(&mut init, WHITE, WHITE, TEXTURE_FORMAT_RGBA);
        encode_bind_sampler_states(&mut init, PIPE_SHADER_FRAGMENT, 0, &[26]);
        encode_create_sampler_view(&mut init, BACKGROUND, BACKGROUND, TEXTURE_FORMAT_BGRA);
        encode_set_viewport(&mut init, width, height);
        encode_set_scissor(&mut init, 0, 0, width, height);
        if !this.submit(gpu, &init) {
            return None;
        }
        if !this.upload(gpu, WHITE, 1, 1, 4, &[255; 4]) {
            return None;
        }
        let pixels = unsafe {
            core::slice::from_raw_parts(background.virt(), width as usize * height as usize * 4)
        };
        if !this.upload(gpu, BACKGROUND, width, height, width * 4, pixels) {
            return None;
        }
        this.tick(gpu);
        if gpu.failed || this.presents == 0 {
            return None;
        }
        crate::log_important!(target: "gfx"; "virgl-ui4: ready {}x{} backend=textured-quads completion=fenced buffers=2 interaction=slot4-software hardware_cursor=disabled\n", width, height);
        Some(this)
    }

    pub(super) fn update_background(&mut self, gpu: &mut VirtioGpuLogo, backing: &DmaRegion) {
        let pixels = unsafe { core::slice::from_raw_parts(backing.virt(), backing.len()) };
        if self.upload(gpu, BACKGROUND, self.width, self.height, self.width * 4, pixels) {
            self.revision = None;
        }
    }

    fn submit(&mut self, gpu: &mut VirtioGpuLogo, commands: &VirglCmdBuf) -> bool {
        self.fence += 1;
        gpu.submit_3d(commands, self.fence)
    }

    fn upload(
        &mut self,
        gpu: &mut VirtioGpuLogo,
        id: u32,
        width: u32,
        height: u32,
        pitch: u32,
        pixels: &[u8],
    ) -> bool {
        let row_bytes = width as usize * 4;
        if width == 0
            || height == 0
            || row_bytes > 240 * 1024
            || (pitch as usize) < row_bytes
            || pixels.len() < (height as usize - 1) * pitch as usize + row_bytes
        {
            return false;
        }
        // Each virgl command has a 16-bit dword length; uploads are bounded
        // independently of surface size and preserve padded producer pitches.
        let chunk_rows = (240 * 1024 / row_bytes).max(1);
        for row in (0..height as usize).step_by(chunk_rows) {
            let count = chunk_rows.min(height as usize - row);
            let mut bytes = Vec::with_capacity(count * row_bytes);
            for y in row..row + count {
                bytes
                    .extend_from_slice(&pixels[y * pitch as usize..y * pitch as usize + row_bytes]);
            }
            let mut commands = VirglCmdBuf::new();
            encode_inline_write_texture_region(
                &mut commands,
                id,
                0,
                row as u32,
                width,
                count as u32,
                width,
                &bytes,
            );
            if !self.submit(gpu, &commands) {
                return false;
            }
        }
        true
    }

    fn quad(
        &self,
        commands: &mut VirglCmdBuf,
        texture: u32,
        placement: crate::ui4::WindowPlacement,
        vbo_offset: u32,
    ) {
        let x0 = placement.x as f32 * 2.0 / self.width as f32 - 1.0;
        let x1 = (placement.x as f32 + placement.width as f32) * 2.0 / self.width as f32 - 1.0;
        let y0 = placement.y as f32 * 2.0 / self.height as f32 - 1.0;
        let y1 = (placement.y as f32 + placement.height as f32) * 2.0 / self.height as f32 - 1.0;
        let a = placement.opacity as f32 / 255.0;
        let vertices = [
            (x0, y0, 0., 0.),
            (x0, y1, 0., 1.),
            (x1, y1, 1., 1.),
            (x0, y0, 0., 0.),
            (x1, y1, 1., 1.),
            (x1, y0, 1., 0.),
        ]
        .map(|(x, y, u, v)| Vertex {
            pos: [x, y, 0., 1.],
            uv: [u, v, 0., 0.],
            color: [a; 4],
        });
        let bytes = unsafe {
            core::slice::from_raw_parts(
                vertices.as_ptr().cast::<u8>(),
                core::mem::size_of_val(&vertices),
            )
        };
        // Inline writes are unsynchronized in virglrenderer. Each draw in
        // this submission owns a disjoint VBO range until its fence retires.
        encode_inline_write_buffer_at(commands, VBO, vbo_offset, bytes);
        encode_set_vertex_buffer(commands, 48, vbo_offset, VBO);
        encode_set_sampler_views(commands, PIPE_SHADER_FRAGMENT, 0, &[texture]);
        encode_draw_vbo_count(commands, 6);
    }

    pub(super) fn tick(&mut self, gpu: &mut VirtioGpuLogo) {
        use crate::ui4::*;
        if gpu.failed {
            return;
        }
        advance_window_open_transitions();
        advance_window_close_transitions();
        let (revision, windows) = crate::ui4::emulator_windows();
        let background_color = EMULATOR_BACKGROUND.load(Ordering::Acquire);
        let interaction_rects = crate::ui4::interaction_overlay_rects();
        let interaction_unchanged = self.interaction_rects.len() == interaction_rects.len()
            && self
                .interaction_rects
                .iter()
                .zip(&interaction_rects)
                .all(|(a, b)| {
                    (a.x, a.y, a.width, a.height, a.color) == (b.x, b.y, b.width, b.height, b.color)
                });
        if self.revision == Some(revision)
            && self.background_color == background_color
            && interaction_unchanged
        {
            return;
        }
        let mut draws = Vec::new();
        // Read leases protect each CPU upload. The host owns a copy after the
        // fenced upload; it never reads producer backing during later draws.
        for window in &windows {
            let Ok(before) = frame_snapshot(window.frame) else {
                return;
            };
            let Ok(lease) = acquire_published_frame(window.frame) else {
                return;
            };
            let result = (|| {
                let snapshot = frame_snapshot(window.frame).ok()?;
                if snapshot.publish_serial != before.publish_serial
                    || before.front_buffer != Some(lease.buffer_index)
                {
                    return None;
                }
                let view = published_rgba_view(lease).ok()?;
                if view.gpu_authored {
                    return None;
                } // Intel resources are never CPU-imported here.
                let existing = self.textures.iter().position(|t| {
                    t.frame == window.frame && t.width == view.width && t.height == view.height
                });
                let index = if let Some(index) = existing {
                    index
                } else {
                    let id = self.next_id;
                    self.next_id = self.next_id.checked_add(1)?;
                    if !gpu.create_3d(
                        id,
                        view.width,
                        view.height,
                        TEXTURE_FORMAT_RGBA,
                        (1 << 1) | (1 << 2) | (1 << 3),
                        false,
                    ) {
                        return None;
                    }
                    let mut commands = VirglCmdBuf::new();
                    encode_create_sampler_view(&mut commands, id, id, TEXTURE_FORMAT_RGBA);
                    encode_create_surface(&mut commands, 0x8000_0000 | id, id, TEXTURE_FORMAT_RGBA);
                    if !self.submit(gpu, &commands) {
                        return None;
                    }
                    self.textures.push(Texture {
                        frame: window.frame,
                        serial: 0,
                        id,
                        width: view.width,
                        height: view.height,
                    });
                    self.textures.len() - 1
                };
                let id = self.textures[index].id;
                if self.textures[index].serial != snapshot.publish_serial {
                    let pixels = unsafe { core::slice::from_raw_parts(view.virt, view.byte_len) };
                    if !self.upload(gpu, id, view.width, view.height, view.pitch, pixels) {
                        return None;
                    }
                    let paints = crate::ui4::emulator_paint::snapshot(lease);
                    if !self.paint(gpu, id, view.width, view.height, &paints) {
                        return None;
                    }
                    self.textures[index].serial = snapshot.publish_serial;
                }
                Some(id)
            })();
            let _ = release_published_frame(lease);
            let Some(texture) = result else {
                return;
            };
            draws.push((texture, window.presentation_placement));
        }
        let target = TARGETS[self.next_target];
        let mut commands = VirglCmdBuf::new();
        encode_set_framebuffer(&mut commands, target);
        encode_set_viewport(&mut commands, self.width, self.height);
        encode_set_scissor(&mut commands, 0, 0, self.width, self.height);
        encode_bind_shader(&mut commands, 21, PIPE_SHADER_FRAGMENT);
        encode_bind_object(&mut commands, VIRGL_OBJECT_BLEND, 30);
        encode_bind_sampler_states(&mut commands, PIPE_SHADER_FRAGMENT, 0, &[26]);
        encode_clear_color(
            &mut commands,
            ((background_color >> 16) & 255) as f32 / 255.,
            ((background_color >> 8) & 255) as f32 / 255.,
            (background_color & 255) as f32 / 255.,
            1.,
        );
        if background_color == 0 {
            self.quad(
                &mut commands,
                BACKGROUND,
                WindowPlacement {
                    x: 0,
                    y: 0,
                    width: self.width,
                    height: self.height,
                    z: 0,
                    opacity: 255,
                    visible: true,
                },
                0,
            );
        }
        for (index, (texture, placement)) in draws.into_iter().enumerate() {
            let offset = (index + 1) * 6 * core::mem::size_of::<Vertex>();
            if offset + 6 * core::mem::size_of::<Vertex>() > 240 * 1024 {
                return;
            }
            self.quad(&mut commands, texture, placement, offset as u32);
        }
        if !self.submit(gpu, &commands)
            || !self.draw_interaction_rects(gpu, &interaction_rects)
            || !gpu.set_scanout(self.scanout, target, self.width, self.height)
            || !gpu.resource_flush(target, self.width, self.height)
        {
            return;
        }
        for window in &windows {
            crate::ui4::emulator_acknowledge(*window);
        }
        self.interaction_rects = interaction_rects;
        self.revision = Some(revision);
        self.background_color = background_color;
        self.next_target ^= 1;
        self.presents += 1;
        if self.presents <= 16 || self.presents.is_power_of_two() {
            crate::log_important!(target: "gfx"; "virgl-ui4: presented frame={} windows={} fence={} revision={}\n", self.presents, windows.len(), self.fence, revision);
        }
        // All draws above retired. Destroy resources whose logical frames left
        // the broker, keeping memory bounded across repeated open/close cycles.
        let mut i = 0;
        while i < self.textures.len() {
            if windows.iter().any(|w| w.frame == self.textures[i].frame) {
                i += 1;
                continue;
            }
            let texture = self.textures.remove(i);
            let mut commands = VirglCmdBuf::new();
            commands.push(virgl_cmd0(3, VIRGL_OBJECT_SURFACE, 1));
            commands.push(0x8000_0000 | texture.id);
            commands.push(virgl_cmd0(3, VIRGL_OBJECT_SAMPLER_VIEW, 1));
            commands.push(texture.id);
            if !self.submit(gpu, &commands) {
                return;
            }
            for kind in [0x203, 0x102] {
                if !gpu.ctrl_submit_bytes(as_bytes(&ResourceCommand {
                    hdr: CtrlHdr {
                        type_: kind,
                        ctx_id: CONTEXT,
                        ..Default::default()
                    },
                    id: texture.id,
                    padding: 0,
                })) {
                    return;
                }
            }
        }
    }
}

const FS_STRAIGHT: &str = "FRAG\nDCL IN[0], TEXCOORD[0], LINEAR\nDCL IN[1], COLOR, LINEAR\nDCL SAMP[0]\nDCL TEMP[0]\nDCL OUT[0], COLOR\n0: TEX TEMP[0], IN[0], SAMP[0], 2D\n1: MUL TEMP[0], TEMP[0], IN[1]\n2: MUL OUT[0].xyz, TEMP[0], TEMP[0].wwww\n3: MOV OUT[0].w, TEMP[0].wwww\n4: END\n";

impl Compositor {
    fn paint(
        &mut self,
        gpu: &mut VirtioGpuLogo,
        target: u32,
        width: u32,
        height: u32,
        paints: &[crate::ui4::emulator_paint::Paint],
    ) -> bool {
        if paints.is_empty() {
            return true;
        }
        let mut images: Vec<(usize, u32)> = Vec::new();
        for paint in paints {
            let texture = if let Some(image) = &paint.image {
                let key = alloc::sync::Arc::as_ptr(image) as usize;
                if let Some((_, id)) = images.iter().find(|(k, _)| *k == key) {
                    *id
                } else {
                    let id = self.next_id;
                    self.next_id += 1;
                    if !gpu.create_3d(
                        id,
                        image.width,
                        image.height,
                        TEXTURE_FORMAT_RGBA,
                        1 << 3,
                        false,
                    ) {
                        return false;
                    }
                    let mut commands = VirglCmdBuf::new();
                    encode_create_sampler_view(&mut commands, id, id, TEXTURE_FORMAT_RGBA);
                    if !self.submit(gpu, &commands)
                        || !self.upload(
                            gpu,
                            id,
                            image.width,
                            image.height,
                            image.width * 4,
                            &image.pixels,
                        )
                    {
                        return false;
                    }
                    images.push((key, id));
                    id
                }
            } else {
                WHITE
            };
            let premultiplied = paint.image.as_ref().is_none_or(|i| i.premultiplied);
            let mut color = paint.color.to_le_bytes().map(|c| c as f32 / 255.0);
            if premultiplied {
                for i in 0..3 {
                    color[i] *= color[3];
                }
            }
            // Bounded triangle batches share one reusable VBO. The fence keeps
            // its next upload behind the preceding draw on the host GPU.
            for points in paint.vertices.chunks(4095) {
                let vertices: Vec<Vertex> = points
                    .iter()
                    .map(|p| Vertex {
                        pos: [
                            p.position[0] * 2.0 / width as f32 - 1.0,
                            p.position[1] * 2.0 / height as f32 - 1.0,
                            0.,
                            1.,
                        ],
                        uv: [p.uv[0], p.uv[1], 0., 0.],
                        color,
                    })
                    .collect();
                let mut commands = VirglCmdBuf::new();
                encode_set_framebuffer(&mut commands, 0x8000_0000 | target);
                encode_set_viewport(&mut commands, width, height);
                encode_set_scissor(&mut commands, 0, 0, width, height);
                encode_bind_shader(
                    &mut commands,
                    if premultiplied { 21 } else { 22 },
                    PIPE_SHADER_FRAGMENT,
                );
                encode_bind_object(
                    &mut commands,
                    VIRGL_OBJECT_BLEND,
                    if paint.source_over { 30 } else { 33 },
                );
                encode_bind_sampler_states(&mut commands, PIPE_SHADER_FRAGMENT, 0, &[27]);
                encode_set_sampler_views(&mut commands, PIPE_SHADER_FRAGMENT, 0, &[texture]);
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        vertices.as_ptr().cast::<u8>(),
                        vertices.len() * core::mem::size_of::<Vertex>(),
                    )
                };
                encode_inline_write_buffer_at(&mut commands, VBO, 0, bytes);
                encode_set_vertex_buffer(&mut commands, 48, 0, VBO);
                encode_draw_vbo_count(&mut commands, vertices.len() as u32);
                if !self.submit(gpu, &commands) {
                    return false;
                }
            }
        }
        for (_, id) in images {
            let mut commands = VirglCmdBuf::new();
            encode_set_sampler_views(&mut commands, PIPE_SHADER_FRAGMENT, 0, &[WHITE]);
            commands.push(virgl_cmd0(3, VIRGL_OBJECT_SAMPLER_VIEW, 1));
            commands.push(id);
            if !self.submit(gpu, &commands) {
                return false;
            }
            for kind in [0x203, 0x102] {
                if !gpu.ctrl_submit_bytes(as_bytes(&ResourceCommand {
                    hdr: CtrlHdr {
                        type_: kind,
                        ctx_id: CONTEXT,
                        ..Default::default()
                    },
                    id,
                    padding: 0,
                })) {
                    return false;
                }
            }
        }
        true
    }
}

impl Compositor {
    /// The native slot-4 builder supplies the same cursors, selection outlines,
    /// dock fields and menus. Draw them last, above every broker window, into
    /// the pending scanout; publish only after these batches have retired.
    fn draw_interaction_rects(
        &mut self,
        gpu: &mut VirtioGpuLogo,
        rects: &[crate::intel::LiveOverlayRect],
    ) -> bool {
        for batch in rects.chunks(682) {
            let mut vertices = Vec::<Vertex>::with_capacity(batch.len() * 6);
            for rect in batch {
                let x0 = rect.x.min(self.width) as f32 * 2.0 / self.width as f32 - 1.0;
                let x1 = rect.x.saturating_add(rect.width).min(self.width) as f32 * 2.0
                    / self.width as f32
                    - 1.0;
                let y0 = rect.y.min(self.height) as f32 * 2.0 / self.height as f32 - 1.0;
                let y1 = rect.y.saturating_add(rect.height).min(self.height) as f32 * 2.0
                    / self.height as f32
                    - 1.0;
                let a = rect.color.a as f32 / 255.0;
                let color = [
                    rect.color.r as f32 / 255.0 * a,
                    rect.color.g as f32 / 255.0 * a,
                    rect.color.b as f32 / 255.0 * a,
                    a,
                ];
                for (x, y) in [(x0, y0), (x0, y1), (x1, y1), (x0, y0), (x1, y1), (x1, y0)] {
                    vertices.push(Vertex {
                        pos: [x, y, 0., 1.],
                        uv: [0.5, 0.5, 0., 0.],
                        color,
                    });
                }
            }
            let mut commands = VirglCmdBuf::new();
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    vertices.as_ptr().cast::<u8>(),
                    vertices.len() * core::mem::size_of::<Vertex>(),
                )
            };
            // The preceding composition/batch fence protects reuse of offset 0.
            encode_inline_write_buffer_at(&mut commands, VBO, 0, bytes);
            encode_set_vertex_buffer(&mut commands, 48, 0, VBO);
            encode_set_sampler_views(&mut commands, PIPE_SHADER_FRAGMENT, 0, &[WHITE]);
            encode_draw_vbo_count(&mut commands, vertices.len() as u32);
            if !self.submit(gpu, &commands) {
                return false;
            }
        }
        true
    }
}
