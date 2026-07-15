use alloc::vec::Vec;

use crate::model::{
    ApplyError, ApplyOutcome, CameraOrbit, Edge, Face, Instance, Mesh, Rgba8, SceneStats,
    Transform, Vec3, ViewCamera,
};

pub const PROTOCOL_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 12;
pub const MAX_PAYLOAD_LEN: usize = 128 * 1024 * 1024;
pub const MAX_VERTICES_PER_COMMAND: usize = 16_777_216;
pub const MAX_EDGES_PER_COMMAND: usize = 16_777_216;
pub const MAX_FACES_PER_COMMAND: usize = 4_194_304;
const MAGIC: [u8; 2] = *b"D3";
const RESPONSE_BIT: u8 = 0x80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Opcode {
    PutMesh = 0x01,
    DeleteMesh = 0x02,
    CopyMesh = 0x03,
    SetVertices = 0x04,
    SetEdges = 0x05,
    SetFaces = 0x06,
    SetColor = 0x07,
    PutInstance = 0x10,
    DeleteInstance = 0x11,
    CopyInstance = 0x12,
    SetInstanceMesh = 0x13,
    SetTransform = 0x14,
    SetLocation = 0x15,
    SetRotation = 0x16,
    SetScale = 0x17,
    Clear = 0x18,
    StartScene = 0x19,
    StopScene = 0x1a,
    GetStats = 0x20,
    Ping = 0x21,
    SetViewCamera = 0x22,
    RequestRender = 0x23,
}

impl Opcode {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::PutMesh,
            0x02 => Self::DeleteMesh,
            0x03 => Self::CopyMesh,
            0x04 => Self::SetVertices,
            0x05 => Self::SetEdges,
            0x06 => Self::SetFaces,
            0x07 => Self::SetColor,
            0x10 => Self::PutInstance,
            0x11 => Self::DeleteInstance,
            0x12 => Self::CopyInstance,
            0x13 => Self::SetInstanceMesh,
            0x14 => Self::SetTransform,
            0x15 => Self::SetLocation,
            0x16 => Self::SetRotation,
            0x17 => Self::SetScale,
            0x18 => Self::Clear,
            0x19 => Self::StartScene,
            0x1a => Self::StopScene,
            0x20 => Self::GetStats,
            0x21 => Self::Ping,
            0x22 => Self::SetViewCamera,
            0x23 => Self::RequestRender,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    PutMesh {
        mesh_id: u64,
        mesh: Mesh,
    },
    DeleteMesh {
        mesh_id: u64,
        cascade: bool,
    },
    CopyMesh {
        source_id: u64,
        target_id: u64,
    },
    SetVertices {
        mesh_id: u64,
        vertices: Vec<Vec3>,
    },
    SetEdges {
        mesh_id: u64,
        edges: Vec<Edge>,
    },
    SetFaces {
        mesh_id: u64,
        faces: Vec<Face>,
    },
    SetColor {
        mesh_id: u64,
        color: Rgba8,
    },
    PutInstance {
        instance_id: u64,
        instance: Instance,
    },
    DeleteInstance {
        instance_id: u64,
    },
    CopyInstance {
        source_id: u64,
        target_id: u64,
    },
    SetInstanceMesh {
        instance_id: u64,
        mesh_id: u64,
    },
    SetTransform {
        instance_id: u64,
        transform: Transform,
    },
    SetLocation {
        instance_id: u64,
        location: Vec3,
    },
    SetRotation {
        instance_id: u64,
        rotation: Vec3,
    },
    SetScale {
        instance_id: u64,
        scale: Vec3,
    },
    Clear,
    StartScene {
        clear: Option<Rgba8>,
    },
    StopScene {
        permanent: bool,
    },
    GetStats,
    Ping {
        nonce: u64,
    },
    SetViewCamera {
        camera: ViewCamera,
        orbit: Option<CameraOrbit>,
    },
    RequestRender,
}

