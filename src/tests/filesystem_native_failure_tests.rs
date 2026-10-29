// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::observation::LocalObservationRequirement;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::options::LocalSnapshotOptions;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

const CONTENT: &[u8] = b"preserved payload";

fn rooted_fixture() -> (tempfile::TempDir, LocalFileSystem) {
    let fixture = tempfile::tempdir().expect("fixture");
    fs::create_dir(fixture.path().join("work")).expect("working directory");
    fs::write(fixture.path().join("work/item"), CONTENT).expect("source payload");
    let mut filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    filesystem
        .set_current_directory(Path::new("/work"))
        .expect("virtual working directory");
    (fixture, filesystem)
}

fn assert_context(error: &LocalFileError, operation: LocalFileOperation, path: Option<&Path>, cwd: Option<&Path>) {
    assert_eq!(error.operation(), operation);
    assert_eq!(error.path(), path);
    assert_eq!(error.current_directory(), cwd);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    assert!(error.to_string().contains("injected native I/O contract failure"));
}

#[test]
fn snapshot_observation_failures_keep_read_context_and_allow_retry() {
    let (fixture, rooted) = rooted_fixture();
    let host = LocalFileSystem::host().expect("host filesystem");
    let native = fs::canonicalize(fixture.path().join("work/item")).expect("native path");
    let options = LocalSnapshotOptions::builder(64).build();
    for (filesystem, operand, expected_path, cwd) in [
        (&host, native.as_path(), native.as_path(), None),
        (
            &rooted,
            Path::new("item"),
            Path::new("/work/item"),
            Some(Path::new("/work")),
        ),
    ] {
        for occurrence in [1, 2] {
            let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, occurrence);
            let error = filesystem
                .read_consistent_with_options(operand, &options)
                .expect_err("initial or final observation should fail");
            assert!(fault.fired(), "observation {occurrence}");
            assert_context(&error, LocalFileOperation::Read, Some(expected_path), cwd);
            drop(fault);
            let snapshot = filesystem
                .read_consistent_with_options(operand, &options)
                .expect("retry snapshot");
            assert_eq!(snapshot.bytes(), CONTENT);
            assert_eq!(fs::read(&native).expect("source preserved"), CONTENT);
        }
    }
}

#[test]
fn snapshot_and_prefix_native_read_failures_keep_context_and_discard_partial_data() {
    let (fixture, rooted) = rooted_fixture();
    let host = LocalFileSystem::host().expect("host filesystem");
    let native = fs::canonicalize(fixture.path().join("work/item")).expect("native path");
    let options = LocalSnapshotOptions::builder(64).build();
    for (filesystem, operand, expected_path, cwd) in [
        (&host, native.as_path(), native.as_path(), None),
        (
            &rooted,
            Path::new("item"),
            Path::new("/work/item"),
            Some(Path::new("/work")),
        ),
    ] {
        for snapshot in [false, true] {
            for occurrence in [1, 2] {
                let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::Read, occurrence);
                let error = if snapshot {
                    filesystem
                        .read_consistent_with_options(operand, &options)
                        .expect_err("snapshot read fails")
                } else {
                    filesystem.read_prefix(operand, 64).expect_err("prefix read fails")
                };
                assert!(fault.fired());
                assert_context(&error, LocalFileOperation::Read, Some(expected_path), cwd);
                drop(fault);
                assert_eq!(filesystem.read_prefix(operand, 64).expect("retry read"), CONTENT);
                assert_eq!(fs::read(&native).expect("source preserved"), CONTENT);
            }
        }
    }
}

