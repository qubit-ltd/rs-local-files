//! Managed local operation contracts and asynchronous execution support.

#[cfg(feature = "async")]
mod async_local_file_system;
#[cfg(feature = "async")]
pub mod complete;
#[cfg(feature = "async")]
mod executor;
#[cfg(feature = "async")]
mod internal;
#[cfg(feature = "async")]
pub mod stream;
#[cfg(feature = "async")]
pub mod temp;
mod types;

#[cfg(feature = "async")]
pub use async_local_file_system::AsyncLocalFileSystem;
#[cfg(feature = "async")]
pub use complete::{
    LocalCopyOperationResult, LocalCopyRequest, LocalCreateDirectoryOperationResult,
    LocalCreateDirectoryRequest, LocalDeleteOperationResult, LocalDeleteRequest,
    LocalReadAtRequest, LocalReadPrefix, LocalReadPrefixRequest, LocalRenameOperationResult,
    LocalRenameRequest,
};
#[cfg(feature = "async")]
pub use executor::LocalIoExecutor;
#[cfg(feature = "async")]
pub use stream::{
    AsyncLocalDirectoryWalker, AsyncLocalFileReader, AsyncLocalFileWriter,
    LOCAL_STREAM_BUFFER_SIZE, LOCAL_WALK_BATCH_BYTES, LOCAL_WALK_BATCH_ENTRIES, LocalStreamCommand,
    LocalStreamCompletion, LocalStreamDriver, LocalStreamRequest, LocalStreamResource,
    LocalWalkCloseObserver, LocalWalkDriver, LocalWalkResource,
};
#[cfg(feature = "async")]
pub use temp::{
    LocalCreateFailure, LocalTempCreateOperation, LocalTempFileAccess, LocalTempResource,
    LocalTempSubmitError, submit_temp_directory, submit_temp_file,
};
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
