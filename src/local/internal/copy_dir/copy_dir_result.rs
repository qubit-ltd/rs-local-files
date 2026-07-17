// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Private result type for recursive directory copies.

use crate::LocalCopyDirError;

/// Result returned by recursive directory-copy internals.
pub(super) type CopyDirResult<T> = std::result::Result<T, LocalCopyDirError>;
