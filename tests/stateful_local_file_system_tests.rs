// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful filesystem, virtual PWD, and explicit-option contract tests.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_local_files::LocalCopyConflictPolicy;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileKind;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalListOptions;
use qubit_local_files::LocalPersistStage;
use qubit_local_files::LocalResult;
use qubit_local_files::LocalSymlinkPolicy;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;
use tempfile::tempdir;

#[test]
fn host_captures_an_instance_pwd_and_clone_configuration_is_independent() {
    let process_pwd = std::env::current_dir().expect("process PWD should be readable");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    assert_eq!(filesystem.current_directory(), process_pwd);

    let directory = tempdir().expect("temporary directory should be created");
    filesystem
        .set_current_directory(directory.path())
        .expect("instance PWD should change");
    let mut cloned = filesystem.clone();
    cloned
        .set_default_list_options(LocalListOptions::new().with_recursive())
        .expect("clone defaults should be configurable");

    assert_eq!(filesystem.current_directory(), directory.path());
    assert!(!filesystem.default_list_options().recursive());
    assert!(cloned.default_list_options().recursive());
}

#[test]
fn rooted_paths_observe_chroot_style_absolute_and_relative_semantics() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("work/project")).expect("fixture PWD should be created");
    fs::write(directory.path().join("at-root"), b"root").expect("root fixture should be written");
    fs::write(directory.path().join("work/value"), b"work").expect("work fixture should be written");

    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    assert_eq!(filesystem.current_directory(), Path::new("/"));
    assert_eq!(filesystem.metadata(Path::new("/at-root")).unwrap().len(), 4);
    filesystem
        .set_current_directory(Path::new("/work/project"))
        .expect("virtual PWD should change");
    assert_eq!(filesystem.metadata(Path::new("../value")).unwrap().len(), 4);
    assert_eq!(filesystem.metadata(Path::new("../../at-root")).unwrap().len(), 4);

    let error = filesystem
        .metadata(Path::new("../../../outside"))
        .expect_err("virtual-root escape must fail before I/O");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    assert_eq!(error.current_directory(), Some(Path::new("/work/project")));
}

#[test]
fn rooted_setters_are_transactional() {
    let directory = tempdir().expect("temporary root should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    let original_pwd = filesystem.current_directory().to_path_buf();
    let original_policy = filesystem.symlink_policy();

    assert!(filesystem.set_current_directory(Path::new("missing")).is_err());
    assert_eq!(filesystem.current_directory(), original_pwd);
    assert!(
        filesystem
            .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
            .is_err()
    );
    assert_eq!(filesystem.symlink_policy(), original_policy);
}

#[cfg(unix)]
#[test]
fn host_reject_policy_applies_to_listing_roots_and_copy_parents() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::create_dir(&target).expect("target directory should be created");
    fs::write(target.join("source"), b"payload").expect("source should be written");
    let link = directory.path().join("link");
    symlink(&target, &link).expect("directory link should be created");

    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("Host should accept the reject policy");

    let list_error = filesystem
        .list(&link)
        .expect_err("listing through the final directory link should be rejected");
    assert_eq!(LocalFileErrorKind::Unsupported, list_error.kind());

    let destination = directory.path().join("copied");
    let copy_error = filesystem
        .copy(&link.join("source"), &destination)
        .expect_err("copying through an intermediate link should be rejected");
    assert_eq!(LocalFileErrorKind::Unsupported, copy_error.error().kind());
    assert!(!destination.exists());

    let list_options = LocalListOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope);
    let entries = filesystem
        .list_with_options(&link, &list_options)
        .expect("an explicit listing override should replace the instance policy")
        .collect::<LocalResult<Vec<_>>>()
        .expect("overridden listing should complete");
    assert_eq!(1, entries.len());

    let copy_options = LocalCopyOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope);
    let _ = filesystem
        .copy_with_options(&link.join("source"), &destination, &copy_options)
        .expect("an explicit copy override should replace the instance policy");
    assert_eq!(b"payload", fs::read(destination).unwrap().as_slice());
}