#[cfg(not(windows))]
#[test]
fn host_binding_root_open_failure_keeps_the_failed_parent_coordinate() {
    let (fixture, _) = rooted_fixture();
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let native = fs::canonicalize(fixture.path().join("work/item")).expect("native path");
    let options = LocalObservedReadOptions::new()
        .with_native_observation(LocalObservationRequirement::Required)
        .with_path_binding(LocalPathBindingOptions::new());
    for (occurrence, path) in [(1, native.as_path()), (2, native.parent().expect("parent"))] {
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::RootOpen, occurrence);
        let error = filesystem
            .open_observed_reader(&native, &options)
            .expect_err("binding root cannot open");
        assert!(fault.fired(), "root open {occurrence}");
        assert_context(&error, LocalFileOperation::OpenRoot, Some(path), None);
        drop(fault);
        let mut reader = filesystem
            .open_observed_reader(&native, &options)
            .expect("retry bound Host reader");
        assert_eq!(reader.read_to_end_limited(64).expect("source bytes"), CONTENT);
        assert_eq!(fs::read(&native).expect("source preserved"), CONTENT);
    }
}

#[test]
fn rooted_reader_metadata_initialization_failure_preserves_reader_context() {
    let (fixture, filesystem) = rooted_fixture();
    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::ReaderMetadata, 1);
    let error = filesystem
        .open_reader(Path::new("item"))
        .expect_err("opened reader metadata fails");
    assert!(fault.fired());
    assert_context(
        &error,
        LocalFileOperation::OpenReader,
        Some(Path::new("/work/item")),
        Some(Path::new("/work")),
    );
    drop(fault);
    assert_eq!(filesystem.read_all(Path::new("item")).expect("retry read"), CONTENT);
    assert_eq!(
        fs::read(fixture.path().join("work/item")).expect("source intact"),
        CONTENT
    );
}

#[test]
fn observed_reader_binding_authority_failures_keep_the_failing_phase_context() {
    let (fixture, filesystem) = rooted_fixture();
    let options = LocalObservedReadOptions::new()
        .with_native_observation(LocalObservationRequirement::Required)
        .with_path_binding(LocalPathBindingOptions::new());
    for (occurrence, operation, cwd) in [
        (1, LocalFileOperation::OpenRoot, None),
        (2, LocalFileOperation::OpenReader, Some(Path::new("/work"))),
        (3, LocalFileOperation::OpenRoot, None),
    ] {
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, occurrence);
        let error = filesystem
            .open_observed_reader(Path::new("item"), &options)
            .expect_err("binding authority acquisition should fail");
        assert!(fault.fired(), "authority clone {occurrence}");
        assert_context(&error, operation, Some(Path::new("/work/item")), cwd);
        drop(fault);
        let mut reader = filesystem
            .open_observed_reader(Path::new("item"), &options)
            .expect("retry bound reader");
        assert_eq!(
            reader.read_to_end_limited(64).expect("bound source remains readable"),
            CONTENT
        );
        assert_eq!(
            fs::read(fixture.path().join("work/item")).expect("source preserved"),
            CONTENT
        );
    }
}

#[test]
fn rooted_reader_and_metadata_failures_preserve_virtual_coordinates() {
    let (fixture, filesystem) = rooted_fixture();
    for reader_operation in [false, true] {
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::EntryMetadata, 1);
        let error = if reader_operation {
            filesystem
                .open_reader(Path::new("item"))
                .expect_err("reader metadata fails")
        } else {
            filesystem
                .target_metadata(Path::new("item"))
                .expect_err("target metadata fails")
        };
        assert!(fault.fired());
        assert_context(
            &error,
            if reader_operation {
                LocalFileOperation::OpenReader
            } else {
                LocalFileOperation::Metadata
            },
            Some(Path::new("/work/item")),
            Some(Path::new("/work")),
        );
        drop(fault);
        assert_eq!(filesystem.read_all(Path::new("item")).expect("retry read"), CONTENT);
        assert_eq!(
            fs::read(fixture.path().join("work/item")).expect("native source preserved"),
            CONTENT
        );
    }
}

