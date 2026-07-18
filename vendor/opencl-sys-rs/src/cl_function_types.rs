// Copyright (c) 2023 Via Technology Ltd.
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

//! FFI bindings for [cl_function_types.h](https://github.com/KhronosGroup/OpenCL-Headers/blob/main/CL/cl_function_types.h)

#![allow(non_camel_case_types)]

pub use super::cl::{
    cl_addressing_mode, cl_bitfield, cl_bool, cl_buffer_create_type, cl_channel_type,
    cl_command_queue, cl_command_queue_info, cl_command_queue_properties, cl_command_type,
    cl_context, cl_context_info, cl_context_properties, cl_device_id, cl_device_info,
    cl_device_partition_property, cl_device_type, cl_event, cl_event_info, cl_filter_mode,
    cl_image_desc, cl_image_format, cl_image_info, cl_kernel, cl_kernel_arg_info,
    cl_kernel_exec_info, cl_kernel_info, cl_kernel_sub_group_info, cl_kernel_work_group_info,
    cl_map_flags, cl_mem, cl_mem_flags, cl_mem_info, cl_mem_migration_flags, cl_mem_object_type,
    cl_mem_properties, cl_pipe_info, cl_pipe_properties, cl_platform_id, cl_platform_info,
    cl_profiling_info, cl_program, cl_program_build_info, cl_program_info, cl_properties,
    cl_queue_properties, cl_sampler, cl_sampler_info, cl_sampler_properties, cl_svm_mem_flags,
};
use super::cl_platform::{cl_int, cl_uint, cl_ulong};
use libc::{c_char, c_void, size_t};

pub type clGetPlatformIDs_t = Option<
    unsafe extern "C" fn(
        num_entries: cl_uint,
        platforms: *mut cl_platform_id,
        num_platforms: *mut cl_uint,
    ) -> cl_int,
>;
pub type clGetPlatformIDs_fn = clGetPlatformIDs_t;

