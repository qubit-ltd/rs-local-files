// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;

/// Worker closure that executes a request inside its accepted operation scope.
///
/// # Type Parameters
///
/// - `R`: Owned request accepted by the worker.
/// - `T`: Successful operation result.
/// - `E`: Domain error returned by the operation.
pub(super) type OperationTask<R, T, E> =
    Box<dyn for<'scope> FnOnce(R, &'scope OperationScope) -> OperationOutcome<T, E> + Send + 'static>;
