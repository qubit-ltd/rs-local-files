// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(any(unix, windows))]
use std::io::{
    Read,
    Write,
};

use qubit_local_files::rooted;
#[cfg(any(unix, windows))]
use qubit_local_files::{
    read,
    write,
};

/// Verifies rooted recursive removal handles a deep tree on a small stack.
#[cfg(target_os = "linux")]
#[test]
fn test_rooted_remove_tree_handles_deep_tree_on_small_stack() {
    const CHILD_ENV: &str = "QUBIT_LOCAL_FILES_ROOTED_REMOVE_SMALL_STACK_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let executable = std::env::current_exe()
            .expect("current test executable should be available");
        let status = std::process::Command::new(executable)
            .args([
                "--exact",
                "rooted::root_tests::test_rooted_remove_tree_handles_deep_tree_on_small_stack",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .current_dir(std::env::temp_dir())
            .status()
            .expect("small-stack child test should launch");
        assert!(status.success(), "small-stack child test should pass");
        return;
    }

    let worker = std::thread::Builder::new()
        .name("small-stack-rooted-remove-test".to_owned())
        .stack_size(128 * 1024)
        .spawn(|| {
            let temporary =
                tempfile::tempdir().expect("temporary root should be created");
            let mut native_path = temporary.path().join("tree");
            for _ in 0..512 {
                native_path.push("d");
            }
            std::fs::create_dir_all(&native_path)
                .expect("deep rooted tree should be created");
            std::fs::write(native_path.join("leaf"), b"payload")
                .expect("deep rooted leaf should be written");
            let root =
                rooted::Root::open(temporary.path()).expect("root should open");
            let tree = rooted::Path::new("tree")
                .expect("rooted tree path should validate");

            root.remove_tree(&tree)
                .expect("deep rooted tree should be removed");
            assert!(!temporary.path().join("tree").exists());
        })
        .expect("small-stack thread should launch");
    worker.join().expect("small-stack thread should complete");
}

/// Verifies rooted metadata preserves final symbolic links without abandoning
/// the opened directory authority.
#[cfg(unix)]
#[test]
fn test_rooted_symlink_metadata_reports_final_entry_kind() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let file =
        rooted::Path::new("value.txt").expect("the path should validate");
    let link =
        rooted::Path::new("value-link").expect("the link path should validate");
    let directory = rooted::Path::new("nested")
        .expect("the directory path should validate");
    let socket = rooted::Path::new("value.sock")
        .expect("the socket path should validate");
    std::fs::write(temp.path().join("value.txt"), b"value")
        .expect("the regular file should be created");
    std::fs::create_dir(temp.path().join("nested"))
        .expect("the nested directory should be created");
    symlink("value.txt", temp.path().join("value-link"))
        .expect("the final symbolic link should be created");
    let _listener = UnixListener::bind(temp.path().join("value.sock"))
        .expect("the Unix socket should be created");

    assert_eq!(
        rooted::EntryKind::Directory,
        root.metadata().expect("root metadata").kind()
    );
    assert_eq!(
        rooted::EntryKind::File,
        root.symlink_metadata(&file).expect("file metadata").kind(),
    );
    assert_eq!(
        rooted::EntryKind::Symlink,
        root.symlink_metadata(&link).expect("link metadata").kind(),
    );
    assert_eq!(
        rooted::EntryKind::Directory,
        root.symlink_metadata(&directory)
            .expect("directory metadata")
            .kind(),
    );
    assert_eq!(
        rooted::EntryKind::Other,
        root.symlink_metadata(&socket)
            .expect("socket metadata")
            .kind(),
    );

    let rooted_metadata = root.symlink_metadata(&file).expect("file metadata");
    let native_metadata =
        std::fs::symlink_metadata(temp.path().join("value.txt"))
            .expect("native file metadata should be available");
    assert_eq!(
        native_metadata.accessed().ok(),
        rooted_metadata.accessed_at()
    );
    assert_eq!(
        native_metadata.modified().ok(),
        rooted_metadata.modified_at()
    );
    assert_eq!(native_metadata.len(), rooted_metadata.size());

    let rooted_root_metadata = root.metadata().expect("root metadata");
    let native_root_metadata =
        std::fs::metadata(temp.path()).expect("native root metadata");
    assert_eq!(
        native_root_metadata.created().ok(),
        rooted_root_metadata.created_at()
    );
}