pub type clGetPlatformInfo_t = Option<
    unsafe extern "C" fn(
        platform: cl_platform_id,
        param_name: cl_platform_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetPlatformInfo_fn = clGetPlatformInfo_t;

pub type clGetDeviceIDs_t = Option<
    unsafe extern "C" fn(
        platform: cl_platform_id,
        device_type: cl_device_type,
        num_entries: cl_uint,
        devices: *mut cl_device_id,
        num_devices: *mut cl_uint,
    ) -> cl_int,
>;
pub type clGetDeviceIDs_fn = clGetDeviceIDs_t;

pub type clGetDeviceInfo_t = Option<
    unsafe extern "C" fn(
        device: cl_device_id,
        param_name: cl_device_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetDeviceInfo_fn = clGetDeviceInfo_t;

pub type clCreateContext_t = Option<
    unsafe extern "C" fn(
        properties: *const cl_context_properties,
        num_devices: cl_uint,
        devices: *const cl_device_id,
        pfn_notify: Option<
            unsafe extern "C" fn(
                errinfo: *const c_char,
                private_info: *const c_void,
                cb: size_t,
                user_data: *mut c_void,
            ),
        >,
        user_data: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_context,
>;
pub type clCreateContext_fn = clCreateContext_t;

pub type clCreateContextFromType_t = Option<
    unsafe extern "C" fn(
        properties: *const cl_context_properties,
        device_type: cl_device_type,
        pfn_notify: Option<
            unsafe extern "C" fn(
                errinfo: *const c_char,
                private_info: *const c_void,
                cb: size_t,
                user_data: *mut c_void,
            ),
        >,
        user_data: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_context,
>;
pub type clCreateContextFromType_fn = clCreateContextFromType_t;

pub type clRetainContext_t = Option<unsafe extern "C" fn(context: cl_context) -> cl_int>;
pub type clRetainContext_fn = clRetainContext_t;

pub type clReleaseContext_t = Option<unsafe extern "C" fn(context: cl_context) -> cl_int>;
pub type clReleaseContext_fn = clReleaseContext_t;

pub type clGetContextInfo_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        param_name: cl_context_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetContextInfo_fn = clGetContextInfo_t;

pub type clRetainCommandQueue_t =
    Option<unsafe extern "C" fn(command_queue: cl_command_queue) -> cl_int>;
pub type clRetainCommandQueue_fn = clRetainCommandQueue_t;

pub type clReleaseCommandQueue_t =
    Option<unsafe extern "C" fn(command_queue: cl_command_queue) -> cl_int>;
pub type clReleaseCommandQueue_fn = clReleaseCommandQueue_t;

pub type clGetCommandQueueInfo_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        param_name: cl_command_queue_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetCommandQueueInfo_fn = clGetCommandQueueInfo_t;

pub type clCreateBuffer_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        size: size_t,
        host_ptr: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateBuffer_fn = clCreateBuffer_t;

pub type clRetainMemObject_t = Option<unsafe extern "C" fn(memobj: cl_mem) -> cl_int>;
pub type clRetainMemObject_fn = clRetainMemObject_t;

pub type clReleaseMemObject_t = Option<unsafe extern "C" fn(memobj: cl_mem) -> cl_int>;
pub type clReleaseMemObject_fn = clReleaseMemObject_t;

pub type clGetSupportedImageFormats_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        image_type: cl_mem_object_type,
        num_entries: cl_uint,
        image_formats: *mut cl_image_format,
        num_image_formats: *mut cl_uint,
    ) -> cl_int,
>;
pub type clGetSupportedImageFormats_fn = clGetSupportedImageFormats_t;

pub type clGetMemObjectInfo_t = Option<
    unsafe extern "C" fn(
        memobj: cl_mem,
        param_name: cl_mem_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetMemObjectInfo_fn = clGetMemObjectInfo_t;

pub type clGetImageInfo_t = Option<
    unsafe extern "C" fn(
        image: cl_mem,
        param_name: cl_image_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetImageInfo_fn = clGetImageInfo_t;

pub type clRetainSampler_t = Option<unsafe extern "C" fn(sampler: cl_sampler) -> cl_int>;
pub type clRetainSampler_fn = clRetainSampler_t;

pub type clReleaseSampler_t = Option<unsafe extern "C" fn(sampler: cl_sampler) -> cl_int>;
pub type clReleaseSampler_fn = clReleaseSampler_t;

pub type clGetSamplerInfo_t = Option<
    unsafe extern "C" fn(
        sampler: cl_sampler,
        param_name: cl_sampler_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetSamplerInfo_fn = clGetSamplerInfo_t;

pub type clCreateProgramWithSource_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        count: cl_uint,
        strings: *mut *const c_char,
        lengths: *const size_t,
        errcode_ret: *mut cl_int,
    ) -> cl_program,
>;
pub type clCreateProgramWithSource_fn = clCreateProgramWithSource_t;

pub type clCreateProgramWithBinary_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        num_devices: cl_uint,
        device_list: *const cl_device_id,
        lengths: *const size_t,
        binaries: *mut *const ::core::ffi::c_uchar,
        binary_status: *mut cl_int,
        errcode_ret: *mut cl_int,
    ) -> cl_program,
>;
pub type clCreateProgramWithBinary_fn = clCreateProgramWithBinary_t;

pub type clRetainProgram_t = Option<unsafe extern "C" fn(program: cl_program) -> cl_int>;
pub type clRetainProgram_fn = clRetainProgram_t;

pub type clReleaseProgram_t = Option<unsafe extern "C" fn(program: cl_program) -> cl_int>;
pub type clReleaseProgram_fn = clReleaseProgram_t;

pub type clBuildProgram_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        num_devices: cl_uint,
        device_list: *const cl_device_id,
        options: *const c_char,
        pfn_notify: Option<unsafe extern "C" fn(program: cl_program, user_data: *mut c_void)>,
        user_data: *mut c_void,
    ) -> cl_int,
>;
pub type clBuildProgram_fn = clBuildProgram_t;

pub type clGetProgramInfo_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        param_name: cl_program_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetProgramInfo_fn = clGetProgramInfo_t;

pub type clGetProgramBuildInfo_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        device: cl_device_id,
        param_name: cl_program_build_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetProgramBuildInfo_fn = clGetProgramBuildInfo_t;

pub type clCreateKernel_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        kernel_name: *const c_char,
        errcode_ret: *mut cl_int,
    ) -> cl_kernel,
>;
pub type clCreateKernel_fn = clCreateKernel_t;

pub type clCreateKernelsInProgram_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        num_kernels: cl_uint,
        kernels: *mut cl_kernel,
        num_kernels_ret: *mut cl_uint,
    ) -> cl_int,
>;
pub type clCreateKernelsInProgram_fn = clCreateKernelsInProgram_t;

pub type clRetainKernel_t = Option<unsafe extern "C" fn(kernel: cl_kernel) -> cl_int>;
pub type clRetainKernel_fn = clRetainKernel_t;

pub type clReleaseKernel_t = Option<unsafe extern "C" fn(kernel: cl_kernel) -> cl_int>;
pub type clReleaseKernel_fn = clReleaseKernel_t;

pub type clSetKernelArg_t = Option<
    unsafe extern "C" fn(
        kernel: cl_kernel,
        arg_index: cl_uint,
        arg_size: size_t,
        arg_value: *const c_void,
    ) -> cl_int,
>;
pub type clSetKernelArg_fn = clSetKernelArg_t;

pub type clGetKernelInfo_t = Option<
    unsafe extern "C" fn(
        kernel: cl_kernel,
        param_name: cl_kernel_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetKernelInfo_fn = clGetKernelInfo_t;

pub type clGetKernelWorkGroupInfo_t = Option<
    unsafe extern "C" fn(
        kernel: cl_kernel,
        device: cl_device_id,
        param_name: cl_kernel_work_group_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetKernelWorkGroupInfo_fn = clGetKernelWorkGroupInfo_t;

pub type clWaitForEvents_t =
    Option<unsafe extern "C" fn(num_events: cl_uint, event_list: *const cl_event) -> cl_int>;
pub type clWaitForEvents_fn = clWaitForEvents_t;

pub type clGetEventInfo_t = Option<
    unsafe extern "C" fn(
        event: cl_event,
        param_name: cl_event_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetEventInfo_fn = clGetEventInfo_t;

pub type clRetainEvent_t = Option<unsafe extern "C" fn(event: cl_event) -> cl_int>;
pub type clRetainEvent_fn = clRetainEvent_t;

pub type clReleaseEvent_t = Option<unsafe extern "C" fn(event: cl_event) -> cl_int>;
pub type clReleaseEvent_fn = clReleaseEvent_t;

pub type clGetEventProfilingInfo_t = Option<
    unsafe extern "C" fn(
        event: cl_event,
        param_name: cl_profiling_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetEventProfilingInfo_fn = clGetEventProfilingInfo_t;

pub type clFlush_t = Option<unsafe extern "C" fn(command_queue: cl_command_queue) -> cl_int>;
pub type clFlush_fn = clFlush_t;

pub type clFinish_t = Option<unsafe extern "C" fn(command_queue: cl_command_queue) -> cl_int>;
pub type clFinish_fn = clFinish_t;

pub type clEnqueueReadBuffer_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        buffer: cl_mem,
        blocking_read: cl_bool,
        offset: size_t,
        size: size_t,
        ptr: *mut c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueReadBuffer_fn = clEnqueueReadBuffer_t;

pub type clEnqueueWriteBuffer_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        buffer: cl_mem,
        blocking_write: cl_bool,
        offset: size_t,
        size: size_t,
        ptr: *const c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueWriteBuffer_fn = clEnqueueWriteBuffer_t;

pub type clEnqueueCopyBuffer_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        src_buffer: cl_mem,
        dst_buffer: cl_mem,
        src_offset: size_t,
        dst_offset: size_t,
        size: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueCopyBuffer_fn = clEnqueueCopyBuffer_t;

pub type clEnqueueReadImage_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        image: cl_mem,
        blocking_read: cl_bool,
        origin: *const size_t,
        region: *const size_t,
        row_pitch: size_t,
        slice_pitch: size_t,
        ptr: *mut c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueReadImage_fn = clEnqueueReadImage_t;

pub type clEnqueueWriteImage_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        image: cl_mem,
        blocking_write: cl_bool,
        origin: *const size_t,
        region: *const size_t,
        input_row_pitch: size_t,
        input_slice_pitch: size_t,
        ptr: *const c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueWriteImage_fn = clEnqueueWriteImage_t;

pub type clEnqueueCopyImage_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        src_image: cl_mem,
        dst_image: cl_mem,
        src_origin: *const size_t,
        dst_origin: *const size_t,
        region: *const size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueCopyImage_fn = clEnqueueCopyImage_t;

pub type clEnqueueCopyImageToBuffer_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        src_image: cl_mem,
        dst_buffer: cl_mem,
        src_origin: *const size_t,
        region: *const size_t,
        dst_offset: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueCopyImageToBuffer_fn = clEnqueueCopyImageToBuffer_t;

pub type clEnqueueCopyBufferToImage_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        src_buffer: cl_mem,
        dst_image: cl_mem,
        src_offset: size_t,
        dst_origin: *const size_t,
        region: *const size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueCopyBufferToImage_fn = clEnqueueCopyBufferToImage_t;

pub type clEnqueueMapBuffer_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        buffer: cl_mem,
        blocking_map: cl_bool,
        map_flags: cl_map_flags,
        offset: size_t,
        size: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
        errcode_ret: *mut cl_int,
    ) -> *mut c_void,
>;
pub type clEnqueueMapBuffer_fn = clEnqueueMapBuffer_t;

pub type clEnqueueMapImage_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        image: cl_mem,
        blocking_map: cl_bool,
        map_flags: cl_map_flags,
        origin: *const size_t,
        region: *const size_t,
        image_row_pitch: *mut size_t,
        image_slice_pitch: *mut size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
        errcode_ret: *mut cl_int,
    ) -> *mut c_void,
>;
pub type clEnqueueMapImage_fn = clEnqueueMapImage_t;

pub type clEnqueueUnmapMemObject_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        memobj: cl_mem,
        mapped_ptr: *mut c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueUnmapMemObject_fn = clEnqueueUnmapMemObject_t;

pub type clEnqueueNDRangeKernel_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        kernel: cl_kernel,
        work_dim: cl_uint,
        global_work_offset: *const size_t,
        global_work_size: *const size_t,
        local_work_size: *const size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueNDRangeKernel_fn = clEnqueueNDRangeKernel_t;

pub type clEnqueueNativeKernel_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        user_func: Option<unsafe extern "C" fn(arg1: *mut c_void)>,
        args: *mut c_void,
        cb_args: size_t,
        num_mem_objects: cl_uint,
        mem_list: *const cl_mem,
        args_mem_loc: *mut *const c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueNativeKernel_fn = clEnqueueNativeKernel_t;

pub type clSetCommandQueueProperty_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        properties: cl_command_queue_properties,
        enable: cl_bool,
        old_properties: *mut cl_command_queue_properties,
    ) -> cl_int,
>;
pub type clSetCommandQueueProperty_fn = clSetCommandQueueProperty_t;

pub type clCreateImage2D_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        image_format: *const cl_image_format,
        image_width: size_t,
        image_height: size_t,
        image_row_pitch: size_t,
        host_ptr: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateImage2D_fn = clCreateImage2D_t;

pub type clCreateImage3D_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        image_format: *const cl_image_format,
        image_width: size_t,
        image_height: size_t,
        image_depth: size_t,
        image_row_pitch: size_t,
        image_slice_pitch: size_t,
        host_ptr: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateImage3D_fn = clCreateImage3D_t;

pub type clEnqueueMarker_t =
    Option<unsafe extern "C" fn(command_queue: cl_command_queue, event: *mut cl_event) -> cl_int>;
pub type clEnqueueMarker_fn = clEnqueueMarker_t;

pub type clEnqueueWaitForEvents_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_events: cl_uint,
        event_list: *const cl_event,
    ) -> cl_int,
>;
pub type clEnqueueWaitForEvents_fn = clEnqueueWaitForEvents_t;

pub type clEnqueueBarrier_t =
    Option<unsafe extern "C" fn(command_queue: cl_command_queue) -> cl_int>;
pub type clEnqueueBarrier_fn = clEnqueueBarrier_t;

pub type clUnloadCompiler_t = Option<unsafe extern "C" fn() -> cl_int>;
pub type clUnloadCompiler_fn = clUnloadCompiler_t;

pub type clGetExtensionFunctionAddress_t =
    Option<unsafe extern "C" fn(func_name: *const c_char) -> *mut c_void>;
pub type clGetExtensionFunctionAddress_fn = clGetExtensionFunctionAddress_t;

pub type clCreateCommandQueue_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        device: cl_device_id,
        properties: cl_command_queue_properties,
        errcode_ret: *mut cl_int,
    ) -> cl_command_queue,
