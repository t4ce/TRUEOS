// Copyright (c) 2022-2024 Via Technology Ltd.
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

//![![crates.io](https://img.shields.io/crates/v/opencl-sys.svg)](https://crates.io/crates/opencl-sys)
//! [![docs.io](https://docs.rs/opencl-sys/badge.svg)](https://docs.rs/opencl-sys/)
//! [![OpenCL 3.0](https://img.shields.io/badge/OpenCL-3.0-blue.svg)](https://www.khronos.org/registry/OpenCL/)
//! [![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
//! ![Rust](https://github.com/kenba/cl3/workflows/Rust/badge.svg)
//!
//! `OpenCL` C FFI bindings for the Rust programming language.
//!
//! # Description
//!
//! Rust [FFI](https://doc.rust-lang.org/nomicon/ffi.html) Bindings to the Khronos
//! [OpenCL C language headers](https://github.com/KhronosGroup/OpenCL-Headers),
//! see the [OpenCL Resource Guide](https://www.khronos.org/opencl/resources).
//!
//! The static API for `OpenCL` versions and extensions is controlled by Rust features
//! such as "`static`", "`CL_VERSION_2_0`" and "`cl_khr_gl_sharing`".  
//!
//! Rust deprecation warnings are given for `OpenCL` API functions that are
//! deprecated by an enabled `OpenCL` version e.g., `clCreateCommandQueue` is
//! deprecated whenever "`CL_VERSION_2_0`" is enabled.
//!
//! The Rust FFI files attempt to match the format and layout of the original
//! C source files instead of [bindgen](https://rust-lang.github.io/rust-bindgen/)
//! output files to ease maintenance.
//!
//! The library is declared [no_std](https://docs.rust-embedded.org/book/intro/no-std.html).
//!
//! ## Use
//!
//! `OpenCL` requires that an appropriate `OpenCL` hardware driver(s) and an `OpenCL` Installable Client Driver (ICD) are installed.\
//! `OpenCL` GPU hardware driver(s) are installed with graphics drivers by the main manufacturers: Nvidia, AMD and Intel.\
//! However, an `OpenCL` ICD must usually be installed manually by:
//!
//! - a Linux package manager, e.g., `sudo apt-get install intel-opencl-icd`
//! - or a Windows download and install from
//!   <https://www.intel.com/content/www/us/en/developer/articles/technical/intel-cpu-runtime-for-opencl-applications-with-sycl-support.html>
//!
//! Note: you do *not* need to install an `OpenCL` ICD from the same manufacturer as your hardware.\
//! In general the more up to date the `OpenCL` ICD, the better.
//!
//! ## Contribution
//!
//! If you want to contribute through code or documentation, the [Contributing](CONTRIBUTING.md)
//! guide is the best place to start.\
//! If you have any questions, please feel free to ask.
//! Just please abide by our [Code of Conduct](CODE_OF_CONDUCT.md).
//!
//! ## License
//!
//! Licensed under the Apache License, Version 2.0, as per Khronos Group `OpenCL`.\
//! You may obtain a copy of the License at: <http://www.apache.org/licenses/LICENSE-2.0>
//!
//! Unless you explicitly state otherwise, any contribution intentionally submitted
//! for inclusion in the work by you, as defined in the Apache-2.0 license,
//! shall be licensed as above, without any additional terms or conditions.
//!
//! `OpenCL` and the `OpenCL` logo are trademarks of Apple Inc. used under license by Khronos.

#![no_std]

extern crate libc;

// Corresponds to opencl.h
mod cl;
mod cl_ext;
// Note: The Intel extensions have been moved into cl_ext.h.
// pub mod cl_ext_intel
mod cl_gl;
mod cl_platform;
// Note: All OpenGL-related extensions have been moved into cl_gl.h.
// pub mod cl_gl_ext

pub use self::cl::*;
pub use self::cl_ext::*;
pub use self::cl_gl::*;
pub use self::cl_platform::*;

pub mod cl_d3d10;
pub mod cl_d3d11;
pub mod cl_dx9_media_sharing;
// Note: Intel DX9 media sharing extensions have been moved into cl_dx9_media_sharing.h.
// pub mod cl_dx9_media_sharing_intel
pub mod cl_egl;

pub mod cl_function_types;
pub mod cl_icd;
pub mod cl_layer;
