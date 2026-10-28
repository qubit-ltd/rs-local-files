// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResult;
use qubit_local_files::directory::LocalResolutionOrigin;
use qubit_local_files::directory::LocalResolutionPolicy;
use qubit_local_files::directory::LocalResolutionScope;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalResolutionLimits;
use qubit_local_files::options::LocalRootOpenOptions;

#[cfg(unix)]
#[test]
fn rooted_filesystem_can_open_the_native_root_directory() {
    LocalFileSystem::rooted(Path::new("/")).expect("the native root directory should open as a Rooted filesystem");
}

#[derive(Debug)]
struct TargetRecorder(Arc<Mutex<Vec<(String, LocalResolutionScope)>>>);

impl LocalResolutionPolicy for TargetRecorder {
    fn validate_component(&self, _: &OsStr, _: LocalResolutionScope, _: LocalResolutionOrigin) -> LocalResult<()> {
        Ok(())
    }

    fn validate_symlink_target(&self, target: &Path, scope: LocalResolutionScope) -> LocalResult<()> {
        self.0
            .lock()
            .expect("hook record should be available")
            .push((target.to_string_lossy().into_owned(), scope));
        Err(LocalFileError::new(
            LocalFileErrorKind::PermissionDenied,
            LocalFileOperation::OpenRoot,
        ))
    }
}

#[derive(Debug)]
struct RejectComponent(&'static str);

impl LocalResolutionPolicy for RejectComponent {
    fn validate_component(
        &self,
        component: &OsStr,
        _: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        if component == OsStr::new(self.0) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::PermissionDenied,
                LocalFileOperation::List,
            ));
        }
        Ok(())
    }

    fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct OriginRecorder(Arc<Mutex<Vec<(String, LocalResolutionScope, LocalResolutionOrigin)>>>);

impl LocalResolutionPolicy for OriginRecorder {
    fn validate_component(
        &self,
        component: &OsStr,
        scope: LocalResolutionScope,
        origin: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        self.0.lock().expect("component record should be available").push((
            component.to_string_lossy().into_owned(),
            scope,
            origin,
        ));
        Ok(())
    }

    fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        Ok(())
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct RejectRawNtTarget(Arc<Mutex<Option<String>>>);

#[cfg(windows)]
impl LocalResolutionPolicy for RejectRawNtTarget {
    fn validate_component(&self, _: &OsStr, _: LocalResolutionScope, _: LocalResolutionOrigin) -> LocalResult<()> {
        Ok(())
    }

    fn validate_symlink_target(&self, target: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        *self.0.lock().expect("raw target record should be available") = Some(target.to_string_lossy().into_owned());
        Err(LocalFileError::new(
            LocalFileErrorKind::PermissionDenied,
            LocalFileOperation::OpenRoot,
        ))
    }
}

#[cfg(unix)]
#[test]
fn zero_hops_rejects_a_real_link_during_opening_root() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should be created");
    symlink("real", tmp.path().join("alias")).expect("symbolic link should be created");
    let options =
        LocalRootOpenOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));

    let error = LocalFileSystem::rooted_with_options(&tmp.path().join("alias"), &options)
        .expect_err("zero hops must reject the root symlink");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn root_target_byte_limit_accepts_exact_limit_and_rejects_next_byte() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should be created");
    symlink("real", tmp.path().join("alias")).expect("symbolic link should be created");

    let exact = LocalRootOpenOptions::new()
        .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_target_bytes(4));
    LocalFileSystem::rooted_with_options(&tmp.path().join("alias"), &exact)
        .expect("target exactly at its byte limit should open");

    let too_small = LocalRootOpenOptions::new()
        .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_target_bytes(3));
    let error = LocalFileSystem::rooted_with_options(&tmp.path().join("alias"), &too_small)
        .expect_err("target above its byte limit must fail");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(
        error
            .resource_limit_error()
            .expect("limit facts should be retained")
            .observed(),
        Some(4),
    );
}