>;
pub type clCreateCommandQueue_fn = clCreateCommandQueue_t;

pub type clCreateSampler_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        normalized_coords: cl_bool,
        addressing_mode: cl_addressing_mode,
        filter_mode: cl_filter_mode,
        errcode_ret: *mut cl_int,
    ) -> cl_sampler,
>;
pub type clCreateSampler_fn = clCreateSampler_t;

pub type clEnqueueTask_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        kernel: cl_kernel,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueTask_fn = clEnqueueTask_t;

pub type clCreateSubBuffer_t = Option<
    unsafe extern "C" fn(
        buffer: cl_mem,
        flags: cl_mem_flags,
        buffer_create_type: cl_buffer_create_type,
        buffer_create_info: *const c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateSubBuffer_fn = clCreateSubBuffer_t;

pub type clSetMemObjectDestructorCallback_t = Option<
    unsafe extern "C" fn(
        memobj: cl_mem,
        pfn_notify: Option<unsafe extern "C" fn(memobj: cl_mem, user_data: *mut c_void)>,
        user_data: *mut c_void,
    ) -> cl_int,
>;
pub type clSetMemObjectDestructorCallback_fn = clSetMemObjectDestructorCallback_t;

pub type clCreateUserEvent_t =
    Option<unsafe extern "C" fn(context: cl_context, errcode_ret: *mut cl_int) -> cl_event>;
pub type clCreateUserEvent_fn = clCreateUserEvent_t;

pub type clSetUserEventStatus_t =
    Option<unsafe extern "C" fn(event: cl_event, execution_status: cl_int) -> cl_int>;
pub type clSetUserEventStatus_fn = clSetUserEventStatus_t;

pub type clSetEventCallback_t = Option<
    unsafe extern "C" fn(
        event: cl_event,
        command_exec_callback_type: cl_int,
        pfn_notify: Option<
            unsafe extern "C" fn(
                event: cl_event,
                event_command_status: cl_int,
                user_data: *mut c_void,
            ),
        >,
        user_data: *mut c_void,
    ) -> cl_int,
>;
pub type clSetEventCallback_fn = clSetEventCallback_t;

pub type clEnqueueReadBufferRect_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        buffer: cl_mem,
        blocking_read: cl_bool,
        buffer_origin: *const size_t,
        host_origin: *const size_t,
        region: *const size_t,
        buffer_row_pitch: size_t,
        buffer_slice_pitch: size_t,
        host_row_pitch: size_t,
        host_slice_pitch: size_t,
        ptr: *mut c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueReadBufferRect_fn = clEnqueueReadBufferRect_t;

pub type clEnqueueWriteBufferRect_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        buffer: cl_mem,
        blocking_write: cl_bool,
        buffer_origin: *const size_t,
        host_origin: *const size_t,
        region: *const size_t,
        buffer_row_pitch: size_t,
        buffer_slice_pitch: size_t,
        host_row_pitch: size_t,
        host_slice_pitch: size_t,
        ptr: *const c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueWriteBufferRect_fn = clEnqueueWriteBufferRect_t;

pub type clEnqueueCopyBufferRect_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        src_buffer: cl_mem,
        dst_buffer: cl_mem,
        src_origin: *const size_t,
        dst_origin: *const size_t,
        region: *const size_t,
        src_row_pitch: size_t,
        src_slice_pitch: size_t,
        dst_row_pitch: size_t,
        dst_slice_pitch: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueCopyBufferRect_fn = clEnqueueCopyBufferRect_t;

pub type clCreateSubDevices_t = Option<
    unsafe extern "C" fn(
        in_device: cl_device_id,
        properties: *const cl_device_partition_property,
        num_devices: cl_uint,
        out_devices: *mut cl_device_id,
        num_devices_ret: *mut cl_uint,
    ) -> cl_int,
>;
pub type clCreateSubDevices_fn = clCreateSubDevices_t;

pub type clRetainDevice_t = Option<unsafe extern "C" fn(device: cl_device_id) -> cl_int>;
pub type clRetainDevice_fn = clRetainDevice_t;

pub type clReleaseDevice_t = Option<unsafe extern "C" fn(device: cl_device_id) -> cl_int>;
pub type clReleaseDevice_fn = clReleaseDevice_t;

pub type clCreateImage_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        image_format: *const cl_image_format,
        image_desc: *const cl_image_desc,
        host_ptr: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateImage_fn = clCreateImage_t;

pub type clCreateProgramWithBuiltInKernels_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        num_devices: cl_uint,
        device_list: *const cl_device_id,
        kernel_names: *const c_char,
        errcode_ret: *mut cl_int,
    ) -> cl_program,
