// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Worker queue implementation used by the local I/O executor.

// Keeps queue ownership, scheduling, and cleanup capacity in one type module.
mod worker_queue;

pub(crate) use worker_queue::WorkerQueue;
