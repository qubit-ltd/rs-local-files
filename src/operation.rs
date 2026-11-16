//! Managed local operation contracts and asynchronous execution support.

#[cfg(feature = "async")]
mod async_local_file_system;
#[cfg(feature = "async")]
mod executor;
#[cfg(feature = "async")]
mod internal;
mod types;

#[cfg(feature = "async")]
pub use async_local_file_system::AsyncLocalFileSystem;
#[cfg(feature = "async")]
pub use executor::LocalIoExecutor;
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