>;
pub type clCreateProgramWithBuiltInKernels_fn = clCreateProgramWithBuiltInKernels_t;

pub type clCompileProgram_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        num_devices: cl_uint,
        device_list: *const cl_device_id,
        options: *const c_char,
        num_input_headers: cl_uint,
        input_headers: *const cl_program,
        header_include_names: *mut *const c_char,
        pfn_notify: Option<unsafe extern "C" fn(program: cl_program, user_data: *mut c_void)>,
        user_data: *mut c_void,
    ) -> cl_int,
>;
pub type clCompileProgram_fn = clCompileProgram_t;

pub type clLinkProgram_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        num_devices: cl_uint,
        device_list: *const cl_device_id,
        options: *const c_char,
        num_input_programs: cl_uint,
        input_programs: *const cl_program,
        pfn_notify: Option<unsafe extern "C" fn(program: cl_program, user_data: *mut c_void)>,
        user_data: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_program,
>;
pub type clLinkProgram_fn = clLinkProgram_t;

pub type clUnloadPlatformCompiler_t =
    Option<unsafe extern "C" fn(platform: cl_platform_id) -> cl_int>;
pub type clUnloadPlatformCompiler_fn = clUnloadPlatformCompiler_t;

pub type clGetKernelArgInfo_t = Option<
    unsafe extern "C" fn(
        kernel: cl_kernel,
        arg_index: cl_uint,
        param_name: cl_kernel_arg_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetKernelArgInfo_fn = clGetKernelArgInfo_t;

pub type clEnqueueFillBuffer_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        buffer: cl_mem,
        pattern: *const c_void,
        pattern_size: size_t,
        offset: size_t,
        size: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueFillBuffer_fn = clEnqueueFillBuffer_t;

pub type clEnqueueFillImage_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        image: cl_mem,
        fill_color: *const c_void,
        origin: *const size_t,
        region: *const size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueFillImage_fn = clEnqueueFillImage_t;

pub type clEnqueueMigrateMemObjects_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_mem_objects: cl_uint,
        mem_objects: *const cl_mem,
        flags: cl_mem_migration_flags,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueMigrateMemObjects_fn = clEnqueueMigrateMemObjects_t;

pub type clEnqueueMarkerWithWaitList_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueMarkerWithWaitList_fn = clEnqueueMarkerWithWaitList_t;

pub type clEnqueueBarrierWithWaitList_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueBarrierWithWaitList_fn = clEnqueueBarrierWithWaitList_t;

pub type clGetExtensionFunctionAddressForPlatform_t =
    Option<unsafe extern "C" fn(platform: cl_platform_id, func_name: *const c_char) -> *mut c_void>;
pub type clGetExtensionFunctionAddressForPlatform_fn = clGetExtensionFunctionAddressForPlatform_t;

pub type clCreateCommandQueueWithProperties_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        device: cl_device_id,
        properties: *const cl_queue_properties,
        errcode_ret: *mut cl_int,
    ) -> cl_command_queue,
