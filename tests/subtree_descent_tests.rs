// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalDirectoryWalker;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
#[cfg(unix)]
use qubit_local_files::options::LocalDirectoryReopenPolicy;
use qubit_local_files::options::LocalListOptions;
#[cfg(unix)]
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("wanted/leaf")).expect("wanted tree should be created");
    fs::create_dir_all(root.join("other/deep/leaf")).expect("other tree should be created");
    fs::write(root.join("wanted/leaf/item"), b"wanted").expect("wanted file should be created");
    fs::write(root.join("other/deep/leaf/item"), b"other").expect("other file should be created");
}

fn paths(walker: LocalDirectoryWalker) -> Vec<PathBuf> {
    let mut paths = walker
        .map(|entry| entry.expect("walk should succeed").relative_path().to_path_buf())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn test_host_descent_visits_target_ancestors_and_target_but_skips_sibling_descendants() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let walker = filesystem
        .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("relative subtree should be accepted");

    assert_eq!(
        vec![
            PathBuf::from("other"),
            PathBuf::from("wanted"),
            PathBuf::from("wanted/leaf"),
            PathBuf::from("wanted/leaf/item")
        ],
        paths(walker),
    );
}

#[test]
fn test_rooted_descent_uses_the_same_relative_subtree_rule() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let walker = filesystem
        .list_with_options(Path::new("."), &LocalListOptions::new().with_recursive())
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("relative subtree should be accepted");

    assert_eq!(
        vec![
            PathBuf::from("other"),
            PathBuf::from("wanted"),
            PathBuf::from("wanted/leaf"),
            PathBuf::from("wanted/leaf/item")
        ],
        paths(walker),
    );
}

#[test]
fn test_subtree_configuration_rejects_non_normal_relative_paths() {
    let directory = tempdir().expect("fixture directory should exist");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    for path in [
        Path::new("/absolute"),
        Path::new("../parent"),
        Path::new("./current"),
        Path::new("nested/./current"),
    ] {
        let walker = filesystem
            .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
            .expect("walker should open");
        let error = walker.with_descend_subtree(path).expect_err("invalid path should fail");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    }
}

#[test]
fn test_file_and_missing_subtrees_keep_matching_entries_and_ancestors() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let open = |subtree: &str| {
        filesystem
            .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
            .expect("walker should open")
            .with_descend_subtree(Path::new(subtree))
            .expect("subtree should be valid")
    };
    assert_eq!(
        vec![
            PathBuf::from("other"),
            PathBuf::from("wanted"),
            PathBuf::from("wanted/leaf"),
            PathBuf::from("wanted/leaf/item")
        ],
        paths(open("wanted/leaf/item")),
    );
    assert_eq!(
        vec![
            PathBuf::from("other"),
            PathBuf::from("wanted"),
            PathBuf::from("wanted/leaf")
        ],
        paths(open("wanted/absent")),
    );
}

#[test]
fn test_empty_subtree_is_unrestricted_and_depth_limit_still_applies() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let empty = filesystem
        .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("walker should open")
        .with_descend_subtree(Path::new(""))
        .expect("empty subtree should be valid");
    assert_eq!(
        vec![
            PathBuf::from("other"),
            PathBuf::from("other/deep"),
            PathBuf::from("other/deep/leaf"),
            PathBuf::from("other/deep/leaf/item"),
            PathBuf::from("wanted"),
            PathBuf::from("wanted/leaf"),
            PathBuf::from("wanted/leaf/item"),
        ],
        paths(empty),
    );
    let shallow = filesystem
        .list_with_options(
            directory.path(),
            &LocalListOptions::new().with_recursive().with_max_depth(1),
        )
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("subtree should be valid");
    assert_eq!(vec![PathBuf::from("other"), PathBuf::from("wanted")], paths(shallow));
}