#[test]
fn rooted_copy_destination_metadata_failures_preserve_both_operands_and_allow_retry() {
    use crate::options::LocalCopyConflictPolicy;
    use crate::options::LocalCopyOptions;
    use crate::outcome::LocalCopyFailureState;

    let (fixture, filesystem) = rooted_fixture();
    let target = fixture.path().join("work/destination");
    let options = LocalCopyOptions::new().with_conflict(LocalCopyConflictPolicy::Overwrite);
    for occurrence in [2, 3] {
        fs::write(&target, b"previous destination").expect("old target");
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::EntryMetadata, occurrence);
        let failure = filesystem
            .copy_with_options(Path::new("item"), Path::new("destination"), &options)
            .expect_err("destination inspection must fail before replacement");
        assert!(fault.fired(), "entry observation {occurrence}");
        assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
        assert_eq!(failure.request_source_path(), Some(Path::new("/work/item")));
        assert_eq!(failure.request_target_path(), Some(Path::new("/work/destination")));
        assert_context(
            failure.error(),
            LocalFileOperation::Copy,
            Some(Path::new("/work/destination")),
            Some(Path::new("/work")),
        );
        assert_eq!(
            fs::read(&target).expect("old destination retained"),
            b"previous destination"
        );
        assert_eq!(
            fs::read(fixture.path().join("work/item")).expect("source retained"),
            CONTENT
        );
        drop(fault);
        let _ = filesystem
            .copy_with_options(Path::new("item"), Path::new("destination"), &options)
            .expect("retry copy");
        assert_eq!(fs::read(&target).expect("retry publication"), CONTENT);
    }
}

#[test]
fn rooted_copy_metadata_failure_matrix_preserves_sources_and_proven_destination_state() {
    use crate::options::LocalCopyConflictPolicy;
    use crate::options::LocalCopyOptions;
    use crate::options::LocalCopyTypeConflictPolicy;
    use crate::outcome::LocalCopyFailureState;
    use crate::policy::LocalAtomicityRequirement;

    for (source_tree, target_tree) in [(false, false), (true, true), (false, true), (true, false)] {
        let mut reached_success_without_fault = false;
        for occurrence in 1..=64 {
            let (fixture, filesystem) = rooted_fixture();
            let source = fixture.path().join("work/source");
            let target = fixture.path().join("work/destination");
            if source_tree {
                fs::create_dir_all(source.join("nested")).expect("source tree");
                fs::write(source.join("alpha"), b"source a").expect("first source leaf");
                fs::write(source.join("nested/beta"), b"source b").expect("second source leaf");
            } else {
                fs::write(&source, CONTENT).expect("source file");
            }
            if target_tree {
                fs::create_dir(&target).expect("target directory");
                fs::write(target.join("old"), b"previous destination").expect("old target child");
            } else {
                fs::write(&target, b"previous destination").expect("old target file");
            }
            let options = LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_atomicity(LocalAtomicityRequirement::NotRequired);
            let options = if source_tree {
                options.with_tree_source()
            } else {
                options.with_entry_source()
            };
            let verify_source = || {
                if source_tree {
                    assert_eq!(
                        fs::read(source.join("alpha")).expect("source alpha preserved"),
                        b"source a"
                    );
                    assert_eq!(
                        fs::read(source.join("nested/beta")).expect("source beta preserved"),
                        b"source b"
                    );
                } else {
                    assert_eq!(fs::read(&source).expect("source file preserved"), CONTENT);
                }
            };
            let verify_complete = || {
                if source_tree {
                    assert_eq!(fs::read(target.join("alpha")).expect("copied alpha"), b"source a");
                    assert_eq!(fs::read(target.join("nested/beta")).expect("copied beta"), b"source b");
                    if target_tree {
                        assert_eq!(
                            fs::read(target.join("old")).expect("unrelated target child retained"),
                            b"previous destination"
                        );
                    }
                } else {
                    assert_eq!(fs::read(&target).expect("copied file"), CONTENT);
                }
            };
            let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::EntryMetadata, occurrence);
            let result = filesystem.copy_with_options(Path::new("source"), Path::new("destination"), &options);
            let fired = fault.fired();
            drop(fault);
            verify_source();
            match result {
                Ok(_) => {
                    verify_complete();
                    if !fired {
                        reached_success_without_fault = true;
                        break;
                    }
                }
                Err(failure) => {
                    assert!(
                        fired,
                        "unexpected native failure for source_tree={source_tree}, target_tree={target_tree}, nth={occurrence}: {failure:?}"
                    );
                    assert_eq!(failure.error().operation(), LocalFileOperation::Copy);
                    assert_eq!(failure.error().current_directory(), Some(Path::new("/work")));
                    assert_eq!(failure.request_source_path(), Some(Path::new("/work/source")));
                    assert_eq!(failure.request_target_path(), Some(Path::new("/work/destination")));
                    assert!(
                        failure
                            .error()
                            .to_string()
                            .contains("injected native I/O contract failure")
                    );
                    assert!(failure.partial_stats().files() <= if source_tree { 2 } else { 1 });
                    assert!(failure.partial_stats().bytes() <= if source_tree { 16 } else { CONTENT.len() as u64 });
                    if failure.state() == LocalCopyFailureState::Unchanged {
                        assert_eq!(failure.partial_stats().files(), 0);
                        assert_eq!(failure.partial_stats().bytes(), 0);
                        if target_tree {
                            assert_eq!(
                                fs::read(target.join("old")).expect("unchanged target child"),
                                b"previous destination"
                            );
                        } else {
                            assert_eq!(
                                fs::read(&target).expect("unchanged target file"),
                                b"previous destination"
                            );
                        }
                    } else if failure.state() == LocalCopyFailureState::Published {
                        verify_complete();
                    } else if source_tree && target.is_dir() {
                        for leaf in ["alpha", "nested/beta"] {
                            if target.join(leaf).exists() {
                                assert_eq!(
                                    fs::read(target.join(leaf)).expect("published leaf readable"),
                                    fs::read(source.join(leaf)).expect("source leaf readable")
                                );
                            }
                        }
                    } else if !source_tree && target.is_file() {
                        let bytes = fs::read(&target).expect("remaining destination");
                        assert!(bytes == CONTENT || bytes == b"previous destination");
                    }
                    let _ = filesystem
                        .copy_with_options(Path::new("source"), Path::new("destination"), &options)
                        .expect("retry after one-shot metadata failure");
                    verify_complete();
                    verify_source();
                }
            }
        }
        assert!(
            reached_success_without_fault,
            "metadata traversal exceeded bounded matrix for source_tree={source_tree}, target_tree={target_tree}"
        );
    }
}

