//! Unified temporary resources backed by host or rooted authority.

mod local_temp_directory;
mod local_temp_file;

/// Private storage implementations for temporary resources.
pub(crate) mod internal;

pub use local_temp_directory::LocalTempDirectory;
pub use local_temp_file::LocalTempFile;
