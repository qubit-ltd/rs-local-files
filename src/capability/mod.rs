// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local filesystem capability snapshots.

mod filesystem_probe;
mod local_file_system_capabilities;
mod local_file_system_capability_support;
mod local_file_system_limits;
mod local_file_system_space;
mod size_limit;

pub use local_file_system_capabilities::LocalFileSystemCapabilities;
pub use local_file_system_capability_support::LocalFileSystemCapabilitySupport;
pub use local_file_system_limits::LocalFileSystemLimits;
pub use local_file_system_space::LocalFileSystemSpace;
pub use size_limit::SizeLimit;

pub(crate) use filesystem_probe::{
    limits as probe_limits,
    space as probe_space,
};
