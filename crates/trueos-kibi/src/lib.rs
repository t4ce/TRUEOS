// SPDX-FileCopyrightText: 2020 Ilaï Deutel & Kibi Contributors
//
// SPDX-License-Identifier: MIT OR Apache-2.0

#![no_std]

//! # Kibi
//!
//! Kibi is a text editor in ≤1024 lines of code.

extern crate alloc;

pub use crate::{config::Config, editor::run, error::Error};

pub mod ansi_escape;
mod config;
mod editor;
mod error;
mod row;
mod syntax;
mod terminal;
