// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Complete resolver calls must keep ordinary-path native work linear.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use super::support::resolution_observation_tests;
use crate::LocalFileOperation;
use crate::LocalSymlinkPolicy;
use crate::directory::LocalResolutionOrigin;
use crate::directory::LocalResolutionPolicy;
use crate::directory::LocalResolutionScope;
use crate::options::LocalResolutionLimits;
use crate::resolution::ResolutionContract;
use crate::rooted::Root;
use crate::rooted_local_file_system::resolve_rooted_path;

#[derive(Debug)]
struct NoopPolicy(Arc<std::sync::atomic::AtomicUsize>);

impl LocalResolutionPolicy for NoopPolicy {
    fn validate_component(
        &self,
        _: &OsStr,
        _: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> crate::LocalResult<()> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn validate_symlink_target(&self, _: &std::path::Path, _: LocalResolutionScope) -> crate::LocalResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct RejectPolicy;

impl LocalResolutionPolicy for RejectPolicy {
    fn validate_component(
        &self,
        component: &OsStr,
        _: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> crate::LocalResult<()> {
        if component == OsStr::new("denied") {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::PermissionDenied,
                LocalFileOperation::Metadata,
            ));
        }
        Ok(())
    }

    fn validate_symlink_target(&self, _: &std::path::Path, _: LocalResolutionScope) -> crate::LocalResult<()> {
        Ok(())
    }
}

/// Counts the actual fast-path operations, including accidental fallbacks.
#[test]
fn test_complete_resolver_uses_linear_ordinary_path_work() {
    let temporary = tempfile::tempdir().expect("fixture should exist");
    let root = Root::open(temporary.path()).expect("authority should open");
    for depth in [1, 8, 32, 64] {
        let mut path = PathBuf::new();
        for _ in 0..depth {
            path.push("d");
        }
        fs::create_dir_all(temporary.path().join(&path)).expect("deep directory should exist");
        path.push("payload");
        fs::write(temporary.path().join(&path), b"payload").expect("leaf should exist");
        for follow_final in [false, true] {
            resolution_observation_tests::reset();
            let resolved = resolve_rooted_path(
                &root,
                &path,
                LocalSymlinkPolicy::FollowWithinScope,
                follow_final,
                LocalFileOperation::Metadata,
            )
            .expect("normal path should resolve");
            assert_eq!(path, resolved.as_path());
            let (metadata, opened, fallback) = resolution_observation_tests::snapshot();
            assert_eq!(depth + usize::from(follow_final), metadata);
            assert_eq!(depth, opened);
            assert_eq!(0, fallback, "ordinary paths must not silently lose the fast path");
        }
    }
}

#[test]
fn test_complete_resolver_keeps_restricted_ordinary_paths_on_linear_cursor() {
    let temporary = tempfile::tempdir().expect("fixture should exist");
    let unrestricted_root = Root::open(temporary.path()).expect("authority should open");
    let policy_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let contracts = [
        ResolutionContract::unrestricted().tightened(LocalResolutionLimits::new().with_max_actual_components(256)),
        ResolutionContract::unrestricted().with_policy(Arc::new(NoopPolicy(Arc::clone(&policy_calls)))),
    ];

    for contract in contracts {
        let root = Root::from_directory_authority_with_contract(
            unrestricted_root.try_clone_authority().expect("authority should clone"),
            unrestricted_root.path().to_path_buf(),
            contract,
        );
        for depth in [8, 32, 64] {
            let mut path = PathBuf::new();
            for _ in 0..depth {
                path.push("d");
            }
            fs::create_dir_all(temporary.path().join(&path)).expect("deep directory should exist");
            path.push("payload");
            fs::write(temporary.path().join(&path), b"payload").expect("leaf should exist");

            resolution_observation_tests::reset();
            let resolved = resolve_rooted_path(
                &root,
                &path,
                LocalSymlinkPolicy::FollowWithinScope,
                true,
                LocalFileOperation::Metadata,
            )
            .expect("normal path should resolve under a contract");
            assert_eq!(path, resolved.as_path());
            let (metadata, opened, fallback) = resolution_observation_tests::snapshot();
            assert_eq!(depth + 1, metadata);
            assert_eq!(depth, opened);
            assert_eq!(0, fallback, "contracted ordinary paths must use the sequential cursor");
        }
    }
    assert_eq!(8 + 32 + 64 + 3, policy_calls.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_complete_resolver_runs_component_policy_before_cursor_io() {
    let temporary = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(temporary.path().join("denied")).expect("directory should exist");
    let unrestricted_root = Root::open(temporary.path()).expect("authority should open");
    let root = Root::from_directory_authority_with_contract(
        unrestricted_root.try_clone_authority().expect("authority should clone"),
        unrestricted_root.path().to_path_buf(),
        ResolutionContract::unrestricted().with_policy(Arc::new(RejectPolicy)),
    );

    resolution_observation_tests::reset();
    let error = resolve_rooted_path(
        &root,
        &PathBuf::from("denied/file"),
        LocalSymlinkPolicy::FollowWithinScope,
        true,
        LocalFileOperation::OpenReader,
    )
    .expect_err("a denied component must stop resolution");

    assert_eq!(crate::LocalFileErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    assert_eq!(Some(std::path::Path::new("denied/file")), error.path());
    assert_eq!((0, 0, 0), resolution_observation_tests::snapshot());
}

#[test]
fn test_complete_resolver_contextualizes_pending_budget_errors() {
    let temporary = tempfile::tempdir().expect("fixture should exist");
    let unrestricted_root = Root::open(temporary.path()).expect("authority should open");
    let root = Root::from_directory_authority_with_contract(
        unrestricted_root.try_clone_authority().expect("authority should clone"),
        unrestricted_root.path().to_path_buf(),
        ResolutionContract::unrestricted().tightened(LocalResolutionLimits::new().with_max_resolved_path_bytes(0)),
    );
    let original = PathBuf::from("foo/../file");

    let error = resolve_rooted_path(
        &root,
        &original,
        LocalSymlinkPolicy::FollowWithinScope,
        true,
        LocalFileOperation::OpenReader,
    )
    .expect_err("zero pending byte budget must fail before access");

    assert_eq!(crate::LocalFileErrorKind::ResourceLimit, error.kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    assert_eq!(Some(original.as_path()), error.path());
    assert_eq!(
        crate::LocalResourceKind::PendingResolutionBytes,
        error.resource_limit_error().expect("typed resource facts").resource()
    );
}
