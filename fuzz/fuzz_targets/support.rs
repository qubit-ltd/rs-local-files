// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared bounded scratch-root ownership for filesystem fuzz targets.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

const MAX_ROOT_ATTEMPTS: u64 = 16;
static NEXT_ROOT_ID: AtomicU64 = AtomicU64::new(0);

/// A uniquely created scratch directory removed on scope exit.
pub(crate) struct FuzzRoot {
    path: PathBuf,
}

impl FuzzRoot {
    /// Atomically creates one process-unique scratch directory.
    ///
    /// Returns `None` after a bounded sequence of collisions or ambient I/O
    /// failures so that environmental setup does not become a fuzz finding.
    pub(crate) fn create(label: &str) -> Option<Self> {
        let first_id = NEXT_ROOT_ID.fetch_add(MAX_ROOT_ATTEMPTS, Ordering::Relaxed);
        for offset in 0..MAX_ROOT_ATTEMPTS {
            let path = std::env::temp_dir().join(format!(
                "qubit-local-files-{label}-{}-{}",
                std::process::id(),
                first_id + offset,
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Some(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Returns the exclusively owned scratch-root path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FuzzRoot {
    /// Performs best-effort cleanup without converting host cleanup failures
    /// into library crash findings.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Shared state machine; other fuzz targets only use the scratch-root owner.
#[allow(dead_code)]
pub(crate) mod lifecycle {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;

    use qubit_io::Output;
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::LocalResult;
    use qubit_local_files::LocalTempDirectory;
    use qubit_local_files::LocalTempFile;
    use qubit_local_files::error::LocalPersistError;
    use qubit_local_files::error::LocalPersistErrorParts;
    use qubit_local_files::options::LocalDeleteOptions;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::options::LocalPersistOptions;
    use qubit_local_files::options::LocalTempCleanupLimits;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::options::LocalWriteMetadataPolicy;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::outcome::LocalPersistFailureState;
    use qubit_local_files::outcome::LocalPersistOutcome;
    use qubit_local_files::outcome::LocalPersistStage;
    use qubit_local_files::outcome::LocalTempSourceState;
    use qubit_local_files::outcome::LocalWriterState;
    use qubit_local_files::policy::LocalDurabilityRequirement;

    use super::FuzzRoot;

    const MAX_OPERATIONS: usize = 64;
    const MAX_ENTRIES: usize = 256;
    const MAX_DEPTH: usize = 16;
    const PAYLOAD: &[u8] = b"original fuzz resource";
    const REPLACEMENT: &[u8] = b"replacement must survive";
    // Bound the model from the fixture's entry/depth limits and largest marker.
    const MAX_SNAPSHOT_BYTES: usize = MAX_ENTRIES * (MAX_DEPTH * 16 + REPLACEMENT.len());

    /// The public resource retained between independently decoded operations.
    #[derive(Debug)]
    enum Resource {
        File(LocalTempFile),
        Directory(LocalTempDirectory),
    }

    /// Publication errors keep ownership and both independent state axes.
    struct Retained {
        resource: Box<Resource>,
        publication: LocalPersistFailureState,
        stage: LocalPersistStage,
        source: LocalTempSourceState,
        target: Option<PathBuf>,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum SnapshotEntry {
        Directory,
        File(Vec<u8>),
        Symlink(PathBuf),
    }

    /// A bounded, non-following snapshot of a published directory tree.
    #[derive(Debug, PartialEq, Eq)]
    struct DirectorySnapshot {
        entries: BTreeMap<PathBuf, SnapshotEntry>,
    }

    impl DirectorySnapshot {
        /// Captures the complete tree within the harness entry and byte budgets.
        fn capture(root: &Path) -> Self {
            fn visit(
                path: &Path,
                relative: &Path,
                entries: &mut BTreeMap<PathBuf, SnapshotEntry>,
                byte_count: &mut usize,
            ) {
                assert!(
                    relative.components().count() <= MAX_DEPTH,
                    "snapshot exceeded its depth budget"
                );
                assert!(
                    entries.len() < MAX_ENTRIES,
                    "snapshot exceeded its entry budget"
                );
                let metadata = fs::symlink_metadata(path).expect("snapshot entry metadata");
                let snapshot_entry = if metadata.file_type().is_symlink() {
                    let target = fs::read_link(path).expect("read snapshot link text");
                    *byte_count = byte_count
                        .checked_add(relative.as_os_str().len())
                        .and_then(|size| size.checked_add(target.as_os_str().len()))
                        .expect("snapshot byte budget arithmetic");
                    assert!(
                        *byte_count <= MAX_SNAPSHOT_BYTES,
                        "snapshot exceeded its byte budget"
                    );
                    SnapshotEntry::Symlink(target)
                } else if metadata.is_dir() {
                    *byte_count = byte_count
                        .checked_add(relative.as_os_str().len())
                        .expect("snapshot byte budget arithmetic");
                    assert!(
                        *byte_count <= MAX_SNAPSHOT_BYTES,
                        "snapshot exceeded its byte budget"
                    );
                    SnapshotEntry::Directory
                } else if metadata.is_file() {
                    let file_size =
                        usize::try_from(metadata.len()).expect("snapshot file size fits usize");
                    *byte_count = byte_count
                        .checked_add(relative.as_os_str().len())
                        .and_then(|size| size.checked_add(file_size))
                        .expect("snapshot byte budget arithmetic");
                    assert!(
                        *byte_count <= MAX_SNAPSHOT_BYTES,
                        "snapshot exceeded its byte budget"
                    );
                    SnapshotEntry::File(fs::read(path).expect("read snapshot file"))
                } else {
                    panic!("unsupported entry in fuzz-owned directory: {path:?}");
                };
                entries.insert(relative.to_path_buf(), snapshot_entry);

                if metadata.is_dir() {
                    for child in fs::read_dir(path).expect("read snapshot directory") {
                        let child = child.expect("read snapshot directory entry");
                        let child_path = child.path();
                        let child_relative = relative.join(child.file_name());
                        visit(&child_path, &child_relative, entries, byte_count);
                    }
                }
            }

            let mut entries = BTreeMap::new();
            let mut byte_count = 0;
            visit(root, Path::new(""), &mut entries, &mut byte_count);
            Self { entries }
        }

        /// Asserts the exact relative paths, entry kinds, link text and bytes.
        fn assert_matches(&self, root: &Path) {
            assert_eq!(
                self,
                &Self::capture(root),
                "published directory tree changed: {root:?}"
            );
        }
    }

    enum PublishedResource {
        File(PathBuf, Vec<u8>),
        Directory(PathBuf, DirectorySnapshot),
    }

    /// Converts named public error parts without discarding the retained guard.
    fn retain<T>(error: LocalPersistError<T>, wrap: fn(T) -> Resource) -> Retained {
        let LocalPersistErrorParts {
            resource,
            state,
            source_state,
            stage,
            resolved_target,
            ..
        } = error.into_parts();
        Retained {
            resource: Box::new(wrap(resource)),
            publication: state,
            stage,
            source: source_state,
            target: resolved_target,
        }
    }

    impl Resource {
        /// Returns the creating authority's namespace path without host
        /// conversion.
        fn path(&self) -> &Path {
            match self {
                Self::File(resource) => resource.path(),
                Self::Directory(resource) => resource.path(),
            }
        }

        /// Reads the guard's current source authority after preceding
        /// operations.
        fn source_state(&self) -> LocalTempSourceState {
            match self {
                Self::File(resource) => resource.source_state(),
                Self::Directory(resource) => resource.source_state(),
            }
        }

        /// Runs public cleanup, preserving the concrete error for assertions.
        fn cleanup(&mut self) -> LocalResult<()> {
            match self {
                Self::File(resource) => resource.cleanup(),
                Self::Directory(resource) => resource.cleanup(),
            }
        }

        /// Publishes, keeps, or retries at an explicit base; failures retain
        /// the resource.
        fn publish(
            self,
            base: &Path,
            target: &Path,
            opcode: u8,
            selector: u8,
            rooted: bool,
        ) -> Result<LocalPersistOutcome, Retained> {
            let durability = match selector % 3 {
                0 => LocalDurabilityRequirement::NotRequired,
                1 => LocalDurabilityRequirement::Preferred,
                _ => LocalDurabilityRequirement::Required,
            };
            let options = LocalPersistOptions::new().with_durability(durability);
            let (invalid_base, invalid_target) = match selector % 6 {
                0 => (Path::new("/"), Path::new("../../outside")),
                1 => (Path::new("relative"), Path::new("published")),
                2 => (Path::new("/."), Path::new("published")),
                3 => (Path::new("/"), Path::new("/absolute")),
                4 => (Path::new("/"), Path::new("")),
                _ => (Path::new("/missing"), Path::new("published")),
            };
            let relative_base = base.join("scratch");
            let relative_target = Path::new("..").join(target.file_name().unwrap_or_default());
            let (explicit_base, explicit_target) = if selector & 1 == 0 {
                (base, Path::new(target.file_name().unwrap_or_default()))
            } else {
                (relative_base.as_path(), relative_target.as_path())
            };
            match self {
                Self::File(resource) => {
                    let result = match opcode {
                        3 => resource.keep(),
                        10 if rooted => resource.persist_at(invalid_base, invalid_target, options),
                        9 => resource.persist_at(explicit_base, explicit_target, options),
                        _ => resource.persist_with(target, options),
                    };
                    result.map_err(|error| retain(error, Self::File))
                }
                Self::Directory(resource) => {
                    let result = match opcode {
                        3 => resource.keep(),
                        10 if rooted => resource.persist_at(invalid_base, invalid_target, options),
                        9 => resource.persist_at(explicit_base, explicit_target, options),
                        _ => resource.persist_with(target, options),
                    };
                    result.map_err(|error| retain(error, Self::Directory))
                }
            }
        }
    }

    /// Converts a namespace path only for adversarial host mutation and
    /// observation. Rooted library calls always receive their original
    /// namespace operands.
    fn host_path(root: &Path, namespace: &Path, rooted: bool) -> PathBuf {
        if rooted {
            root.join(namespace.strip_prefix(Path::new("/")).expect("rooted absolute path"))
        } else {
            namespace.to_path_buf()
        }
    }

    /// Bounds every cleanup attempt, including automatic Drop cleanup.
    fn cleanup_limits() -> LocalTempCleanupLimits {
        LocalTempCleanupLimits::new()
            .with_max_entries(MAX_ENTRIES)
            .with_max_depth(MAX_DEPTH)
            .with_max_pending_path_bytes(128 * 1024)
    }

    #[cfg(test)]
    mod directory_snapshot_tests {
        use std::fs;

        use super::{DirectorySnapshot, FuzzRoot};

        #[test]
        fn snapshot_rejects_payload_loss_and_accepts_remaining_tree() {
            let root = FuzzRoot::create("snapshot-test").expect("create fixture root");
            let root_path = root.path();
            fs::create_dir_all(root_path.join("nested")).expect("create fixture tree");
            fs::write(root_path.join("payload"), b"payload").expect("write payload");
            fs::write(root_path.join("nested/leaf"), b"leaf").expect("write nested leaf");

            let complete = DirectorySnapshot::capture(root_path);
            fs::remove_file(root_path.join("payload")).expect("remove only payload");
            assert!(std::panic::catch_unwind(|| complete.assert_matches(root_path)).is_err());

            let remaining = DirectorySnapshot::capture(root_path);
            remaining.assert_matches(root_path);
        }

        #[cfg(unix)]
        #[test]
        fn snapshot_records_symlink_text_without_following_it() {
            use std::os::unix::fs::symlink;

            let root = FuzzRoot::create("symlink-snapshot-test").expect("create fixture root");
            let outside_root =
                FuzzRoot::create("symlink-snapshot-outside").expect("create outside root");
            let root_path = root.path();
            let outside = outside_root.path().join("target");
            fs::write(&outside, b"outside").expect("write external target");
            symlink(&outside, root_path.join("link")).expect("create fixture link");

            let snapshot = DirectorySnapshot::capture(root_path);
            fs::write(&outside, b"changed").expect("mutate link target");
            snapshot.assert_matches(root_path);
            fs::remove_file(root_path.join("link")).expect("remove original fixture link");
            symlink("different-target", root_path.join("link")).expect("replace fixture link text");
            assert!(std::panic::catch_unwind(|| snapshot.assert_matches(root_path)).is_err());
        }
    }

    /// Asserts saved replacement bytes and every published target survive later
    /// actions.
    fn assert_preserved(replacements: &[PathBuf], published: &[PublishedResource]) {
        for marker in replacements {
            assert_eq!(fs::read(marker).expect("replacement remains readable"), REPLACEMENT);
        }
        for resource in published {
            match resource {
                PublishedResource::File(path, bytes) => {
                    assert!(
                        fs::symlink_metadata(path)
                            .expect("published file remains")
                            .is_file()
                    );
                    assert_eq!(
                        &fs::read(path).expect("published bytes remain readable"),
                        bytes
                    );
                }
                PublishedResource::Directory(path, snapshot) => snapshot.assert_matches(path),
            }
        }
    }

    /// Runs at most 64 input-selected operations under one independently owned
    /// parent. A cumulative allocation budget includes sandboxes, markers
    /// and retained originals; a maximum 13-level child chain leaves two
    /// levels for the resource and sandbox plus one level for its payload
    /// file. Filesystem assertion failures are fuzz findings; ambient
    /// initial setup errors skip.
    pub(crate) fn run(data: &[u8], rooted: bool) {
        let Some(root) = FuzzRoot::create(if rooted { "rooted-lifecycle" } else { "lifecycle" }) else {
            return;
        };
        let filesystem = if rooted {
            LocalFileSystem::rooted(root.path())
        } else {
            LocalFileSystem::host()
        };
        let Ok(filesystem) = filesystem else {
            return;
        };
        let parent = if rooted { Path::new("/") } else { root.path() };
        fs::create_dir(root.path().join("scratch")).expect("create explicit publication base");
        let mut remaining_entries = MAX_ENTRIES - 2;
        let mut expected_source = LocalTempSourceState::Owned;
        let mut source_replaced = false;
        let mut expected_bytes = PAYLOAD.to_vec();
        let mut writable = true;
        let mut current: Option<Resource> = None;
        let mut replacements = Vec::new();
        let mut published: Vec<PublishedResource> = Vec::new();

        for (index, operation) in data.chunks(2).take(MAX_OPERATIONS).enumerate() {
            let opcode = operation[0] % 14;
            let selector = operation.get(1).copied().unwrap_or_default();
            if opcode == 11 {
                let entries = filesystem
                    .list_with_options(parent, &LocalListOptions::new().with_recursive())
                    .expect("list owned scratch parent")
                    .collect::<Result<Vec<_>, _>>()
                    .expect("walk bounded scratch tree");
                assert!(entries.len() < MAX_ENTRIES, "harness exceeded its entry budget");
                assert_preserved(&replacements, &published);
                continue;
            }
            if opcode == 12 && remaining_entries >= 3 {
                // Writer staging may temporarily own a sandbox and payload as well.
                remaining_entries -= 3;
                let target = parent.join("writer-payload");
                let policy = if selector & 1 == 0 {
                    LocalWriteMetadataPolicy::PreserveExisting
                } else {
                    LocalWriteMetadataPolicy::UseStaging
                };
                let mut writer = filesystem
                    .open_writer_with_options(
                        &target,
                        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy),
                    )
                    .expect("open bounded writer");
                Output::write_fully(&mut writer, PAYLOAD).expect("write staged payload");
                let outcome = writer.commit().expect("commit staged payload");
                assert_eq!(outcome.state(), LocalWriterState::Committed);
                let path = host_path(root.path(), &target, rooted);
                assert_eq!(fs::read(&path).expect("read committed bytes"), PAYLOAD);
                continue;
            }
            if opcode == 13 {
                let writer_target = parent.join("writer-payload");
                let _ = filesystem
                    .delete_file_with_options(&writer_target, &LocalDeleteOptions::new().with_missing_ok())
                    .expect("delete optional writer payload");
                assert!(!host_path(root.path(), &writer_target, rooted).exists());
                let outcome = filesystem
                    .delete_file_with_options(
                        &parent.join("always-absent"),
                        &LocalDeleteOptions::new().with_missing_ok(),
                    )
                    .expect("delete tolerates missing scratch child");
                assert!(!outcome.deleted());
                continue;
            }
            if opcode == 0 {
                if current.is_none() && remaining_entries >= 4 {
                    // Reserve sandbox, resource, directory payload, and a keep target.
                    remaining_entries -= 4;
                    expected_source = LocalTempSourceState::Owned;
                    source_replaced = false;
                    expected_bytes = PAYLOAD.to_vec();
                    writable = true;
                    current = if selector & 1 == 0 {
                        filesystem
                            .create_temp_file_with_options(
                                &LocalTempFileOptions::new().with_parent(parent).with_max_attempts(4),
                            )
                            .ok()
                            .map(|mut resource| {
                                Output::write_fully(&mut resource, PAYLOAD).expect("write original fuzz file");
                                Resource::File(resource)
                            })
                    } else {
                        filesystem
                            .create_temp_directory_with_options(
                                &LocalTempDirectoryOptions::new()
                                    .with_parent(parent)
                                    .with_max_attempts(4)
                                    .with_cleanup_limits(cleanup_limits()),
                            )
                            .ok()
                            .map(|resource| {
                                fs::write(host_path(root.path(), resource.path(), rooted).join("payload"), PAYLOAD)
                                    .expect("write original directory marker");
                                Resource::Directory(resource)
                            })
                    };
                }
                continue;
            }
            let Some(mut resource) = current.take() else {
                continue;
            };
            assert_eq!(resource.source_state(), expected_source, "independent source model");
            let source_path = host_path(root.path(), resource.path(), rooted);
            match opcode {
                1 if resource.source_state() == LocalTempSourceState::Owned
                    && remaining_entries >= 2
                    && !replacements
                        .iter()
                        .any(|path: &PathBuf| path == &source_path || path.parent() == Some(source_path.as_path())) =>
                {
                    // Save the actual original first: replacing by unlink/create could reuse its
                    // identity.
                    fs::rename(&source_path, root.path().join(format!("saved-{index}")))
                        .expect("save original entity before external replacement");
                    let marker = if matches!(resource, Resource::Directory(_)) {
                        fs::create_dir(&source_path).expect("create replacement directory");
                        source_path.join("payload")
                    } else {
                        source_path.clone()
                    };
                    fs::write(&marker, REPLACEMENT).expect("write external replacement marker");
                    replacements.push(marker);
                    source_replaced = true;
                    remaining_entries -= 2;
                    current = Some(resource);
                }
                2 | 3 | 7 | 9 | 10 => {
                    let target = if opcode == 10 {
                        PathBuf::from("relative-invalid")
                    } else {
                        parent.join(format!("published-{index}"))
                    };
                    let target_host = if opcode == 10 {
                        None
                    } else {
                        Some(host_path(root.path(), &target, rooted))
                    };
                    let conflict = opcode == 7 && remaining_entries >= 1;
                    if conflict {
                        fs::write(target_host.as_ref().expect("conflict target"), REPLACEMENT)
                            .expect("create conflicting target");
                        remaining_entries -= 1;
                    }
                    let previous = expected_source;
                    let is_file = matches!(resource, Resource::File(_));
                    let directory_snapshot = if !is_file
                        && previous != LocalTempSourceState::Indeterminate
                        && !source_replaced
                        && source_path.is_dir()
                    {
                        Some(DirectorySnapshot::capture(&source_path))
                    } else {
                        None
                    };
                    match resource.publish(parent, &target, opcode, selector, rooted) {
                        Ok(outcome) => {
                            assert_eq!(previous, LocalTempSourceState::Owned);
                            assert!(!source_replaced, "replaced source cannot publish");
                            assert!(!conflict && opcode != 10, "invalid publication succeeded");
                            let path = host_path(root.path(), outcome.path(), rooted);
                            assert!(path.exists(), "successful publication must exist");
                            if is_file {
                                let bytes = fs::read(&path).expect("published file");
                                assert_eq!(bytes, expected_bytes, "published payload matches independent model");
                                published.push(PublishedResource::File(path, bytes));
                            } else {
                                published.push(PublishedResource::Directory(
                                    path,
                                    directory_snapshot
                                        .expect("owned directory has a publication snapshot"),
                                ));
                            }
                        }
                        Err(mut retained) => {
                            assert_eq!(retained.source, retained.resource.source_state());
                            if previous != LocalTempSourceState::Owned || opcode == 10 {
                                assert_eq!(retained.publication, LocalPersistFailureState::NotPublished);
                                assert_eq!(retained.source, previous);
                            } else if source_replaced {
                                assert_eq!(retained.publication, LocalPersistFailureState::NotPublished);
                                assert_eq!(retained.source, LocalTempSourceState::Indeterminate);
                            } else if conflict {
                                assert_eq!(retained.publication, LocalPersistFailureState::NotPublished);
                                assert_eq!(retained.source, LocalTempSourceState::Owned);
                            }
                            expected_source = match retained.publication {
                                LocalPersistFailureState::Published => LocalTempSourceState::CleanupRequired,
                                LocalPersistFailureState::Indeterminate => LocalTempSourceState::Indeterminate,
                                LocalPersistFailureState::NotPublished if previous != LocalTempSourceState::Owned => {
                                    previous
                                }
                                LocalPersistFailureState::NotPublished if source_replaced && opcode != 10 => {
                                    LocalTempSourceState::Indeterminate
                                }
                                LocalPersistFailureState::NotPublished => LocalTempSourceState::Owned,
                                _ => panic!("unknown publication state"),
                            };
                            assert_eq!(retained.source, expected_source);
                            if opcode == 10 && previous == LocalTempSourceState::Owned {
                                assert_eq!(retained.stage, LocalPersistStage::ResolveTarget);
                            }
                            if opcode == 10 && previous == LocalTempSourceState::Owned && writable {
                                if let Resource::File(file) = &mut *retained.resource {
                                    Output::write_fully(file, b"retained")
                                        .expect("invalid operands retain writable file");
                                    expected_bytes.extend_from_slice(b"retained");
                                }
                            } else if opcode != 10 {
                                writable = false;
                            }
                            if retained.publication == LocalPersistFailureState::Published {
                                let path =
                                    host_path(root.path(), retained.target.as_ref().expect("published target"), rooted);
                                if is_file {
                                    let bytes = fs::read(&path).expect("published file");
                                    assert_eq!(bytes, expected_bytes, "published payload matches independent model");
                                    published.push(PublishedResource::File(path, bytes));
                                } else {
                                    published.push(PublishedResource::Directory(
                                        path,
                                        directory_snapshot
                                            .expect("owned directory has a publication snapshot"),
                                    ));
                                }
                                // A retained published guard may clean its sandbox, never its target.
                                let cleanup = retained.resource.cleanup();
                                expected_source = if cleanup.is_ok() {
                                    LocalTempSourceState::Released
                                } else {
                                    LocalTempSourceState::CleanupRequired
                                };
                                assert_eq!(retained.resource.source_state(), expected_source);
                            } else if let Some(path) = &target_host
                                && !conflict
                                && retained.publication == LocalPersistFailureState::NotPublished
                            {
                                assert!(!path.exists(), "NotPublished created a target");
                            }
                            current = Some(*retained.resource);
                        }
                    }
                    if conflict {
                        let path = target_host.expect("conflict target");
                        assert_eq!(fs::read(&path).expect("conflict bytes preserved"), REPLACEMENT);
                        fs::remove_file(path).expect("remove harness conflict for subsequent retry");
                    }
                }
                4 => {
                    let previous = expected_source;
                    let result = resource.cleanup();
                    writable = false;
                    if previous == LocalTempSourceState::Indeterminate {
                        assert!(result.is_err(), "indeterminate cleanup must refuse deletion");
                    }
                    if result.is_ok() {
                        assert_eq!(resource.source_state(), LocalTempSourceState::Released);
                        assert!(resource.cleanup().is_ok(), "Released cleanup must be idempotent");
                    }
                    expected_source = if result.is_ok() {
                        LocalTempSourceState::Released
                    } else if previous == LocalTempSourceState::Indeterminate || source_replaced {
                        LocalTempSourceState::Indeterminate
                    } else if !source_path.exists() {
                        LocalTempSourceState::CleanupRequired
                    } else {
                        previous
                    };
                    assert_eq!(resource.source_state(), expected_source, "cleanup source model");
                    current = Some(resource);
                }
                5 => {
                    let zero_budget = matches!(&resource, Resource::Directory(directory)
                        if directory.cleanup_limits().max_entries() == Some(0)
                            && directory.source_state() == LocalTempSourceState::Owned);
                    let previous = expected_source;
                    let full_budget = match &resource {
                        Resource::File(_) => true,
                        Resource::Directory(directory) => {
                            directory.cleanup_limits().max_entries() == Some(MAX_ENTRIES)
                                && directory.cleanup_limits().max_depth() == Some(MAX_DEPTH)
                                && directory.cleanup_limits().max_pending_path_bytes() == Some(128 * 1024)
                        }
                    };
                    drop(resource);
                    if previous == LocalTempSourceState::Owned && !source_replaced && full_budget {
                        assert!(!source_path.exists(), "Drop reclaims owned source within full budget");
                    }
                    if zero_budget {
                        assert!(source_path.exists(), "Drop must retain the explicit zero-entry budget");
                    } else if previous == LocalTempSourceState::Released {
                        assert!(!source_path.exists(), "Released guard cannot resurrect source");
                    }
                }
                6 => {
                    if let Resource::Directory(directory) = &mut resource {
                        let limits = match selector % 4 {
                            0 => cleanup_limits().with_max_entries(0),
                            1 => cleanup_limits().with_max_depth(0),
                            2 => cleanup_limits().with_max_pending_path_bytes(0),
                            _ => cleanup_limits(),
                        };
                        directory.set_cleanup_limits(limits);
                    }
                    current = Some(resource);
                }
                8 => {
                    if matches!(resource, Resource::Directory(_))
                        && resource.source_state() == LocalTempSourceState::Owned
                        && source_path.is_dir()
                        && !replacements
                            .iter()
                            .any(|path| path.parent() == Some(source_path.as_path()))
                    {
                        let depth = usize::from(selector % 13) + 1;
                        if remaining_entries > depth {
                            let mut child = source_path.join(format!("tree-{index}"));
                            for _ in 1..depth {
                                child.push("nested");
                            }
                            fs::create_dir_all(&child).expect("grow bounded directory chain");
                            fs::write(child.join("payload"), PAYLOAD).expect("write bounded tree leaf");
                            remaining_entries -= depth + 1;
                        }
                    }
                    current = Some(resource);
                }
                _ => current = Some(resource),
            }
            assert_preserved(&replacements, &published);
        }
        drop(current);
        assert_preserved(&replacements, &published);
        // FuzzRoot outlives all tested guards and independently reclaims saved
        // entities, replacements, publications and any tree a bounded
        // cleanup intentionally left.
    }
}