#[test]
fn test_zero_entry_budget_remains_enforced_with_subtree_pruning() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let mut walker = filesystem
        .list_with_options(
            directory.path(),
            &LocalListOptions::new().with_recursive().with_max_entries(0),
        )
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("subtree should be valid");
    assert_eq!(
        LocalFileErrorKind::ResourceLimit,
        walker
            .next()
            .expect("first entry should hit budget")
            .expect_err("budget should fail")
            .kind(),
    );
}

#[cfg(unix)]
#[test]
fn test_subtree_pruning_preserves_symlink_policy_and_reopen_behavior() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    symlink("other/deep", directory.path().join("outside-link")).expect("link should be created");
    let mut filesystem = LocalFileSystem::host().expect("host filesystem should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("reject policy should be accepted");
    let rejected = filesystem
        .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("subtree should be valid");
    assert!(!paths(rejected).contains(&PathBuf::from("outside-link/deep/leaf")));

    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect("follow policy should be accepted");
    let followed = filesystem
        .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("subtree should be valid");
    assert!(!paths(followed).contains(&PathBuf::from("outside-link/deep/leaf")));

    let reopened = filesystem
        .list_with_options(
            directory.path(),
            &LocalListOptions::new()
                .with_recursive()
                .with_max_open_directories(1)
                .with_reopen_policy(LocalDirectoryReopenPolicy::Reopen),
        )
        .expect("walker should open")
        .with_descend_subtree(Path::new("wanted/leaf"))
        .expect("subtree should be valid");
    assert!(paths(reopened).contains(&PathBuf::from("wanted/leaf/item")));
}

#[test]
fn test_subtree_configuration_is_rejected_after_iteration_starts() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let mut walker = filesystem
        .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("walker should open");
    assert!(walker.next().expect("root has entries").is_ok());
    let error = walker
        .with_descend_subtree(Path::new("wanted"))
        .expect_err("late configuration should fail");
    assert_eq!(LocalFileErrorKind::InvalidState, error.kind());
}

#[cfg(unix)]
#[test]
fn test_subtree_descent_compares_unix_non_utf8_native_components() {
    use std::os::unix::ffi::OsStringExt;

    let directory = tempdir().expect("fixture directory should exist");
    let target = PathBuf::from(std::ffi::OsString::from_vec(b"wanted-\xff".to_vec())).join("leaf");
    fs::create_dir_all(directory.path().join(&target)).expect("non-UTF-8 target should be created");
    fs::write(directory.path().join(&target).join("item"), b"wanted").expect("target file should be created");
    fs::create_dir_all(directory.path().join("other/deep")).expect("sibling subtree should be created");

    let host = LocalFileSystem::host().expect("host filesystem should open");
    let host_paths = paths(
        host.list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
            .expect("host walker should open")
            .with_descend_subtree(&target)
            .expect("non-UTF-8 target should be accepted"),
    );
    let rooted = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let rooted_paths = paths(
        rooted
            .list_with_options(Path::new("."), &LocalListOptions::new().with_recursive())
            .expect("rooted walker should open")
            .with_descend_subtree(&target)
            .expect("non-UTF-8 target should be accepted"),
    );

    for observed in [host_paths, rooted_paths] {
        assert!(observed.contains(&target));
        assert!(observed.contains(&target.join("item")));
        assert!(!observed.contains(&PathBuf::from("other/deep")));
    }
}

#[cfg(windows)]
#[test]
fn test_subtree_descent_compares_windows_unicode_native_components() {
    let directory = tempdir().expect("fixture directory should exist");
    let target = PathBuf::from("目标目录").join("叶子");
    fs::create_dir_all(directory.path().join(&target)).expect("Unicode target should be created");
    fs::write(directory.path().join(&target).join("项目.txt"), b"wanted")
        .expect("Unicode target file should be created");
    fs::create_dir_all(directory.path().join("其他目录/深层")).expect("sibling subtree should be created");

    let host = LocalFileSystem::host().expect("host filesystem should open");
    let host_paths = paths(
        host.list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
            .expect("host walker should open")
            .with_descend_subtree(&target)
            .expect("Unicode target should be accepted"),
    );
    let rooted = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let rooted_paths = paths(
        rooted
            .list_with_options(Path::new("."), &LocalListOptions::new().with_recursive())
            .expect("rooted walker should open")
            .with_descend_subtree(&target)
            .expect("Unicode target should be accepted"),
    );

    for observed in [host_paths, rooted_paths] {
        assert!(observed.contains(&target));
        assert!(observed.contains(&target.join("项目.txt")));
        assert!(!observed.contains(&PathBuf::from("其他目录/深层")));
    }
}

