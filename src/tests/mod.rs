// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod error;
mod file_system;
mod lf3_contract_tests;
mod lf3_authority_tests;
mod lf3_read_tests;
#[cfg(unix)]
mod lf3_observation_unix_tests;
mod lf3_observation_tests;
#[cfg(windows)]
mod lf3_observation_windows_tests;
#[cfg(unix)]
mod lf3_privacy_unix_tests;
#[cfg(windows)]
mod lf3_privacy_windows_tests;
mod lf3_sync_tests;
mod local;
mod local_file_metadata_tests;
mod outcome;
mod path;
pub(crate) mod rooted;
mod vectored_read_tests;
mod vectored_tests;
mod write;
