// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::io::Error;
use std::mem;
use std::path::Path;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalResult;
use qubit_local_files::directory::LocalResolutionOrigin;
use qubit_local_files::directory::LocalResolutionPolicy;
use qubit_local_files::directory::LocalResolutionScope;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::error::LocalResourceLimitError;
use qubit_local_files::observation::LocalSecurityObservation;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalResolutionLimits;
use qubit_local_files::options::LocalRootOpenOptions;
use qubit_local_files::policy::LocalOwnerAccessPolicy;

#[test]
fn test_refactor_api_keeps_required_value_traits() {
    fn require_copy<T: Copy>() {}
    require_copy::<LocalResolutionLimits>();
    require_copy::<LocalOwnerAccessPolicy>();
    require_copy::<LocalObservedReadOptions>();
}

#[test]
fn test_resource_limit_error_preserves_historical_facts_through_io_conversion() {
    let facts = LocalResourceLimitError::new(LocalResourceKind::SymlinkTargetBytes, 8, 0, 1);
    assert_eq!(facts.observed(), None);
    assert!(!facts.overflowed());
    let error = LocalFileError::from_io(LocalFileOperation::Metadata, None, None, Error::other(facts));
    let retained = error.resource_limit_error().expect("resource facts remain available");
    assert_eq!(retained.resource(), LocalResourceKind::SymlinkTargetBytes);
    assert_eq!(retained.requested(), 1);
    assert_eq!(retained.observed(), None);
    assert!(!retained.overflowed());
}

#[test]
fn test_refactor_api_types_are_constructible_and_accessible() {
    let limits = LocalResolutionLimits::new()
        .with_max_symlink_hops(2)
        .with_max_symlink_target_bytes(64)
        .with_max_actual_components(8)
        .with_max_resolved_path_bytes(256);
    let _root = LocalRootOpenOptions::new().with_resolution_limits(limits);
    let _policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let _ = LocalSecurityObservationOptions::new();
    #[derive(Debug)]
    struct Policy;
    impl LocalResolutionPolicy for Policy {
        fn validate_component(&self, _: &OsStr, _: LocalResolutionScope, _: LocalResolutionOrigin) -> LocalResult<()> {
            Ok(())
        }
        fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
            Ok(())
        }
    }
    let _: &dyn LocalResolutionPolicy = &Policy;
    let _ = (LocalResolutionOrigin::Input, LocalResolutionScope::Descendant);
    let _ = mem::size_of::<LocalSecurityObservation>();
}