#[test]
fn test_default_walker_remains_unrestricted_without_subtree_configuration() {
    let directory = tempdir().expect("fixture directory should exist");
    fixture(directory.path());
    let host = LocalFileSystem::host().expect("host filesystem should open");
    let host_paths = paths(
        host.list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
            .expect("host walker should open"),
    );
    let rooted = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let rooted_paths = paths(
        rooted
            .list_with_options(Path::new("."), &LocalListOptions::new().with_recursive())
            .expect("rooted walker should open"),
    );

    for observed in [host_paths, rooted_paths] {
        assert!(observed.contains(&PathBuf::from("other/deep/leaf/item")));
        assert!(observed.contains(&PathBuf::from("wanted/leaf/item")));
    }
}

#[cfg(feature = "test-support")]
#[test]
fn test_pruned_host_branch_does_not_trigger_descent_fault_hook() {
    use qubit_local_files::test_support::install_test_fault;

    let directory = tempdir().expect("fixture directory should exist");
    fs::create_dir_all(directory.path().join("other/deep")).expect("unrelated branch should be created");
    fs::write(directory.path().join("wanted"), b"target").expect("target file should be created");
    let _fault = install_test_fault("walker-descend-canonicalize").expect("descent fault should install");
    let host = LocalFileSystem::host().expect("host filesystem should open");
    let walker = host
        .list_with_options(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("host walker should open")
        .with_descend_subtree(Path::new("wanted"))
        .expect("file subtree should be accepted");

    let observed = paths(walker);

    assert!(observed.contains(&PathBuf::from("wanted")));
    assert!(observed.contains(&PathBuf::from("other")));
    assert!(!observed.contains(&PathBuf::from("other/deep")));
}

#[test]
fn test_subtree_pruning_uses_less_entry_budget_than_unfiltered_walk() {
    let directory = tempdir().expect("fixture directory should exist");
    for branch in ["branch-a", "branch-b", "branch-c", "branch-d"] {
        fs::create_dir_all(directory.path().join(branch)).expect("sibling directory should be created");
        fs::write(directory.path().join(branch).join("child"), b"unrelated")
            .expect("sibling descendant should be created");
    }
    fs::write(directory.path().join("wanted"), b"target").expect("target file should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let options = LocalListOptions::new().with_recursive().with_max_entries(5);
    let filtered = filesystem
        .list_with_options(directory.path(), &options)
        .expect("filtered walker should open")
        .with_descend_subtree(Path::new("wanted"))
        .expect("target file subtree should be accepted")
        .collect::<Result<Vec<_>, _>>()
        .expect("root entries should fit the exact budget when siblings are pruned");
    let mut filtered_paths = filtered
        .iter()
        .map(|entry| entry.relative_path().to_path_buf())
        .collect::<Vec<_>>();
    filtered_paths.sort();
    assert_eq!(
        vec![
            PathBuf::from("branch-a"),
            PathBuf::from("branch-b"),
            PathBuf::from("branch-c"),
            PathBuf::from("branch-d"),
            PathBuf::from("wanted"),
        ],
        filtered_paths,
    );

    let unfiltered = filesystem
        .list_with_options(directory.path(), &options)
        .expect("unfiltered walker should open")
        .collect::<Result<Vec<_>, _>>()
        .expect_err("descending into sibling contents should exceed the root-entry budget");
    assert_eq!(LocalFileErrorKind::ResourceLimit, unfiltered.kind());
}