/// Verifies the focused rooted API exposes durable atomic replacement.
#[cfg(any(unix, windows))]
#[test]
fn test_rooted_atomic_write_commits_and_aborts() {
    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let path =
        rooted::Path::new("value.txt").expect("the path should validate");

    let mut writer = root
        .begin_atomic_write(&path)
        .expect("the rooted atomic writer should begin");
    writer
        .write_all(b"committed")
        .expect("the replacement should be staged");
    writer.commit().expect("the replacement should commit");
    assert_eq!(
        b"committed",
        std::fs::read(temp.path().join("value.txt"))
            .expect("the committed file should be readable")
            .as_slice(),
    );

    let mut writer = root
        .begin_atomic_write(&path)
        .expect("the second rooted atomic writer should begin");
    writer
        .write_all(b"aborted")
        .expect("the abandoned replacement should be staged");
    writer.abort().expect("the replacement should abort");
    assert_eq!(
        b"committed",
        std::fs::read(temp.path().join("value.txt"))
            .expect("the original committed file should remain")
            .as_slice(),
    );
}

/// Verifies rooted ordinary I/O returns native file handles.
#[cfg(any(unix, windows))]
#[test]
fn test_rooted_read_and_write_use_open_directory_authority() {
    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    assert_eq!(
        std::path::absolute(temp.path()).expect("path should be absolute"),
        root.path(),
    );
    let path = rooted::Path::new("nested/value.txt")
        .expect("the path should validate");

    let mut file = root
        .open_writer(&path, &write::OpenOptions::default().with_parents())
        .expect("the rooted writer should open");
    file.write_all(b"rooted").expect("bytes should be written");
    drop(file);

    let mut file = root
        .open_reader(&path, &read::OpenOptions::default())
        .expect("the rooted reader should open");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("bytes should be read");
    assert_eq!("rooted", content);
}

/// Verifies lexical escapes are rejected before rooted I/O.
#[test]
fn test_rooted_path_rejects_escape_components() {
    assert!(rooted::Path::new("../escape").is_err());
    assert!(rooted::Path::new("/absolute").is_err());
    assert!(rooted::Path::new("nested/./file").is_err());
}

/// Verifies only existing directories can become rooted authorities.
#[test]
fn test_rooted_root_rejects_missing_paths_and_regular_files() {
    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let file = temp.path().join("file");
    std::fs::write(&file, b"payload").expect("the file should be created");

    assert!(rooted::Root::open(&file).is_err());
    assert!(rooted::Root::open(&temp.path().join("missing")).is_err());
}

/// Verifies symbolic links cannot redirect an ordinary rooted open.
#[cfg(unix)]
#[test]
fn test_rooted_open_rejects_symbolic_link_escape() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let outside =
        tempfile::tempdir().expect("an outside directory should be created");
    std::fs::write(outside.path().join("secret"), b"secret")
        .expect("the outside fixture should be written");
    symlink(outside.path(), temp.path().join("linked"))
        .expect("the intermediate link should be created");

    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let path =
        rooted::Path::new("linked/secret").expect("the path should validate");
    assert!(
        root.open_reader(&path, &read::OpenOptions::default())
            .is_err()
    );
}

/// Verifies rooted native writers expose every supported opening mode.
#[cfg(any(unix, windows))]
#[test]
fn test_rooted_native_writer_supports_all_modes() {
    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let path =
        rooted::Path::new("value.txt").expect("the path should validate");

    let mut file = root
        .open_writer(&path, &write::OpenOptions::new(write::Mode::CreateNew))
        .expect("create-new should create a missing file");
    file.write_all(b"first").expect("bytes should be written");
    drop(file);

    let mut file = root
        .open_writer(
            &path,
            &write::OpenOptions::new(write::Mode::OpenExistingAtStart),
        )
        .expect("open-existing should open the file");
    file.write_all(b"F").expect("bytes should be overwritten");
    drop(file);

    let mut file = root
        .open_writer(
            &path,
            &write::OpenOptions::new(write::Mode::AppendExisting),
        )
        .expect("append-existing should open the file");
    file.write_all(b"!").expect("bytes should be appended");
    drop(file);

    let created_path =
        rooted::Path::new("created.txt").expect("the path should validate");
    let mut file = root
        .open_writer(
            &created_path,
            &write::OpenOptions::new(write::Mode::AppendOrCreate),
        )
        .expect("append-or-create should create a missing file");
    file.write_all(b"created")
        .expect("bytes should be appended to the new file");
    drop(file);

    let mut file = root
        .open_writer(
            &path,
            &write::OpenOptions::new(write::Mode::CreateOrTruncate),
        )
        .expect("create-or-truncate should open the file");
    file.write_all(b"reset").expect("bytes should be written");
    drop(file);

    assert_eq!(
        b"reset",
        std::fs::read(temp.path().join("value.txt"))
            .expect("the file should be readable")
            .as_slice(),
    );
    assert_eq!(
        b"created",
        std::fs::read(temp.path().join("created.txt"))
            .expect("the created file should be readable")
            .as_slice(),
    );
}

