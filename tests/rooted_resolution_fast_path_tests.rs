// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.
// =============================================================================
//! Public rooted-operation coverage for the component resolution fast path.

use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalFileKind;
#[cfg(unix)]
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

/// Existing normal components resolve through a deep rooted path and retain
/// the public metadata and reader contracts.
#[test]
fn rooted_deep_normal_path_preserves_metadata_and_reader_behavior() {
    let temporary = tempdir().expect("temporary root should be created");
    fs::create_dir_all(temporary.path().join("a/b/c")).expect("nested directories should be created");
    fs::write(temporary.path().join("a/b/c/payload"), b"payload").expect("payload should be written");
    let rooted = LocalFileSystem::rooted(temporary.path()).expect("root authority should open");

    let metadata = rooted
        .metadata(Path::new("a/b/c/payload"))
        .expect("deep metadata should resolve");
    assert_eq!(LocalFileKind::File, metadata.kind());

    let mut reader = rooted
        .open_reader(Path::new("a/b/c/payload"))
        .expect("deep reader should resolve");
    let mut content = Vec::new();
    reader
        .read_to_end(&mut content)
        .expect("deep reader should read payload");
    assert_eq!(b"payload", content.as_slice());
}

/// The same component cursor is exercised by directory creation, atomic
/// writer publication, and rooted listing operations.
#[test]
fn rooted_fast_path_supports_create_write_and_list() {
    let temporary = tempdir().expect("temporary root should be created");
    let rooted = LocalFileSystem::rooted(temporary.path()).expect("root authority should open");

    let created = rooted
        .create_directory_with_options(Path::new("a/b/c"), &LocalCreateDirectoryOptions::new().with_recursive())
        .expect("deep directory should be created");
    assert!(created.created());

    let mut writer = rooted
        .open_writer_with_options(
            Path::new("a/b/c/payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("deep writer should open");
    writer.write_all(b"payload").expect("writer should accept bytes");
    let _ = writer.commit().expect("writer should publish payload");

    let entries = rooted
        .list_with_options(Path::new("a"), &LocalListOptions::new().with_recursive())
        .expect("deep directory should be listable")
        .collect::<Result<Vec<_>, _>>()
        .expect("rooted listing should complete");
    assert!(entries.iter().any(|entry| entry.path() == Path::new("/a/b/c/payload")));
}

/// A symbolic link causes the rooted resolver to use its existing expansion
/// rules and still follows a final link for reader operations.
#[cfg(unix)]
#[test]
fn rooted_link_path_preserves_symlink_fallback_behavior() {
    use std::os::unix::fs::symlink;

    let temporary = tempdir().expect("temporary root should be created");
    fs::create_dir_all(temporary.path().join("a/target")).expect("target directory should be created");
    fs::write(temporary.path().join("a/target/payload"), b"through-link").expect("target payload should be written");
    symlink("target", temporary.path().join("a/link")).expect("symbolic link should be created");
    let rooted = LocalFileSystem::rooted(temporary.path()).expect("root authority should open");

    let mut reader = rooted
        .open_reader(Path::new("a/link/payload"))
        .expect("link traversal should follow existing rooted policy");
    let mut content = Vec::new();
    reader
        .read_to_end(&mut content)
        .expect("link traversal should read payload");
    assert_eq!(b"through-link", content.as_slice());
}

/// Symlink expansion remains on the existing resolver when the cursor sees a
/// link. This covers final-entry metadata, followed reads and lists, policy
/// rejection, dangling targets, cycles, and virtual-root boundary escapes.
#[cfg(unix)]
#[test]
fn rooted_fast_path_fallback_preserves_symlink_policy_matrix() {
    use std::os::unix::fs::symlink;

    let temporary = tempdir().expect("temporary root should be created");
    fs::create_dir_all(temporary.path().join("target/nested")).expect("target directories should be created");
    fs::write(temporary.path().join("target/nested/payload"), b"through-link")
        .expect("target payload should be written");
    symlink("/target", temporary.path().join("absolute-link")).expect("absolute link should be created");
    symlink("missing", temporary.path().join("dangling")).expect("dangling link should be created");
    symlink("cycle-b", temporary.path().join("cycle-a")).expect("first cycle link should be created");
    symlink("cycle-a", temporary.path().join("cycle-b")).expect("second cycle link should be created");
    symlink("../../outside", temporary.path().join("escape")).expect("escaping link should be created");

    let mut rooted = LocalFileSystem::rooted(temporary.path()).expect("root authority should open");
    rooted
        .set_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .expect("rooted in-scope policy should be accepted");

    let link_metadata = rooted
        .metadata(Path::new("absolute-link"))
        .expect("metadata must preserve final-link semantics");
    assert_eq!(LocalFileKind::Symlink, link_metadata.kind());

    assert_eq!(
        b"through-link",
        rooted
            .read_prefix(Path::new("/absolute-link/nested/payload"), 64)
            .expect("followed absolute link should read")
            .as_slice(),
    );
    let linked_entries = rooted
        .list_with_options(Path::new("absolute-link"), &LocalListOptions::new().with_recursive())
        .expect("followed link directory should list")
        .collect::<Result<Vec<_>, _>>()
        .expect("linked listing should complete");
    assert!(linked_entries.iter().any(|entry| entry.path().ends_with("payload")));

    let dangling = rooted
        .open_reader(Path::new("dangling"))
        .expect_err("followed dangling link must fail");
    assert_eq!(LocalFileErrorKind::NotFound, dangling.kind());

    let cycle = rooted
        .open_reader(Path::new("cycle-a"))
        .expect_err("symlink cycle must terminate");
    assert_eq!(LocalFileErrorKind::InvalidPath, cycle.kind());

    let escape = rooted
        .open_reader(Path::new("escape"))
        .expect_err("symlink escape must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidPath, escape.kind());

    rooted
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("reject policy should be accepted");
    let rejected = rooted
        .open_reader(Path::new("absolute-link/nested/payload"))
        .expect_err("Reject must refuse intermediate symlink traversal");
    assert_eq!(LocalFileErrorKind::Unsupported, rejected.kind());
    assert_eq!(
        LocalFileKind::Symlink,
        rooted
            .metadata(Path::new("absolute-link"))
            .expect("Reject must still permit final-entry metadata")
            .kind(),
    );
}
