// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # Qubit Local Files
//!
//! Local filesystem utilities for Rust.
//!
//! This crate provides small, standard-library-first helpers for local paths,
//! file names, temporary files and directories, recursive directory operations,
//! and durable same-directory atomic writes. Existing-file atomic replacement
//! uses strict platform-native metadata preservation and reports an explicit
//! post-failure destination state.
//!
//! Legacy root-level namespace APIs are intentionally unavailable:
//!
//! ```compile_fail
//! use qubit_local_files::LocalFiles;
//! ```
//!
//! The deprecated metadata compatibility alias is also unavailable:
//!
//! ```compile_fail
//! use qubit_local_files::metadata::read_link;
//! ```

pub mod atomic;
pub mod copy;
pub mod directory;
mod local;
pub mod metadata;
pub mod path;
pub mod read;
pub mod remove;
pub mod rename;
pub mod rooted;
pub mod temp;
pub mod write;

pub(crate) use local::{
    LocalAtomicCommitError,
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteOptions,
    LocalAtomicWriteStage,
    LocalCopyConflictPolicy,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalCopyTypeConflictPolicy,
    LocalPersistError,
    LocalPersistOptions,
    LocalPersistStage,
    LocalRelativePath,
};
