// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted filesystem operations.
// qubit-style: allow source-test-pair
// Platform behavior is covered through public rooted integration tests.

mod directory;
mod handle;
mod namespace_mutation;
mod symlink;
mod volume_probe;

pub(crate) use directory::create_rooted_directory;
pub(crate) use directory::open_root_directory_reader;
pub(crate) use directory::open_rooted_directory_reader;
pub(crate) use directory::read_root_directory;
pub(crate) use directory::read_rooted_directory;
pub(crate) use directory::remove_rooted_entry;
pub(crate) use handle::open_root_directory;
pub(crate) use handle::open_rooted_native_reader;
pub(crate) use handle::open_rooted_native_writer;
pub(crate) use handle::read_rooted_symlink_metadata;
pub(crate) use namespace_mutation::rename_rooted_entry;
pub(crate) use namespace_mutation::set_rooted_permissions;
pub(crate) use symlink::create_rooted_symlink;
pub(crate) use symlink::read_rooted_link;
pub(crate) use symlink::rooted_link_targets_directory;
pub(crate) use volume_probe::probe_windows_limits;
pub(crate) use volume_probe::probe_windows_space;