>;
pub type clCreateCommandQueueWithProperties_fn = clCreateCommandQueueWithProperties_t;

pub type clCreatePipe_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_mem_flags,
        pipe_packet_size: cl_uint,
        pipe_max_packets: cl_uint,
        properties: *const cl_pipe_properties,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreatePipe_fn = clCreatePipe_t;

pub type clGetPipeInfo_t = Option<
    unsafe extern "C" fn(
        pipe: cl_mem,
        param_name: cl_pipe_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetPipeInfo_fn = clGetPipeInfo_t;

pub type clSVMAlloc_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        flags: cl_svm_mem_flags,
        size: size_t,
        alignment: cl_uint,
    ) -> *mut c_void,
>;
pub type clSVMAlloc_fn = clSVMAlloc_t;

pub type clSVMFree_t = Option<unsafe extern "C" fn(context: cl_context, svm_pointer: *mut c_void)>;
pub type clSVMFree_fn = clSVMFree_t;

pub type clCreateSamplerWithProperties_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        sampler_properties: *const cl_sampler_properties,
        errcode_ret: *mut cl_int,
    ) -> cl_sampler,
>;
pub type clCreateSamplerWithProperties_fn = clCreateSamplerWithProperties_t;

pub type clSetKernelArgSVMPointer_t = Option<
    unsafe extern "C" fn(kernel: cl_kernel, arg_index: cl_uint, arg_value: *const c_void) -> cl_int,
