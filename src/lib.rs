// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! # Qubit Local Files
//!
//! Unified native local filesystem operations for Rust.
//!
//! [`LocalFileSystem`] provides host-wide operations. [`RootedLocalFileSystem`]
//! anchors descendant operations to an opened directory descriptor or handle.
//! [`LocalFileNames`] and [`LocalPaths`] provide native lexical utilities,
//! while readers, writers, walkers, and temporary resources retain explicit
//! ownership and lifecycle state.
//!
//! Scattered legacy free-function namespaces are intentionally unavailable in
//! normal builds. Coverage instrumentation temporarily exposes them for the
//! existing implementation-level integration suite, so this doctest also
//! contains an invalid use that must fail under either configuration:
//!
//! ```compile_fail
//! use qubit_local_files::directory;
//! let _: () = directory;
//! ```
//!
//! Rooted authority is exposed only through the unified stateful type in
//! normal builds. Coverage instrumentation uses the same temporary visibility
//! for implementation tests:
//!
//! ```compile_fail
//! use qubit_local_files::rooted::Root;
//! let _: () = Root::open;
//! ```
//!
//! ```compile_fail
//! use qubit_local_files::backend::copy::file;
//! ```
//!
//! ```compile_fail
//! use qubit_local_files::LocalTempDir;
//! ```
//!
//! ```compile_fail
//! use qubit_local_files::temp::LocalTempDirectory;
//! ```

#[cfg(coverage)]
pub mod atomic;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod atomic;
mod capability;
#[cfg(coverage)]
pub mod copy;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod copy;
#[cfg(coverage)]
pub mod directory;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod directory;
mod error;
#[cfg(coverage)]
pub mod local;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod local;
mod local_file_kind;
mod local_file_metadata;
mod local_file_names;
mod local_file_reader;
mod local_file_system;
mod local_path_codec;
mod local_paths;
#[cfg(coverage)]
pub mod metadata;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod metadata;
mod options;
mod outcome;
#[cfg(coverage)]
pub mod path;
#[cfg(coverage)]
pub mod read;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod read;
#[cfg(coverage)]
pub mod remove;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod remove;
#[cfg(coverage)]
pub mod rename;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod rename;
#[cfg(coverage)]
pub mod rooted;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod rooted;
mod rooted_local_file_system;
mod temp;
mod walk;
#[cfg(coverage)]
pub mod write;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod write;
mod writer;

pub use capability::{
    LocalFileSystemCapabilities,
    LocalPathLengthUnit,
    LocalPathLimit,
};
pub use error::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileErrorSource,
    LocalFileOperation,
    LocalMutationState,
    LocalPathCodecError,
    LocalResult,
};
pub use local::{
    LocalCopyConflictPolicy,
    LocalCopyTypeConflictPolicy,
    LocalPersistError,
    LocalPersistFailureState,
    LocalPersistOptions,
    LocalPersistStage,
};
pub use local_file_kind::LocalFileKind;
pub use local_file_metadata::LocalFileMetadata;
pub use local_file_names::LocalFileNames;
pub use local_file_reader::LocalFileReader;
pub use local_file_system::LocalFileSystem;
pub use local_path_codec::LocalPathCodec;
pub use local_paths::LocalPaths;
pub use options::{
    LocalAtomicityRequirement,
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalDurabilityRequirement,
    LocalListOptions,
    LocalMetadataPreservePolicy,
    LocalReadOptions,
    LocalRenameOptions,
    LocalSymlinkPolicy,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};
pub use outcome::{
    LocalCopyFailure,
    LocalCopyFailureState,
    LocalCopyMethod,
    LocalCopyOutcome,
    LocalCopyResult,
    LocalCopyStats,
    LocalCreateDirectoryOutcome,
    LocalDeleteOutcome,
    LocalPersistMethod,
    LocalPersistOutcome,
    LocalRenameFailure,
    LocalRenameFailureState,
    LocalRenameOutcome,
    LocalRenameResult,
};
pub use rooted_local_file_system::RootedLocalFileSystem;
pub use temp::{
    LocalTempDirectory,
    LocalTempFile,
};
pub use walk::{
    LocalDirectoryEntry,
    LocalDirectoryWalker,
};
pub use writer::{
    LocalFileCommitError,
    LocalFileWriter,
    LocalWriteOutcome,
    LocalWriterState,
};

pub(crate) use local::{
    LocalAtomicCommitError,
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteOptions,
    LocalAtomicWriteStage,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalRelativePath,
};
