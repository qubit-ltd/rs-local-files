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
//! [`path::LocalFileNames`] and [`path::LocalPaths`] provide native lexical
//! utilities, while readers, writers, walkers, and temporary resources retain
//! explicit ownership and lifecycle state.
//!
//! The former crate-root Host convenience functions were removed. Use
//! [`LocalFileSystem::host`] and its instance methods instead.
//!
//! ```compile_fail
//! use qubit_local_files::open_writer;
//! ```
//!
//! Domain value types are available from their stable modules rather than the
//! crate root.
//!
//! ```compile_fail
//! use qubit_local_files::LocalCopyOptions;
//! ```
//!
//! ```compile_fail
//! use qubit_local_files::LocalFileSystemProtocols;
//! ```
pub mod capability;
pub mod error;
mod file_system;
mod local;
mod local_file_kind;
mod local_file_metadata;
mod local_file_permissions;
mod local_file_reader;
mod local_file_system;
mod local_file_system_scope;
mod local_file_system_validation;
pub mod options;
pub mod outcome;
pub mod path;
pub mod policy;
mod read;
mod rooted;
mod rooted_local_file_system;
mod temp;
#[cfg(not(feature = "test-support"))]
mod test_support;
#[cfg(feature = "test-support")]
pub mod test_support;
mod walk;
mod write;
mod writer;

pub(crate) use capability::LocalFileSystemCapabilities;
pub(crate) use capability::LocalFileSystemLimits;
pub(crate) use capability::LocalFileSystemSpace;
pub(crate) use capability::LocalPathLengthUnit;
pub(crate) use capability::SizeLimit;
pub use error::LocalFileError;
pub(crate) use error::LocalFileErrorKind;
pub(crate) use error::LocalFileOperation;
pub(crate) use error::LocalPathCodecError;
pub(crate) use error::LocalResourceKind;
pub(crate) use error::LocalResourceLimitError;
pub use error::LocalResult;
pub(crate) use local::LocalAtomicCommitError;
pub(crate) use local::LocalAtomicDestinationState;
pub(crate) use local::LocalAtomicWriteError;
pub(crate) use local::LocalAtomicWriteOptions;
pub(crate) use local::LocalAtomicWriteStage;
pub(crate) use local::LocalCopyConflictPolicy;
pub(crate) use local::LocalCopyDirError;
pub(crate) use local::LocalCopyDirOptions;
pub(crate) use local::LocalCopyDirStage;
pub(crate) use local::LocalCopyDirStats;
pub(crate) use local::LocalCopyTypeConflictPolicy;
pub(crate) use local::LocalPersistError;
pub(crate) use local::LocalPersistFailureState;
pub(crate) use local::LocalPersistOptions;
pub(crate) use local::LocalPersistStage;
pub(crate) use local::LocalRelativePath;
pub(crate) use local_file_kind::LocalFileKind;
pub(crate) use local_file_metadata::LocalFileMetadata;
pub(crate) use local_file_permissions::LocalFilePermissions;
pub use local_file_reader::LocalFileReader;
pub use local_file_system::LocalFileSystem;
pub(crate) use local_file_system_scope::LocalFileSystemScope;
pub(crate) use options::LocalCopyOptions;
pub(crate) use options::LocalCopySourceMode;
pub(crate) use options::LocalCreateDirectoryOptions;
pub(crate) use options::LocalDeleteOptions;
pub(crate) use options::LocalDirectoryReopenPolicy;
pub(crate) use options::LocalListOptions;
pub(crate) use options::LocalMetadataPreservePolicy;
pub(crate) use options::LocalReadOptions;
pub(crate) use options::LocalRenameOptions;
pub(crate) use options::LocalTempDirectoryOptions;
pub(crate) use options::LocalTempFileOptions;
pub(crate) use options::LocalWalkErrorPolicy;
pub(crate) use options::LocalWriteMode;
pub(crate) use options::LocalWriteOptions;
pub(crate) use outcome::LocalCopyFailure;
pub(crate) use outcome::LocalCopyFailureState;
pub(crate) use outcome::LocalCopyMethod;
pub(crate) use outcome::LocalCopyOutcome;
pub(crate) use outcome::LocalCopyResult;
pub(crate) use outcome::LocalCopyStats;
pub(crate) use outcome::LocalCreateDirectoryOutcome;
pub(crate) use outcome::LocalDeleteOutcome;
pub(crate) use outcome::LocalPersistCleanupState;
pub(crate) use outcome::LocalPersistMethod;
pub(crate) use outcome::LocalPersistOutcome;
pub(crate) use outcome::LocalRenameFailure;
pub(crate) use outcome::LocalRenameFailureState;
pub(crate) use outcome::LocalRenameOutcome;
pub(crate) use outcome::LocalRenameResult;
pub(crate) use outcome::LocalWritePublicationMethod;
pub(crate) use path::LocalFileNames;
pub(crate) use path::LocalNamespacePath;
pub(crate) use path::LocalPathCodec;
pub(crate) use policy::LocalAtomicityRequirement;
pub(crate) use policy::LocalDurabilityRequirement;
pub(crate) use policy::LocalSymlinkPolicy;
pub use temp::LocalTempDirectory;
pub use temp::LocalTempFile;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub(crate) use test_support::TestFaultPlan;
pub(crate) use walk::LocalDirectoryEntry;
pub use walk::LocalDirectoryWalker;
pub(crate) use writer::LocalFileCommitError;
pub use writer::LocalFileWriter;
pub(crate) use writer::LocalWriteFailureState;
pub(crate) use writer::LocalWriteOutcome;
pub(crate) use writer::LocalWriterState;
