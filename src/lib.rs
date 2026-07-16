// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! # Qubit Local Files
//!
//! Local filesystem utilities for Rust.
//!
//! This crate provides small, standard-library-first helpers for local paths,
//! file names, temporary files and directories, recursive directory operations,
//! and durable same-directory atomic writes.

mod local;

pub use local::{
    FileBuffering,
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
    LocalAtomicWriter,
    LocalCopyConflictPolicy,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalCopyTypeConflictPolicy,
    LocalFileReader,
    LocalFileWriter,
    LocalFilenames,
    LocalFiles,
    LocalPersistError,
    LocalPersistOptions,
    LocalRelativePath,
    LocalRoot,
    LocalRootAtomicWriter,
    LocalTempDir,
    LocalTempFile,
};
