//! Public contracts for managed local filesystem operations.
//!
//! The operation dependency is part of the common contract. The optional
//! `async` feature gates the local execution facade, not these value types.

mod types;

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
