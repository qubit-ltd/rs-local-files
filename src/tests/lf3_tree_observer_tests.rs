// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;

use crate::LocalFileKind;
use crate::LocalFileSystem;
use crate::directory::LocalTreeEntry;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTreeValidationOptions;
use crate::policy::LocalPrivacyRequirement;

#[test]
fn lf3_tree_observer_streams_created_entry() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"entry".as_slice();
    let observation = stage
        .write_file_new(Path::new("file"), &mut bytes, 5)
        .expect("file should be written");
    assert_eq!(observation.metadata().kind(), LocalFileKind::File);
    assert!(observation.identity().is_some());
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("observer should open");
    let first = observer
        .next()
        .expect("entry should be yielded")
        .expect("entry should observe");
    assert_eq!(first.relative_path(), Path::new("file"));
    assert_eq!(first.observation().metadata().kind(), LocalFileKind::File);
    assert!(observer.next().is_none());
}

#[test]
fn lf_review_tree_finite_depth_accepts_complete_shallow_tree() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    let expected = [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("expected")];
    let validation = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1));
    stage
        .verify_entries(&expected, &validation)
        .expect("a fully observed depth-one tree is within budget");
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf_review_tree_depth_zero_and_boundary() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let empty = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("empty stage");
    empty
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(0)),
        )
        .expect("empty tree is within depth zero");
    empty.cleanup().expect("empty cleanup");

    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    let expected = [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("expected")];
    stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1)),
        )
        .expect("direct child is within depth one");
    let error = stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(0)),
        )
        .expect_err("direct child exceeds depth zero");
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::ResourceLimit);
    stage.cleanup().expect("stage cleanup");
}

#[cfg(unix)]
#[test]
fn lf_review_tree_directory_links_are_entries() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let root = fixture.path().join(stage.relative_name());
    std::fs::create_dir(root.join("dir")).expect("directory");
    std::fs::write(root.join("dir/file"), b"x").expect("file");
    symlink("dir", root.join("alias")).expect("directory link");
    let mut entries = Vec::new();
    let options = LocalListOptions::new().with_symlink_policy(crate::policy::LocalSymlinkPolicy::FollowWithinScope);
    for item in stage.observe_tree_with_options(&options).expect("observer") {
        let item = item.expect("entry");
        entries.push((item.relative_path().to_path_buf(), item.observation().metadata().kind()));
    }
    assert!(
        entries
            .iter()
            .any(|(path, kind)| path == Path::new("alias") && *kind == LocalFileKind::Symlink)
    );
    assert!(!entries.iter().any(|(path, _)| path == Path::new("alias/file")));
    let expected = [
        ("dir", LocalFileKind::Directory),
        ("dir/file", LocalFileKind::File),
        ("alias", LocalFileKind::Symlink),
    ]
    .map(|(path, kind)| LocalTreeEntry::new(Path::new(path).to_path_buf(), kind).expect("expected"));
    stage
        .verify_entries(&expected, &LocalTreeValidationOptions::new())
        .expect("entry-only tree matches");
    stage.cleanup().expect("cleanup");
}

#[cfg(unix)]
#[test]
fn lf_review_tree_dangling_and_loop_links_are_entries() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let root = fixture.path().join(stage.relative_name());
    symlink("missing", root.join("dangling")).expect("dangling link");
    symlink(".", root.join("loop")).expect("loop link");
    let expected = ["dangling", "loop"]
        .map(|path| LocalTreeEntry::new(Path::new(path).to_path_buf(), LocalFileKind::Symlink).expect("expected"));
    stage
        .verify_entries(&expected, &LocalTreeValidationOptions::new())
        .expect("link targets are not traversed");
    let failure = stage
        .verify_entries(&expected, &LocalTreeValidationOptions::new().with_reject_symlinks())
        .expect_err("explicit symlink rule rejects links");
    assert_eq!(failure.kind(), crate::error::LocalFileErrorKind::RequirementNotMet);
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf_review_tree_duplicate_expected_paths_are_invalid_options() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let duplicate = [
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("entry"),
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("entry"),
    ];
    let error = stage
        .verify_entries(&duplicate, &LocalTreeValidationOptions::new())
        .expect_err("duplicate expected paths are invalid");
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::InvalidOptions);
    let different_kinds = [
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("entry"),
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::Directory).expect("entry"),
    ];
    let error = stage
        .verify_entries(&different_kinds, &LocalTreeValidationOptions::new())
        .expect_err("same path with different kinds is still a duplicate");
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::InvalidOptions);
}

