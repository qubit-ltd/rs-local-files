// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// An observed owner-policy entry retained with its open handle and sync
/// ability.
///
/// The recorded identity binds mutations and verification to the entry opened
/// during preparation; the final flag records whether that handle supports
/// sync.
pub(in crate::directory) type RetainedOwnerPolicyEntry = (
    std::path::PathBuf,
    crate::LocalFileKind,
    Option<crate::observation::LocalFileIdentity>,
    std::fs::File,
    bool,
);
