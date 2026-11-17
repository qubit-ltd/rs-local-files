// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Defines the stream's read or write direction.
#[path = "stream_state/direction.rs"]
mod direction;
// Retains an in-flight seek until its worker result is observed.
#[path = "stream_state/pending_seek.rs"]
mod pending_seek;
// Owns accepted bytes until the worker confirms a write.
#[path = "stream_state/pending_write.rs"]
mod pending_write;
// Preserves an asynchronous I/O error across later operations.
#[path = "stream_state/saved_error.rs"]
mod saved_error;
// Coordinates read, write, seek, flush, and close state.
#[path = "stream_state/stream_state.rs"]
mod state;
// Retains a directory walk's pending batch and overflow entry.
#[path = "stream_state/walk_state.rs"]
mod walk_state;
// Reports the configured limit for one directory-walker batch.
#[path = "stream_state/walker_resource_limit.rs"]
mod walker_resource_limit;

pub(super) use direction::Direction;
use pending_seek::PendingSeek;
use pending_write::PendingWrite;
use saved_error::SavedError;
pub(super) use state::StreamState;
pub(super) use walk_state::WalkState;
pub(super) use walker_resource_limit::WalkerResourceLimit;
pub(super) use walker_resource_limit::resource_limit;
