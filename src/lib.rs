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

pub use capability::LocalFileSystemLimits;
pub use capability::LocalFileSystemProtocols;
pub use capability::LocalFileSystemSpace;
pub use capability::SizeLimit;
pub use error::LocalFileError;
pub use error::LocalFileErrorKind;
pub use error::LocalFileErrorSource;
pub use error::LocalFileOperation;
pub use error::LocalPathCodecError;
pub use error::LocalResult;
pub(crate) use local::LocalAtomicCommitError;
pub(crate) use local::LocalAtomicDestinationState;
pub(crate) use local::LocalAtomicWriteError;
pub(crate) use local::LocalAtomicWriteOptions;
pub(crate) use local::LocalAtomicWriteStage;
pub use local::LocalCopyConflictPolicy;
pub(crate) use local::LocalCopyDirError;
pub(crate) use local::LocalCopyDirOptions;
pub(crate) use local::LocalCopyDirStage;
pub(crate) use local::LocalCopyDirStats;
pub use local::LocalCopyTypeConflictPolicy;
pub use local::LocalPersistError;
pub use local::LocalPersistFailureState;
pub use local::LocalPersistOptions;
pub use local::LocalPersistStage;
pub(crate) use local::LocalRelativePath;
#[cfg(feature = "internal-test-support")]
#[doc(hidden)]
pub use local::TestFaultGuard;
#[cfg(feature = "internal-test-support")]
#[doc(hidden)]
pub use local::install_test_fault;
pub use local_file_kind::LocalFileKind;
pub use local_file_metadata::LocalFileMetadata;
pub use local_file_names::LocalFileNames;
pub use local_file_reader::LocalFileReader;
pub use local_file_system::LocalFileSystem;
pub use local_file_system_scope::LocalFileSystemScope;
pub use local_path_codec::LocalPathCodec;
pub use local_paths::LocalPaths;
pub use options::LocalAtomicityRequirement;
pub use options::LocalCopyOptions;
pub use options::LocalCopySourceMode;
pub use options::LocalCreateDirectoryOptions;
pub use options::LocalDeleteOptions;
pub use options::LocalDirectoryReopenPolicy;
pub use options::LocalDurabilityRequirement;
pub use options::LocalListOptions;
pub use options::LocalMetadataPreservePolicy;
pub use options::LocalReadOptions;
pub use options::LocalRenameOptions;
pub use options::LocalSymlinkPolicy;
pub use options::LocalTempDirectoryOptions;
pub use options::LocalTempFileOptions;
pub use options::LocalWalkErrorPolicy;
pub use options::LocalWriteMode;
pub use options::LocalWriteOptions;
pub use outcome::LocalCopyFailure;
pub use outcome::LocalCopyFailureState;
pub use outcome::LocalCopyMethod;
pub use outcome::LocalCopyOutcome;
pub use outcome::LocalCopyResult;
pub use outcome::LocalCopyStats;
pub use outcome::LocalCreateDirectoryOutcome;
pub use outcome::LocalDeleteOutcome;
pub use outcome::LocalPersistCleanupState;
pub use outcome::LocalPersistMethod;
pub use outcome::LocalPersistOutcome;
pub use outcome::LocalRenameFailure;
pub use outcome::LocalRenameFailureState;
pub use outcome::LocalRenameOutcome;
pub use outcome::LocalRenameResult;
pub use outcome::LocalWritePublicationMethod;
pub use temp::LocalTempDirectory;
pub use temp::LocalTempFile;
pub use walk::LocalDirectoryEntry;
pub use walk::LocalDirectoryWalker;
pub use writer::LocalFileCommitError;
pub use writer::LocalFileWriter;
pub use writer::LocalWriteFailureState;
pub use writer::LocalWriteOutcome;
pub use writer::LocalWriterState;
