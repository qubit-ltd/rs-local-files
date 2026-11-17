// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// `LocalFileError` intentionally retains structured paths, source errors,
// cleanup errors, and recoverable temporary ownership. Boxing the public
// result solely for this lint would change its stable error representation.
#![allow(clippy::result_large_err)]

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
//! Namespace binding is an internal operation; callers pass native
//! [`std::path::Path`] values to [`LocalFileSystem`] methods.
//!
//! ```compile_fail
//! use qubit_local_files::path::LocalNamespacePath;
//! ```
//!
//! ```compile_fail
//! use qubit_local_files::path::LocalPathResolver;
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
#![cfg_attr(
    not(feature = "digest"),
    doc = r#"
```compile_fail
use qubit_local_files::LocalConditionalWriter;
```
"#
)]
pub mod capability;
// Compiles the exact README, user-guide, and design Rust examples as doctests.
pub mod directory;
#[cfg(doctest)]
mod documentation_tests;
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
pub mod observation;
pub mod operation;
pub mod options;
pub use observation::LocalFileSnapshot;
#[cfg(feature = "async")]
pub use operation::AsyncLocalDirectoryWalker;
#[cfg(feature = "async")]
pub use operation::AsyncLocalFileReader;
#[cfg(feature = "async")]
pub use operation::AsyncLocalFileSystem;
#[cfg(feature = "async")]
pub use operation::AsyncLocalFileWriter;
#[cfg(feature = "async")]
pub use operation::AsyncLocalOperation;
#[cfg(feature = "async")]
pub use operation::LOCAL_STAGE_STREAM_CHUNK_SIZE;
#[cfg(feature = "async")]
pub use operation::LOCAL_STREAM_BUFFER_SIZE;
#[cfg(feature = "async")]
pub use operation::LOCAL_WALK_BATCH_BYTES;
#[cfg(feature = "async")]
pub use operation::LOCAL_WALK_BATCH_ENTRIES;
#[cfg(all(feature = "async", feature = "digest"))]
pub use operation::LocalConditionalWriterCommandError;
#[cfg(all(feature = "async", feature = "digest"))]
pub use operation::LocalConditionalWriterCommitRequest;
#[cfg(feature = "async")]
pub use operation::LocalCopyOperationResult;
#[cfg(feature = "async")]
pub use operation::LocalCopyRequest;
#[cfg(feature = "async")]
pub use operation::LocalCreateDirectoryOperationResult;
#[cfg(feature = "async")]
pub use operation::LocalCreateDirectoryRequest;
#[cfg(feature = "async")]
pub use operation::LocalDeleteOperationResult;
#[cfg(feature = "async")]
pub use operation::LocalDeleteRequest;
pub use operation::LocalDurabilityState;
pub use operation::LocalExecutionConfig;
pub use operation::LocalExecutorConfigError;
pub use operation::LocalExecutorCreateError;
pub use operation::LocalFactObserver;
pub use operation::LocalFactSnapshot;
#[cfg(feature = "async")]
pub use operation::LocalIoExecutor;
pub use operation::LocalModifiedRange;
pub use operation::LocalOperation;
pub use operation::LocalOperationOptions;
pub use operation::LocalPublicationState;
#[cfg(feature = "async")]
pub use operation::LocalReadAtRequest;
#[cfg(feature = "async")]
pub use operation::LocalReadExport;
#[cfg(feature = "async")]
pub use operation::LocalReadLease;
#[cfg(feature = "async")]
pub use operation::LocalReadPrefix;
#[cfg(feature = "async")]
pub use operation::LocalReadPrefixRequest;
pub use operation::LocalReadRequest;
#[cfg(feature = "async")]
pub use operation::LocalReadResource;
#[cfg(feature = "async")]
pub use operation::LocalRenameOperationResult;
#[cfg(feature = "async")]
pub use operation::LocalRenameRequest;
#[cfg(feature = "async")]
pub use operation::LocalStageFailure;
#[cfg(feature = "async")]
pub use operation::LocalStageOperation;
#[cfg(feature = "async")]
pub use operation::LocalStageOptions;
#[cfg(feature = "async")]
pub use operation::LocalStageStreamSubmitError;
#[cfg(feature = "async")]
pub use operation::LocalStageSubmitError;
#[cfg(feature = "async")]
pub use operation::LocalStagedTreeDiskReservation;
#[cfg(feature = "async")]
pub use operation::LocalStagedTreeFile;
#[cfg(feature = "async")]
pub use operation::LocalStagedTreePublishError;
#[cfg(feature = "async")]
pub use operation::LocalStagedTreePublishRequest;
#[cfg(feature = "async")]
pub use operation::LocalStagedTreeRecovery;
#[cfg(feature = "async")]
pub use operation::LocalStagedTreeReservedError;
#[cfg(feature = "async")]
pub use operation::LocalStreamCommand;
#[cfg(feature = "async")]
pub use operation::LocalStreamCompletion;
#[cfg(feature = "async")]
pub use operation::LocalStreamDriver;
#[cfg(feature = "async")]
pub use operation::LocalStreamRequest;
#[cfg(feature = "async")]
pub use operation::LocalStreamResource;
pub use operation::LocalSubmitError;
pub use operation::LocalSubmitRejection;
#[cfg(feature = "async")]
pub use operation::LocalTempCreateOperation;
#[cfg(feature = "async")]
pub use operation::LocalTempFileAccess;
#[cfg(feature = "async")]
pub use operation::LocalTempLifecycleCommand;
#[cfg(feature = "async")]
pub use operation::LocalTempLifecycleError;
#[cfg(feature = "async")]
pub use operation::LocalTempLifecycleOperation;
#[cfg(feature = "async")]
pub use operation::LocalTempLifecycleRequest;
#[cfg(feature = "async")]
pub use operation::LocalTempLifecycleResult;
#[cfg(feature = "async")]
pub use operation::LocalTempPersistFailure;
#[cfg(feature = "async")]
pub use operation::LocalTempResource;
#[cfg(feature = "async")]
pub use operation::LocalTempSubmitError;
#[cfg(feature = "async")]
pub use operation::LocalTempWorkerCommandFailure;
#[cfg(feature = "async")]
pub use operation::LocalWalkCloseObserver;
#[cfg(feature = "async")]
pub use operation::LocalWalkDriver;
#[cfg(feature = "async")]
pub use operation::LocalWalkResource;
#[cfg(feature = "async")]
pub use operation::LocalWriterCommandError;
#[cfg(feature = "async")]
pub use operation::LocalWriterCommitRequest;
#[cfg(feature = "async")]
pub use operation::stage_bytes;
#[cfg(feature = "async")]
pub use operation::stage_stream;
#[cfg(feature = "async")]
pub use operation::submit_temp_directory;
#[cfg(feature = "async")]
pub use operation::submit_temp_file;
pub mod outcome;
pub mod path;
pub mod policy;
mod read;
#[cfg(feature = "digest")]
pub mod recovery;
pub(crate) mod resolution;
mod rooted;
mod rooted_local_file_system;
mod temp;
#[cfg(not(feature = "test-support"))]
mod test_support;
#[cfg(feature = "test-support")]
pub mod test_support;
#[cfg(test)]
mod tests;
mod vectored;
mod walk;
mod write;
mod writer;

