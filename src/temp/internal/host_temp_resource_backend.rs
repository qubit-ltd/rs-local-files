// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Temporary-resource behavior is covered through public integration tests.
//! Host-bound temporary-resource storage.

/// Marks a temporary resource whose path is already bound to the host
/// namespace.
#[derive(Debug)]
pub(crate) struct HostTempResourceBackend;