#[test]
fn lf_review_tree_expected_path_budget_is_checked_before_matching() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let expected = [LocalTreeEntry::new(Path::new("long-name").to_path_buf(), LocalFileKind::File).expect("entry")];
    let error = stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_seen_name_bytes(2)),
        )
        .expect_err("expected path bytes exceed budget before observing tree");
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::ResourceLimit);
}

#[test]
fn lf_review_tree_zero_deadline_and_entry_limit_fail() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    let expected = [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("expected")];
    for validation in [
        LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_deadline(std::time::Duration::ZERO)),
        LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(0)),
    ] {
        let error = stage
            .verify_entries(&expected, &validation)
            .expect_err("zero budget rejects tree");
        assert_eq!(error.kind(), crate::error::LocalFileErrorKind::ResourceLimit);
    }
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf_review_tree_observer_depth_still_limits_display() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    assert!(
        stage
            .observe_tree_with_options(&LocalListOptions::new().with_max_depth(0))
            .expect("observer")
            .next()
            .is_none()
    );
    assert!(
        stage
            .observe_tree_with_options(&LocalListOptions::new().with_max_depth(1))
            .expect("observer")
            .next()
            .expect("entry")
            .is_ok()
    );
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf3_tree_observer_defers_entry_reads_until_next() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut first_bytes = b"first".as_slice();
    stage
        .write_file_new(Path::new("first"), &mut first_bytes, 5)
        .expect("first file should be written");
    let mut second_bytes = b"second".as_slice();
    stage
        .write_file_new(Path::new("second"), &mut second_bytes, 6)
        .expect("second file should be written");
    crate::directory::reset_test_observation_count();
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("observer should open");
    assert_eq!(0, crate::directory::test_observation_count());
    observer
        .next()
        .expect("first entry should be yielded")
        .expect("entry should observe");
    assert_eq!(1, crate::directory::test_observation_count());
    observer
        .next()
        .expect("second entry should be yielded")
        .expect("entry should observe");
    assert_eq!(2, crate::directory::test_observation_count());
    assert!(observer.next().is_none());
}

#[test]
fn lf3_tree_entry_rejects_escaping_paths_and_special_kinds() {
    assert!(LocalTreeEntry::new(Path::new("../escape").to_path_buf(), LocalFileKind::File).is_err());
    assert!(LocalTreeEntry::new(Path::new("/absolute").to_path_buf(), LocalFileKind::File).is_err());
    assert!(LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::Fifo).is_err());
    assert!(LocalTreeEntry::new(Path::new("dir/file").to_path_buf(), LocalFileKind::File).is_ok());
}

#[test]
fn lf3_tree_validation_checks_exact_entries_and_rejects_symlink_opt_in() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"x".as_slice();
    stage
        .write_file_new(Path::new("file"), &mut bytes, 1)
        .expect("file should be written");
    let expected =
        [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("entry should validate")];
    stage
        .verify_entries(&expected, &LocalTreeValidationOptions::new())
        .expect("exact entries should match");
    assert!(stage.verify_entries(&[], &LocalTreeValidationOptions::new()).is_err());
    let _ = fs::remove_file(fixture.path().join(stage.relative_name()).join("file"));
}

#[test]
fn lf3_tree_write_new_enforces_no_replace_and_stream_limit() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut too_many = b"long".as_slice();
    assert!(stage.write_file_new(Path::new("limited"), &mut too_many, 3).is_err());
    assert!(
        stage
            .directory()
            .entry_metadata(Path::new("limited"))
            .expect("metadata query should succeed")
            .is_none()
    );
    let mut first = b"one".as_slice();
    stage
        .write_file_new(Path::new("file"), &mut first, 3)
        .expect("first create should succeed");
    let mut second = b"two".as_slice();
    assert!(stage.write_file_new(Path::new("file"), &mut second, 3).is_err());
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("observer should open");
    assert_eq!(
        observer
            .next()
            .expect("one item expected")
            .expect("item should observe")
            .relative_path(),
        Path::new("file")
    );
    assert!(observer.next().is_none());
}

