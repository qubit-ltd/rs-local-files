// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use crate::LocalFileSystem;
use crate::LocalSymlinkPolicy;
use crate::error::LocalFileErrorKind;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalObservedReader;
use crate::options::LocalReadAllOptions;
use crate::options::LocalSnapshotOptions;

#[test]
fn lf3_read_complete_limit_utf8() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let path = fixture.path().join("bytes");
    std::fs::write(&path, [1_u8, 2, 3]).expect("write fixture");
    let fs = LocalFileSystem::host().expect("host filesystem");
    assert_eq!(
        fs.read_all_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(3))
            .unwrap(),
        [1, 2, 3]
    );
    assert_eq!(
        fs.read_all_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(2))
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::ResourceLimit
    );
    std::fs::write(&path, [255_u8]).expect("write invalid UTF-8");
    assert_eq!(
        fs.read_text(Path::new(path.as_os_str())).unwrap_err().kind(),
        LocalFileErrorKind::InvalidEncoding
    );
    assert!(
        fs.read_all_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(u64::MAX))
            .is_ok()
    );
    std::fs::write(&path, b"").expect("write empty file");
    assert!(
        fs.read_all_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(0))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn lf3_read_all_rejects_growth_at_sentinel() {
    use std::io::Write;

    let fixture = tempfile::tempdir().expect("temporary directory");
    let path = fixture.path().join("growing");
    std::fs::write(&path, b"abc").expect("write initial bytes");
    let fs = LocalFileSystem::host().expect("host filesystem");
    let appended_path = path.clone();
    let _hook = crate::observation::snapshot::install_read_hook(move || {
        std::fs::OpenOptions::new()
            .append(true)
            .open(appended_path)
            .expect("open append handle")
            .write_all(b"d")
            .expect("append growth byte");
    });
    assert_eq!(
        fs.read_all_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(3))
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::ResourceLimit,
    );
}

#[test]
fn lf3_read_snapshot_returns_same_handle_observations() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let path = fixture.path().join("snapshot");
    std::fs::write(&path, b"payload").expect("write fixture");
    let fs = LocalFileSystem::host().expect("host filesystem");
    let snapshot = fs
        .read_consistent_with_options(&path, &LocalSnapshotOptions::builder(7).build())
        .unwrap();
    assert_eq!(snapshot.bytes(), b"payload");
    assert_eq!(snapshot.before().identity(), snapshot.after().identity());
}

#[test]
fn lf3_read_rooted_paths_stay_virtual() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    std::fs::write(fixture.path().join("payload"), b"rooted").expect("write fixture");
    let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    assert_eq!(fs.canonicalize(Path::new("/")).unwrap(), Path::new("/"));
    assert_eq!(fs.canonicalize(Path::new("/payload")).unwrap(), Path::new("/payload"));
    assert_eq!(fs.target_metadata(Path::new("/payload")).unwrap().len(), 6);
}

#[test]
fn lf3_read_snapshot_detects_same_handle_mutation() {
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;

    let mut file = tempfile::tempfile().expect("temporary file");
    file.write_all(b"before").expect("write initial bytes");
    let reader =
        crate::LocalFileReader::from_file(file.try_clone().expect("clone file handle")).expect("construct reader");
    let observed = LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required)
        .expect("capture initial observation");
    let before = observed.observation().clone();
    file.set_len(0).expect("truncate file");
    file.seek(SeekFrom::Start(0)).expect("rewind file");
    file.write_all(b"changed content").expect("write changed bytes");
    let after = observed.observe_now().expect("refresh same handle");
    assert!(!crate::observation::snapshot::observations_match(
        &before,
        &after,
        b"before".len() as u64,
    ));
}

#[test]
fn lf3_read_consistent_rejects_mutation_during_collection() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let path = fixture.path().join("changing");
    std::fs::write(&path, b"before").expect("write initial bytes");
    let fs = LocalFileSystem::host().expect("host filesystem");
    let changed_path = path.clone();
    let _hook = crate::observation::snapshot::install_read_hook(move || {
        std::fs::write(changed_path, b"changed while collecting").expect("mutate opened file");
    });
    assert_eq!(
        fs.read_consistent_with_options(&path, &LocalSnapshotOptions::builder(64).build())
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::ChangedDuringRead,
    );
}

#[test]
#[cfg(unix)]
fn lf3_read_consistent_keeps_atomic_replacement_separate_from_open_handle() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let path = fixture.path().join("source");
    let moved = fixture.path().join("moved-original");
    std::fs::write(&path, b"opened object").expect("write initial bytes");
    let fs = LocalFileSystem::host().expect("host filesystem");
    let replaced_path = path.clone();
    let moved_path = moved.clone();
    let _hook = crate::observation::snapshot::install_read_hook(move || {
        std::fs::rename(&replaced_path, &moved_path).expect("rename original object");
        std::fs::write(&replaced_path, b"replacement object").expect("install replacement object");
    });
    let result = fs.read_consistent_with_options(&path, &LocalSnapshotOptions::builder(64).build());
    match result {
        Ok(snapshot) => assert_eq!(snapshot.bytes(), b"opened object"),
        Err(error) => assert_eq!(error.kind(), LocalFileErrorKind::ChangedDuringRead),
    }
    assert_eq!(std::fs::read(path).unwrap(), b"replacement object");
}

#[test]
#[cfg(unix)]
fn lf3_read_non_regular_fifo_is_rejected_without_content_read() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let fixture = tempfile::tempdir().expect("temporary directory");
    let fifo = fixture.path().join("pipe");
    let encoded = CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path has no NUL");
    let status = unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) };
    assert_eq!(status, 0, "create FIFO");
    let fs = LocalFileSystem::host().expect("host filesystem");
    assert_eq!(fs.read_all(&fifo).unwrap_err().kind(), LocalFileErrorKind::TypeConflict);
}

#[test]
#[cfg(unix)]
fn lf3_read_path_queries_follow_host_target_policy() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("temporary directory");
    let target = fixture.path().join("target");
    let link = fixture.path().join("link");
    std::fs::write(&target, b"content").expect("write target");
    symlink(&target, &link).expect("create symlink");
    let fs = LocalFileSystem::host().expect("host filesystem");
    assert_eq!(fs.target_metadata(&link).unwrap().len(), 7);
    assert_eq!(fs.canonicalize(&link).unwrap(), std::fs::canonicalize(&target).unwrap());
    let options = LocalReadAllOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject);
    assert!(fs.read_all_with_options(&link, &options).is_err());
    let mut restricted = LocalFileSystem::host().unwrap();
    restricted.set_symlink_policy(LocalSymlinkPolicy::Reject).unwrap();
    assert!(restricted.canonicalize(&link).is_err());
}
