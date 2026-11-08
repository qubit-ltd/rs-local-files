// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// An observed stage entry's path, kind, and optional native identity.
///
/// The identity is retained from the collection phase so later phases can
/// reject entries that were removed or replaced before mutation.
pub(in crate::directory) type OwnerPolicyEntry = (
    std::path::PathBuf,
    crate::LocalFileKind,
    Option<crate::observation::LocalFileIdentity>,
);
