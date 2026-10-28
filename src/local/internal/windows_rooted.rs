// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted filesystem operations.
// Platform behavior is covered through public rooted integration tests.

mod directory;
mod file_io;
mod handle;
mod namespace_mutation;
pub(in crate::local::internal) mod native;
mod symlink;
mod volume_probe;

pub(crate) use directory::create_rooted_directory;
pub(crate) use directory::open_root_directory_reader;
pub(crate) use directory::open_rooted_directory_reader;
pub(crate) use directory::read_rooted_directory;
pub(crate) use directory::remove_rooted_entry;
pub(crate) use directory::remove_rooted_entry_typed;
pub(crate) use directory::remove_rooted_owned_read_only_file;
pub(crate) use file_io::open_rooted_native_writer;
#[cfg(feature = "digest")]
pub(in crate::local::internal) use handle::open_conditional_parent;
pub(crate) use handle::open_root_directory;
pub(crate) use handle::open_rooted_component_directory;
pub(crate) use handle::open_rooted_native_reader;
pub(crate) use handle::read_rooted_component_metadata;
pub(crate) use handle::read_rooted_symlink_metadata;
#[cfg(feature = "digest")]
pub(in crate::local::internal) use namespace_mutation::rename_conditional_entry_at;
pub(crate) use namespace_mutation::rename_rooted_entry;
pub(crate) use namespace_mutation::set_rooted_permissions;
#[cfg(feature = "digest")]
pub(in crate::local::internal) use native::open_conditional_entry_at;
pub(crate) use symlink::RootedLinkTargetRead;
pub(crate) use symlink::create_rooted_symlink;
pub(crate) use symlink::normalize_rooted_link_target;
pub(crate) use symlink::read_rooted_link;
pub(crate) use symlink::read_rooted_link_limited;
pub(crate) use symlink::rooted_link_targets_directory;
pub(crate) use volume_probe::probe_windows_limits;
pub(crate) use volume_probe::probe_windows_space;
