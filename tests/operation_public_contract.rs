use std::sync::Arc;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalExecutorCreateError;
use qubit_local_files::LocalFactObserver;
use qubit_local_files::LocalFactSnapshot;
use qubit_local_files::LocalOperation;
use qubit_local_files::LocalOperationOptions;
use qubit_local_files::LocalReadRequest;
use qubit_local_files::LocalSubmitError;
use qubit_local_files::LocalSubmitRejection;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::PreparedExternalOperation;

struct OpaqueRequest;

fn assert_send_sync_static<T: Send + Sync + 'static>() {}
fn assert_copy<T: Copy>() {}

#[test]
fn lf1_types_are_available_from_the_crate_root() {
    assert_send_sync_static::<LocalFactObserver>();
    assert_send_sync_static::<LocalFactSnapshot>();
    assert_send_sync_static::<LocalExecutionConfig>();
    assert_send_sync_static::<LocalExecutorCreateError>();
    assert_send_sync_static::<LocalOperationOptions>();
    assert_send_sync_static::<LocalReadRequest>();
    assert_copy::<LocalFactSnapshot>();
    let _: Option<LocalOperation<Vec<u8>>> = None;
    let _: Option<LocalSubmitRejection> = None;
}

#[test]
fn rejected_request_type_accepts_an_opaque_non_clone_non_debug_payload() {
    fn recover(error: LocalSubmitError<OpaqueRequest>) -> OpaqueRequest {
        error.into_request()
    }

    let _recover = recover;
}

// This generic external test compiles against only the dependency's public
// crate root. It deliberately does not import downstream-private adapters.
#[allow(dead_code)]
fn submit_prepared<T, E, P>(manager: &OperationManager, prepared: Arc<P>)
where
    T: Send + 'static,
    E: Send + 'static,
    P: PreparedExternalOperation<T, E>,
{
    let _ = manager.try_submit_external(OperationOptions::default(), prepared);
}
