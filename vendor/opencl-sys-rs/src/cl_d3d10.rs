// Copyright (c) 2021-2023 Via Technology Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! FFI bindings for [cl_d3d10.h](https://github.com/KhronosGroup/OpenCL-Headers/blob/main/CL/cl_d3d10.h).
//!
//! `cl_d3d10.h` contains `OpenCL` extensions that provide interoperability with `Direct3D` 10.
//! `OpenCL` extensions are documented in the [OpenCL-Registry](https://github.com/KhronosGroup/OpenCL-Registry)

#![allow(non_camel_case_types, non_upper_case_globals)]

use super::cl::{
    cl_command_queue, cl_command_type, cl_context, cl_context_info, cl_device_id, cl_event,
    cl_image_info, cl_mem, cl_mem_flags, cl_mem_info, cl_mem_object_type, cl_platform_id,
};
use super::cl_platform::{DXGI_FORMAT, cl_int, cl_uint};
use libc::c_void;

// cl_khr_d3d10_sharing

pub type cl_d3d10_device_source_khr = cl_uint;
pub type cl_d3d10_device_set_khr = cl_uint;

// Error Codes
pub const CL_INVALID_D3D10_DEVICE_KHR: cl_int = -1002;
pub const CL_INVALID_D3D10_RESOURCE_KHR: cl_int = -1003;
pub const CL_D3D10_RESOURCE_ALREADY_ACQUIRED_KHR: cl_int = -1004;
pub const CL_D3D10_RESOURCE_NOT_ACQUIRED_KHR: cl_int = -1005;

// cl_d3d10_device_source_khr
pub const CL_D3D10_DEVICE_KHR: cl_d3d10_device_source_khr = 0x4010;
pub const CL_D3D10_DXGI_ADAPTER_KHR: cl_d3d10_device_source_khr = 0x4011;

// cl_d3d10_device_set_khr
pub const CL_PREFERRED_DEVICES_FOR_D3D10_KHR: cl_d3d10_device_set_khr = 0x4012;
pub const CL_ALL_DEVICES_FOR_D3D10_KHR: cl_d3d10_device_set_khr = 0x4013;

// cl_context_info
pub const CL_CONTEXT_D3D10_DEVICE_KHR: cl_context_info = 0x4014;
pub const CL_CONTEXT_D3D10_PREFER_SHARED_RESOURCES_KHR: cl_context_info = 0x402C;

// cl_mem_info
pub const CL_MEM_D3D10_RESOURCE_KHR: cl_mem_info = 0x4015;

// cl_image_info
pub const CL_IMAGE_D3D10_SUBRESOURCE_KHR: cl_image_info = 0x4016;

// cl_command_type
pub const CL_COMMAND_ACQUIRE_D3D10_OBJECTS_KHR: cl_command_type = 0x4017;
pub const CL_COMMAND_RELEASE_D3D10_OBJECTS_KHR: cl_command_type = 0x4018;

pub type ID3D10Buffer_ptr = *mut c_void;
pub type ID3D10Texture2D_ptr = *mut c_void;
pub type ID3D10Texture3D_ptr = *mut c_void;

pub type clGetDeviceIDsFromD3D10KHR_t = Option<
    unsafe extern "C" fn(
        platform: cl_platform_id,
        d3d_device_source: cl_d3d10_device_source_khr,
        d3d_object: *mut c_void,
        d3d_device_set: cl_d3d10_device_set_khr,
        num_entries: cl_uint,
        devices: *mut cl_device_id,
        num_devices: *mut cl_uint,
    ) -> cl_int,
>;
pub type clGetDeviceIDsFromD3D10KHR_fn = clGetDeviceIDsFromD3D10KHR_t;

pub type clCreateFromD3D10BufferKHR_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        resource: ID3D10Buffer_ptr,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromD3D10BufferKHR_fn = clCreateFromD3D10BufferKHR_t;

pub type clCreateFromD3D10Texture2DKHR_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        resource: ID3D10Texture2D_ptr,
        subresource: cl_uint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromD3D10Texture2DKHR_fn = clCreateFromD3D10Texture2DKHR_t;

pub type clCreateFromD3D10Texture3DKHR_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        resource: ID3D10Texture3D_ptr,
        subresource: cl_uint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromD3D10Texture3DKHR_fn = clCreateFromD3D10Texture3DKHR_t;

pub type clEnqueueAcquireD3D10ObjectsKHR_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_objects: cl_uint,
        mem_objects: *const cl_mem,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueAcquireD3D10ObjectsKHR_fn = clEnqueueAcquireD3D10ObjectsKHR_t;

pub type clEnqueueReleaseD3D10ObjectsKHR_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_objects: cl_uint,
        mem_objects: *const cl_mem,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueReleaseD3D10ObjectsKHR_fn = clEnqueueReleaseD3D10ObjectsKHR_t;

// when cl_khr_d3d10_sharing is supported

pub type clGetSupportedD3D10TextureFormatsINTEL_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        image_type: cl_mem_object_type,
        num_entries: cl_uint,
        d3d10_formats: *mut DXGI_FORMAT,
        num_surface_formats: *mut cl_uint,
    ) -> cl_int,
>;

pub type clGetSupportedD3D10TextureFormatsINTEL_fn = clGetSupportedD3D10TextureFormatsINTEL_t;

#[cfg_attr(not(target_os = "macos"), link(name = "OpenCL"))]
#[cfg_attr(target_os = "macos", link(name = "OpenCL", kind = "framework"))]
#[cfg(feature = "cl_khr_d3d10_sharing")]
#[cfg(feature = "static")]
unsafe extern "system" {

    pub fn clGetSupportedD3D10TextureFormatsINTEL(
        context: cl_context,
        flags: cl_mem_flags,
        image_type: cl_mem_object_type,
        num_entries: cl_uint,
        d3d10_formats: *mut DXGI_FORMAT,
        num_surface_formats: *mut cl_uint,
    ) -> cl_int;
}
