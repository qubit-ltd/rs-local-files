// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Handle-bound local namespace authorities.

#[path = "authority.rs"]
mod authority_dispatch;
mod authority_path;
mod host_authority;
mod rooted_authority;
mod symlink_resolver;

pub(crate) use authority_dispatch::Authority;
pub(crate) use authority_path::AuthorityPath;
pub(crate) use authority_path::HostPath;
pub(crate) use host_authority::HostAuthority;
pub(crate) use rooted_authority::RootedAuthority;

#[cfg(test)]
mod authority_tests;
