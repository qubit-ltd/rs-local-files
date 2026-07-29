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
mod rooted_temp_resource_backend;

pub(crate) use host_temp_resource_backend::HostTempResourceBackend;
pub(crate) use local_temp_resource_backend::{
    LocalTempResourceBackend,
    LocalTempResourceState,
};
pub(crate) use rooted_temp_resource_backend::RootedTempResourceBackend;
