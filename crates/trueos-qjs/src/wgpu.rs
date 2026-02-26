//! WebGPU shim scaffold for TRUEOS QJS integration.
//!
//! Goal: expose the object model and core entrypoints Pixi 8-style renderers
//! expect, while keeping behavior explicit (`Unsupported`) until wired.

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

pub const WGPU_SHIM_ENABLED: bool = false;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WgpuError {
    Unsupported,
    Invalid,
}

pub type WgpuResult<T> = core::result::Result<T, WgpuError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gpu {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuAdapter {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuDevice {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuQueue {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuBuffer {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuTexture {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuTextureView {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuSampler {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuShaderModule {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuBindGroupLayout {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuPipelineLayout {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuBindGroup {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuRenderPipeline {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuCommandEncoder {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuRenderPassEncoder {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuCommandBuffer {
    pub id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferDescriptor {
    pub size: u64,
    pub usage: u32,
    pub mapped_at_creation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextureDescriptor {
    pub width: u32,
    pub height: u32,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub format: u32,
    pub usage: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplerDescriptor {
    pub mag_filter: u32,
    pub min_filter: u32,
    pub mipmap_filter: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShaderModuleDescriptor<'a> {
    pub code_wgsl: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderPipelineDescriptor {
    pub primitive_topology: u32,
    pub color_format: u32,
    pub depth_stencil_format: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPassColorAttachment {
    pub view: GpuTextureView,
    pub load_op: u32,
    pub store_op: u32,
    pub clear_rgb: u32,
    pub clear_a: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPassDescriptor<'a> {
    pub color_attachments: &'a [RenderPassColorAttachment],
}

pub fn init_wgpu_shim() -> i32 {
    0
}

pub fn navigator_gpu() -> Gpu {
    Gpu { id: 1 }
}

pub fn gpu_request_adapter(_gpu: Gpu) -> WgpuResult<GpuAdapter> {
    Err(WgpuError::Unsupported)
}

pub fn adapter_request_device(_adapter: GpuAdapter) -> WgpuResult<(GpuDevice, GpuQueue)> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_buffer(_device: GpuDevice, _desc: BufferDescriptor) -> WgpuResult<GpuBuffer> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_texture(
    _device: GpuDevice,
    _desc: TextureDescriptor,
) -> WgpuResult<GpuTexture> {
    Err(WgpuError::Unsupported)
}

pub fn texture_create_view(_texture: GpuTexture) -> WgpuResult<GpuTextureView> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_sampler(
    _device: GpuDevice,
    _desc: SamplerDescriptor,
) -> WgpuResult<GpuSampler> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_shader_module(
    _device: GpuDevice,
    _desc: ShaderModuleDescriptor<'_>,
) -> WgpuResult<GpuShaderModule> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_bind_group_layout(
    _device: GpuDevice,
    _entries: &[u32],
) -> WgpuResult<GpuBindGroupLayout> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_pipeline_layout(
    _device: GpuDevice,
    _layouts: &[GpuBindGroupLayout],
) -> WgpuResult<GpuPipelineLayout> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_bind_group(
    _device: GpuDevice,
    _layout: GpuBindGroupLayout,
    _entries: &[u32],
) -> WgpuResult<GpuBindGroup> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_render_pipeline(
    _device: GpuDevice,
    _layout: GpuPipelineLayout,
    _desc: RenderPipelineDescriptor,
) -> WgpuResult<GpuRenderPipeline> {
    Err(WgpuError::Unsupported)
}

pub fn device_create_command_encoder(_device: GpuDevice) -> WgpuResult<GpuCommandEncoder> {
    Err(WgpuError::Unsupported)
}

pub fn encoder_begin_render_pass(
    _encoder: GpuCommandEncoder,
    _desc: RenderPassDescriptor<'_>,
) -> WgpuResult<GpuRenderPassEncoder> {
    Err(WgpuError::Unsupported)
}

pub fn pass_set_pipeline(
    _pass: GpuRenderPassEncoder,
    _pipeline: GpuRenderPipeline,
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn pass_set_bind_group(
    _pass: GpuRenderPassEncoder,
    _index: u32,
    _group: GpuBindGroup,
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn pass_set_vertex_buffer(
    _pass: GpuRenderPassEncoder,
    _slot: u32,
    _buf: GpuBuffer,
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn pass_set_index_buffer(
    _pass: GpuRenderPassEncoder,
    _buf: GpuBuffer,
    _fmt: u32,
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn pass_draw(
    _pass: GpuRenderPassEncoder,
    _vertex_count: u32,
    _instance_count: u32,
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn pass_draw_indexed(
    _pass: GpuRenderPassEncoder,
    _index_count: u32,
    _instance_count: u32,
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn pass_end(_pass: GpuRenderPassEncoder) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn encoder_finish(_encoder: GpuCommandEncoder) -> WgpuResult<GpuCommandBuffer> {
    Err(WgpuError::Unsupported)
}

pub fn queue_submit(_queue: GpuQueue, _cmds: &[GpuCommandBuffer]) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn queue_write_buffer(
    _queue: GpuQueue,
    _buf: GpuBuffer,
    _offset: u64,
    _data: &[u8],
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn queue_write_texture(
    _queue: GpuQueue,
    _tex: GpuTexture,
    _width: u32,
    _height: u32,
    _data: &[u8],
) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn queue_on_submitted_work_done(_queue: GpuQueue) -> WgpuResult<()> {
    Err(WgpuError::Unsupported)
}

pub fn list_expected_surface_for_pixi8() -> Vec<&'static str> {
    vec![
        "navigator.gpu.requestAdapter",
        "GPUAdapter.requestDevice",
        "GPUDevice.createBuffer",
        "GPUDevice.createTexture",
        "GPUTexture.createView",
        "GPUDevice.createSampler",
        "GPUDevice.createShaderModule",
        "GPUDevice.createBindGroupLayout",
        "GPUDevice.createPipelineLayout",
        "GPUDevice.createBindGroup",
        "GPUDevice.createRenderPipeline",
        "GPUDevice.createCommandEncoder",
        "GPUCommandEncoder.beginRenderPass",
        "GPURenderPassEncoder.setPipeline",
        "GPURenderPassEncoder.setBindGroup",
        "GPURenderPassEncoder.setVertexBuffer",
        "GPURenderPassEncoder.setIndexBuffer",
        "GPURenderPassEncoder.draw",
        "GPURenderPassEncoder.drawIndexed",
        "GPURenderPassEncoder.end",
        "GPUCommandEncoder.finish",
        "GPUQueue.submit",
        "GPUQueue.writeBuffer",
        "GPUQueue.writeTexture",
        "GPUQueue.onSubmittedWorkDone",
    ]
}
