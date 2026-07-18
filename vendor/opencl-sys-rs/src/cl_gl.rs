// Copyright (c) 2022-2023 Via Technology Ltd.
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

//! FFI bindings for [cl_gl.h](https://github.com/KhronosGroup/OpenCL-Headers/blob/main/CL/cl_gl.h)\
//! `OpenCL` extensions are documented in the [OpenCL-Registry](https://github.com/KhronosGroup/OpenCL-Registry)

#![allow(non_camel_case_types, non_upper_case_globals)]

use super::cl::{
    cl_command_queue, cl_context, cl_context_properties, cl_event, cl_mem, cl_mem_flags,
    cl_mem_object_type,
};
use libc::{c_void, size_t};

use super::cl_platform::{cl_int, cl_uint};

pub type cl_gl_object_type = cl_uint;
pub type cl_gl_texture_info = cl_uint;
pub type cl_gl_platform_info = cl_uint;
pub type cl_GLsync = *mut c_void;

pub type cl_GLuint = u32;
pub type cl_GLint = i32;
pub type cl_GLenum = u32;

pub type cl_gl_context_info = cl_uint;

// cl_gl_object_type = 0x2000 - 0x200F enum values are currently taken
pub const CL_GL_OBJECT_BUFFER: cl_gl_object_type = 0x2000;
pub const CL_GL_OBJECT_TEXTURE2D: cl_gl_object_type = 0x2001;
pub const CL_GL_OBJECT_TEXTURE3D: cl_gl_object_type = 0x2002;
pub const CL_GL_OBJECT_RENDERBUFFER: cl_gl_object_type = 0x2003;
// #ifdef CL_VERSION_1_2
pub const CL_GL_OBJECT_TEXTURE2D_ARRAY: cl_gl_object_type = 0x200E;
pub const CL_GL_OBJECT_TEXTURE1D: cl_gl_object_type = 0x200F;
pub const CL_GL_OBJECT_TEXTURE1D_ARRAY: cl_gl_object_type = 0x2010;
pub const CL_GL_OBJECT_TEXTURE_BUFFER: cl_gl_object_type = 0x2011;
// #endif

// cl_gl_texture_info
pub const CL_GL_TEXTURE_TARGET: cl_gl_texture_info = 0x2004;
pub const CL_GL_MIPMAP_LEVEL: cl_gl_texture_info = 0x2005;
// #ifdef CL_VERSION_1_2
pub const CL_GL_NUM_SAMPLES: cl_gl_texture_info = 0x2012;
// #endif

pub type clGetGLContextInfoKHR_t = Option<
    unsafe extern "C" fn(
        properties: *const cl_context_properties,
        param_name: cl_gl_context_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetGLContextInfoKHR_fn = clGetGLContextInfoKHR_t;

pub type clCreateFromGLBuffer_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        bufobj: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromGLBuffer_fn = clCreateFromGLBuffer_t;

pub type clCreateFromGLTexture_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        target: cl_GLenum,
        miplevel: cl_GLint,
        texture: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromGLTexture_fn = clCreateFromGLTexture_t;

pub type clCreateFromGLRenderbuffer_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        renderbuffer: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromGLRenderbuffer_fn = clCreateFromGLRenderbuffer_t;

pub type clGetGLObjectInfo_t = Option<
    unsafe extern "C" fn(
        memobj: cl_mem,
        gl_object_type: *mut cl_gl_object_type,
        gl_object_name: *mut cl_GLuint,
    ) -> cl_int,
>;
pub type clGetGLObjectInfo_fn = clGetGLObjectInfo_t;

pub type clGetGLTextureInfo_t = Option<
    unsafe extern "C" fn(
        memobj: cl_mem,
        param_name: cl_gl_texture_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetGLTextureInfo_fn = clGetGLTextureInfo_t;

pub type clEnqueueAcquireGLObjects_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_objects: cl_uint,
        mem_objects: *const cl_mem,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueAcquireGLObjects_fn = clEnqueueAcquireGLObjects_t;

pub type clEnqueueReleaseGLObjects_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_objects: cl_uint,
        mem_objects: *const cl_mem,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueReleaseGLObjects_fn = clEnqueueReleaseGLObjects_t;

pub type clCreateFromGLTexture2D_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        target: cl_GLenum,
        miplevel: cl_GLint,
        texture: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromGLTexture2D_fn = clCreateFromGLTexture2D_t;

pub type clCreateFromGLTexture3D_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        target: cl_GLenum,
        miplevel: cl_GLint,
        texture: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateFromGLTexture3D_fn = clCreateFromGLTexture3D_t;

pub type clCreateEventFromGLsyncKHR_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        sync: cl_GLsync,
        errcode_ret: *mut cl_int,
    ) -> cl_event,
>;
pub type clCreateEventFromGLsyncKHR_fn = clCreateEventFromGLsyncKHR_t;

pub type clGetSupportedGLTextureFormatsINTEL_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        image_type: cl_mem_object_type,
        num_entries: cl_uint,
        gl_formats: *mut cl_GLenum,
        num_texture_formats: *mut cl_uint,
    ) -> cl_int,
