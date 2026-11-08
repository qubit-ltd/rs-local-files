// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResult;
use qubit_local_files::directory::LocalResolutionOrigin;
use qubit_local_files::directory::LocalResolutionPolicy;
use qubit_local_files::directory::LocalResolutionScope;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
#[cfg(unix)]
use qubit_local_files::error::LocalResourceKind;
#[cfg(unix)]
use qubit_local_files::options::LocalResolutionLimits;
use qubit_local_files::options::LocalRootOpenOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
#[cfg(unix)]
use sha2::Digest;

#[derive(Debug)]
struct DenyForbidden;

impl LocalResolutionPolicy for DenyForbidden {
    fn validate_component(
        &self,
        name: &OsStr,
        scope: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        if scope == LocalResolutionScope::Descendant && name == OsStr::new("forbidden") {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_reason("RLF4 denied component"),
            );
        }
        Ok(())
    }

    fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        Ok(())
    }
}

fn entries(root: &Path) -> Vec<String> {
    fn visit(root: &Path, dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("fixture directory should be readable") {
            let entry = entry.expect("fixture entry should be readable");
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("entry should be beneath fixture");
            found.push(relative.to_string_lossy().into_owned());
            if entry.file_type().expect("entry type should be readable").is_dir() {
                visit(root, &path, found);
            }
        }
    }
    let mut found = Vec::new();
    visit(root, root, &mut found);
    found.sort();
    found
}

#[cfg(unix)]
fn pending_path_bytes(path: &Path) -> usize {
    path.components()
        .map(|component| component.as_os_str().len())
        .sum::<usize>()
        + path.components().count()
}

#[test]
fn test_conditional_writer_inherits_rooted_component_policy_without_side_effects() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut filesystem = LocalFileSystem::rooted_with_options(
        fixture.path(),
        &LocalRootOpenOptions::new().with_resolution_policy(Arc::new(DenyForbidden)),
    )
    .expect("rooted filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("Reject should be selected");
    let before = entries(fixture.path());
    let mut rejected = Vec::new();

    for (relative, create_parent) in [
        ("forbidden", false),
        ("forbidden/file", true),
        ("new/forbidden/file", true),
    ] {
        let mut options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
        if create_parent {
            options = options.with_create_parent();
        }
        let result = filesystem.open_conditional_writer::<sha2::Sha256>(
            Path::new(relative),
            LocalWritePrecondition::Absent,
            &options,
        );
        if let Err(error) = result {
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
            assert_eq!(error.operation(), LocalFileOperation::OpenWriter);
            assert_eq!(error.reason(), Some("RLF4 denied component"));
            assert_eq!(error.path(), Some(Path::new(relative)));
            rejected.push(relative);
        }
    }
    assert_eq!(rejected, vec!["forbidden", "forbidden/file", "new/forbidden/file"]);

    #[cfg(unix)]
    {
        std::fs::write(fixture.path().join("forbidden"), b"old").expect("existing target");
        let error = filesystem
            .open_conditional_writer::<sha2::Sha256>(
                Path::new("forbidden"),
                LocalWritePrecondition::Content(sha2::Sha256::digest(b"old")),
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .expect_err("Content target component should be rejected");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
        assert_eq!(error.operation(), LocalFileOperation::OpenWriter);
        assert_eq!(error.reason(), Some("RLF4 denied component"));
        assert_eq!(error.path(), Some(Path::new("forbidden")));
        assert_eq!(
            std::fs::read(fixture.path().join("forbidden")).expect("old target"),
            b"old"
        );
    }

    let after = entries(fixture.path());
    let mut expected_after = before;
    #[cfg(unix)]
    expected_after.push("forbidden".to_owned());
    expected_after.sort();
    assert_eq!(after, expected_after, "rejected requests must not create entries");
}

#[test]
#[cfg(unix)]
fn test_conditional_writer_inherits_finite_resolution_budgets_and_unlimited_default() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path().canonicalize().expect("fixture root should resolve");
    let root_components = root
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    let mut component_limited = LocalFileSystem::rooted_with_options(
        &root,
        &LocalRootOpenOptions::new()
            .with_resolution_limits(LocalResolutionLimits::new().with_max_actual_components(root_components + 1)),
    )
    .expect("budget should be sufficient to open fixture root");
    component_limited
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("Reject should be selected");
    let component_relative = std::iter::repeat_n("a", root_components + 2)
        .collect::<Vec<_>>()
        .join("/");
    let component_error = component_limited
        .open_conditional_writer::<sha2::Sha256>(
            Path::new(&component_relative),
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("descendant components should exceed inherited component budget");
    assert_eq!(component_error.kind(), LocalFileErrorKind::ResourceLimit);
    let component_facts = component_error
        .resource_limit_error()
        .expect("component limit facts should be retained");
    assert_eq!(
        component_facts.resource(),
        LocalResourceKind::PendingResolutionComponents
    );
    assert_eq!(component_facts.limit(), root_components + 1);
    assert_eq!(component_facts.observed(), Some(root_components + 2));

    let root_bytes = root.as_os_str().len();
    let mut byte_limited = LocalFileSystem::rooted_with_options(
        &root,
        &LocalRootOpenOptions::new()
            .with_resolution_limits(LocalResolutionLimits::new().with_max_resolved_path_bytes(root_bytes + 1)),
    )
    .expect("byte budget should be sufficient to open fixture root");
    byte_limited
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("Reject should be selected");
    let byte_component = "x".repeat(200);
    let byte_component_count = (root_bytes + 2).div_ceil(byte_component.len() + 1);
    let long_relative = std::iter::repeat_n(byte_component.as_str(), byte_component_count)
        .collect::<Vec<_>>()
        .join("/");
    let long_path = Path::new(&long_relative);
    let byte_error = byte_limited
        .open_conditional_writer::<sha2::Sha256>(
            long_path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("descendant bytes should exceed inherited path-byte budget");
    assert_eq!(byte_error.kind(), LocalFileErrorKind::ResourceLimit);
    let byte_facts = byte_error
        .resource_limit_error()
        .expect("path byte limit facts should be retained");
    assert_eq!(byte_facts.resource(), LocalResourceKind::PendingResolutionBytes);
    assert_eq!(byte_facts.limit(), root_bytes + 1);
    assert_eq!(byte_facts.observed(), Some(pending_path_bytes(long_path)));

    let mut unrestricted = LocalFileSystem::rooted(&root).expect("root without finite budget should open");
    unrestricted
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("Reject should be selected");
    let _writer = unrestricted
        .open_conditional_writer::<sha2::Sha256>(
            Path::new("unlimited"),
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("default None limits should permit an ordinary request");
}