#[cfg(all(feature = "digest", any(target_os = "linux", target_os = "macos")))]
#[test]
fn conditional_authority_failure_keeps_writer_context_and_creates_no_staging() {
    use crate::options::LocalWriteMode;
    use crate::options::LocalWriteOptions;
    use crate::options::LocalWritePrecondition;
    use crate::policy::LocalSymlinkPolicy;
    use sha2::Digest;
    use sha2::Sha256;

    let (fixture, mut filesystem) = rooted_fixture();
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("conditional link policy");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 1);
    let error = filesystem
        .open_conditional_writer::<Sha256>(
            Path::new("item"),
            LocalWritePrecondition::Content(Sha256::digest(CONTENT)),
            &options,
        )
        .expect_err("conditional authority clone fails");
    assert!(fault.fired());
    assert_context(
        &error,
        LocalFileOperation::OpenWriter,
        Some(Path::new("/work/item")),
        Some(Path::new("/work")),
    );
    assert_eq!(
        fs::read_dir(fixture.path().join("work"))
            .expect("directory readable")
            .count(),
        1
    );
    drop(fault);
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            Path::new("item"),
            LocalWritePrecondition::Content(Sha256::digest(CONTENT)),
            &options,
        )
        .expect("retry conditional writer");
    let _ = writer.abort().expect("abort retry without publishing");
    assert_eq!(
        fs::read(fixture.path().join("work/item")).expect("old payload preserved"),
        CONTENT
    );
}