#[test]
fn explicit_options_replace_instance_defaults() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source should be written");
    fs::write(directory.path().join("target"), b"old").expect("target should be written");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_default_copy_options(LocalCopyOptions::new().with_conflict(LocalCopyConflictPolicy::Overwrite))
        .expect("default copy options should be accepted");

    let _ = filesystem
        .copy(Path::new("source"), Path::new("target"))
        .expect("ordinary copy should use the configured overwrite default");
    let error = filesystem
        .copy_with_options(Path::new("source"), Path::new("target"), &LocalCopyOptions::new())
        .expect_err("explicit options must not merge the overwrite default");
    assert_eq!(error.error().kind(), LocalFileErrorKind::AlreadyExists);
}

#[test]
fn host_temp_options_do_not_create_an_unrequested_parent() {
    let directory = tempdir().expect("temporary directory should be created");
    let file_parent = directory.path().join("missing-file-parent");
    let directory_parent = directory.path().join("missing-directory-parent");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");

    let file_error = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&file_parent))
        .expect_err("temporary-file creation must not create an unrequested parent");
    assert_eq!(LocalFileErrorKind::NotFound, file_error.kind());
    assert!(!file_parent.exists());

    let directory_error = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&directory_parent))
        .expect_err("temporary-directory creation must not create an unrequested parent");
    assert_eq!(LocalFileErrorKind::NotFound, directory_error.kind());
    assert!(!directory_parent.exists());
}

#[test]
fn callers_can_wrap_a_filesystem_in_their_own_lock() {
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let shared = Arc::new(Mutex::new(filesystem));
    let cloned = Arc::clone(&shared);
    let thread = std::thread::spawn(move || cloned.lock().unwrap().current_directory().to_path_buf());
    assert_eq!(thread.join().unwrap(), shared.lock().unwrap().current_directory());
}

#[cfg(unix)]
#[test]
fn rooted_constructor_follows_its_one_time_root_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let real_root = directory.path().join("real");
    fs::create_dir(&real_root).expect("real root should be created");
    fs::write(real_root.join("value"), b"payload").expect("fixture should be written");
    let root_link = directory.path().join("root-link");
    symlink(&real_root, &root_link).expect("root symlink should be created");

    let filesystem = LocalFileSystem::rooted(&root_link).expect("root constructor should follow its final symlink");
    assert_eq!(filesystem.diagnostic_root(), Some(root_link.as_path()));
    assert_eq!(filesystem.metadata(Path::new("/value")).unwrap().len(), 7);
}

#[cfg(unix)]
#[test]
fn rooted_symlink_targets_use_virtual_root_and_dot_parent_semantics() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("etc/nested")).expect("fixture directories should be created");
    fs::write(directory.path().join("etc/value"), b"payload").expect("fixture should be written");
    symlink("/etc", directory.path().join("absolute-link")).expect("absolute link should be created");
    symlink("./nested/../value", directory.path().join("etc/relative-link")).expect("relative link should be created");

    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    assert_eq!(filesystem.metadata(Path::new("/absolute-link/value")).unwrap().len(), 7,);
    assert_eq!(
        filesystem.metadata(Path::new("/etc/relative-link")).unwrap().kind(),
        LocalFileKind::Symlink,
    );
    assert_eq!(
        filesystem.read_prefix(Path::new("/etc/relative-link"), 16).unwrap(),
        b"payload",
    );
}

#[cfg(unix)]
#[test]
fn rooted_symlink_escape_and_cycles_are_rejected() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    symlink("../../outside", directory.path().join("escape")).expect("escape link should be created");
    symlink("second", directory.path().join("first")).expect("first cycle link should be created");
    symlink("first", directory.path().join("second")).expect("second cycle link should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");

    let escape = filesystem
        .open_reader(Path::new("/escape"))
        .expect_err("symlink traversal beyond virtual root must fail");
    assert_eq!(escape.kind(), LocalFileErrorKind::InvalidPath);
    let cycle = filesystem
        .open_reader(Path::new("/first"))
        .expect_err("symlink cycle must terminate");
    assert_eq!(cycle.kind(), LocalFileErrorKind::InvalidPath);
}

