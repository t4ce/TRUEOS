#![no_std]

extern crate alloc;

pub mod model;
pub mod protocol;
pub mod render;

pub use model::{
    ApplyError, ApplyOutcome, Edge, Face, Instance, InstanceId, Mesh, MeshId, Rgba8, Scene,
    SceneLimits, SceneStats, Transform, Vec3, ViewCamera,
};
pub use protocol::{
    Command, DecodeError, DecodedRequest, DecodedResponse, EncodeError, Frame, FrameDecoder,
    HEADER_LEN, ImageFormat, MAX_EDGES_PER_COMMAND, MAX_FACES_PER_COMMAND, MAX_PAYLOAD_LEN,
    MAX_VERTICES_PER_COMMAND, Opcode, PROTOCOL_VERSION, RenderImage, Response, ResponseError,
    decode_response, encode_command, encode_response,
};
pub use render::{ProjectedMesh, project_scene};
