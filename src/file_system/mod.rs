//! Shared local filesystem instance state.

mod local_file_system_core;
mod local_file_system_defaults;

#[cfg(all(test, feature = "test-support"))]
mod local_file_system_core_tests;

pub(crate) use local_file_system_core::LocalFileSystemCore;
pub(crate) use local_file_system_defaults::LocalFileSystemDefaults;
