// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod local_operation;
pub use local_operation::LocalOperation;
#[cfg(feature = "async")]
mod async_local_operation;
#[cfg(feature = "async")]
pub use async_local_operation::AsyncLocalOperation;
mod local_read_request;
pub use local_read_request::LocalReadRequest;
mod local_operation_options;
pub use local_operation_options::LocalOperationOptions;
mod local_execution_config;
pub use local_execution_config::LocalExecutionConfig;
mod local_executor_config_error;
pub use local_executor_config_error::LocalExecutorConfigError;
mod local_executor_create_error;
pub use local_executor_create_error::LocalExecutorCreateError;
mod local_modified_range;
pub use local_modified_range::LocalModifiedRange;
mod local_publication_state;
pub use local_publication_state::LocalPublicationState;
mod local_durability_state;
pub use local_durability_state::LocalDurabilityState;
mod local_fact_snapshot;
pub use local_fact_snapshot::LocalFactSnapshot;
mod local_fact_observer;
pub use local_fact_observer::LocalFactObserver;
mod local_submit_rejection;
pub use local_submit_rejection::LocalSubmitRejection;
mod local_submit_error;
pub use local_submit_error::LocalSubmitError;

#[cfg(test)]
mod tests {
    use super::LocalSubmitError;
    use super::LocalSubmitRejection;

    /// Represents opaque request within the local operation subsystem.
    struct OpaqueRequest {
        /// Stores the identity associated with this value.
        identity: Box<()>,
    }

    /// Runs the test rejected request round trips the same non clone non debug
    /// value behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_rejected_request_round_trips_the_same_non_clone_non_debug_value() {
        let request = OpaqueRequest { identity: Box::new(()) };
        let identity = &*request.identity as *const ();
        let rejected = LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded);
        assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);
        let returned = rejected.into_request();

        assert_eq!(&*returned.identity as *const (), identity);
    }
}
