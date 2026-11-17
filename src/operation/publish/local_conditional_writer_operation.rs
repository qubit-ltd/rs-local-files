// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalConditionalWriterCommandError;
use super::LocalConditionalWriterCommitRequest;
use crate::operation::LocalFactObserver;
use crate::operation::LocalOperation;
use crate::operation::LocalSubmitError;
use crate::outcome::LocalConditionalWriteOutcome;

/// Result of submitting a digest-preconditioned local writer commit.
///
/// # Type Parameters
///
/// - `D`: Digest type used for the conditional comparison and outcome.
pub type LocalConditionalWriterOperation<D> = Result<
    (
        LocalOperation<LocalConditionalWriteOutcome<D>, LocalConditionalWriterCommandError<D>>,
        LocalFactObserver,
    ),
    LocalSubmitError<LocalConditionalWriterCommitRequest<D>>,
>;
