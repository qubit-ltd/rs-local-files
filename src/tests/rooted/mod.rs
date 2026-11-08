// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod deletion_type_tests;
mod metadata_tests;
mod path_support_tests;
mod permissions_tests;
mod root_authority_contract_tests;
mod root_tests;
#[cfg(unix)]
mod rooted_constructor_tests;
mod rooted_local_file_system_tests;
mod support_tests;
mod temp_creation_tests;

mod resolution_complexity_tests;
pub(crate) mod support;
