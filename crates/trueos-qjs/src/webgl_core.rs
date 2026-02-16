extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use crate::webgl_texture::WebGlTextureState;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WebGlVertexAttrib {
    pub(crate) enabled: bool,
    pub(crate) size: i32,
    pub(crate) ty: u32,
    pub(crate) normalized: bool,
    pub(crate) stride: i32,
    pub(crate) offset: usize,
    pub(crate) buffer: u32,
}

#[derive(Clone)]
pub(crate) struct WebGlDecodedVertex {
    pub(crate) x_px: f32,
    pub(crate) y_px: f32,
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
    pub(crate) a: u8,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct WebGlDrawElementsCacheKey {
    pub(crate) count: usize,
    pub(crate) index_off: usize,
    pub(crate) element_array_buffer: u32,
    pub(crate) element_array_version: u32,
    pub(crate) pos: WebGlVertexAttrib,
    pub(crate) pos_buffer_version: u32,
    pub(crate) col: Option<WebGlVertexAttrib>,
    pub(crate) col_buffer_version: u32,
}

#[derive(Clone)]
pub(crate) struct WebGlDrawElementsCache {
    pub(crate) key: WebGlDrawElementsCacheKey,
    pub(crate) verts: Vec<WebGlDecodedVertex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebGlUniformKind {
    Other,
    TranslationMatrix,
    ProjectionMatrix,
}

#[derive(Clone)]
pub(crate) struct WebGlActiveAttrib {
    pub(crate) name: Vec<u8>,
    pub(crate) gl_type: u32,
    pub(crate) size: i32,
}

#[derive(Clone)]
pub(crate) struct WebGlActiveUniform {
    pub(crate) name: Vec<u8>,
    pub(crate) gl_type: u32,
    pub(crate) size: i32,
}

#[derive(Clone, Default)]
pub(crate) struct WebGlVaoState {
    pub(crate) element_array_buffer: u32,
    pub(crate) attribs: BTreeMap<u32, WebGlVertexAttrib>,
}

pub(crate) struct WebGlState {
    pub(crate) array_buffer: u32,
    pub(crate) element_array_buffer: u32,
    pub(crate) buffers: BTreeMap<u32, Vec<u8>>,
    pub(crate) buffer_versions: BTreeMap<u32, u32>,
    pub(crate) next_buffer_version: u32,
    pub(crate) attribs: BTreeMap<u32, WebGlVertexAttrib>,
    pub(crate) attrib_name_to_loc: BTreeMap<Vec<u8>, u32>,
    pub(crate) attrib_loc_to_name: BTreeMap<u32, Vec<u8>>,
    pub(crate) next_attrib_loc: u32,
    pub(crate) uniform_locs: BTreeMap<u32, WebGlUniformKind>,
    pub(crate) uniform_name_to_loc: BTreeMap<Vec<u8>, u32>,
    pub(crate) next_uniform_loc: u32,
    pub(crate) shader_types: BTreeMap<u32, u32>,
    pub(crate) shader_sources: BTreeMap<u32, Vec<u8>>,
    pub(crate) program_shaders: BTreeMap<u32, Vec<u32>>,
    pub(crate) program_active_attribs: BTreeMap<u32, Vec<WebGlActiveAttrib>>,
    pub(crate) program_active_uniforms: BTreeMap<u32, Vec<WebGlActiveUniform>>,
    pub(crate) translation_matrix: [f32; 9],
    pub(crate) projection_matrix: [f32; 9],
    pub(crate) has_translation_matrix: bool,
    pub(crate) has_projection_matrix: bool,
    pub(crate) clear_rgb: u32,
    pub(crate) viewport_w: i32,
    pub(crate) viewport_h: i32,
    pub(crate) enabled_blend: bool,
    pub(crate) enabled_cull_face: bool,
    pub(crate) enabled_depth_test: bool,
    pub(crate) enabled_scissor_test: bool,
    pub(crate) front_face_mode: u32,
    pub(crate) cull_face_mode: u32,
    pub(crate) blend_src_rgb: u32,
    pub(crate) blend_dst_rgb: u32,
    pub(crate) blend_src_alpha: u32,
    pub(crate) blend_dst_alpha: u32,
    pub(crate) blend_eq_rgb: u32,
    pub(crate) blend_eq_alpha: u32,
    pub(crate) current_vao: u32,
    pub(crate) vao0: WebGlVaoState,
    pub(crate) vaos: BTreeMap<u32, WebGlVaoState>,
    pub(crate) textures: WebGlTextureState,
    pub(crate) pending_frame_clear_rgb: u32,
    pub(crate) pending_frame_active: bool,
    pub(crate) pending_frame_vtx: Vec<u8>,
    pub(crate) draw_elements_cache: Option<WebGlDrawElementsCache>,
}

pub(crate) static WEBGL_STATE: Mutex<WebGlState> = Mutex::new(WebGlState {
    array_buffer: 0,
    element_array_buffer: 0,
    buffers: BTreeMap::new(),
    buffer_versions: BTreeMap::new(),
    next_buffer_version: 1,
    attribs: BTreeMap::new(),
    attrib_name_to_loc: BTreeMap::new(),
    attrib_loc_to_name: BTreeMap::new(),
    next_attrib_loc: 0,
    uniform_locs: BTreeMap::new(),
    uniform_name_to_loc: BTreeMap::new(),
    next_uniform_loc: 1,
    shader_types: BTreeMap::new(),
    shader_sources: BTreeMap::new(),
    program_shaders: BTreeMap::new(),
    program_active_attribs: BTreeMap::new(),
    program_active_uniforms: BTreeMap::new(),
    translation_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    projection_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    has_translation_matrix: false,
    has_projection_matrix: false,
    clear_rgb: 0x00_08_18_30,
    viewport_w: 0,
    viewport_h: 0,
    enabled_blend: false,
    enabled_cull_face: false,
    enabled_depth_test: false,
    enabled_scissor_test: false,
    front_face_mode: 0x0901,
    cull_face_mode: 0x0405,
    blend_src_rgb: 1,
    blend_dst_rgb: 0,
    blend_src_alpha: 1,
    blend_dst_alpha: 0,
    blend_eq_rgb: 0x8006,
    blend_eq_alpha: 0x8006,
    current_vao: 0,
    vao0: WebGlVaoState {
        element_array_buffer: 0,
        attribs: BTreeMap::new(),
    },
    vaos: BTreeMap::new(),
    textures: WebGlTextureState {
        active_unit: 0,
        unpack_alignment: 4,
        bound_tex2d_by_unit: BTreeMap::new(),
        params: BTreeMap::new(),
        images: BTreeMap::new(),
    },
    pending_frame_clear_rgb: 0x00_08_18_30,
    pending_frame_active: false,
    pending_frame_vtx: Vec::new(),
    draw_elements_cache: None,
});
