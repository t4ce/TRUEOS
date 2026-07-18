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

//! FFI bindings for [cl_layer.h](https://github.com/KhronosGroup/OpenCL-Headers/blob/main/CL/cl_layer.h)

#![allow(non_camel_case_types, non_snake_case)]

pub use super::cl_icd::cl_icd_dispatch;
use super::cl_platform::{cl_int, cl_uint};
use libc::{c_void, size_t};

// cl_loader_layers
pub type cl_layer_info = cl_uint;
pub type cl_layer_api_version = cl_uint;

pub const CL_LAYER_API_VERSION: cl_layer_info = 0x4240;
pub const CL_LAYER_NAME: cl_layer_info = 0x4241;

pub const CL_LAYER_API_VERSION_100: cl_layer_api_version = 100;

pub type clGetLayerInfo_t = Option<
    unsafe extern "C" fn(
        param_name: cl_layer_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int,
>;
pub type clGetLayerInfo_fn = clGetLayerInfo_t;
pub type clInitLayer_t = Option<
    unsafe extern "C" fn(
        num_entries: cl_uint,
        target_dispatch: *const cl_icd_dispatch,
        num_entries_ret: *mut cl_uint,
        layer_dispatch_ret: *mut *const cl_icd_dispatch,
    ) -> cl_int,
>;
pub type clInitLayer_fn = clInitLayer_t;

pub type pfn_clGetLayerInfo = clGetLayerInfo_t;
pub type pfn_clInitLayer = clInitLayer_t;

#[cfg_attr(not(target_os = "macos"), link(name = "OpenCL"))]
#[cfg_attr(target_os = "macos", link(name = "OpenCL", kind = "framework"))]
#[cfg(feature = "cl_loader_layers")]
#[cfg(feature = "static")]
unsafe extern "system" {
    pub fn clGetLayerInfo(
        param_name: cl_layer_info,
        param_value_size: size_t,
        param_value: *mut c_void,
        param_value_size_ret: *mut size_t,
    ) -> cl_int;

    pub fn clInitLayer(
        num_entries: cl_uint,
        target_dispatch: *const cl_icd_dispatch,
        num_entries_ret: *mut cl_uint,
        layer_dispatch_ret: *mut *const cl_icd_dispatch,
    ) -> cl_int;
}