#[test]
fn lf3_tree_cleanup_refuses_replaced_stage_name() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let name = stage.relative_name().to_os_string();
    let stage_path = fixture.path().join(&name);
    fs::rename(&stage_path, fixture.path().join("moved")).expect("original stage should be moved for replacement test");
    fs::create_dir(&stage_path).expect("replacement should be created");
    let failure = stage.cleanup().expect_err("cleanup must reject a replacement");
    assert_eq!(failure.cause().kind(), crate::error::LocalFileErrorKind::InvalidState);
    assert!(stage_path.is_dir(), "replacement entry must remain untouched");
    assert!(
        fixture.path().join("moved").is_dir(),
        "renamed original must remain available"
    );
}

#[test]
fn lf3_tree_cleanup_refuses_replaced_owned_child() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"original".as_slice();
    stage
        .write_file_new(Path::new("file"), &mut bytes, 8)
        .expect("file should be written");
    let stage_path = fixture.path().join(stage.relative_name());
    fs::rename(stage_path.join("file"), fixture.path().join("moved-file")).expect("original child should be moved");
    fs::write(stage_path.join("file"), b"replacement").expect("replacement should be created");
    let failure = stage.cleanup().expect_err("cleanup must reject a replaced owned child");
    assert_eq!(failure.cause().kind(), crate::error::LocalFileErrorKind::InvalidState);
    assert_eq!(
        b"replacement".as_slice(),
        fs::read(stage_path.join("file")).expect("replacement must remain")
    );
    assert_eq!(
        b"original".as_slice(),
        fs::read(fixture.path().join("moved-file")).expect("original must remain")
    );
}

#[cfg(unix)]
#[test]
fn lf3_tree_staged_directory_can_require_private_creation() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("private stage should be created");
    let metadata =
        fs::symlink_metadata(fixture.path().join(stage.relative_name())).expect("stage metadata should be readable");
    assert_eq!(0o700, metadata.permissions().mode() & 0o777);
    let mut bytes = b"private".as_slice();
    stage
        .write_file_new(Path::new("secret"), &mut bytes, 7)
        .expect("private file should be written");
    let metadata = fs::symlink_metadata(fixture.path().join(stage.relative_name()).join("secret"))
        .expect("private file metadata should be readable");
    assert_eq!(0o600, metadata.permissions().mode() & 0o777);
    let expected =
        [LocalTreeEntry::new(Path::new("secret").to_path_buf(), LocalFileKind::File).expect("entry should validate")];
    stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private access should validate independently");
    stage.cleanup().expect("stage cleanup should succeed");
}

#[cfg(unix)]
#[test]
fn lf3_tree_hardlink_validation_counts_links_inside_the_checked_tree() {
    use std::fs::hard_link;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"shared".as_slice();
    stage
        .write_file_new(Path::new("a"), &mut bytes, 6)
        .expect("source should be written");
    let stage_path = fixture.path().join(stage.relative_name());
    hard_link(stage_path.join("a"), stage_path.join("b")).expect("in-tree link should be created");
    let expected = [
        LocalTreeEntry::new(Path::new("a").to_path_buf(), LocalFileKind::File).expect("entry should validate"),
        LocalTreeEntry::new(Path::new("b").to_path_buf(), LocalFileKind::File).expect("entry should validate"),
    ];
    stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_reject_external_hardlinks(),
        )
        .expect("in-tree links should count toward the checked scope");
    hard_link(stage_path.join("a"), fixture.path().join("outside")).expect("external link should be created");
    assert!(
        stage
            .verify_entries(
                &expected,
                &LocalTreeValidationOptions::new().with_reject_external_hardlinks()
            )
            .is_err()
    );
}

#[test]
fn lf3_tree_validation_rejects_a_truncated_walk() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"x".as_slice();
    stage
        .write_file_new(Path::new("file"), &mut bytes, 1)
        .expect("file should be written");
    let expected =
        [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("entry should validate")];
    let options = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(0));
    assert!(stage.verify_entries(&expected, &options).is_err());
}