>;
pub type clSetKernelArgSVMPointer_fn = clSetKernelArgSVMPointer_t;

pub type clSetKernelExecInfo_t = Option<
    unsafe extern "C" fn(
        kernel: cl_kernel,
        param_name: cl_kernel_exec_info,
        param_value_size: size_t,
        param_value: *const c_void,
    ) -> cl_int,
>;
pub type clSetKernelExecInfo_fn = clSetKernelExecInfo_t;

pub type clEnqueueSVMFree_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_svm_pointers: cl_uint,
        svm_pointers: *mut *mut c_void,
        pfn_free_func: Option<
            unsafe extern "C" fn(
                queue: cl_command_queue,
                num_svm_pointers: cl_uint,
                svm_pointers: *mut *mut c_void,
                user_data: *mut c_void,
            ),
        >,
        user_data: *mut c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueSVMFree_fn = clEnqueueSVMFree_t;

pub type clEnqueueSVMMemcpy_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        blocking_copy: cl_bool,
        dst_ptr: *mut c_void,
        src_ptr: *const c_void,
        size: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueSVMMemcpy_fn = clEnqueueSVMMemcpy_t;

pub type clEnqueueSVMMemFill_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        svm_ptr: *mut c_void,
        pattern: *const c_void,
        pattern_size: size_t,
        size: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueSVMMemFill_fn = clEnqueueSVMMemFill_t;

