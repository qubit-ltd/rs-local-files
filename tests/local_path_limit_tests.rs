// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for native path-length limits.

use std::ffi::OsStr;

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileNames;
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalFileSystemLimits;
use qubit_local_files::LocalResourceKind;
use qubit_local_files::SizeLimit;

/// Verifies a path-length limit preserves both its numeric bound and unit.
#[test]
fn test_local_file_system_limits_preserve_each_dimension() {
    let limits = LocalFileSystemLimits::new(SizeLimit::Maximum(260), SizeLimit::Unknown);

    assert_eq!(SizeLimit::Maximum(260), limits.max_path_bytes());
    assert_eq!(SizeLimit::Unknown, limits.max_file_name_bytes());
}

/// Verifies zero cannot configure a component byte limit.
#[test]
fn test_local_file_names_reject_zero_component_limit() {
    let error = LocalFileNames::portable()
        .with_max_component_bytes(0)
        .expect_err("zero component capacity must be rejected");

    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    assert_eq!(LocalFileOperation::ValidateName, error.operation());
    let limit = error
        .resource_limit_error()
        .expect("component capacity failure should retain limit facts");
    assert_eq!(LocalResourceKind::PathComponentBytes, limit.resource());
    assert_eq!(0, limit.limit());
    assert_eq!(1, limit.requested());
}

/// Verifies configured component limits measure portable UTF-8 bytes.
#[test]
fn test_local_file_names_enforce_configured_component_limit() {
    let names = LocalFileNames::portable()
        .with_max_component_bytes(3)
        .expect("positive component capacity should be accepted");

    names
        .validate(OsStr::new("abc"))
        .expect("component at configured capacity should be accepted");
    let error = names
        .validate(OsStr::new("abcd"))
        .expect_err("component beyond configured capacity must be rejected");
    let limit = error
        .resource_limit_error()
        .expect("component overflow should retain limit facts");
    assert_eq!(LocalResourceKind::PathComponentBytes, limit.resource());
    assert_eq!(3, limit.limit());
    assert_eq!(4, limit.requested());
}

/// Verifies component-size limits are caller-owned and can be cleared.
#[test]
fn test_local_file_names_component_limit_is_opt_in() {
    let long_name = "a".repeat(300);
    let names = LocalFileNames::portable();
    assert_eq!(names.max_component_bytes(), None);
    names
        .validate(OsStr::new(&long_name))
        .expect("portable validation must not invent a filesystem limit");

    let names = names
        .with_max_component_bytes(255)
        .expect("positive explicit limit should be accepted");
    assert_eq!(names.max_component_bytes(), Some(255));
    let names = names.without_max_component_bytes();
    assert_eq!(names.max_component_bytes(), None);
    names
        .validate(OsStr::new(&long_name))
        .expect("clearing the explicit limit should remove the budget");
}
