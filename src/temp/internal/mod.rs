// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private authority-carrying temporary-resource backends.

mod host_temp_resource_backend;
mod local_temp_resource_backend;
mod local_temp_resource_state;
mod rooted_temp_resource_backend;
mod temp_entry_identity;
mod temp_parent;

pub(crate) use host_temp_resource_backend::HostTempResourceBackend;
pub(crate) use local_temp_resource_backend::LocalTempResourceBackend;
pub(crate) use local_temp_resource_state::LocalTempResourceState;
pub(crate) use rooted_temp_resource_backend::RootedTempResourceBackend;
pub(crate) use temp_entry_identity::TempEntryIdentity;
pub(crate) use temp_parent::{
    host as prepare_host_parent,
    rooted as prepare_rooted_parent,
};
