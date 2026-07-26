// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::{
    Read,
    Write,
};

use qubit_local_files::{
    read,
    rooted,
    write,
};

/// Verifies rooted metadata preserves final symbolic links without abandoning
/// the opened directory authority.
#[cfg(unix)]
#[test]
fn test_rooted_symlink_metadata_reports_final_entry_kind() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let file =
        rooted::Path::new("value.txt").expect("the path should validate");
    let link =
        rooted::Path::new("value-link").expect("the link path should validate");
    std::fs::write(temp.path().join("value.txt"), b"value")
        .expect("the regular file should be created");
    symlink("value.txt", temp.path().join("value-link"))
        .expect("the final symbolic link should be created");

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
}

/// Verifies rooted ordinary I/O returns native file handles.
#[cfg(unix)]
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
#[cfg(unix)]
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
