// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod coverage_binding_tests;
mod coverage_directory_handle_tests;
mod coverage_staged_directory_tests;
mod deletion_native_failure_tests;
mod directory_native_failure_tests;
mod error;
mod file_system;
mod filesystem_native_failure_tests;
mod lf3_authority_tests;
#[cfg(feature = "digest")]
mod lf3_conditional_tests;
mod lf3_contract_tests;
mod lf3_observation_tests;
#[cfg(unix)]
mod lf3_observation_unix_tests;
#[cfg(windows)]
mod lf3_observation_windows_tests;
#[cfg(target_os = "linux")]
mod lf3_owner_policy_limit_tests;
#[cfg(unix)]
mod lf3_privacy_unix_tests;
#[cfg(windows)]
mod lf3_privacy_windows_tests;
mod lf3_read_tests;
mod lf3_staged_sync_tests;
mod lf3_sync_tests;
mod lf3_temp_tests;
mod lf3_tree_observer_tests;
mod lf3_tree_publish_tests;
mod local;
mod local_file_metadata_tests;
pub(crate) mod native_io_fault_tests;
mod outcome;
mod path;
pub(crate) mod rooted;
mod security_observation_value_tests;
mod tree_privacy_failure_tests;
mod vectored_read_tests;
mod vectored_tests;
mod write;