/// Verifies namespace operations remain anchored after the diagnostic root is
/// renamed.
#[cfg(any(unix, windows))]
#[test]
fn test_rooted_namespace_operations_survive_root_rename() {
    let temp = tempfile::tempdir().expect("a temporary parent should exist");
    let original = temp.path().join("original");
    let renamed = temp.path().join("renamed");
    std::fs::create_dir(&original).expect("the original root should exist");
    let root = rooted::Root::open(&original).expect("the root should open");
    std::fs::rename(&original, &renamed).expect("the root should be renamed");

    let nested = rooted::Path::new("nested/deeper")
        .expect("the directory path should validate");
    root.create_dir_all(&nested)
        .expect("the nested directory should be created");
    std::fs::write(renamed.join("nested/deeper/value.txt"), b"value")
        .expect("the fixture should be written");

    let directory =
        rooted::Path::new("nested").expect("the path should validate");
    let entries = root
        .read_dir(&directory)
        .expect("the rooted directory should be listed");
    assert_eq!(1, entries.len());
    assert_eq!(std::ffi::OsStr::new("deeper"), entries[0].name());
    assert_eq!(rooted::EntryKind::Directory, entries[0].metadata().kind());

    let source = rooted::Path::new("nested/deeper/value.txt")
        .expect("the source should validate");
    let destination = rooted::Path::new("nested/deeper/moved.txt")
        .expect("the destination should validate");
    root.rename_without_replacing(&source, &destination)
        .expect("the file should be renamed");
    root.remove_tree(&directory)
        .expect("the directory should be removed recursively");
    assert!(!renamed.join("nested").exists());
}

/// Verifies rooted directory creation and no-replace rename semantics.
#[cfg(any(unix, windows))]
#[test]
fn test_rooted_named_directory_and_rename_operations() {
    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let directory =
        rooted::Path::new("a/b").expect("the directory path should validate");

    root.create_dir_all(&directory)
        .expect("recursive creation should succeed");
    root.create_dir_all(&directory)
        .expect("exists-ok creation should succeed");
    root.ensure_dir_all(&directory)
        .expect("ensure-dir-all should accept an existing directory");
    let ensured = rooted::Path::new("ensured")
        .expect("the directory path should validate");
    root.ensure_dir(&ensured)
        .expect("ensure-dir should create one missing directory");

    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");
    std::fs::write(temp.path().join("source"), b"source")
        .expect("the source should be written");
    std::fs::write(temp.path().join("destination"), b"destination")
        .expect("the destination should be written");

    assert!(
        root.rename_without_replacing(&source, &destination)
            .is_err()
    );
    root.rename(&source, &destination)
        .expect("overwrite rename should succeed");
    assert_eq!(
        b"source",
        std::fs::read(temp.path().join("destination"))
            .expect("the destination should remain readable")
            .as_slice(),
    );
}

/// Verifies named removal operations enforce their advertised entry types.
#[cfg(any(unix, windows))]
#[test]
fn test_rooted_named_removal_operations_are_type_strict() {
    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    std::fs::write(temp.path().join("file"), b"value")
        .expect("the file should be written");
    std::fs::create_dir(temp.path().join("directory"))
        .expect("the directory should be created");
    let file =
        rooted::Path::new("file").expect("the file path should validate");
    let directory = rooted::Path::new("directory")
        .expect("the directory path should validate");

    assert!(root.remove_empty_dir(&file).is_err());
    assert!(root.remove_file(&directory).is_err());
    root.remove_file(&file).expect("the file should be removed");
    root.remove_empty_dir(&directory)
        .expect("the empty directory should be removed");
}

