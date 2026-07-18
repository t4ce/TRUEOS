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

//! FFI bindings for [cl_icd.h](https://github.com/KhronosGroup/OpenCL-Headers/blob/main/CL/cl_icd.h)

#![allow(non_camel_case_types, non_snake_case)]

pub use super::cl_egl::*;
pub use super::cl_ext::*;
pub use super::cl_function_types::*;
pub use super::cl_gl::*;

// Windows
pub use super::cl_d3d10::*;
pub use super::cl_d3d11::*;
pub use super::cl_dx9_media_sharing::*;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct _cl_icd_dispatch {
    pub clGetPlatformIDs: clGetPlatformIDs_t,
    pub clGetPlatformInfo: clGetPlatformInfo_t,
    pub clGetDeviceIDs: clGetDeviceIDs_t,
    pub clGetDeviceInfo: clGetDeviceInfo_t,
    pub clCreateContext: clCreateContext_t,
    pub clCreateContextFromType: clCreateContextFromType_t,
    pub clRetainContext: clRetainContext_t,
    pub clReleaseContext: clReleaseContext_t,
    pub clGetContextInfo: clGetContextInfo_t,
    pub clCreateCommandQueue: clCreateCommandQueue_t,
    pub clRetainCommandQueue: clRetainCommandQueue_t,
    pub clReleaseCommandQueue: clReleaseCommandQueue_t,
    pub clGetCommandQueueInfo: clGetCommandQueueInfo_t,
    pub clSetCommandQueueProperty: clSetCommandQueueProperty_t,
    pub clCreateBuffer: clCreateBuffer_t,
    pub clCreateImage2D: clCreateImage2D_t,
    pub clCreateImage3D: clCreateImage3D_t,
    pub clRetainMemObject: clRetainMemObject_t,
    pub clReleaseMemObject: clReleaseMemObject_t,
    pub clGetSupportedImageFormats: clGetSupportedImageFormats_t,
    pub clGetMemObjectInfo: clGetMemObjectInfo_t,
    pub clGetImageInfo: clGetImageInfo_t,
    pub clCreateSampler: clCreateSampler_t,
    pub clRetainSampler: clRetainSampler_t,
    pub clReleaseSampler: clReleaseSampler_t,
    pub clGetSamplerInfo: clGetSamplerInfo_t,
    pub clCreateProgramWithSource: clCreateProgramWithSource_t,
    pub clCreateProgramWithBinary: clCreateProgramWithBinary_t,
    pub clRetainProgram: clRetainProgram_t,
    pub clReleaseProgram: clReleaseProgram_t,
    pub clBuildProgram: clBuildProgram_t,
    pub clUnloadCompiler: clUnloadCompiler_t,
    pub clGetProgramInfo: clGetProgramInfo_t,
    pub clGetProgramBuildInfo: clGetProgramBuildInfo_t,
    pub clCreateKernel: clCreateKernel_t,
    pub clCreateKernelsInProgram: clCreateKernelsInProgram_t,
    pub clRetainKernel: clRetainKernel_t,
    pub clReleaseKernel: clReleaseKernel_t,
    pub clSetKernelArg: clSetKernelArg_t,
    pub clGetKernelInfo: clGetKernelInfo_t,
    pub clGetKernelWorkGroupInfo: clGetKernelWorkGroupInfo_t,
    pub clWaitForEvents: clWaitForEvents_t,
    pub clGetEventInfo: clGetEventInfo_t,
    pub clRetainEvent: clRetainEvent_t,
    pub clReleaseEvent: clReleaseEvent_t,
    pub clGetEventProfilingInfo: clGetEventProfilingInfo_t,
    pub clFlush: clFlush_t,
    pub clFinish: clFinish_t,
    pub clEnqueueReadBuffer: clEnqueueReadBuffer_t,
    pub clEnqueueWriteBuffer: clEnqueueWriteBuffer_t,
    pub clEnqueueCopyBuffer: clEnqueueCopyBuffer_t,
    pub clEnqueueReadImage: clEnqueueReadImage_t,
    pub clEnqueueWriteImage: clEnqueueWriteImage_t,
    pub clEnqueueCopyImage: clEnqueueCopyImage_t,
    pub clEnqueueCopyImageToBuffer: clEnqueueCopyImageToBuffer_t,
    pub clEnqueueCopyBufferToImage: clEnqueueCopyBufferToImage_t,
    pub clEnqueueMapBuffer: clEnqueueMapBuffer_t,
    pub clEnqueueMapImage: clEnqueueMapImage_t,
    pub clEnqueueUnmapMemObject: clEnqueueUnmapMemObject_t,
    pub clEnqueueNDRangeKernel: clEnqueueNDRangeKernel_t,
    pub clEnqueueTask: clEnqueueTask_t,
    pub clEnqueueNativeKernel: clEnqueueNativeKernel_t,
    pub clEnqueueMarker: clEnqueueMarker_t,
    pub clEnqueueWaitForEvents: clEnqueueWaitForEvents_t,
    pub clEnqueueBarrier: clEnqueueBarrier_t,
    pub clGetExtensionFunctionAddress: clGetExtensionFunctionAddress_t,

    pub clCreateFromGLBuffer: clCreateFromGLBuffer_t,
    pub clCreateFromGLTexture2D: clCreateFromGLTexture2D_t,
    pub clCreateFromGLTexture3D: clCreateFromGLTexture3D_t,
    pub clCreateFromGLRenderbuffer: clCreateFromGLRenderbuffer_t,
    pub clGetGLObjectInfo: clGetGLObjectInfo_t,
    pub clGetGLTextureInfo: clGetGLTextureInfo_t,
    pub clEnqueueAcquireGLObjects: clEnqueueAcquireGLObjects_t,
    pub clEnqueueReleaseGLObjects: clEnqueueReleaseGLObjects_t,
    pub clGetGLContextInfoKHR: clGetGLContextInfoKHR_t,

    // cl_khr_d3d10_sharing
    pub clGetDeviceIDsFromD3D10KHR: clGetDeviceIDsFromD3D10KHR_t,
    pub clCreateFromD3D10BufferKHR: clCreateFromD3D10BufferKHR_t,
    pub clCreateFromD3D10Texture2DKHR: clCreateFromD3D10Texture2DKHR_t,
    pub clCreateFromD3D10Texture3DKHR: clCreateFromD3D10Texture3DKHR_t,
    pub clEnqueueAcquireD3D10ObjectsKHR: clEnqueueAcquireD3D10ObjectsKHR_t,
    pub clEnqueueReleaseD3D10ObjectsKHR: clEnqueueReleaseD3D10ObjectsKHR_t,

    // OpenCL 1.1
    pub clSetEventCallback: clSetEventCallback_t,
    pub clCreateSubBuffer: clCreateSubBuffer_t,
    pub clSetMemObjectDestructorCallback: clSetMemObjectDestructorCallback_t,
    pub clCreateUserEvent: clCreateUserEvent_t,
    pub clSetUserEventStatus: clSetUserEventStatus_t,
    pub clEnqueueReadBufferRect: clEnqueueReadBufferRect_t,
    pub clEnqueueWriteBufferRect: clEnqueueWriteBufferRect_t,
    pub clEnqueueCopyBufferRect: clEnqueueCopyBufferRect_t,

    // cl_ext_device_fission
    pub clCreateSubDevicesEXT: clCreateSubDevicesEXT_t,
    pub clRetainDeviceEXT: clRetainDeviceEXT_t,
    pub clReleaseDeviceEXT: clReleaseDeviceEXT_t,
    pub clCreateEventFromGLsyncKHR: clCreateEventFromGLsyncKHR_t,

    // OpenCL 1.2
    pub clCreateSubDevices: clCreateSubDevices_t,
    pub clRetainDevice: clRetainDevice_t,
    pub clReleaseDevice: clReleaseDevice_t,
    pub clCreateImage: clCreateImage_t,
    pub clCreateProgramWithBuiltInKernels: clCreateProgramWithBuiltInKernels_t,
    pub clCompileProgram: clCompileProgram_t,
    pub clLinkProgram: clLinkProgram_t,
    pub clUnloadPlatformCompiler: clUnloadPlatformCompiler_t,
    pub clGetKernelArgInfo: clGetKernelArgInfo_t,
    pub clEnqueueFillBuffer: clEnqueueFillBuffer_t,
    pub clEnqueueFillImage: clEnqueueFillImage_t,
    pub clEnqueueMigrateMemObjects: clEnqueueMigrateMemObjects_t,
    pub clEnqueueMarkerWithWaitList: clEnqueueMarkerWithWaitList_t,
    pub clEnqueueBarrierWithWaitList: clEnqueueBarrierWithWaitList_t,
    pub clGetExtensionFunctionAddressForPlatform: clGetExtensionFunctionAddressForPlatform_t,
    pub clCreateFromGLTexture: clCreateFromGLTexture_t,

    // cl_khr_d3d11_sharing and cl_khr_dx9_media_sharing
    pub clGetDeviceIDsFromD3D11KHR: clGetDeviceIDsFromD3D11KHR_t,
    pub clCreateFromD3D11BufferKHR: clCreateFromD3D11BufferKHR_t,
    pub clCreateFromD3D11Texture2DKHR: clCreateFromD3D11Texture2DKHR_t,
    pub clCreateFromD3D11Texture3DKHR: clCreateFromD3D11Texture3DKHR_t,
    pub clCreateFromDX9MediaSurfaceKHR: clCreateFromDX9MediaSurfaceKHR_t,
    pub clEnqueueAcquireD3D11ObjectsKHR: clEnqueueAcquireD3D11ObjectsKHR_t,
    pub clEnqueueReleaseD3D11ObjectsKHR: clEnqueueReleaseD3D11ObjectsKHR_t,
    pub clGetDeviceIDsFromDX9MediaAdapterKHR: clGetDeviceIDsFromDX9MediaAdapterKHR_t,
    pub clEnqueueAcquireDX9MediaSurfacesKHR: clEnqueueAcquireDX9MediaSurfacesKHR_t,
    pub clEnqueueReleaseDX9MediaSurfacesKHR: clEnqueueReleaseDX9MediaSurfacesKHR_t,

    // cl_khr_egl_image
    pub clCreateFromEGLImageKHR: clCreateFromEGLImageKHR_t,
    pub clEnqueueAcquireEGLObjectsKHR: clEnqueueAcquireEGLObjectsKHR_t,
    pub clEnqueueReleaseEGLObjectsKHR: clEnqueueReleaseEGLObjectsKHR_t,

    // cl_khr_egl_event
    pub clCreateEventFromEGLSyncKHR: clCreateEventFromEGLSyncKHR_t,

    // OpenCL 2.0
    pub clCreateCommandQueueWithProperties: clCreateCommandQueueWithProperties_t,
    pub clCreatePipe: clCreatePipe_t,
    pub clGetPipeInfo: clGetPipeInfo_t,
    pub clSVMAlloc: clSVMAlloc_t,
    pub clSVMFree: clSVMFree_t,
    pub clEnqueueSVMFree: clEnqueueSVMFree_t,
    pub clEnqueueSVMMemcpy: clEnqueueSVMMemcpy_t,
    pub clEnqueueSVMMemFill: clEnqueueSVMMemFill_t,
    pub clEnqueueSVMMap: clEnqueueSVMMap_t,
    pub clEnqueueSVMUnmap: clEnqueueSVMUnmap_t,
    pub clCreateSamplerWithProperties: clCreateSamplerWithProperties_t,
    pub clSetKernelArgSVMPointer: clSetKernelArgSVMPointer_t,
    pub clSetKernelExecInfo: clSetKernelExecInfo_t,

    // cl_khr_sub_groups
    pub clGetKernelSubGroupInfoKHR: clGetKernelSubGroupInfoKHR_t,

    // OpenCL 2.1
    pub clCloneKernel: clCloneKernel_t,
    pub clCreateProgramWithIL: clCreateProgramWithIL_t,
    pub clEnqueueSVMMigrateMem: clEnqueueSVMMigrateMem_t,
    pub clGetDeviceAndHostTimer: clGetDeviceAndHostTimer_t,
    pub clGetHostTimer: clGetHostTimer_t,
    pub clGetKernelSubGroupInfo: clGetKernelSubGroupInfo_t,
    pub clSetDefaultDeviceCommandQueue: clSetDefaultDeviceCommandQueue_t,

    // OpenCL 2.2
    pub clSetProgramReleaseCallback: clSetProgramReleaseCallback_t,
    pub clSetProgramSpecializationConstant: clSetProgramSpecializationConstant_t,

    // OpenCL 3.0
    pub clCreateBufferWithProperties: clCreateBufferWithProperties_t,
    pub clCreateImageWithProperties: clCreateImageWithProperties_t,
    pub clSetContextDestructorCallback: clSetContextDestructorCallback_t,
}
pub type cl_icd_dispatch = _cl_icd_dispatch;
