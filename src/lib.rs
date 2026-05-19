/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # Qubit Local FS
//!
//! Local filesystem utilities for Rust.
//!
//! This crate provides small, standard-library-first helpers for local paths,
//! file names, temporary files and directories, recursive directory operations,
//! and durable same-directory atomic writes.

mod local;

pub use local::{
    LocalCopyDirOptions,
    LocalCopyDirStats,
    LocalFilenames,
    LocalFiles,
    LocalTempDir,
    LocalTempFile,
};