pub type clEnqueueSVMMap_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        blocking_map: cl_bool,
        flags: cl_map_flags,
        svm_ptr: *mut c_void,
        size: size_t,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueSVMMap_fn = clEnqueueSVMMap_t;

pub type clEnqueueSVMUnmap_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        svm_ptr: *mut c_void,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueSVMUnmap_fn = clEnqueueSVMUnmap_t;

pub type clSetDefaultDeviceCommandQueue_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        device: cl_device_id,
        command_queue: cl_command_queue,
    ) -> cl_int,
>;
pub type clSetDefaultDeviceCommandQueue_fn = clSetDefaultDeviceCommandQueue_t;

pub type clGetDeviceAndHostTimer_t = Option<
    unsafe extern "C" fn(
        device: cl_device_id,
        device_timestamp: *mut cl_ulong,
        host_timestamp: *mut cl_ulong,
    ) -> cl_int,
>;
pub type clGetDeviceAndHostTimer_fn = clGetDeviceAndHostTimer_t;

pub type clGetHostTimer_t =
    Option<unsafe extern "C" fn(device: cl_device_id, host_timestamp: *mut cl_ulong) -> cl_int>;
pub type clGetHostTimer_fn = clGetHostTimer_t;

pub type clCreateProgramWithIL_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        il: *const c_void,
        length: size_t,
        errcode_ret: *mut cl_int,
    ) -> cl_program,