/// Verifies Rooted walkers expose reusable virtual paths while retaining
/// authority-local diagnostic hints separately.
#[cfg(unix)]
#[test]
fn rooted_walker_paths_and_symlink_entries_use_virtual_namespace_semantics() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("target")).expect("target directory should be created");
    fs::write(directory.path().join("target/payload"), b"payload").expect("payload should be written");
    symlink("/target", directory.path().join("link")).expect("virtual absolute link should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");

    let walker = filesystem
        .list_with_options(Path::new("/"), &LocalListOptions::new().with_recursive())
        .expect("Rooted root should be listable");
    assert_eq!(Path::new("/"), walker.root());
    let entries = walker
        .collect::<LocalResult<Vec<_>>>()
        .expect("Rooted traversal should complete");
    let link = entries
        .iter()
        .find(|entry| entry.path() == Path::new("/link"))
        .expect("logical link entry should be returned");
    assert_eq!(LocalFileKind::Symlink, link.metadata().kind());
    let linked_payload = entries
        .iter()
        .find(|entry| entry.path() == Path::new("/link/payload"))
        .expect("followed descendants should retain the logical link path");
    assert_eq!(Path::new("link/payload"), linked_payload.relative_path());
    assert_eq!(
        Some(directory.path().join("target/payload").as_path()),
        linked_payload.diagnostic_path(),
    );
}

/// Verifies a Rooted absolute symlink target of `/` re-enters the virtual root
/// rather than becoming an empty-path parsing failure.
#[cfg(unix)]
#[test]
fn rooted_walker_can_follow_a_symlink_to_virtual_root() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("payload"), b"payload").expect("payload should be written");
    symlink("/", directory.path().join("root-link")).expect("root link should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");

    let entries = filesystem
        .list(Path::new("/root-link"))
        .expect("a link to virtual root should be listable")
        .collect::<LocalResult<Vec<_>>>()
        .expect("virtual root link traversal should complete");
    assert!(
        entries
            .iter()
            .any(|entry| entry.path() == Path::new("/root-link/payload"))
    );
}

/// Verifies temporary resources retain creation-time PWD semantics and expose
/// only virtual namespace paths through their public identity APIs.
#[test]
fn rooted_temp_resources_retain_their_creation_pwd_snapshot() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("first")).expect("first PWD should be created");
    fs::create_dir_all(directory.path().join("second")).expect("second PWD should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/first"))
        .expect("first PWD should be selected");
    let mut temporary = filesystem.create_temp_file().expect("temporary file should be created");
    assert!(temporary.path().starts_with(Path::new("/first")));
    temporary
        .write_all(b"payload")
        .expect("temporary payload should be written");

    filesystem
        .set_current_directory(Path::new("/second"))
        .expect("filesystem PWD should be independently mutable");
    let outcome = temporary
        .persist(Path::new("published"))
        .expect("relative persistence should use the creation PWD snapshot");
    assert_eq!(Path::new("/first/published"), outcome.path());
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("first/published"))
            .expect("payload should publish")
            .as_slice(),
    );

    let temporary_directory = filesystem
        .create_temp_directory()
        .expect("temporary directory should be created in the second PWD");
    assert!(temporary_directory.path().starts_with(Path::new("/second")));
    assert!(
        temporary_directory
            .child(Path::new("child"))
            .expect("child path should resolve")
            .is_absolute()
    );
}

#[test]
fn temp_file_persist_preserves_directory_qualified_target_intent() {
    let directory = tempdir().expect("temporary root should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    let mut temporary = filesystem.create_temp_file().expect("temporary file should be created");
    temporary
        .write_all(b"payload")
        .expect("temporary payload should be written");

    let error = temporary
        .persist(Path::new("published/"))
        .expect_err("a temporary file cannot satisfy a directory-qualified target");

    assert_eq!(LocalPersistStage::ResolveTarget, error.stage());
    assert!(
        !directory.path().join("published").exists(),
        "rejected persistence must not publish a file",
    );
}

/// Verifies writers expose virtual destination identity and remain bound to
/// the PWD snapshot captured when they were opened.
#[test]
fn rooted_writer_retains_its_open_time_pwd_snapshot() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("first")).expect("first PWD should be created");
    fs::create_dir_all(directory.path().join("second")).expect("second PWD should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/first"))
        .expect("first PWD should be selected");
    let mut writer = filesystem
        .open_writer(Path::new("payload"))
        .expect("writer should open");
    assert_eq!(Path::new("/first/payload"), writer.path());
    assert_eq!(
        Some(directory.path().join("first/payload").as_path()),
        writer.diagnostic_path(),
    );

    filesystem
        .set_current_directory(Path::new("/second"))
        .expect("filesystem PWD should be independently mutable");
    writer.write_all(b"payload").expect("writer should accept bytes");
    let _ = writer
        .commit()
        .expect("writer should publish to its original destination");
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("first/payload"))
            .expect("payload should publish")
            .as_slice(),
    );
    assert!(!directory.path().join("second/payload").exists());
}

