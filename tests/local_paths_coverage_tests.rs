// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::{
    LocalFileErrorKind,
    LocalFileOperation,
    LocalPaths,
};

/// Verifies absolute host paths remain unchanged when binding a path group.
#[test]
fn test_bind_host_paths_preserves_all_absolute_paths() {
    let source = std::env::temp_dir().join("qubit-local-files-source");
    let target = std::env::temp_dir().join("qubit-local-files-target");

    let [bound_source, bound_target] =
        LocalPaths::bind_host_paths([source.as_path(), target.as_path()])
            .expect("absolute paths should bind without a cwd lookup");

    assert_eq!(source, bound_source);
    assert_eq!(target, bound_target);
}

/// Verifies lexical containment supports equality and rejects unrelated paths.
#[test]
fn test_is_lexically_within_distinguishes_equal_and_unrelated_paths() {
    let root = Path::new("relative-root");

    assert!(
        LocalPaths::is_lexically_within(root, root)
            .expect("equal relative paths should compare")
    );
    assert!(
        !LocalPaths::is_lexically_within(Path::new("another-root/child"), root,)
            .expect("unrelated relative paths should compare")
    );

    let error = LocalPaths::is_lexically_within(
        Path::new("/absolute-root"),
        Path::new("relative-root"),
    )
    .expect_err("mixed absolute and relative forms must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ComposePath, error.operation());
}

/// Verifies descendant composition rejects each authority-changing shape.
#[test]
fn test_compose_descendant_rejects_empty_absolute_and_dot_paths() {
    let base = Path::new("/safe/base");

    for descendant in [
        Path::new(""),
        Path::new("/outside"),
        Path::new("./child"),
        Path::new("child/../outside"),
    ] {
        let error = LocalPaths::compose_descendant(base, descendant)
            .expect_err("unsafe descendant shape must be rejected");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
        assert_eq!(Some(descendant), error.path());
    }
}

/// Verifies descendant composition preserves the validated lexical hierarchy.
#[test]
fn test_compose_descendant_joins_normal_relative_components() {
    let composed = LocalPaths::compose_descendant(
        Path::new("/safe/base"),
        Path::new("nested/child"),
    )
    .expect("normal relative descendants should compose");

    assert_eq!(Path::new("/safe/base/nested/child"), composed);
}

/// Verifies canonical conversions reject empty, separator-bearing, and invalid
/// root shapes before composing native paths.
#[test]
fn test_canonical_path_conversions_reject_unsafe_component_shapes() {
    for components in [vec![""], vec!["safe", ""], vec!["safe", "%2F"]] {
        let error = LocalPaths::from_canonical_relative_components(components)
            .expect_err("relative canonical components must be normal paths");
        assert_eq!(LocalFileOperation::ComposePath, error.operation());
    }

    let error =
        LocalPaths::to_canonical_relative_components(Path::new("/absolute"))
            .expect_err(
                "absolute paths cannot be encoded as relative components",
            );
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());

    #[cfg(unix)]
    {
        let root = LocalPaths::from_canonical_absolute_components(vec![""])
            .expect("the Unix root is a valid canonical absolute path");
        assert_eq!(Path::new("/"), root);

        let error =
            LocalPaths::from_canonical_absolute_components(vec!["", "%2F"])
                .expect_err(
                    "an encoded separator cannot be an absolute component",
                );
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    }
}

/// Verifies direct bindings and conversions cover both successful relative
/// paths and their empty or dot-component rejection cases.
#[test]
fn test_path_conversions_cover_relative_binding_and_normal_components() {
    let relative = Path::new("coverage-relative/child");
    let bound = LocalPaths::bind_host_path(relative).expect(
        "relative host paths should bind against the current directory",
    );
    assert!(bound.is_absolute());
    assert!(bound.ends_with(relative));

    let canonical =
        LocalPaths::from_canonical_relative_components(vec!["safe", "nested"])
            .expect("vector-backed canonical components should decode");
    assert_eq!(Path::new("safe/nested"), canonical);
    assert_eq!(
        vec!["safe".to_owned(), "nested".to_owned()],
        LocalPaths::to_canonical_relative_components(&canonical)
            .expect("normal relative components should encode"),
    );
    assert!(
        LocalPaths::from_canonical_relative_components(Vec::new()).is_err()
    );
    assert!(
        LocalPaths::to_canonical_relative_components(Path::new(".")).is_err()
    );
    assert_eq!(
        Path::new("single"),
        LocalPaths::from_canonical_relative_components(vec!["single"])
            .expect("iterator-backed relative components should decode"),
    );

    #[cfg(unix)]
    {
        let absolute = LocalPaths::from_canonical_absolute_components(vec![
            "", "tmp", "coverage",
        ])
        .expect("vector-backed absolute components should decode");
        assert_eq!(Path::new("/tmp/coverage"), absolute);
        assert_eq!(
            vec!["".to_owned(), "tmp".to_owned(), "coverage".to_owned()],
            LocalPaths::to_canonical_absolute_components(&absolute)
                .expect("normal absolute components should encode"),
        );
        assert!(
            LocalPaths::to_canonical_absolute_components(Path::new(
                "/tmp/./coverage"
            ))
            .is_err()
        );
        assert_eq!(
            Path::new("/"),
            LocalPaths::from_canonical_absolute_components(vec![""])
                .expect("iterator-backed root components should decode"),
        );
    }
}

/// Verifies non-UTF-8 Unix native components are represented losslessly by
/// canonical escaped-byte text.
#[cfg(unix)]
#[test]
fn test_canonical_conversions_encode_non_utf8_native_components() {
    use std::{
        ffi::OsString,
        os::unix::ffi::OsStringExt,
    };

    let relative = std::path::PathBuf::from(OsString::from_vec(vec![0xff]));
    assert_eq!(
        vec!["%FF".to_owned()],
        LocalPaths::to_canonical_relative_components(&relative)
            .expect("non-UTF-8 relative component should encode losslessly"),
    );

    let mut absolute = std::path::PathBuf::from("/tmp");
    absolute.push(OsString::from_vec(vec![0xff]));
    assert_eq!(
        vec!["".to_owned(), "tmp".to_owned(), "%FF".to_owned()],
        LocalPaths::to_canonical_absolute_components(&absolute)
            .expect("non-UTF-8 absolute component should encode losslessly"),
    );
}

/// Runs one coverage-only path fault in an isolated child process.
#[cfg(coverage)]
fn run_path_fault<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if let Some(selected) = std::env::var_os(COVERAGE_FAULT_ENV) {
        if selected == std::ffi::OsStr::new(fault) {
            action();
        }
        return;
    }
    let executable = std::env::current_exe()
        .expect("coverage test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies path binding retains structured errors when the host cwd lookup
/// cannot establish an authority snapshot.
#[cfg(coverage)]
#[test]
fn test_path_binding_reports_injected_current_directory_failures() {
    const TEST_NAME: &str =
        "test_path_binding_reports_injected_current_directory_failures";
    run_path_fault(TEST_NAME, "local-path-bind-cwd", || {
        let error = LocalPaths::bind_host_path(Path::new("relative"))
            .expect_err("injected cwd lookup must fail relative binding");
        assert_eq!(LocalFileOperation::BindPath, error.operation());
    });
    run_path_fault(TEST_NAME, "local-paths-bind-cwd", || {
        let error = LocalPaths::bind_host_paths([
            Path::new("source"),
            Path::new("target"),
        ])
        .expect_err("injected cwd lookup must fail group binding");
        assert_eq!(LocalFileOperation::BindPath, error.operation());
    });
}