pub(crate) use capability::LocalFileSystemCapabilities;
pub(crate) use capability::LocalFileSystemLimits;
pub(crate) use capability::LocalFileSystemSpace;
pub(crate) use capability::LocalPathLengthUnit;
pub(crate) use capability::SizeLimit;
pub use directory::LocalTreeAccessError;
pub use directory::LocalTreePublishError;
pub use error::LocalFileError;
pub(crate) use error::LocalFileErrorKind;
pub(crate) use error::LocalFileOperation;
pub(crate) use error::LocalPathCodecError;
pub(crate) use error::LocalResourceKind;
pub(crate) use error::LocalResourceLimitError;
pub use error::LocalResult;
pub use error::LocalTempSealError;
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
pub use options::LocalSealOptions;
pub(crate) use options::LocalTempDirectoryOptions;
pub(crate) use options::LocalTempFileOptions;
pub use options::LocalTreePublishOptions;
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
pub use outcome::LocalTempAccessState;
pub use outcome::LocalTreeAccessOutcome;
pub use outcome::LocalTreeAccessState;
pub use outcome::LocalTreeMutationState;
pub use outcome::LocalTreePublication;
pub use outcome::LocalTreePublishOutcome;
pub(crate) use outcome::LocalWritePublicationMethod;
pub(crate) use path::LocalFileNames;
pub(crate) use path::LocalNamespacePath;
pub(crate) use path::LocalPathCodec;
pub(crate) use policy::LocalAtomicityRequirement;
pub(crate) use policy::LocalDurabilityRequirement;
pub(crate) use policy::LocalSymlinkPolicy;
pub use temp::LocalSealedTempFile;
pub use temp::LocalTempDirectory;
pub use temp::LocalTempFile;
pub use temp::LocalTempRecovery;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub(crate) use test_support::TestFaultPlan;
pub(crate) use walk::LocalDirectoryEntry;
pub use walk::LocalDirectoryWalker;
#[cfg(feature = "digest")]
pub use writer::LocalConditionalWriter;
pub(crate) use writer::LocalFileCommitError;
pub use writer::LocalFileWriter;
pub(crate) use writer::LocalWriteFailureState;
pub use writer::LocalWriteOutcome;
pub(crate) use writer::LocalWriterState;