/// Verifies two-path failures retain normalized virtual operands and the PWD
/// snapshot used to bind them.
#[test]
fn rooted_copy_and_rename_failures_report_virtual_paths() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("work")).expect("PWD should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/work"))
        .expect("PWD should be selected");

    let copy = filesystem
        .copy(Path::new("missing"), Path::new("target"))
        .expect_err("missing copy source should fail");
    assert_eq!(Some(Path::new("/work/missing")), copy.request_source_path());
    assert_eq!(Some(Path::new("/work/target")), copy.request_target_path());
    assert_eq!(Some(Path::new("/work/missing")), copy.error().path());
    assert_eq!(Some(Path::new("/work")), copy.error().current_directory());

    let rename = filesystem
        .rename(Path::new("missing"), Path::new("target"))
        .expect_err("missing rename source should fail");
    assert_eq!(Some(Path::new("/work/missing")), rename.error().path());
    assert_eq!(Some(Path::new("/work/target")), rename.error().target());
    assert_eq!(Some(Path::new("/work")), rename.error().current_directory());
}

/// Verifies walker errors retain the PWD from walker creation, even after the
/// originating filesystem instance changes directory.
#[test]
fn rooted_walker_errors_retain_their_creation_pwd_snapshot() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("first/listing")).expect("first listing directory should be created");
    fs::create_dir(directory.path().join("second")).expect("second PWD should be created");
    fs::write(directory.path().join("first/listing/entry"), b"payload").expect("listing entry should be written");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/first"))
        .expect("first PWD should be selected");
    let mut walker = filesystem
        .list_with_options(Path::new("listing"), &LocalListOptions::new().with_max_entries(0))
        .expect("walker should open before consuming its entry budget");

    filesystem
        .set_current_directory(Path::new("/second"))
        .expect("filesystem PWD should change independently");
    let error = walker
        .next()
        .expect("the existing entry should consume the budget")
        .expect_err("the zero-entry budget should reject the entry");

    assert_eq!(Some(Path::new("/first")), error.current_directory());
    assert_eq!(Some(Path::new("/first/listing")), error.path());
}

/// Verifies validation and protected-root failures consistently retain the
/// instance PWD used by the public operation.
#[test]
fn rooted_facade_validation_errors_retain_pwd_context() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("work")).expect("PWD should be created");
    fs::write(directory.path().join("work/file"), b"payload").expect("file should be written");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/work"))
        .expect("PWD should be selected");

    let policy = filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect_err("Rooted scope should reject cross-scope following");
    assert_eq!(Some(Path::new("/work")), policy.current_directory());

    let options = filesystem
        .set_default_list_options(LocalListOptions::new().with_max_open_directories(0))
        .expect_err("zero open-directory budget should be rejected");
    assert_eq!(Some(Path::new("/work")), options.current_directory());

    let metadata = filesystem
        .metadata(Path::new("file/"))
        .expect_err("directory-qualified file should be rejected");
    assert_eq!(Some(Path::new("/work")), metadata.current_directory());

    let reader = filesystem
        .open_reader(Path::new("file/"))
        .expect_err("a reader cannot open directory-qualified file syntax");
    assert_eq!(Some(Path::new("/work")), reader.current_directory());

    let deletion = filesystem
        .delete_file(Path::new("file/"))
        .expect_err("a file deletion cannot discard directory intent");
    assert_eq!(Some(Path::new("/work")), deletion.current_directory());
    assert!(
        directory.path().join("work/file").is_file(),
        "rejected directory-qualified deletion must leave the file intact",
    );

    for (source, target) in [("file/", "copied"), ("file", "copied/")] {
        let copy = filesystem
            .copy(Path::new(source), Path::new(target))
            .expect_err("file copy cannot discard directory intent");
        assert_eq!(Some(Path::new("/work")), copy.error().current_directory());
        assert!(
            !directory.path().join("work/copied").exists(),
            "rejected directory-qualified copy must not publish a target",
        );
    }

    for (source, target) in [("file/", "renamed"), ("file", "renamed/")] {
        let rename = filesystem
            .rename(Path::new(source), Path::new(target))
            .expect_err("file rename cannot discard directory intent");
        assert_eq!(Some(Path::new("/work")), rename.error().current_directory());
        assert!(
            directory.path().join("work/file").is_file(),
            "rejected directory-qualified rename must leave the source intact",
        );
        assert!(
            !directory.path().join("work/renamed").exists(),
            "rejected directory-qualified rename must not publish a target",
        );
    }

    let root = filesystem
        .create_directory(Path::new("/"))
        .expect_err("the existing virtual root should be reported");
    assert_eq!(Some(Path::new("/work")), root.current_directory());

    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_max_attempts(0))
        .expect_err("zero temporary-name attempts should be rejected");
    assert_eq!(Some(Path::new("/work")), temporary.current_directory());
}