#[cfg(unix)]
#[test]
fn root_target_hook_receives_raw_target_before_follow_and_keeps_its_error() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should be created");
    symlink("real//./", tmp.path().join("alias")).expect("symbolic link should be created");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let options = LocalRootOpenOptions::new().with_resolution_policy(Arc::new(TargetRecorder(recorded.clone())));

    let error = LocalFileSystem::rooted_with_options(&tmp.path().join("alias"), &options)
        .expect_err("target hook should reject before target access");
    assert_eq!(error.kind(), LocalFileErrorKind::PermissionDenied);
    assert_eq!(
        recorded.lock().expect("hook record should be available").as_slice(),
        [("real//./".to_owned(), LocalResolutionScope::OpeningRoot)],
    );
}

#[cfg(unix)]
#[test]
fn opening_root_keeps_input_origin_on_suffix_after_symlink_expansion() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should exist");
    std::fs::create_dir_all(tmp.path().join("real/child")).expect("target hierarchy should exist");
    symlink("real", tmp.path().join("alias")).expect("root alias should exist");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let options = LocalRootOpenOptions::new().with_resolution_policy(Arc::new(OriginRecorder(recorded.clone())));

    LocalFileSystem::rooted_with_options(&tmp.path().join("alias/child"), &options)
        .expect("root should open through the aliased directory");

    let calls = recorded.lock().expect("component record should be available");
    assert!(calls.contains(&(
        "real".to_owned(),
        LocalResolutionScope::OpeningRoot,
        LocalResolutionOrigin::SymlinkTarget,
    )));
    assert!(calls.contains(&(
        "child".to_owned(),
        LocalResolutionScope::OpeningRoot,
        LocalResolutionOrigin::Input,
    )));
}

#[cfg(unix)]
#[test]
fn reject_policy_refuses_intermediate_root_symlink() {
    use std::os::unix::fs::symlink;

    use qubit_local_files::policy::LocalSymlinkPolicy;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should be created");
    symlink("real", tmp.path().join("alias")).expect("symbolic link should be created");
    let options = LocalRootOpenOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject);

    let error = LocalFileSystem::rooted_with_options(&tmp.path().join("alias"), &options)
        .expect_err("Reject must apply during root opening");
    assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
}

#[cfg(unix)]
#[test]
fn root_component_limit_counts_only_normal_components() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    let target = tmp.path().join("real");
    std::fs::create_dir(&target).expect("target directory should be created");
    let component_count = target
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    let exact = LocalRootOpenOptions::new()
        .with_resolution_limits(LocalResolutionLimits::new().with_max_actual_components(component_count));
    LocalFileSystem::rooted_with_options(&target, &exact).expect("exact component budget should suffice");

    let too_small = LocalRootOpenOptions::new()
        .with_resolution_limits(LocalResolutionLimits::new().with_max_actual_components(component_count - 1));
    let error = LocalFileSystem::rooted_with_options(&target, &too_small)
        .expect_err("one fewer component must exceed the budget");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn opening_root_checks_missing_and_file_predecessors_before_parent_steps() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("ok")).expect("target directory should be created");
    std::fs::write(tmp.path().join("file"), b"not a directory").expect("file should be created");

    let missing = tmp.path().join("missing").join("..").join("ok");
    assert!(LocalFileSystem::rooted_with_options(&missing, &LocalRootOpenOptions::new()).is_err());
    let file = tmp.path().join("file").join("..").join("ok");
    let error = LocalFileSystem::rooted_with_options(&file, &LocalRootOpenOptions::new())
        .expect_err("a file predecessor must not be lexically folded away");
    assert_eq!(error.kind(), LocalFileErrorKind::NotDirectory);
}

#[cfg(unix)]
#[test]
fn opening_root_enforces_pending_component_budget_after_link_expansion() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("fixture directory should exist");
    std::fs::create_dir_all(tmp.path().join("real/a/b")).expect("target hierarchy should exist");
    symlink("real", tmp.path().join("alias")).expect("root alias should exist");
    let options =
        LocalRootOpenOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_actual_components(2));

    let error = LocalFileSystem::rooted_with_options(&tmp.path().join("alias/a/b"), &options)
        .expect_err("expanded target and its input suffix must fit one pending component budget");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn opening_root_detects_an_active_symlink_identity_cycle() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("fixture directory should exist");
    symlink("second", tmp.path().join("first")).expect("first link should exist");
    symlink("first", tmp.path().join("second")).expect("second link should exist");

    let error = LocalFileSystem::rooted_with_options(&tmp.path().join("first"), &LocalRootOpenOptions::new())
        .expect_err("active link identities must detect a cycle");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

