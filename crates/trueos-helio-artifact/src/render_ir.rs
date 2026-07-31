//! Borrowed parser for Helio's normalized, scene-independent render IR v1.

use core::str;

pub const MAGIC: [u8; 8] = *b"HELIOIR\0";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 256;
pub const SECTION_NAME: &str = "render/ir-v1.bin";

const KNOWN_STATE_FLAGS: u32 = (1 << 6) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexFormat {
    Uint16,
    Uint32,
}

impl IndexFormat {
    pub const fn byte_width(self) -> usize {
        match self {
            Self::Uint16 => 2,
            Self::Uint32 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureFormat {
    Bgra8UnormSrgb,
    Depth32Float,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveTopology {
    TriangleList,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontFace {
    Ccw,
    Cw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CullMode {
    None,
    Front,
    Back,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompareFunction {
    Less,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VertexFormat {
    Float32x3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    StorageBuffer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShaderStages(u32);

impl ShaderStages {
    pub const VERTEX: Self = Self(1);
    pub const FRAGMENT: Self = Self(2);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateFlags(u32);

impl StateFlags {
    pub const DEPTH_WRITE: Self = Self(1 << 0);
    pub const COLOR_STORE: Self = Self(1 << 1);
    pub const DEPTH_STORE: Self = Self(1 << 2);
    pub const BINDING_READ_ONLY: Self = Self(1 << 3);
    pub const COLOR_CLEAR: Self = Self(1 << 4);
    pub const DEPTH_CLEAR: Self = Self(1 << 5);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertexAttribute {
    pub shader_location: u32,
    pub format: VertexFormat,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertexBuffer<'a> {
    pub id: ResourceId,
    pub data: &'a [u8],
    pub stride: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexBuffer<'a> {
    pub id: ResourceId,
    pub data: &'a [u8],
    pub format: IndexFormat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shader<'a> {
    pub wgsl: &'a str,
    pub vertex_entry: &'a str,
    pub fragment_entry: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBinding<'a> {
    pub buffer_id: ResourceId,
    pub minimum_size: u32,
    pub dynamic_slot: &'a str,
    pub group: u32,
    pub binding: u32,
    pub kind: BindingKind,
    pub visibility: ShaderStages,
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipelineState {
    pub color_format: TextureFormat,
    pub depth_format: TextureFormat,
    pub topology: PrimitiveTopology,
    pub front_face: FrontFace,
    pub cull_mode: CullMode,
    pub depth_compare: CompareFunction,
    pub flags: StateFlags,
    pub color_write_mask: u32,
    pub clear_color: [f32; 4],
    pub clear_depth: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawIndexed {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
}

/// One normalized render program. V1 intentionally represents the single
/// indexed render pass needed by Helio's simple cube, without naming the cube.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Program<'a> {
    pub vertex: VertexBuffer<'a>,
    pub index: IndexBuffer<'a>,
    pub attributes: [VertexAttribute; 3],
    pub attribute_count: usize,
    pub shader: Shader<'a>,
    pub camera: CameraBinding<'a>,
    pub pipeline: PipelineState,
    pub draw: DrawIndexed,
    pub output_dynamic_slot: &'a str,
    pub pass_label: &'a str,
}

impl<'a> Program<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_LEN || bytes.get(..8) != Some(MAGIC.as_slice()) {
            return Err(Error::BadMagic);
        }
        let version = read_u16(bytes, 8)?;
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        if usize::from(read_u16(bytes, 10)?) != HEADER_LEN {
            return Err(Error::MalformedHeader);
        }
        if to_usize(read_u32(bytes, 12))? != bytes.len() {
            return Err(Error::LengthMismatch);
        }
        if read_u32(bytes, 16) != 0
            || read_u16(bytes, 66)? != 0
            || read_u16(bytes, 82)? != 0
            || read_u16(bytes, 90)? != 0
            || read_u16(bytes, 238)? != 0
            || bytes[184..196].iter().any(|byte| *byte != 0)
            || bytes[246..256].iter().any(|byte| *byte != 0)
        {
            return Err(Error::NonZeroReserved);
        }

        let vertex_stride = read_u32(bytes, 32);
        if vertex_stride == 0 {
            return Err(Error::InvalidVertexLayout);
        }
        let vertex_data = payload_slice(bytes, read_u32(bytes, 24), read_u32(bytes, 28))?;
        if vertex_data.len() % to_usize(vertex_stride)? != 0 {
            return Err(Error::InvalidVertexLayout);
        }

        let index_format = match read_u32(bytes, 48) {
            1 => IndexFormat::Uint16,
            2 => IndexFormat::Uint32,
            _ => return Err(Error::InvalidEnum),
        };
        let index_data = payload_slice(bytes, read_u32(bytes, 40), read_u32(bytes, 44))?;
        if index_data.len() % index_format.byte_width() != 0 {
            return Err(Error::InvalidIndexData);
        }

        let wgsl = payload_str(bytes, read_u32(bytes, 68), read_u32(bytes, 72))?;
        let vertex_entry =
            payload_str(bytes, read_u32(bytes, 76), u32::from(read_u16(bytes, 80)?))?;
        let fragment_entry =
            payload_str(bytes, read_u32(bytes, 84), u32::from(read_u16(bytes, 88)?))?;
        let camera_dynamic_slot =
            payload_str(bytes, read_u32(bytes, 60), u32::from(read_u16(bytes, 64)?))?;
        let output_dynamic_slot =
            payload_str(bytes, read_u32(bytes, 232), u32::from(read_u16(bytes, 236)?))?;
        let pass_label =
            payload_str(bytes, read_u32(bytes, 240), u32::from(read_u16(bytes, 244)?))?;
        if wgsl.is_empty()
            || vertex_entry.is_empty()
            || fragment_entry.is_empty()
            || camera_dynamic_slot.is_empty()
            || output_dynamic_slot.is_empty()
        {
            return Err(Error::EmptyRequiredString);
        }

        let attribute_count = to_usize(read_u32(bytes, 144))?;
        if attribute_count > 3 {
            return Err(Error::TooManyAttributes);
        }
        let mut attributes = [VertexAttribute {
            shader_location: 0,
            format: VertexFormat::Float32x3,
            offset: 0,
        }; 3];
        let mut index = 0usize;
        while index < attribute_count {
            let base = 148 + index * 12;
            let format = match read_u32(bytes, base + 4) {
                1 => VertexFormat::Float32x3,
                _ => return Err(Error::InvalidEnum),
            };
            let attribute = VertexAttribute {
                shader_location: read_u32(bytes, base),
                format,
                offset: read_u32(bytes, base + 8),
            };
            let attribute_end = attribute
                .offset
                .checked_add(vertex_format_width(format))
                .ok_or(Error::InvalidVertexLayout)?;
            if attribute_end > vertex_stride
                || attributes[..index]
                    .iter()
                    .any(|other| other.shader_location == attribute.shader_location)
            {
                return Err(Error::InvalidVertexLayout);
            }
            attributes[index] = attribute;
            index += 1;
        }
        // Unused inline attribute slots are required to remain zero.
        let unused_start = 148 + attribute_count * 12;
        if bytes[unused_start..184].iter().any(|byte| *byte != 0) {
            return Err(Error::NonZeroReserved);
        }

        let state_bits = read_u32(bytes, 116);
        if state_bits & !KNOWN_STATE_FLAGS != 0 {
            return Err(Error::UnknownFlags);
        }
        let flags = StateFlags(state_bits);
        let color_write_mask = read_u32(bytes, 120);
        if color_write_mask & !0xF != 0 {
            return Err(Error::UnknownFlags);
        }

        let visibility_bits = read_u32(bytes, 208);
        if visibility_bits == 0 || visibility_bits & !0x3 != 0 {
            return Err(Error::UnknownFlags);
        }
        let visibility = ShaderStages(visibility_bits);
        let binding_kind = match read_u32(bytes, 204) {
            1 => BindingKind::StorageBuffer,
            _ => return Err(Error::InvalidEnum),
        };

        let draw = DrawIndexed {
            index_count: read_u32(bytes, 212),
            instance_count: read_u32(bytes, 216),
            first_index: read_u32(bytes, 220),
            base_vertex: read_u32(bytes, 224) as i32,
            first_instance: read_u32(bytes, 228),
        };
        let available_indices = index_data.len() / index_format.byte_width();
        let draw_end = usize::try_from(draw.first_index)
            .ok()
            .and_then(|first| {
                usize::try_from(draw.index_count)
                    .ok()
                    .and_then(|count| first.checked_add(count))
            })
            .ok_or(Error::InvalidDraw)?;
        if draw.index_count == 0 || draw.instance_count == 0 || draw_end > available_indices {
            return Err(Error::InvalidDraw);
        }

        let clear_color = [
            f32::from_bits(read_u32(bytes, 124)),
            f32::from_bits(read_u32(bytes, 128)),
            f32::from_bits(read_u32(bytes, 132)),
            f32::from_bits(read_u32(bytes, 136)),
        ];
        let clear_depth = f32::from_bits(read_u32(bytes, 140));
        if clear_color.iter().any(|component| !component.is_finite()) || !clear_depth.is_finite() {
            return Err(Error::NonFiniteClearValue);
        }

        let color_format = match read_u32(bytes, 92) {
            1 => TextureFormat::Bgra8UnormSrgb,
            _ => return Err(Error::InvalidEnum),
        };
        let depth_format = match read_u32(bytes, 96) {
            1 => TextureFormat::Depth32Float,
            _ => return Err(Error::InvalidEnum),
        };
        let topology = match read_u32(bytes, 100) {
            1 => PrimitiveTopology::TriangleList,
            _ => return Err(Error::InvalidEnum),
        };
        let front_face = match read_u32(bytes, 104) {
            1 => FrontFace::Ccw,
            2 => FrontFace::Cw,
            _ => return Err(Error::InvalidEnum),
        };
        let cull_mode = match read_u32(bytes, 108) {
            0 => CullMode::None,
            1 => CullMode::Front,
            2 => CullMode::Back,
            _ => return Err(Error::InvalidEnum),
        };
        let depth_compare = match read_u32(bytes, 112) {
            1 => CompareFunction::Less,
            _ => return Err(Error::InvalidEnum),
        };

        let minimum_size = read_u32(bytes, 56);
        if minimum_size == 0 {
            return Err(Error::InvalidBinding);
        }

        let vertex_id = ResourceId(read_u32(bytes, 20));
        let index_id = ResourceId(read_u32(bytes, 36));
        let camera_id = ResourceId(read_u32(bytes, 52));
        if vertex_id.0 == 0
            || index_id.0 == 0
            || camera_id.0 == 0
            || vertex_id == index_id
            || vertex_id == camera_id
            || index_id == camera_id
        {
            return Err(Error::InvalidResourceId);
        }

        Ok(Self {
            vertex: VertexBuffer {
                id: vertex_id,
                data: vertex_data,
                stride: vertex_stride,
            },
            index: IndexBuffer {
                id: index_id,
                data: index_data,
                format: index_format,
            },
            attributes,
            attribute_count,
            shader: Shader {
                wgsl,
                vertex_entry,
                fragment_entry,
            },
            camera: CameraBinding {
                buffer_id: camera_id,
                minimum_size,
                dynamic_slot: camera_dynamic_slot,
                group: read_u32(bytes, 196),
                binding: read_u32(bytes, 200),
                kind: binding_kind,
                visibility,
                read_only: flags.contains(StateFlags::BINDING_READ_ONLY),
            },
            pipeline: PipelineState {
                color_format,
                depth_format,
                topology,
                front_face,
                cull_mode,
                depth_compare,
                flags,
                color_write_mask,
                clear_color,
                clear_depth,
            },
            draw,
            output_dynamic_slot,
            pass_label,
        })
    }

    pub fn attributes(&self) -> &[VertexAttribute] {
        &self.attributes[..self.attribute_count]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BadMagic,
    UnsupportedVersion(u16),
    MalformedHeader,
    LengthMismatch,
    OutOfBounds,
    NonZeroReserved,
    InvalidUtf8,
    EmptyRequiredString,
    InvalidEnum,
    UnknownFlags,
    TooManyAttributes,
    InvalidVertexLayout,
    InvalidIndexData,
    InvalidResourceId,
    InvalidBinding,
    InvalidDraw,
    NonFiniteClearValue,
}

fn payload_slice(bytes: &[u8], offset: u32, len: u32) -> Result<&[u8], Error> {
    let start = to_usize(offset)?;
    let len = to_usize(len)?;
    let end = start.checked_add(len).ok_or(Error::OutOfBounds)?;
    if start < HEADER_LEN {
        return Err(Error::OutOfBounds);
    }
    bytes.get(start..end).ok_or(Error::OutOfBounds)
}

fn payload_str(bytes: &[u8], offset: u32, len: u32) -> Result<&str, Error> {
    str::from_utf8(payload_slice(bytes, offset, len)?).map_err(|_| Error::InvalidUtf8)
}

const fn vertex_format_width(format: VertexFormat) -> u32 {
    match format {
        VertexFormat::Float32x3 => 12,
    }
}

fn to_usize(value: u32) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| Error::OutOfBounds)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes.get(offset..offset + 2).ok_or(Error::OutOfBounds)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let raw = &bytes[offset..offset + 4];
    u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn fixture() -> Vec<u8> {
        let vertex = [0xA5; 36];
        let indices = [0u8, 0, 1, 0, 2, 0];
        let wgsl = b"@vertex fn vs_main() {} @fragment fn fs_main() {}";
        let strings: [&[u8]; 5] = [
            wgsl,
            b"vs_main",
            b"fs_main",
            b"camera.view_proj",
            b"output.surface",
        ];
        let pass = b"SimpleCubePass";
        let total = HEADER_LEN
            + vertex.len()
            + indices.len()
            + strings.iter().map(|item| item.len()).sum::<usize>()
            + pass.len();
        let mut bytes = vec![0u8; total];
        bytes[..8].copy_from_slice(&MAGIC);
        put_u16(&mut bytes, 8, VERSION);
        put_u16(&mut bytes, 10, HEADER_LEN as u16);
        put_u32(&mut bytes, 12, total as u32);

        let mut payload = HEADER_LEN;
        let vertex_offset = append(&mut bytes, &mut payload, &vertex);
        let index_offset = append(&mut bytes, &mut payload, &indices);
        let wgsl_offset = append(&mut bytes, &mut payload, strings[0]);
        let vs_offset = append(&mut bytes, &mut payload, strings[1]);
        let fs_offset = append(&mut bytes, &mut payload, strings[2]);
        let camera_offset = append(&mut bytes, &mut payload, strings[3]);
        let output_offset = append(&mut bytes, &mut payload, strings[4]);
        let pass_offset = append(&mut bytes, &mut payload, pass);

        put_u32(&mut bytes, 20, 1);
        put_u32(&mut bytes, 24, vertex_offset);
        put_u32(&mut bytes, 28, vertex.len() as u32);
        put_u32(&mut bytes, 32, 36);
        put_u32(&mut bytes, 36, 2);
        put_u32(&mut bytes, 40, index_offset);
        put_u32(&mut bytes, 44, indices.len() as u32);
        put_u32(&mut bytes, 48, 1);
        put_u32(&mut bytes, 52, 3);
        put_u32(&mut bytes, 56, 64);
        put_u32(&mut bytes, 60, camera_offset);
        put_u16(&mut bytes, 64, strings[3].len() as u16);
        put_u32(&mut bytes, 68, wgsl_offset);
        put_u32(&mut bytes, 72, wgsl.len() as u32);
        put_u32(&mut bytes, 76, vs_offset);
        put_u16(&mut bytes, 80, strings[1].len() as u16);
        put_u32(&mut bytes, 84, fs_offset);
        put_u16(&mut bytes, 88, strings[2].len() as u16);
        put_u32(&mut bytes, 92, 1);
        put_u32(&mut bytes, 96, 1);
        put_u32(&mut bytes, 100, 1);
        put_u32(&mut bytes, 104, 1);
        put_u32(&mut bytes, 108, 2);
        put_u32(&mut bytes, 112, 1);
        put_u32(&mut bytes, 116, 0b11_1111);
        put_u32(&mut bytes, 120, 0xF);
        for (offset, value) in [
            (124, 0.1f32),
            (128, 0.2),
            (132, 0.3),
            (136, 1.0),
            (140, 1.0),
        ] {
            put_u32(&mut bytes, offset, value.to_bits());
        }
        put_u32(&mut bytes, 144, 3);
        for (index, (location, offset)) in [(0, 0), (1, 12), (2, 24)].into_iter().enumerate() {
            let base = 148 + index * 12;
            put_u32(&mut bytes, base, location);
            put_u32(&mut bytes, base + 4, 1);
            put_u32(&mut bytes, base + 8, offset);
        }
        put_u32(&mut bytes, 196, 0);
        put_u32(&mut bytes, 200, 0);
        put_u32(&mut bytes, 204, 1);
        put_u32(&mut bytes, 208, 1);
        put_u32(&mut bytes, 212, 3);
        put_u32(&mut bytes, 216, 1);
        put_u32(&mut bytes, 232, output_offset);
        put_u16(&mut bytes, 236, strings[4].len() as u16);
        put_u32(&mut bytes, 240, pass_offset);
        put_u16(&mut bytes, 244, pass.len() as u16);
        bytes
    }

    #[test]
    fn parses_complete_ir() {
        let bytes = fixture();
        let program = Program::parse(&bytes).unwrap();
        assert_eq!(program.vertex.id, ResourceId(1));
        assert_eq!(program.vertex.data.len(), 36);
        assert_eq!(program.index.format, IndexFormat::Uint16);
        assert_eq!(program.shader.vertex_entry, "vs_main");
        assert_eq!(program.camera.dynamic_slot, "camera.view_proj");
        assert_eq!(program.output_dynamic_slot, "output.surface");
        assert_eq!(program.attributes().len(), 3);
        assert_eq!(program.draw.index_count, 3);
        assert_eq!(program.pipeline.cull_mode, CullMode::Back);
    }

    #[test]
    fn every_truncation_is_rejected() {
        let bytes = fixture();
        for len in 0..bytes.len() {
            assert!(Program::parse(&bytes[..len]).is_err(), "accepted len {len}");
        }
    }

    #[test]
    fn rejects_payload_ranges_into_header() {
        let mut bytes = fixture();
        put_u32(&mut bytes, 24, 24);
        assert_eq!(Program::parse(&bytes).unwrap_err(), Error::OutOfBounds);
    }

    #[test]
    fn rejects_draw_past_index_data() {
        let mut bytes = fixture();
        put_u32(&mut bytes, 212, 4);
        assert_eq!(Program::parse(&bytes).unwrap_err(), Error::InvalidDraw);
    }

    #[test]
    fn rejects_unknown_state_and_visibility_bits() {
        let mut bytes = fixture();
        put_u32(&mut bytes, 116, 1 << 31);
        assert_eq!(Program::parse(&bytes).unwrap_err(), Error::UnknownFlags);

        let mut bytes = fixture();
        put_u32(&mut bytes, 208, 1 << 31);
        assert_eq!(Program::parse(&bytes).unwrap_err(), Error::UnknownFlags);
    }

    #[test]
    fn rejects_nonzero_reserved_regions() {
        for offset in [16, 66, 82, 90, 184, 238, 246, 255] {
            let mut bytes = fixture();
            bytes[offset] = 1;
            assert_eq!(
                Program::parse(&bytes).unwrap_err(),
                Error::NonZeroReserved,
                "offset {offset}"
            );
        }
    }

    #[test]
    fn rejects_bad_utf8_and_non_finite_clear() {
        let mut bytes = fixture();
        let vs = read_u32(&bytes, 76) as usize;
        bytes[vs] = 0xFF;
        assert_eq!(Program::parse(&bytes).unwrap_err(), Error::InvalidUtf8);

        let mut bytes = fixture();
        put_u32(&mut bytes, 124, f32::NAN.to_bits());
        assert_eq!(Program::parse(&bytes).unwrap_err(), Error::NonFiniteClearValue);
    }

    fn append(bytes: &mut [u8], cursor: &mut usize, data: &[u8]) -> u32 {
        let offset = *cursor;
        bytes[offset..offset + data.len()].copy_from_slice(data);
        *cursor += data.len();
        offset as u32
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
