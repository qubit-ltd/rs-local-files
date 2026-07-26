// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Focused public API aliases used by detailed integration tests.

#[cfg(coverage)]
pub(super) use qubit_local_files::atomic::Error as LocalAtomicWriteError;
pub(super) use qubit_local_files::atomic::{
    DestinationState as LocalAtomicDestinationState,
    Options as LocalAtomicWriteOptions,
    Stage as LocalAtomicWriteStage,
    Writer as LocalAtomicWriter,
};
pub(super) use qubit_local_files::copy::{
    ConflictPolicy as LocalCopyConflictPolicy,
    Options as LocalCopyDirOptions,
    Stage as LocalCopyDirStage,
    Statistics as LocalCopyDirStats,
    TypeConflictPolicy as LocalCopyTypeConflictPolicy,
};
pub(super) use qubit_local_files::rooted::Path as LocalRelativePath;
#[cfg(unix)]
pub(super) use qubit_local_files::rooted::Root as LocalRoot;
pub(super) use qubit_local_files::temp::{
    PersistOptions as LocalPersistOptions,
    PersistStage as LocalPersistStage,
    TempDir as LocalTempDir,
    TempFile as LocalTempFile,
};