#[test]
fn lf3_tree_validation_enforces_name_entry_and_deadline_budgets() {
    use std::time::Duration;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"x".as_slice();
    stage
        .write_file_new(Path::new("file"), &mut bytes, 1)
        .expect("file should be written");
    let expected =
        [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("entry should validate")];
    let name_limit = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_seen_name_bytes(0));
    assert_eq!(
        crate::error::LocalFileErrorKind::ResourceLimit,
        stage
            .verify_entries(&expected, &name_limit)
            .expect_err("exact entry indexing must charge path bytes")
            .kind()
    );
    let entry_limit = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(0));
    assert!(stage.verify_entries(&expected, &entry_limit).is_err());
    let deadline = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_deadline(Duration::ZERO));
    assert!(stage.verify_entries(&expected, &deadline).is_err());
    let inconsistent = LocalTreeValidationOptions::new().with_exact_entries(Vec::new());
    assert_eq!(
        crate::error::LocalFileErrorKind::InvalidOptions,
        stage
            .verify_entries(&expected, &inconsistent)
            .expect_err("conflicting exact sets must reject")
            .kind()
    );
}

#[test]
fn lf3_tree_observer_respects_open_directory_limit() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let _first = stage
        .directory()
        .create_directory_with_options(Path::new("one"), &crate::options::LocalCreateDirectoryOptions::new())
        .expect("first directory should be created");
    let _nested = stage
        .directory()
        .create_directory_with_options(
            Path::new("one/two"),
            &crate::options::LocalCreateDirectoryOptions::new(),
        )
        .expect("nested directory should be created");
    let mut bytes = b"leaf".as_slice();
    stage
        .write_file_new(Path::new("one/two/leaf"), &mut bytes, 4)
        .expect("leaf should be written");
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new().with_max_open_directories(1))
        .expect("bounded observer should open");
    let entries = observer
        .by_ref()
        .collect::<crate::error::LocalResult<Vec<_>>>()
        .expect("directory reopening should stay within the handle limit");
    assert!(
        entries
            .iter()
            .any(|entry| entry.relative_path() == Path::new("one/two/leaf"))
    );
}

#[test]
fn lf3_tree_create_error_retains_structured_failure() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let error = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_max_attempts(0))
        .expect_err("zero attempts must be rejected");
    assert_eq!(crate::error::LocalFileErrorKind::InvalidOptions, error.cause().kind());
    assert_eq!(
        crate::outcome::LocalTreeMutationState::NotStarted,
        error.mutation_state()
    );
    assert!(error.into_parts().1.is_none());
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_tree_private_stage_retries_name_collisions_without_deleting_conflicts() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let fault =
        crate::local::install_test_fault("temp-private-directory-collision").expect("collision fault should install");
    let stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new()
                .with_privacy(LocalPrivacyRequirement::Required)
                .with_max_attempts(2),
        )
        .expect("stage allocation should retry after collision");
    drop(fault);
    stage.cleanup().expect("created stage should clean up");
}

#[cfg(unix)]
#[test]
fn lf3_tree_private_stage_does_not_claim_or_cleanup_replaced_name() {
    use std::sync::Arc;
    use std::sync::Mutex;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let fixture_path = fixture.path().to_path_buf();
    let moved_original = fixture.path().join("moved-original");
    let moved_original_for_hook = moved_original.clone();
    let replacement_path = Arc::new(Mutex::new(None));
    let replacement_path_for_hook = Arc::clone(&replacement_path);
    let _hook = crate::directory::install_stage_created_hook(move |name| {
        let created = fixture_path.join(name);
        std::fs::rename(&created, &moved_original_for_hook).expect("move original private stage");
        std::fs::create_dir(&created).expect("install replacement directory");
        std::fs::write(created.join("sentinel"), b"replacement").expect("write replacement sentinel");
        *replacement_path_for_hook.lock().expect("replacement path lock") = Some(created);
    });

    let error = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect_err("replaced stage name must be rejected");
    assert_eq!(
        error.cause().kind(),
        crate::error::LocalFileErrorKind::ChangedDuringRead
    );
    drop(error);
    let replacement_path = replacement_path
        .lock()
        .expect("replacement path lock")
        .clone()
        .expect("replacement path should be recorded");
    assert_eq!(
        std::fs::read(replacement_path.join("sentinel")).expect("replacement must remain"),
        b"replacement"
    );
    assert!(
        moved_original.is_dir(),
        "original entry is not confused with its replacement"
    );
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_tree_partial_private_stage_retains_identity_and_cleanup_owner() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let fault = crate::local::install_test_fault("lf3-tree-private-directory-after-open")
        .expect("post-open fault should install");
    let error = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new()
                .with_privacy(LocalPrivacyRequirement::Required)
                .with_max_attempts(1),
        )
        .expect_err("injected post-open failure should retain the partial stage");
    drop(fault);
    assert_eq!(crate::outcome::LocalTreeMutationState::Started, error.mutation_state());
    let name = error
        .stage_name()
        .expect("partial stage name should be available")
        .to_os_string();
    assert!(
        error.stage_identity().is_some(),
        "partial stage identity should be available"
    );
    assert_eq!(1, error.retained_resources().len());
    let (cause, stage, mut resources) = error.into_parts();
    assert!(stage.is_none());
    assert!(
        cause.temp_recovery().is_some(),
        "native partial owner should remain attached to the cause"
    );
    resources
        .pop()
        .expect("tree cleanup owner should be retained")
        .cleanup()
        .expect("identity-matching partial stage should clean up");
    drop(cause);
    assert!(!fixture.path().join(name).exists());
}

