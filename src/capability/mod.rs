// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local filesystem capabilities and runtime observations.

mod filesystem_probe;
mod local_file_system_capabilities;
mod local_file_system_limits;
mod local_file_system_space;
mod local_path_length_unit;
mod size_limit;

pub(crate) use filesystem_probe::limits as probe_limits;
pub(crate) use filesystem_probe::space as probe_space;
pub use local_file_system_capabilities::LocalFileSystemCapabilities;
pub use local_file_system_limits::LocalFileSystemLimits;
pub use local_file_system_space::LocalFileSystemSpace;
pub use local_path_length_unit::LocalPathLengthUnit;
pub use size_limit::SizeLimit;
