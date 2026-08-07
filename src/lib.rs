// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # Qubit Local Files
//!
//! Unified native local filesystem operations for Rust.
//!
//! [`LocalFileSystem`] provides direct Host access or descendant operations
//! anchored to an opened Rooted directory descriptor or handle.
//! [`LocalFileNames`] and [`LocalPaths`] provide native lexical utilities,
//! while readers, writers, walkers, and temporary resources retain explicit
//! ownership and lifecycle state.
//!
//! The former crate-root Host convenience functions were removed. Use
//! [`LocalFileSystem::host`] and its instance methods instead.
//!
//! ```compile_fail
//! use qubit_local_files::open_writer;
//! ```
mod capability;
mod error;
mod local;
mod local_file_kind;
mod local_file_metadata;
mod local_file_names;
mod local_file_reader;
mod local_file_system;
mod local_file_system_scope;
mod local_path_codec;
mod local_paths;
mod options;
mod outcome;
mod read;
mod rooted;
mod rooted_local_file_system;
mod temp;
mod walk;
mod write;
mod writer;

pub use capability::{
    LocalFileSystemCapabilities,
    LocalFileSystemLimits,
    LocalFileSystemSpace,
    SizeLimit,
};
pub use error::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileErrorSource,
    LocalFileOperation,
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
pub use local_file_system_scope::LocalFileSystemScope;
pub use local_path_codec::LocalPathCodec;
pub use local_paths::LocalPaths;
pub use options::{
    LocalAtomicityRequirement,
    LocalCopyOptions,
    LocalCopySourceMode,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalDirectoryReopenPolicy,
    LocalDurabilityRequirement,
    LocalListOptions,
    LocalMetadataPreservePolicy,
    LocalReadOptions,
    LocalRenameOptions,
    LocalSymlinkPolicy,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWalkErrorPolicy,
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
    LocalWritePublicationMethod,
};
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
    LocalWriteFailureState,
    LocalWriteOutcome,
    LocalWriterState,
};

pub(crate) use local::LocalRelativePath;

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
};