/// Verifies recursive removal unlinks symbolic links instead of traversing
/// them.
#[cfg(unix)]
#[test]
fn test_rooted_recursive_remove_does_not_follow_links() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let outside =
        tempfile::tempdir().expect("an outside directory should be created");
    std::fs::write(outside.path().join("preserved"), b"outside")
        .expect("the outside fixture should be written");
    std::fs::create_dir(temp.path().join("tree"))
        .expect("the tree should be created");
    symlink(outside.path(), temp.path().join("tree/link"))
        .expect("the link should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let tree = rooted::Path::new("tree").expect("the path should validate");

    root.remove_tree(&tree)
        .expect("the rooted tree should be removed");

    assert!(!temp.path().join("tree").exists());
    assert_eq!(
        b"outside",
        std::fs::read(outside.path().join("preserved"))
            .expect("the outside fixture should remain")
            .as_slice(),
    );
}

/// Verifies rooted namespace errors remain contained and permissions are
/// applied through opened descriptors.
#[cfg(unix)]
#[test]
fn test_rooted_namespace_error_and_permission_paths() {
    use std::os::unix::fs::{
        MetadataExt,
        symlink,
    };

    let temp = tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(temp.path().join("first"), b"first")
        .expect("the first file should be written");
    std::fs::write(temp.path().join("second"), b"second")
        .expect("the second file should be written");
    std::fs::hard_link(temp.path().join("first"), temp.path().join("alias"))
        .expect("the hard link should be created");
    std::fs::create_dir(temp.path().join("nonempty"))
        .expect("the nonempty directory should be created");
    std::fs::write(temp.path().join("nonempty/child"), b"child")
        .expect("the child should be written");
    symlink("first", temp.path().join("link"))
        .expect("the symbolic link should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");

    let entries = root
        .read_root_dir()
        .expect("the root directory should be listed");
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].name() <= pair[1].name())
    );

    let direct = rooted::Path::new("direct").expect("the path should validate");
    root.create_dir(&direct)
        .expect("the direct directory should be created");
    let first = rooted::Path::new("first").expect("the path should validate");
    assert!(root.create_dir(&first).is_err());
    assert!(root.read_dir(&first).is_err());

    let nonempty =
        rooted::Path::new("nonempty").expect("the path should validate");
    assert!(root.remove_empty_dir(&nonempty).is_err());

    root.set_permissions(&first, rooted::Permissions::from_unix_mode(0o640))
        .expect("the file permissions should be set");
    root.set_permissions(&direct, rooted::Permissions::from_unix_mode(0o750))
        .expect("the directory permissions should be set");
    assert_eq!(
        0o640,
        std::fs::metadata(temp.path().join("first"))
            .expect("the file metadata should be readable")
            .mode()
            & 0o777,
    );
    assert_eq!(
        0o750,
        std::fs::metadata(temp.path().join("direct"))
            .expect("the directory metadata should be readable")
            .mode()
            & 0o777,
    );

    let first_metadata = root
        .symlink_metadata(&first)
        .expect("the file should be inspected");
    let alias = rooted::Path::new("alias").expect("the path should validate");
    let alias_metadata = root
        .symlink_metadata(&alias)
        .expect("the alias should be inspected");
    let second = rooted::Path::new("second").expect("the path should validate");
    let second_metadata = root
        .symlink_metadata(&second)
        .expect("the second file should be inspected");
    assert_eq!(Some(0o640), first_metadata.permissions().unix_mode());
    assert!(first_metadata.is_same_file(&alias_metadata));
    assert!(!first_metadata.is_same_file(&second_metadata));

    let link = rooted::Path::new("link").expect("the path should validate");
    assert!(
        root.set_permissions(&link, rooted::Permissions::from_unix_mode(0o600))
            .is_err()
    );
    let missing =
        rooted::Path::new("missing").expect("the path should validate");
    assert!(
        root.set_permissions(
            &missing,
            rooted::Permissions::from_unix_mode(0o600)
        )
        .is_err()
    );
}

/// Verifies rooted permission updates report an operating-system rejection.
#[cfg(target_os = "linux")]
#[test]
fn test_rooted_permission_update_reports_read_only_pseudo_file() {
    let root = rooted::Root::open(std::path::Path::new("/proc"))
        .expect("the proc filesystem root should open");
    let path = rooted::Path::new("version").expect("the path should validate");

    root.set_permissions(&path, rooted::Permissions::from_unix_mode(0o600))
        .expect_err("the proc pseudo-file should reject chmod");
}
