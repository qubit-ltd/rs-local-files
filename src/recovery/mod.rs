// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Read-only access to files retained for conditional-write recovery.

mod local_recovery_file;
mod local_recovery_file_role;

pub use local_recovery_file::LocalRecoveryFile;
pub use local_recovery_file_role::LocalRecoveryFileRole;