/// Verifies two-path lexical failures preserve both caller inputs and the one
/// PWD snapshot used to parse them.
#[test]
fn rooted_two_path_lexical_failures_preserve_request_context() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("work")).expect("PWD should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/work"))
        .expect("PWD should be selected");

    let copy = filesystem
        .copy(Path::new("source"), Path::new("../../target"))
        .expect_err("destination escape should fail during path binding");
    assert_eq!(Some(Path::new("source")), copy.request_source_path());
    assert_eq!(Some(Path::new("../../target")), copy.request_target_path());
    assert_eq!(Some(Path::new("source")), copy.error().path());
    assert_eq!(Some(Path::new("../../target")), copy.error().target());
    assert_eq!(Some(Path::new("/work")), copy.error().current_directory());

    let rename = filesystem
        .rename(Path::new("../../source"), Path::new("target"))
        .expect_err("source escape should fail during path binding");
    assert_eq!(Some(Path::new("../../source")), rename.error().path());
    assert_eq!(Some(Path::new("target")), rename.error().target());
    assert_eq!(Some(Path::new("/work")), rename.error().current_directory());
}

/// Verifies opened mutable resources keep their creation-time PWD for later
/// structured failures.
#[cfg(unix)]
#[test]
fn rooted_writer_and_temp_cleanup_errors_retain_creation_pwd() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("work")).expect("PWD should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/work"))
        .expect("PWD should be selected");

    let mut writer = filesystem.open_writer(Path::new("output")).expect("writer should open");
    let _ = writer.abort().expect("first abort should succeed");
    let writer_error = writer.abort().expect_err("a terminal writer cannot be aborted again");
    assert_eq!(Some(Path::new("/work")), writer_error.current_directory());

    let mut temporary = filesystem.create_temp_file().expect("temporary file should be created");
    let physical = directory.path().join(
        temporary
            .path()
            .strip_prefix(Path::new("/"))
            .expect("Rooted temp path should be virtual absolute"),
    );
    let moved = physical.with_extension("moved");
    fs::rename(&physical, &moved).expect("original identity should remain alive elsewhere");
    fs::write(&physical, b"replacement").expect("replacement entry should be written");
    let cleanup = temporary
        .cleanup()
        .expect_err("cleanup should reject an entry with a different identity");
    assert_eq!(Some(Path::new("/work")), cleanup.current_directory());
}

/// Verifies a temporary resource resolves failed persistence targets against
/// its own creation-time PWD rather than later filesystem state.
#[test]
fn rooted_temp_persist_errors_retain_creation_pwd() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("first")).expect("first PWD should be created");
    fs::create_dir_all(directory.path().join("second")).expect("second PWD should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_current_directory(Path::new("/first"))
        .expect("first PWD should be selected");
    let temporary = filesystem.create_temp_file().expect("temporary file should be created");
    filesystem
        .set_current_directory(Path::new("/second"))
        .expect("filesystem PWD should change independently");

    let error = temporary
        .persist(Path::new("../../escape"))
        .expect_err("persistence beyond the virtual root should fail");
    assert_eq!(Path::new("../../escape"), error.requested_target());
    assert_eq!(Some(Path::new("/first")), error.error().current_directory());
}
