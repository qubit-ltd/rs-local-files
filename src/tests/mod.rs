// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod coverage_staged_directory_tests;
mod deletion_native_failure_tests;
mod directory;
mod directory_access_policy_tests;
mod directory_copy_deadline_tests;
mod directory_list_deadline_tests;
mod directory_native_failure_tests;
mod error;
mod filesystem_native_failure_tests;
mod host_copy_completion_deadline_tests;
#[cfg(unix)]
mod host_tree_symlink_deadline_tests;
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
mod lf3_privacy_windows_access_transition_tests;
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
mod local_native_file_details_tests;
pub(crate) mod native_io_fault_tests;
#[cfg(unix)]
mod native_publish_failure_tests;
#[cfg(unix)]
mod observation;
mod observed_reader_binding_deadline_tests;
mod observed_reader_context_tests;
mod observed_reader_deadline_overflow_tests;
mod observed_reader_refresh_tests;
mod outcome;
mod path;
mod recovery;
mod resolution;
pub(crate) mod rooted;
mod rooted_copy_deadline_tests;
mod security_observation_value_tests;
mod staged_sync_native_failure_tests;
mod staged_write_input_limit_tests;
mod temp;
#[cfg(all(feature = "digest", feature = "test-support"))]
mod test_conditional_hooks_tests;
#[cfg(all(feature = "digest", feature = "test-support"))]
mod test_support_conditional_hooks_tests;
mod tree_privacy_failure_tests;
mod vectored_read_tests;
mod vectored_tests;
mod write;
mod writer;
