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
//! Host convenience functions provide direct process-wide operations, while
//! [`LocalFileSystem`] configures either Host access or descendant operations
//! anchored to an opened Rooted directory descriptor or handle.
//! [`LocalFileNames`] and [`LocalPaths`] provide native lexical utilities,
//! while readers, writers, walkers, and temporary resources retain explicit
//! ownership and lifecycle state.
mod capability;
mod error;
mod host;
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
    LocalPathLengthUnit,
    LocalPathLimit,
};
pub use error::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileErrorSource,
    LocalFileOperation,
    LocalPathCodecError,
    LocalResult,
};
pub use host::{
    copy,
    create_directory,
    create_temp_directory,
    create_temp_file,
    delete_directory,
    delete_file,
    list,
    metadata,
    open_reader,
    open_writer,
    rename,
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
