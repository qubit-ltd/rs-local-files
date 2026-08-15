//! Configured local filesystem construction and immutable limits.

mod local_copy_limits;
mod local_file_system_builder;
mod local_file_system_core;
mod local_walk_limits;

#[cfg(all(test, feature = "test-support"))]
mod local_file_system_core_tests;

pub use local_copy_limits::LocalCopyLimits;
pub use local_file_system_builder::LocalFileSystemBuilder;
pub(crate) use local_file_system_core::LocalFileSystemCore;
pub use local_walk_limits::LocalWalkLimits;
