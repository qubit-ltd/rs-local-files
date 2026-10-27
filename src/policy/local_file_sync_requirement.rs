// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Requested synchronization of a file's contents and metadata.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalFileSyncRequirement {
    /// Do not request an explicit file synchronization.
    #[default]
    NotRequired,
    /// Attempt synchronization and report whether it succeeded.
    Preferred,
    /// Require synchronization before publishing the file.
    Required,
}