>;
pub type clGetSupportedGLTextureFormatsINTEL_fn = clGetSupportedGLTextureFormatsINTEL_t;

#[cfg_attr(not(target_os = "macos"), link(name = "OpenCL"))]
#[cfg_attr(target_os = "macos", link(name = "OpenCL", kind = "framework"))]
#[cfg(feature = "static")]
unsafe extern "system" {

    pub fn clCreateFromGLBuffer(
        context: cl_context,
        flags: cl_mem_flags,
        bufobj: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem;

    #[cfg(feature = "CL_VERSION_1_2")]
    pub fn clCreateFromGLTexture(
        context: cl_context,
        flags: cl_mem_flags,
        target: cl_GLenum,
        miplevel: cl_GLint,
        texture: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem;

    pub fn clCreateFromGLRenderbuffer(
        context: cl_context,
        flags: cl_mem_flags,
        renderbuffer: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem;

    pub fn clGetGLObjectInfo(
        memobj: cl_mem,
        gl_object_type: *mut cl_gl_object_type,
        gl_object_name: *mut cl_GLuint,
    ) -> cl_int;

    pub fn clGetGLTextureInfo(
        memobj: cl_mem,
        param_name: cl_gl_texture_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int;

    pub fn clEnqueueAcquireGLObjects(
        command_queue: cl_command_queue,
        num_objects: cl_uint,
        mem_objects: *const cl_mem,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int;

    pub fn clEnqueueReleaseGLObjects(
        command_queue: cl_command_queue,
        num_objects: cl_uint,
        mem_objects: *const cl_mem,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int;

    // Deprecated OpenCL 1.1 APIs
    #[cfg_attr(
        any(
            feature = "CL_VERSION_1_2",
            feature = "CL_VERSION_2_0",
            feature = "CL_VERSION_2_1",
            feature = "CL_VERSION_2_2",
            feature = "CL_VERSION_3_0"
        ),
        deprecated(
            since = "0.1.0",
            note = "From CL_VERSION_1_2 use clCreateFromGLTexture"
        )
    )]
    pub fn clCreateFromGLTexture2D(
        context: cl_context,
        flags: cl_mem_flags,
        texture_target: cl_GLenum,
        miplevel: cl_GLint,
        texture: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem;

    #[cfg_attr(
        any(
            feature = "CL_VERSION_1_2",
            feature = "CL_VERSION_2_0",
            feature = "CL_VERSION_2_1",
            feature = "CL_VERSION_2_2",
            feature = "CL_VERSION_3_0"
        ),
        deprecated(
            since = "0.1.0",
            note = "From CL_VERSION_1_2 use clCreateFromGLTexture"
        )
    )]
    pub fn clCreateFromGLTexture3D(
        context: cl_context,
        flags: cl_mem_flags,
        texture_target: cl_GLenum,
        miplevel: cl_GLint,
        texture: cl_GLuint,
        errcode_ret: *mut cl_int,
    ) -> cl_mem;

}

// cl_khr_gl_sharing extension
// * NOTE: Originally lower case: `cl_kgr_gl_sharing`
pub const CL_KHR_GL_SHARING: cl_int = 1;

// Additional Error Codes
pub const CL_INVALID_GL_SHAREGROUP_REFERENCE_KHR: cl_int = -1000;

// cl_gl_context_info
pub const CL_CURRENT_DEVICE_FOR_GL_CONTEXT_KHR: cl_gl_context_info = 0x2006;
pub const CL_DEVICES_FOR_GL_CONTEXT_KHR: cl_gl_context_info = 0x2007;

// Additional cl_context_properties
pub const CL_GL_CONTEXT_KHR: cl_context_properties = 0x2008;
pub const CL_EGL_DISPLAY_KHR: cl_context_properties = 0x2009;
pub const CL_GLX_DISPLAY_KHR: cl_context_properties = 0x200A;
pub const CL_WGL_HDC_KHR: cl_context_properties = 0x200B;
pub const CL_CGL_SHAREGROUP_KHR: cl_context_properties = 0x200C;

// cl_khr_gl_event extension
pub const CL_COMMAND_GL_FENCE_SYNC_OBJECT_KHR: cl_uint = 0x200D;

#[cfg_attr(not(target_os = "macos"), link(name = "OpenCL"))]
#[cfg_attr(target_os = "macos", link(name = "OpenCL", kind = "framework"))]
#[cfg(feature = "static")]
unsafe extern "system" {

    pub fn clGetGLContextInfoKHR(
        properties: *const cl_context_properties,
        param_name: cl_gl_context_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int;

    pub fn clCreateEventFromGLsyncKHR(
        context: cl_context,
        sync: cl_GLsync,
        errcode_ret: *mut cl_int,
    ) -> cl_event;

    pub fn clGetSupportedGLTextureFormatsINTEL(
        context: cl_context,
        flags: cl_mem_flags,
        image_type: cl_mem_object_type,
        num_entries: cl_uint,
        gl_formats: *mut cl_GLenum,
        num_texture_formats: *mut cl_uint,
    ) -> cl_int;
}