#[test]
fn lf3_tree_recovery_location_survives_drop_and_reopens_by_identity() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let name = stage.relative_name().to_os_string();
    let location = stage.retain_in_place();
    assert!(location.identity().is_some());
    let path = location.namespace_path().to_path_buf();
    drop(location);
    assert!(
        path.is_dir(),
        "dropping a recovery location must not delete the retained tree"
    );
    let identity = parent
        .observe_entry(Path::new(&name))
        .expect("retained stage should be observable")
        .identity()
        .cloned()
        .expect("identity should be available");
    let reopened = parent
        .open_staged_directory(Path::new(&name), &identity, &LocalTreeValidationOptions::new())
        .expect("stage should reopen only by observed identity");
    reopened.cleanup().expect("reopened stage should clean up");
    assert!(!path.exists());
}

#[test]
fn lf3_tree_cleanup_limit_failure_retains_retryable_owner() {
    use std::time::Duration;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let name = stage.relative_name().to_os_string();
    stage.set_cleanup_limits(crate::options::LocalTempCleanupLimits::new().with_deadline(Duration::ZERO));
    let error = stage
        .cleanup()
        .expect_err("zero cleanup deadline must retain the stage");
    assert_eq!(crate::error::LocalFileErrorKind::ResourceLimit, error.cause().kind());
    let mut resources = error.into_retained_resources();
    let mut resource = resources.pop().expect("cleanup failure must retain an owner");
    resource.set_cleanup_limits(crate::options::LocalTempCleanupLimits::new());
    resource
        .cleanup()
        .expect("owner should clean after the limit is relaxed");
    assert!(!fixture.path().join(name).exists());
}

#[cfg(unix)]
#[test]
fn lf3_tree_observer_observes_symlink_entry_without_following_it() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let stage_path = fixture.path().join(stage.relative_name());
    fs::write(stage_path.join("target"), b"target").expect("target should exist");
    symlink("target", stage_path.join("link")).expect("link should exist");
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("observer should open");
    let mut saw_link = false;
    for entry in &mut observer {
        let entry = entry.expect("each entry should be observed");
        if entry.relative_path() == Path::new("link") {
            saw_link = true;
            assert_eq!(entry.observation().metadata().kind(), LocalFileKind::Symlink);
            assert!(entry.observation().identity().is_some());
        }
    }
    assert!(saw_link);
    let expected = [
        LocalTreeEntry::new(Path::new("target").to_path_buf(), LocalFileKind::File)
            .expect("target entry should validate"),
        LocalTreeEntry::new(Path::new("link").to_path_buf(), LocalFileKind::Symlink)
            .expect("link entry should validate"),
    ];
    assert!(
        stage
            .verify_entries(&expected, &LocalTreeValidationOptions::new().with_reject_symlinks())
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn lf3_tree_special_file_rejection_is_independent() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let fifo = fixture.path().join(stage.relative_name()).join("pipe");
    let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("fixture path has no NUL");
    // SAFETY: `path` is NUL terminated and points to a writable child path in the
    // fixture.
    assert_eq!(0, unsafe { libc::mkfifo(path.as_ptr(), 0o600) });
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("observer should open");
    let observed = observer
        .next()
        .expect("FIFO should be yielded")
        .expect("FIFO should be observed");
    assert_eq!(LocalFileKind::Fifo, observed.observation().metadata().kind());
    assert!(observer.next().is_none());
    assert_eq!(
        crate::error::LocalFileErrorKind::RequirementNotMet,
        stage
            .verify_entries(&[], &LocalTreeValidationOptions::new().with_reject_special_files())
            .expect_err("special-file rule should reject FIFO")
            .kind()
    );
}
