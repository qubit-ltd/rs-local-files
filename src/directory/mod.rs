//! Fixed directory authorities and native component policies.

pub(crate) mod binding;
mod component_policy;
mod handle;

pub use component_policy::LocalComponentPolicy;
pub use handle::LocalDirectoryHandle;
pub(crate) use handle::retain_directory_chain;