>;
pub type clCreateProgramWithIL_fn = clCreateProgramWithIL_t;

pub type clCloneKernel_t =
    Option<unsafe extern "C" fn(source_kernel: cl_kernel, errcode_ret: *mut cl_int) -> cl_kernel>;
pub type clCloneKernel_fn = clCloneKernel_t;

pub type clGetKernelSubGroupInfo_t = Option<
    unsafe extern "C" fn(
        kernel: cl_kernel,
        device: cl_device_id,
        param_name: cl_kernel_sub_group_info,
        input_value_size: size_t,
        input_value: *const c_void,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetKernelSubGroupInfo_fn = clGetKernelSubGroupInfo_t;

pub type clEnqueueSVMMigrateMem_t = Option<
    unsafe extern "C" fn(
        command_queue: cl_command_queue,
        num_svm_pointers: cl_uint,
        svm_pointers: *mut *const c_void,
        sizes: *const size_t,
        flags: cl_mem_migration_flags,
        num_events_in_wait_list: cl_uint,
        event_wait_list: *const cl_event,
        event: *mut cl_event,
    ) -> cl_int,
>;
pub type clEnqueueSVMMigrateMem_fn = clEnqueueSVMMigrateMem_t;

pub type clSetProgramSpecializationConstant_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        spec_id: cl_uint,
        spec_size: size_t,
        spec_value: *const c_void,
    ) -> cl_int,
>;
pub type clSetProgramSpecializationConstant_fn = clSetProgramSpecializationConstant_t;

pub type clSetProgramReleaseCallback_t = Option<
    unsafe extern "C" fn(
        program: cl_program,
        pfn_notify: Option<unsafe extern "C" fn(program: cl_program, user_data: *mut c_void)>,
        user_data: *mut c_void,
    ) -> cl_int,
>;
pub type clSetProgramReleaseCallback_fn = clSetProgramReleaseCallback_t;

pub type clSetContextDestructorCallback_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        pfn_notify: Option<unsafe extern "C" fn(context: cl_context, user_data: *mut c_void)>,
        user_data: *mut c_void,
    ) -> cl_int,
>;
pub type clSetContextDestructorCallback_fn = clSetContextDestructorCallback_t;

pub type clCreateBufferWithProperties_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        properties: *const cl_mem_properties,
        flags: cl_mem_flags,
        size: size_t,
        host_ptr: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateBufferWithProperties_fn = clCreateBufferWithProperties_t;

pub type clCreateImageWithProperties_t = Option<
    unsafe extern "C" fn(
        context: cl_context,
        properties: *const cl_mem_properties,
        flags: cl_mem_flags,
        image_format: *const cl_image_format,
        image_desc: *const cl_image_desc,
        host_ptr: *mut c_void,
        errcode_ret: *mut cl_int,
    ) -> cl_mem,
>;
pub type clCreateImageWithProperties_fn = clCreateImageWithProperties_t;
