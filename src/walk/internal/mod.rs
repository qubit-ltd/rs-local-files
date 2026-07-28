// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private state for lazy host directory traversal.

mod rooted_walk_frame;
mod rooted_walk_state;
mod walk_frame;

pub(super) use rooted_walk_frame::RootedWalkFrame;
pub(super) use rooted_walk_state::RootedWalkState;
pub(super) use walk_frame::WalkFrame;