#[cfg(feature = "test-support")]
#[test]
fn rooted_native_reader_open_failure_keeps_reader_context_and_source() {
    let (fixture, filesystem) = rooted_fixture();
    let fault = crate::test_support::install_test_fault("rooted-copy-source-open-native").expect("native open fault");
    let error = filesystem
        .open_reader(Path::new("item"))
        .expect_err("native source open fails");
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.path(), Some(Path::new("/work/item")));
    assert_eq!(error.current_directory(), Some(Path::new("/work")));
    assert!(error.typed_source().is_some());
    drop(fault);
    assert_eq!(filesystem.read_all(Path::new("item")).expect("retry read"), CONTENT);
    assert_eq!(
        fs::read(fixture.path().join("work/item")).expect("source intact"),
        CONTENT
    );
}

#[cfg(unix)]
#[test]
fn host_dangling_final_link_errors_preserve_link_and_target_then_allow_retry() {
    let fixture = tempfile::tempdir().expect("fixture");
    let parent = fs::canonicalize(fixture.path()).expect("canonical fixture");
    let link = parent.join("link");
    let target = parent.join("missing-target");
    let sentinel = parent.join("sentinel");
    fs::write(&sentinel, CONTENT).expect("unrelated user data");
    std::os::unix::fs::symlink("missing-target", &link).expect("dangling final link");
    let filesystem = LocalFileSystem::host().expect("default Host follows final links");
    for error in [
        filesystem
            .target_metadata(&link)
            .expect_err("dangling target metadata fails"),
        filesystem
            .canonicalize(&link)
            .expect_err("dangling final link cannot canonicalize"),
    ] {
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.kind(), crate::LocalFileErrorKind::NotFound);
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
        assert_eq!(error.path(), Some(link.as_path()));
        assert_eq!(error.current_directory(), None);
        assert!(error.typed_source().is_some());
        assert_eq!(
            fs::read_link(&link).expect("link target text retained"),
            Path::new("missing-target")
        );
        assert!(
            fs::symlink_metadata(&link)
                .expect("link entry retained")
                .file_type()
                .is_symlink()
        );
        assert!(!target.exists(), "failed metadata lookup must not create the target");
        assert_eq!(fs::read(&sentinel).expect("unrelated data preserved"), CONTENT);
    }
    fs::write(&target, b"target created for retry").expect("supply real target");
    assert_eq!(
        filesystem.target_metadata(&link).expect("retry target metadata").kind(),
        crate::LocalFileKind::File
    );
    assert_eq!(filesystem.canonicalize(&link).expect("retry canonical path"), target);
    assert_eq!(
        fs::read_link(&link).expect("link still unchanged"),
        Path::new("missing-target")
    );
    assert_eq!(
        fs::read(&target).expect("retry target unchanged"),
        b"target created for retry"
    );
}

#[test]
fn rooted_directory_qualified_canonicalization_reports_each_root_metadata_failure() {
    let fixture = tempfile::tempdir().expect("fixture");
    let sentinel = fixture.path().join("sentinel");
    fs::write(&sentinel, CONTENT).expect("rooted user data");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    assert_eq!(
        filesystem.current_directory().expect("initial virtual PWD"),
        Path::new("/")
    );
    for occurrence in [1, 2] {
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::RootMetadata, occurrence);
        let error = filesystem
            .canonicalize(Path::new("."))
            .expect_err("root metadata phase fails");
        assert!(fault.fired(), "root metadata occurrence {occurrence}");
        assert_context(
            &error,
            LocalFileOperation::Metadata,
            Some(Path::new("/")),
            Some(Path::new("/")),
        );
        assert_eq!(fs::read(&sentinel).expect("user data preserved"), CONTENT);
        assert_eq!(
            filesystem.current_directory().expect("virtual PWD preserved"),
            Path::new("/")
        );
        drop(fault);
        assert_eq!(
            filesystem
                .canonicalize(Path::new("."))
                .expect("retry qualified canonicalization"),
            Path::new("/")
        );
        assert_eq!(fs::read(&sentinel).expect("user data intact after retry"), CONTENT);
    }
}
