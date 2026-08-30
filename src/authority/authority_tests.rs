// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;

use super::HostAuthority;
use super::RootedAuthority;
use crate::LocalFileErrorKind;
use crate::LocalSymlinkPolicy;

/// Serializes tests that mutate the process current directory.
static CURRENT_DIRECTORY_LOCK: Mutex<()> = Mutex::new(());

/// Restores the process current directory when a test finishes.
struct CurrentDirectoryGuard {
    /// Original current directory.
    original: PathBuf,
    /// Lock held for the lifetime of the directory mutation.
    _lock: MutexGuard<'static, ()>,
}

impl CurrentDirectoryGuard {
    /// Changes the process current directory until the returned guard drops.
    fn set(path: &Path) -> Self {
        let lock = CURRENT_DIRECTORY_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let original = env::current_dir().expect("original current directory should be readable");
        env::set_current_dir(path).expect("test current directory should be set");
        Self { original, _lock: lock }
    }
}

impl Drop for CurrentDirectoryGuard {
    /// Restores the original process current directory.
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("original current directory should be restored");
    }
}

/// Verifies relative Host paths remain bound to the construction cwd handle.
#[test]
fn test_host_authority_keeps_construction_cwd() {
    let first = tempfile::tempdir().expect("first temporary directory should be created");
    let second = tempfile::tempdir().expect("second temporary directory should be created");
    fs::write(first.path().join("bound.txt"), b"first").expect("fixture should be written");
    let _cwd = CurrentDirectoryGuard::set(first.path());
    let authority = HostAuthority::bind_current(LocalSymlinkPolicy::Reject)
        .expect("Host authority should bind the current directory");
    env::set_current_dir(second.path()).expect("process current directory should be changed");

    let path = authority
        .resolve(Path::new("bound.txt"))
        .expect("relative path should resolve");
    assert_eq!(
        authority.read_all(&path).expect("bound file should be readable"),
        b"first",
    );
}

/// Verifies a Rooted authority remains anchored after its path is renamed.
#[test]
fn test_rooted_authority_survives_diagnostic_root_rename() {
    let parent = tempfile::tempdir().expect("temporary parent should be created");
    let root = parent.path().join("root");
    fs::create_dir(&root).expect("root directory should be created");
    fs::write(root.join("entry"), b"value").expect("fixture should be written");
    let authority = RootedAuthority::open(&root, LocalSymlinkPolicy::Reject).expect("Rooted authority should open");
    fs::rename(&root, parent.path().join("moved")).expect("diagnostic root path should be renamed");

    let path = authority
        .resolve(Path::new("entry"))
        .expect("contained path should resolve");
    assert_eq!(
        authority
            .read_all(&path)
            .expect("renamed-root entry should be readable"),
        b"value",
    );
}

/// Verifies rooted lexical parent escape is rejected before namespace I/O.
#[test]
fn test_rooted_authority_rejects_parent_escape() {
    let root = tempfile::tempdir().expect("temporary root should be created");
    let authority =
        RootedAuthority::open(root.path(), LocalSymlinkPolicy::Reject).expect("Rooted authority should open");

    let error = authority
        .resolve(Path::new("../escape"))
        .expect_err("parent escape should be rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

/// Verifies contained symbolic links resolve relative to the retained root.
#[cfg(unix)]
#[test]
fn test_rooted_authority_follows_contained_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temporary root should be created");
    fs::write(root.path().join("target"), b"inside").expect("target fixture should be written");
    symlink("target", root.path().join("link")).expect("contained symbolic link should be created");
    let authority = RootedAuthority::open(root.path(), LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted authority should open");

    let path = authority
        .resolve(Path::new("link"))
        .expect("contained symbolic link should resolve");
    assert_eq!(authority.read_all(&path).expect("target should be readable"), b"inside",);
}

/// Verifies relative symbolic-link targets cannot escape a retained root.
#[cfg(unix)]
#[test]
fn test_rooted_authority_rejects_symlink_parent_escape() {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().expect("temporary parent should be created");
    let root = parent.path().join("root");
    fs::create_dir(&root).expect("root directory should be created");
    symlink("../outside", root.join("escape")).expect("escaping symbolic link should be created");
    let authority =
        RootedAuthority::open(&root, LocalSymlinkPolicy::FollowWithinScope).expect("Rooted authority should open");

    let error = authority
        .resolve(Path::new("escape"))
        .expect_err("escaping link should be rejected during resolution");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

/// Verifies absolute symbolic-link targets cannot escape a retained root.
#[cfg(unix)]
#[test]
fn test_rooted_authority_rejects_absolute_symlink_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temporary root should be created");
    symlink("/tmp/outside", root.path().join("escape")).expect("absolute symbolic link should be created");
    let authority = RootedAuthority::open(root.path(), LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted authority should open");

    let error = authority
        .resolve(Path::new("escape"))
        .expect_err("absolute link should be rejected during resolution");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

/// Verifies symbolic-link cycles fail deterministically during resolution.
#[cfg(unix)]
#[test]
fn test_rooted_authority_rejects_symlink_cycle() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temporary root should be created");
    symlink("second", root.path().join("first")).expect("first symbolic link should be created");
    symlink("first", root.path().join("second")).expect("second symbolic link should be created");
    let authority = RootedAuthority::open(root.path(), LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted authority should open");

    let error = authority
        .resolve(Path::new("first"))
        .expect_err("symbolic-link cycle should be rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

/// Verifies Host relative links are resolved through the bound cwd handle.
#[cfg(unix)]
#[test]
fn test_host_authority_follows_contained_bound_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temporary root should be created");
    fs::write(root.path().join("target"), b"bound").expect("target fixture should be written");
    symlink("target", root.path().join("link")).expect("contained symbolic link should be created");
    let _cwd = CurrentDirectoryGuard::set(root.path());
    let authority = HostAuthority::bind_current(LocalSymlinkPolicy::FollowWithinScope)
        .expect("Host authority should bind the current directory");

    let path = authority
        .resolve(Path::new("link"))
        .expect("contained Host link should resolve");
    assert_eq!(authority.read_all(&path).expect("target should be readable"), b"bound",);
}

/// Verifies Host rename accepts relative and absolute paths in one authority.
#[test]
fn test_host_authority_renames_bound_path_to_absolute_path() {
    let root = tempfile::tempdir().expect("temporary root should be created");
    fs::write(root.path().join("source"), b"value").expect("source fixture should be written");
    let _cwd = CurrentDirectoryGuard::set(root.path());
    let authority = HostAuthority::bind_current(LocalSymlinkPolicy::Reject)
        .expect("Host authority should bind the current directory");
    let source = authority
        .resolve(Path::new("source"))
        .expect("relative source should resolve");
    let target = authority
        .resolve(&root.path().join("target"))
        .expect("absolute target should resolve");

    authority
        .rename(&source, &target, false)
        .expect("mixed Host rename should succeed");
    assert_eq!(
        fs::read(root.path().join("target")).expect("renamed target should be readable"),
        b"value",
    );
}