#[cfg(unix)]
#[test]
fn rooted_writer_allows_only_the_final_leaf_to_be_missing() {
    use qubit_io::Output;

    let tmp = tempfile::tempdir().expect("fixture directory should exist");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");

    let mut writer = filesystem
        .open_writer(Path::new("new-leaf"))
        .expect("a missing final leaf may be created");
    assert_eq!(writer.write(b"data").expect("leaf should be writable"), 4);
    let _outcome = writer.commit().expect("new leaf should publish");
    assert_eq!(std::fs::read(tmp.path().join("new-leaf")).unwrap(), b"data");

    assert!(filesystem.open_writer(Path::new("missing-parent/leaf")).is_err());
}

#[cfg(windows)]
#[test]
fn opening_root_target_hook_sees_and_can_reject_raw_nt_substitute_name() {
    use std::os::windows::fs::symlink_dir;

    let tmp = tempfile::tempdir().expect("fixture directory should exist");
    let target = tmp.path().join("target");
    std::fs::create_dir(&target).expect("target directory should exist");
    let alias = tmp.path().join("alias");
    match symlink_dir(&target, &alias) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("symbolic-link fixture should be created: {error}"),
    }
    let recorded = Arc::new(Mutex::new(None));
    let options = LocalRootOpenOptions::new().with_resolution_policy(Arc::new(RejectRawNtTarget(recorded.clone())));

    let error = LocalFileSystem::rooted_with_options(&alias, &options)
        .expect_err("raw target policy should reject before target normalization or access");

    assert_eq!(error.kind(), LocalFileErrorKind::PermissionDenied);
    let raw_target = recorded
        .lock()
        .expect("raw target record should be available")
        .clone()
        .expect("target hook should run");
    assert!(
        raw_target.starts_with(r"\??\"),
        "hook received normalized target: {raw_target:?}"
    );
}

#[cfg(unix)]
#[test]
fn observed_reader_limits_tighten_the_filesystem_contract() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should be created");
    std::fs::write(tmp.path().join("real/file"), b"contents").expect("file should be created");
    symlink("real", tmp.path().join("alias")).expect("symbolic link should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let options =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));

    let error = filesystem
        .open_observed_reader(std::path::Path::new("alias/file"), &options)
        .expect_err("reader-specific zero-hop limit must apply before opening");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn child_directory_options_cannot_loosen_parent_resolution_limits() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("child")).expect("child should be created");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should be created");
    std::fs::write(tmp.path().join("real/file"), b"contents").expect("file should be created");
    symlink("../real", tmp.path().join("child/alias")).expect("symbolic link should be created");
    let parent_options =
        LocalRootOpenOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let filesystem = LocalFileSystem::rooted_with_options(tmp.path(), &parent_options).expect("root should open");
    let child = filesystem
        .open_directory_with_options(
            std::path::Path::new("child"),
            &LocalDirectoryOpenOptions::new()
                .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(4)),
        )
        .expect("child directory should open");

    let error = child
        .open_observed_reader(std::path::Path::new("alias/file"), &LocalObservedReadOptions::new())
        .expect_err("child options must not relax the root's zero-hop contract");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn walker_rejects_a_discovered_entry_with_the_inherited_resolution_policy() {
    use qubit_local_files::options::LocalListOptions;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("private")).expect("discovered directory should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(
            Path::new("."),
            &LocalDirectoryOpenOptions::new().with_resolution_policy(Arc::new(RejectComponent("private"))),
        )
        .expect("fixed authority should open");
    let mut walker = directory
        .list_with_options(Path::new("."), &LocalListOptions::new())
        .expect("walker should open before it discovers descendants");

    let error = walker
        .next()
        .expect("discovered entry should yield an error")
        .expect_err("resolution policy must reject a name discovered by the walker rather than filter it");
    assert_eq!(error.kind(), LocalFileErrorKind::PermissionDenied);
}
