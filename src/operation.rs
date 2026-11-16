// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Managed local operation contracts and asynchronous execution support.

#[cfg(feature = "async")]
mod async_local_file_system;
#[cfg(feature = "async")]
pub mod complete;
#[cfg(feature = "async")]
mod executor;
#[cfg(feature = "async")]
pub mod handoff;
#[cfg(feature = "async")]
mod internal;
#[cfg(feature = "async")]
pub mod publish;
#[cfg(feature = "async")]
pub mod stream;
#[cfg(feature = "async")]
pub mod temp;
mod types;

#[cfg(feature = "async")]
pub use async_local_file_system::AsyncLocalFileSystem;
#[cfg(feature = "async")]
pub use complete::LocalCopyOperationResult;
#[cfg(feature = "async")]
pub use complete::LocalCopyRequest;
#[cfg(feature = "async")]
pub use complete::LocalCreateDirectoryOperationResult;
#[cfg(feature = "async")]
pub use complete::LocalCreateDirectoryRequest;
#[cfg(feature = "async")]
pub use complete::LocalDeleteOperationResult;
#[cfg(feature = "async")]
pub use complete::LocalDeleteRequest;
#[cfg(feature = "async")]
pub use complete::LocalReadAtRequest;
#[cfg(feature = "async")]
pub use complete::LocalReadPrefix;
#[cfg(feature = "async")]
pub use complete::LocalReadPrefixRequest;
#[cfg(feature = "async")]
pub use complete::LocalRenameOperationResult;
#[cfg(feature = "async")]
pub use complete::LocalRenameRequest;
#[cfg(feature = "async")]
pub use executor::LocalIoExecutor;
#[cfg(feature = "async")]
pub use handoff::LOCAL_STAGE_STREAM_CHUNK_SIZE;
#[cfg(feature = "async")]
pub use handoff::LocalReadExport;
#[cfg(feature = "async")]
pub use handoff::LocalReadLease;
#[cfg(feature = "async")]
pub use handoff::LocalReadResource;
#[cfg(feature = "async")]
pub use handoff::LocalStageFailure;
#[cfg(feature = "async")]
pub use handoff::LocalStageOperation;
#[cfg(feature = "async")]
pub use handoff::LocalStageOptions;
#[cfg(feature = "async")]
pub use handoff::LocalStageStreamSubmitError;
#[cfg(feature = "async")]
pub use handoff::LocalStageSubmitError;
#[cfg(feature = "async")]
pub use handoff::stage_bytes;
#[cfg(feature = "async")]
pub use handoff::stage_stream;
#[cfg(all(feature = "async", feature = "digest"))]
pub use publish::LocalConditionalWriterCommandError;
#[cfg(all(feature = "async", feature = "digest"))]
pub use publish::LocalConditionalWriterCommitRequest;
#[cfg(feature = "async")]
pub use publish::LocalStagedTreeDiskReservation;
#[cfg(feature = "async")]
pub use publish::LocalStagedTreeFile;
#[cfg(feature = "async")]
pub use publish::LocalStagedTreePublishError;
#[cfg(feature = "async")]
pub use publish::LocalStagedTreePublishRequest;
#[cfg(feature = "async")]
pub use publish::LocalStagedTreeRecovery;
#[cfg(feature = "async")]
pub use publish::LocalStagedTreeReservedError;
#[cfg(feature = "async")]
pub use publish::LocalTempLifecycleCommand;
#[cfg(feature = "async")]
pub use publish::LocalTempLifecycleError;
#[cfg(feature = "async")]
pub use publish::LocalTempLifecycleOperation;
#[cfg(feature = "async")]
pub use publish::LocalTempLifecycleRequest;
#[cfg(feature = "async")]
pub use publish::LocalTempLifecycleResult;
#[cfg(feature = "async")]
pub use publish::LocalTempPersistFailure;
#[cfg(feature = "async")]
pub use publish::LocalTempWorkerCommandFailure;
#[cfg(feature = "async")]
pub use publish::LocalWriterCommandError;
#[cfg(feature = "async")]
pub use publish::LocalWriterCommitRequest;
#[cfg(feature = "async")]
pub use stream::AsyncLocalDirectoryWalker;
#[cfg(feature = "async")]
pub use stream::AsyncLocalFileReader;
#[cfg(feature = "async")]
pub use stream::AsyncLocalFileWriter;
#[cfg(feature = "async")]
pub use stream::LOCAL_STREAM_BUFFER_SIZE;
#[cfg(feature = "async")]
pub use stream::LOCAL_WALK_BATCH_BYTES;
#[cfg(feature = "async")]
pub use stream::LOCAL_WALK_BATCH_ENTRIES;
#[cfg(feature = "async")]
pub use stream::LocalStreamCommand;
#[cfg(feature = "async")]
pub use stream::LocalStreamCompletion;
#[cfg(feature = "async")]
pub use stream::LocalStreamDriver;
#[cfg(feature = "async")]
pub use stream::LocalStreamRequest;
#[cfg(feature = "async")]
pub use stream::LocalStreamResource;
#[cfg(feature = "async")]
pub use stream::LocalWalkCloseObserver;
#[cfg(feature = "async")]
pub use stream::LocalWalkDriver;
#[cfg(feature = "async")]
pub use stream::LocalWalkResource;
#[cfg(feature = "async")]
pub use temp::LocalCreateFailure;
#[cfg(feature = "async")]
pub use temp::LocalTempCreateOperation;
#[cfg(feature = "async")]
pub use temp::LocalTempFileAccess;
#[cfg(feature = "async")]
pub use temp::LocalTempResource;
#[cfg(feature = "async")]
pub use temp::LocalTempSubmitError;
#[cfg(feature = "async")]
pub use temp::submit_temp_directory;
#[cfg(feature = "async")]
pub use temp::submit_temp_file;
#[cfg(feature = "async")]
pub use types::AsyncLocalOperation;
pub use types::LocalDurabilityState;
pub use types::LocalExecutionConfig;
pub use types::LocalExecutorConfigError;
pub use types::LocalExecutorCreateError;
pub use types::LocalFactObserver;
pub use types::LocalFactSnapshot;
pub use types::LocalModifiedRange;
pub use types::LocalOperation;
pub use types::LocalOperationOptions;
pub use types::LocalPublicationState;
pub use types::LocalReadRequest;
pub use types::LocalSubmitError;
pub use types::LocalSubmitRejection;
