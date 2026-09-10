// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private authority-carrying temporary-resource backends.

mod host_temp_resource_backend;
mod keep_target;
mod local_temp_resource_backend;
mod local_temp_resource_core;
mod local_temp_resource_state;
mod persist_target;
mod rooted_temp_resource_backend;
mod temp_entry_identity;
mod temp_parent;

pub(crate) use host_temp_resource_backend::HostTempResourceBackend;
pub(crate) use keep_target::generated_target;
pub(crate) use local_temp_resource_backend::LocalTempResourceBackend;
pub(crate) use local_temp_resource_core::LocalTempResourceCore;
pub(crate) use local_temp_resource_state::LocalTempResourceState;
pub(crate) use persist_target::prepare_persist_target;
pub(crate) use persist_target::validate_persist_base;
pub(crate) use rooted_temp_resource_backend::RootedTempResourceBackend;
pub(crate) use temp_entry_identity::TempEntryIdentity;
pub(crate) use temp_parent::host as prepare_host_parent;
pub(crate) use temp_parent::rooted as prepare_rooted_parent;

mod temp_directory_delete_backend;
pub(crate) use temp_directory_delete_backend::TempDirectoryDeleteBackend;