impl Command {
    pub fn opcode(&self) -> Opcode {
        match self {
            Self::PutMesh { .. } => Opcode::PutMesh,
            Self::DeleteMesh { .. } => Opcode::DeleteMesh,
            Self::CopyMesh { .. } => Opcode::CopyMesh,
            Self::SetVertices { .. } => Opcode::SetVertices,
            Self::SetEdges { .. } => Opcode::SetEdges,
            Self::SetFaces { .. } => Opcode::SetFaces,
            Self::SetColor { .. } => Opcode::SetColor,
            Self::PutInstance { .. } => Opcode::PutInstance,
            Self::DeleteInstance { .. } => Opcode::DeleteInstance,
            Self::CopyInstance { .. } => Opcode::CopyInstance,
            Self::SetInstanceMesh { .. } => Opcode::SetInstanceMesh,
            Self::SetTransform { .. } => Opcode::SetTransform,
            Self::SetLocation { .. } => Opcode::SetLocation,
            Self::SetRotation { .. } => Opcode::SetRotation,
            Self::SetScale { .. } => Opcode::SetScale,
            Self::Clear => Opcode::Clear,
            Self::StartScene { .. } => Opcode::StartScene,
            Self::StopScene { .. } => Opcode::StopScene,
            Self::GetStats => Opcode::GetStats,
            Self::Ping { .. } => Opcode::Ping,
            Self::SetViewCamera { .. } => Opcode::SetViewCamera,
            Self::RequestRender => Opcode::RequestRender,
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::PutMesh { .. } => "put_mesh",
            Self::DeleteMesh { .. } => "delete_mesh",
            Self::CopyMesh { .. } => "copy_mesh",
            Self::SetVertices { .. } => "set_vertices",
            Self::SetEdges { .. } => "set_edges",
            Self::SetFaces { .. } => "set_faces",
            Self::SetColor { .. } => "set_color",
            Self::PutInstance { .. } => "put_instance",
            Self::DeleteInstance { .. } => "delete_instance",
            Self::CopyInstance { .. } => "copy_instance",
            Self::SetInstanceMesh { .. } => "set_instance_mesh",
            Self::SetTransform { .. } => "set_transform",
            Self::SetLocation { .. } => "set_location",
            Self::SetRotation { .. } => "set_rotation",
            Self::SetScale { .. } => "set_scale",
            Self::Clear => "clear",
            Self::StartScene { .. } => "start_scene",
            Self::StopScene { .. } => "stop_scene",
            Self::GetStats => "get_stats",
            Self::Ping { .. } => "ping",
            Self::SetViewCamera { .. } => "set_view_camera",
            Self::RequestRender => "request_render",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub request_id: u32,
    pub opcode: Opcode,
    pub is_response: bool,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    BufferLimit,
    BadMagic,
    UnsupportedVersion,
    UnknownOpcode,
    PayloadTooLarge,
    Truncated,
    TrailingBytes,
    InvalidBoolean,
    CountOverflow,
    UnexpectedResponse,
    UnexpectedRequest,
    InvalidResponse,
    UnknownErrorCode,
    CollectionLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    CountOverflow,
    FaceTooLarge,
    PayloadTooLarge,
    CollectionLimit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedRequest {
    pub request_id: u32,
    pub opcode: Opcode,
    pub command: Result<Command, DecodeError>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedResponse {
    pub request_id: u32,
    pub opcode: Opcode,
    pub response: Result<Response, DecodeError>,
}

pub struct FrameDecoder {
    buffer: Vec<u8>,
    start: usize,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            start: 0,
        }
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len().saturating_sub(self.start)
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<(), DecodeError> {
        if self.buffered_len().saturating_add(bytes.len()) > MAX_PAYLOAD_LEN + HEADER_LEN {
            return Err(DecodeError::BufferLimit);
        }
        if self.start != 0 && self.start >= self.buffer.len() / 2 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>, DecodeError> {
        let Some((request_id, opcode, is_response, payload_len)) = self.next_header()? else {
            return Ok(None);
        };
        let frame_len = HEADER_LEN + payload_len;
        let payload = self.buffer[self.start + HEADER_LEN..self.start + frame_len].to_vec();
        self.consume(frame_len);
        Ok(Some(Frame {
            request_id,
            opcode,
            is_response,
            payload,
        }))
    }

    /// Decodes directly from the receive buffer, avoiding a full payload copy for large meshes.
    /// Header/framing failures are returned by the outer result. Payload/schema failures retain
    /// the request ID and opcode so a server can send a correlated error reply.
    pub fn next_request(&mut self) -> Result<Option<DecodedRequest>, DecodeError> {
        let Some((request_id, opcode, is_response, payload_len)) = self.next_header()? else {
            return Ok(None);
        };
        let frame_len = HEADER_LEN + payload_len;
        if is_response {
            self.consume(frame_len);
            return Err(DecodeError::UnexpectedResponse);
        }
        let payload = &self.buffer[self.start + HEADER_LEN..self.start + frame_len];
        let command = decode_command_payload(opcode, payload);
        self.consume(frame_len);
        Ok(Some(DecodedRequest {
            request_id,
            opcode,
            command,
        }))
    }

    /// Decodes a reply directly from the receive buffer. Image bytes are copied only into the
    /// returned `RenderImage`, rather than through an intermediate full-frame payload.
    pub fn next_response(&mut self) -> Result<Option<DecodedResponse>, DecodeError> {
        let Some((request_id, opcode, is_response, payload_len)) = self.next_header()? else {
            return Ok(None);
        };
        let frame_len = HEADER_LEN + payload_len;
        if !is_response {
            self.consume(frame_len);
            return Err(DecodeError::UnexpectedRequest);
        }
        let payload = &self.buffer[self.start + HEADER_LEN..self.start + frame_len];
        let response = decode_response_payload(payload);
        self.consume(frame_len);
        Ok(Some(DecodedResponse {
            request_id,
            opcode,
            response,
        }))
    }

    fn next_header(&self) -> Result<Option<(u32, Opcode, bool, usize)>, DecodeError> {
        let available = &self.buffer[self.start..];
        if available.len() < HEADER_LEN {
            return Ok(None);
        }
        if available[0..2] != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        if available[2] != PROTOCOL_VERSION {
            return Err(DecodeError::UnsupportedVersion);
        }
        let is_response = available[3] & RESPONSE_BIT != 0;
        let opcode =
            Opcode::from_u8(available[3] & !RESPONSE_BIT).ok_or(DecodeError::UnknownOpcode)?;
        let request_id = u32::from_le_bytes(available[4..8].try_into().unwrap());
        let payload_len = u32::from_le_bytes(available[8..12].try_into().unwrap()) as usize;
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(DecodeError::PayloadTooLarge);
        }
        let frame_len = HEADER_LEN + payload_len;
        if available.len() < frame_len {
            return Ok(None);
        }
        Ok(Some((request_id, opcode, is_response, payload_len)))
    }

    fn consume(&mut self, frame_len: usize) {
        self.start += frame_len;
        if self.start == self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
        }
    }
}

pub fn decode_command(frame: Frame) -> Result<Command, DecodeError> {
    if frame.is_response {
        return Err(DecodeError::UnexpectedResponse);
    }
    decode_command_payload(frame.opcode, &frame.payload)
}

fn decode_command_payload(opcode: Opcode, payload: &[u8]) -> Result<Command, DecodeError> {
    let mut reader = Reader::new(payload);
    let command = match opcode {
        Opcode::PutMesh => {
            let mesh_id = reader.u64()?;
            let color = reader.rgba()?;
            let vertices = reader.vertices()?;
            let edges = reader.edges()?;
            let faces = reader.faces()?;
            Command::PutMesh {
                mesh_id,
                mesh: Mesh::new(vertices, edges, faces, color),
            }
        }
        Opcode::DeleteMesh => Command::DeleteMesh {
            mesh_id: reader.u64()?,
            cascade: reader.boolean()?,
        },
        Opcode::CopyMesh => Command::CopyMesh {
            source_id: reader.u64()?,
            target_id: reader.u64()?,
        },
        Opcode::SetVertices => Command::SetVertices {
            mesh_id: reader.u64()?,
            vertices: reader.vertices()?,
        },
        Opcode::SetEdges => Command::SetEdges {
            mesh_id: reader.u64()?,
            edges: reader.edges()?,
        },
        Opcode::SetFaces => Command::SetFaces {
            mesh_id: reader.u64()?,
            faces: reader.faces()?,
        },
        Opcode::SetColor => Command::SetColor {
            mesh_id: reader.u64()?,
            color: reader.rgba()?,
        },
        Opcode::PutInstance => Command::PutInstance {
            instance_id: reader.u64()?,
            instance: Instance::new(reader.u64()?, reader.transform()?),
        },
        Opcode::DeleteInstance => Command::DeleteInstance {
            instance_id: reader.u64()?,
        },
        Opcode::CopyInstance => Command::CopyInstance {
            source_id: reader.u64()?,
            target_id: reader.u64()?,
        },
        Opcode::SetInstanceMesh => Command::SetInstanceMesh {
            instance_id: reader.u64()?,
            mesh_id: reader.u64()?,
        },
        Opcode::SetTransform => Command::SetTransform {
            instance_id: reader.u64()?,
            transform: reader.transform()?,
        },
        Opcode::SetLocation => Command::SetLocation {
            instance_id: reader.u64()?,
            location: reader.vec3()?,
        },
        Opcode::SetRotation => Command::SetRotation {
            instance_id: reader.u64()?,
            rotation: reader.vec3()?,
        },
        Opcode::SetScale => Command::SetScale {
            instance_id: reader.u64()?,
            scale: reader.vec3()?,
        },
        Opcode::Clear => Command::Clear,
        Opcode::StartScene => Command::StartScene {
            clear: if reader.is_empty() {
                None
            } else {
                Some(reader.rgba()?)
            },
        },
        Opcode::StopScene => Command::StopScene {
            permanent: if reader.is_empty() {
                false
            } else {
                reader.boolean()?
            },
        },
        Opcode::GetStats => Command::GetStats,
        Opcode::Ping => Command::Ping {
            nonce: reader.u64()?,
        },
        Opcode::SetViewCamera => {
            let camera = reader.camera()?;
            let orbit = if reader.is_empty() {
                None
            } else {
                Some(reader.camera_orbit()?)
            };
            Command::SetViewCamera { camera, orbit }
        }
        Opcode::RequestRender => Command::RequestRender,
    };
    reader.finish()?;
    Ok(command)
}

pub fn encode_command(request_id: u32, command: &Command) -> Result<Vec<u8>, EncodeError> {
    validate_encodable(command)?;
    let mut payload = Vec::new();
    match command {
        Command::PutMesh { mesh_id, mesh } => {
            put_u64(&mut payload, *mesh_id);
            put_rgba(&mut payload, mesh.color);
            put_vertices(&mut payload, &mesh.vertices);
            put_edges(&mut payload, &mesh.edges);
            put_faces(&mut payload, &mesh.faces);
        }
        Command::DeleteMesh { mesh_id, cascade } => {
            put_u64(&mut payload, *mesh_id);
            payload.push(u8::from(*cascade));
        }
        Command::CopyMesh {
            source_id,
            target_id,
        }
        | Command::CopyInstance {
            source_id,
            target_id,
        } => {
            put_u64(&mut payload, *source_id);
            put_u64(&mut payload, *target_id);
        }
        Command::SetVertices { mesh_id, vertices } => {
            put_u64(&mut payload, *mesh_id);
            put_vertices(&mut payload, vertices);
        }
        Command::SetEdges { mesh_id, edges } => {
            put_u64(&mut payload, *mesh_id);
            put_edges(&mut payload, edges);
        }
        Command::SetFaces { mesh_id, faces } => {
            put_u64(&mut payload, *mesh_id);
            put_faces(&mut payload, faces);
        }
        Command::SetColor { mesh_id, color } => {
            put_u64(&mut payload, *mesh_id);
            put_rgba(&mut payload, *color);
        }
        Command::PutInstance {
            instance_id,
            instance,
        } => {
            put_u64(&mut payload, *instance_id);
            put_u64(&mut payload, instance.mesh_id);
            put_transform(&mut payload, instance.transform);
        }
        Command::DeleteInstance { instance_id } => put_u64(&mut payload, *instance_id),
        Command::SetInstanceMesh {
            instance_id,
            mesh_id,
        } => {
            put_u64(&mut payload, *instance_id);
            put_u64(&mut payload, *mesh_id);
        }
        Command::SetTransform {
            instance_id,
            transform,
        } => {
            put_u64(&mut payload, *instance_id);
            put_transform(&mut payload, *transform);
        }
        Command::SetLocation {
            instance_id,
            location,
        } => {
            put_u64(&mut payload, *instance_id);
            put_vec3(&mut payload, *location);
        }
        Command::SetRotation {
            instance_id,
            rotation,
        } => {
            put_u64(&mut payload, *instance_id);
            put_vec3(&mut payload, *rotation);
        }
        Command::SetScale { instance_id, scale } => {
            put_u64(&mut payload, *instance_id);
            put_vec3(&mut payload, *scale);
        }
        Command::StartScene { clear } => {
            if let Some(color) = clear {
                put_rgba(&mut payload, *color);
            }
        }
        Command::StopScene { permanent } => {
            if *permanent {
                payload.push(1);
            }
        }
        Command::Clear | Command::GetStats => {}
        Command::Ping { nonce } => put_u64(&mut payload, *nonce),
        Command::SetViewCamera { camera, orbit } => {
            put_camera(&mut payload, *camera);
            if let Some(orbit) = orbit {
                put_camera_orbit(&mut payload, *orbit);
            }
        }
        Command::RequestRender => {}
    }
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(EncodeError::PayloadTooLarge);
    }
    Ok(encode_frame(command.opcode() as u8, request_id, &payload))
}

fn validate_encodable(command: &Command) -> Result<(), EncodeError> {
    fn count(len: usize, max: usize) -> Result<(), EncodeError> {
        if len > u32::MAX as usize {
            Err(EncodeError::CountOverflow)
        } else if len > max {
            Err(EncodeError::CollectionLimit)
        } else {
            Ok(())
        }
    }

    fn faces(values: &[Face]) -> Result<(), EncodeError> {
        count(values.len(), MAX_FACES_PER_COMMAND)?;
        if values
            .iter()
            .any(|face| face.vertices.len() > u16::MAX as usize)
        {
            return Err(EncodeError::FaceTooLarge);
        }
        Ok(())
    }

    match command {
        Command::PutMesh { mesh, .. } => {
            count(mesh.vertices.len(), MAX_VERTICES_PER_COMMAND)?;
            count(mesh.edges.len(), MAX_EDGES_PER_COMMAND)?;
            faces(&mesh.faces)?;
        }
        Command::SetVertices { vertices, .. } => count(vertices.len(), MAX_VERTICES_PER_COMMAND)?,
        Command::SetEdges { edges, .. } => count(edges.len(), MAX_EDGES_PER_COMMAND)?,
        Command::SetFaces { faces: values, .. } => faces(values)?,
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseError {
    Decode(DecodeError),
    Apply(ApplyError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ImageFormat {
    Jpeg = 1,
    Png = 2,
}

impl ImageFormat {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Jpeg),
            2 => Some(Self::Png),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderImage {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    Applied(ApplyOutcome),
    Stats(SceneStats),
    Pong(u64),
    RenderImage(RenderImage),
    Error(ResponseError),
}

pub fn encode_response(opcode: Opcode, request_id: u32, response: &Response) -> Vec<u8> {
    let mut payload = Vec::new();
    match response {
        Response::Applied(outcome) => {
            payload.push(0);
            payload.push(0);
            put_u32(&mut payload, outcome.affected);
            put_stats(&mut payload, outcome.stats);
        }
        Response::Stats(stats) => {
            payload.push(0);
            payload.push(1);
            put_stats(&mut payload, *stats);
        }
        Response::Pong(nonce) => {
            payload.push(0);
            payload.push(2);
            put_u64(&mut payload, *nonce);
        }
        Response::RenderImage(image) => {
            payload.push(0);
            payload.push(3);
            payload.push(image.format as u8);
            put_u32(&mut payload, image.width);
            put_u32(&mut payload, image.height);
            payload.extend_from_slice(&image.bytes);
        }
        Response::Error(error) => {
            payload.push(error_code(*error));
        }
    }
    encode_frame((opcode as u8) | RESPONSE_BIT, request_id, &payload)
}

pub fn decode_response(frame: Frame) -> Result<Response, DecodeError> {
    if !frame.is_response {
        return Err(DecodeError::UnexpectedRequest);
    }
    decode_response_payload(&frame.payload)
}

fn decode_response_payload(payload: &[u8]) -> Result<Response, DecodeError> {
    let mut reader = Reader::new(payload);
    let status = reader.u8()?;
    let response = if status != 0 {
        Response::Error(decode_error_code(status).ok_or(DecodeError::UnknownErrorCode)?)
    } else {
        match reader.u8()? {
            0 => Response::Applied(ApplyOutcome {
                affected: reader.u32()?,
                stats: reader.stats()?,
            }),
            1 => Response::Stats(reader.stats()?),
            2 => Response::Pong(reader.u64()?),
            3 => Response::RenderImage(RenderImage {
                format: ImageFormat::from_u8(reader.u8()?).ok_or(DecodeError::InvalidResponse)?,
                width: reader.u32()?,
                height: reader.u32()?,
                bytes: reader.remaining(),
            }),
            _ => return Err(DecodeError::InvalidResponse),
        }
    };
    reader.finish()?;
    Ok(response)
}

fn encode_frame(opcode: u8, request_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&MAGIC);
    out.push(PROTOCOL_VERSION);
    out.push(opcode);
    put_u32(&mut out, request_id);
    put_u32(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    out
}

fn error_code(error: ResponseError) -> u8 {
    match error {
        ResponseError::Decode(error) => match error {
            DecodeError::BufferLimit => 1,
            DecodeError::BadMagic => 2,
            DecodeError::UnsupportedVersion => 3,
            DecodeError::UnknownOpcode => 4,
            DecodeError::PayloadTooLarge => 5,
            DecodeError::Truncated => 6,
            DecodeError::TrailingBytes => 7,
            DecodeError::InvalidBoolean => 8,
            DecodeError::CountOverflow => 9,
            DecodeError::UnexpectedResponse => 10,
            DecodeError::UnexpectedRequest => 11,
            DecodeError::InvalidResponse => 12,
            DecodeError::UnknownErrorCode => 13,
            DecodeError::CollectionLimit => 14,
        },
        ResponseError::Apply(error) => match error {
            ApplyError::MeshMissing => 32,
            ApplyError::InstanceMissing => 33,
            ApplyError::TargetExists => 34,
            ApplyError::MeshInUse => 35,
            ApplyError::MeshLimit => 36,
            ApplyError::InstanceLimit => 37,
            ApplyError::VertexLimit => 38,
            ApplyError::EdgeLimit => 39,
            ApplyError::FaceLimit => 40,
            ApplyError::FaceVertexLimit => 41,
            ApplyError::FaceTooSmall => 42,
            ApplyError::VertexIndexOutOfRange => 43,
            ApplyError::NonFiniteVector => 44,
            ApplyError::InvalidClipPlanes => 45,
            ApplyError::InvalidFieldOfView => 46,
            ApplyError::ZeroViewDirection => 47,
            ApplyError::ZeroUpAxis => 48,
            ApplyError::ParallelCameraAxes => 49,
            ApplyError::InvalidOrbitScale => 50,
        },
    }
}

fn decode_error_code(code: u8) -> Option<ResponseError> {
    Some(match code {
        1 => ResponseError::Decode(DecodeError::BufferLimit),
        2 => ResponseError::Decode(DecodeError::BadMagic),
        3 => ResponseError::Decode(DecodeError::UnsupportedVersion),
        4 => ResponseError::Decode(DecodeError::UnknownOpcode),
        5 => ResponseError::Decode(DecodeError::PayloadTooLarge),
        6 => ResponseError::Decode(DecodeError::Truncated),
        7 => ResponseError::Decode(DecodeError::TrailingBytes),
        8 => ResponseError::Decode(DecodeError::InvalidBoolean),
        9 => ResponseError::Decode(DecodeError::CountOverflow),
        10 => ResponseError::Decode(DecodeError::UnexpectedResponse),
        11 => ResponseError::Decode(DecodeError::UnexpectedRequest),
        12 => ResponseError::Decode(DecodeError::InvalidResponse),
        13 => ResponseError::Decode(DecodeError::UnknownErrorCode),
        14 => ResponseError::Decode(DecodeError::CollectionLimit),
        32 => ResponseError::Apply(ApplyError::MeshMissing),
        33 => ResponseError::Apply(ApplyError::InstanceMissing),
        34 => ResponseError::Apply(ApplyError::TargetExists),
        35 => ResponseError::Apply(ApplyError::MeshInUse),
        36 => ResponseError::Apply(ApplyError::MeshLimit),
        37 => ResponseError::Apply(ApplyError::InstanceLimit),
        38 => ResponseError::Apply(ApplyError::VertexLimit),
        39 => ResponseError::Apply(ApplyError::EdgeLimit),
        40 => ResponseError::Apply(ApplyError::FaceLimit),
        41 => ResponseError::Apply(ApplyError::FaceVertexLimit),
        42 => ResponseError::Apply(ApplyError::FaceTooSmall),
        43 => ResponseError::Apply(ApplyError::VertexIndexOutOfRange),
        44 => ResponseError::Apply(ApplyError::NonFiniteVector),
        45 => ResponseError::Apply(ApplyError::InvalidClipPlanes),
        46 => ResponseError::Apply(ApplyError::InvalidFieldOfView),
        47 => ResponseError::Apply(ApplyError::ZeroViewDirection),
        48 => ResponseError::Apply(ApplyError::ZeroUpAxis),
        49 => ResponseError::Apply(ApplyError::ParallelCameraAxes),
        50 => ResponseError::Apply(ApplyError::InvalidOrbitScale),
        _ => return None,
    })
}

fn put_stats(out: &mut Vec<u8>, stats: SceneStats) {
    put_u32(out, stats.mesh_count);
    put_u32(out, stats.instance_count);
    put_u64(out, stats.vertex_count);
    put_u64(out, stats.edge_count);
    put_u64(out, stats.face_count);
    put_u64(out, stats.mesh_bytes);
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_vec3(out: &mut Vec<u8>, value: Vec3) {
    out.extend_from_slice(&value.x.to_le_bytes());
    out.extend_from_slice(&value.y.to_le_bytes());
    out.extend_from_slice(&value.z.to_le_bytes());
}

fn put_transform(out: &mut Vec<u8>, value: Transform) {
    put_vec3(out, value.location);
    put_vec3(out, value.rotation);
    put_vec3(out, value.scale);
}

fn put_camera(out: &mut Vec<u8>, value: ViewCamera) {
    put_vec3(out, value.position);
    put_vec3(out, value.view_direction);
    put_vec3(out, value.up_axis);
    out.extend_from_slice(&value.near_plane.to_le_bytes());
    out.extend_from_slice(&value.far_plane.to_le_bytes());
    out.extend_from_slice(&value.vertical_fov.to_le_bytes());
}

fn put_camera_orbit(out: &mut Vec<u8>, value: CameraOrbit) {
    put_vec3(out, value.look_at);
    put_vec3(out, value.rotation);
    out.extend_from_slice(&value.scale[0].to_le_bytes());
    out.extend_from_slice(&value.scale[1].to_le_bytes());
    out.extend_from_slice(&value.angular_speed.to_le_bytes());
}

fn put_rgba(out: &mut Vec<u8>, value: Rgba8) {
    out.extend_from_slice(&[value.r, value.g, value.b, value.a]);
}

fn put_vertices(out: &mut Vec<u8>, values: &[Vec3]) {
    put_u32(out, values.len() as u32);
    for &value in values {
        put_vec3(out, value);
    }
}

fn put_edges(out: &mut Vec<u8>, values: &[Edge]) {
    put_u32(out, values.len() as u32);
    for value in values {
        put_u32(out, value.a);
        put_u32(out, value.b);
    }
}

fn put_faces(out: &mut Vec<u8>, values: &[Face]) {
    put_u32(out, values.len() as u32);
    for face in values {
        put_u16(out, face.vertices.len() as u16);
        for &index in &face.vertices {
            put_u32(out, index);
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn finish(self) -> Result<(), DecodeError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(DecodeError::CountOverflow)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(DecodeError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn boolean(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::InvalidBoolean),
        }
    }

    fn vec3(&mut self) -> Result<Vec3, DecodeError> {
        let x = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        let y = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        let z = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        Ok(Vec3::new(x, y, z))
    }

    fn transform(&mut self) -> Result<Transform, DecodeError> {
        Ok(Transform {
            location: self.vec3()?,
            rotation: self.vec3()?,
            scale: self.vec3()?,
        })
    }

    fn camera(&mut self) -> Result<ViewCamera, DecodeError> {
        Ok(ViewCamera {
            position: self.vec3()?,
            view_direction: self.vec3()?,
            up_axis: self.vec3()?,
            near_plane: f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
            far_plane: f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
            vertical_fov: f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
        })
    }

    fn camera_orbit(&mut self) -> Result<CameraOrbit, DecodeError> {
        Ok(CameraOrbit {
            look_at: self.vec3()?,
            rotation: self.vec3()?,
            scale: [
                f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
                f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
            ],
            angular_speed: f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
        })
    }

    fn remaining(&mut self) -> Vec<u8> {
        let bytes = self.bytes[self.offset..].to_vec();
        self.offset = self.bytes.len();
        bytes
    }

    fn stats(&mut self) -> Result<SceneStats, DecodeError> {
        Ok(SceneStats {
            mesh_count: self.u32()?,
            instance_count: self.u32()?,
            vertex_count: self.u64()?,
            edge_count: self.u64()?,
            face_count: self.u64()?,
            mesh_bytes: self.u64()?,
        })
    }

    fn rgba(&mut self) -> Result<Rgba8, DecodeError> {
        let bytes = self.take(4)?;
        Ok(Rgba8::new(bytes[0], bytes[1], bytes[2], bytes[3]))
    }

    fn count(&mut self, element_size: usize, max: usize) -> Result<usize, DecodeError> {
        let count = self.u32()? as usize;
        if count > max {
            return Err(DecodeError::CollectionLimit);
        }
        let bytes = count
            .checked_mul(element_size)
            .ok_or(DecodeError::CountOverflow)?;
        if bytes > self.bytes.len().saturating_sub(self.offset) {
            return Err(DecodeError::Truncated);
        }
        Ok(count)
    }

    fn vertices(&mut self) -> Result<Vec<Vec3>, DecodeError> {
        let count = self.count(12, MAX_VERTICES_PER_COMMAND)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.vec3()?);
        }
        Ok(values)
    }

    fn edges(&mut self) -> Result<Vec<Edge>, DecodeError> {
        let count = self.count(8, MAX_EDGES_PER_COMMAND)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(Edge::new(self.u32()?, self.u32()?));
        }
        Ok(values)
    }

    fn faces(&mut self) -> Result<Vec<Face>, DecodeError> {
        let count = self.count(2, MAX_FACES_PER_COMMAND)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let vertex_count = self.u16()? as usize;
            let byte_count = vertex_count
                .checked_mul(4)
                .ok_or(DecodeError::CountOverflow)?;
            if byte_count > self.bytes.len().saturating_sub(self.offset) {
                return Err(DecodeError::Truncated);
            }
            let mut vertices = Vec::with_capacity(vertex_count);
            for _ in 0..vertex_count {
                vertices.push(self.u32()?);
            }
            values.push(Face::new(vertices));
        }
        Ok(values)
    }
}
