// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared bounded scratch-root ownership for filesystem fuzz targets.

mod fuzz_root;
pub(crate) use fuzz_root::FuzzRoot;

/// Shared state machine; other fuzz targets only use the scratch-root owner.
#[allow(dead_code)]
pub(crate) mod lifecycle {
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;

    use qubit_io::Output;
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::LocalResult;
    use qubit_local_files::options::LocalDeleteOptions;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::options::LocalTempCleanupLimits;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::options::LocalWriteMetadataPolicy;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::outcome::LocalPersistFailureState;
    use qubit_local_files::outcome::LocalPersistStage;
    use qubit_local_files::outcome::LocalTempSourceState;
    use qubit_local_files::outcome::LocalWriterState;

    use super::FuzzRoot;

    mod directory_snapshot;
    mod lifecycle_witness;
    mod published_resource;
    mod resource;
    mod snapshot_entry;

    use directory_snapshot::DirectorySnapshot;
    use lifecycle_witness::LifecycleWitness;
    use published_resource::PublishedResource;
    use resource::Resource;
    use snapshot_entry::SnapshotEntry;

    const MAX_OPERATIONS: usize = 64;
    const MAX_ENTRIES: usize = 256;
    const MAX_DEPTH: usize = 16;
    const PAYLOAD: &[u8] = b"original fuzz resource";
    const REPLACEMENT: &[u8] = b"replacement must survive";
    // Bound the model from the fixture's entry/depth limits and largest marker.
    const MAX_SNAPSHOT_BYTES: usize = MAX_ENTRIES * (MAX_DEPTH * 16 + REPLACEMENT.len());

    /// Turns fixture setup failures into clear harness failures while leaving
    /// environmental setup for the caller to skip before this helper runs.
    fn require_created<T>(result: LocalResult<T>, kind: &str) -> T {
        match result {
            Ok(resource) => resource,
            Err(error) => panic!("prepared lifecycle fixture could not create {kind}: {error}"),
        }
    }

    /// Fails a deterministic fixture when expected publication is rejected.
    fn assert_expected_publication_success(expected_success: bool, succeeded: bool) {
        assert!(
            !expected_success || succeeded,
            "valid owned fixture publication was unexpectedly rejected"
        );
    }

    /// Converts namespace paths to host paths only for adversarial mutation and
    /// observation; rooted library calls retain their original operands.
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

        use super::DirectorySnapshot;
        use super::FuzzRoot;

        #[test]
        fn test_snapshot_rejects_payload_loss_and_accepts_remaining_tree() {
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
        fn test_snapshot_records_symlink_text_without_following_it() {
            use std::os::unix::fs::symlink;

            let root = FuzzRoot::create("symlink-snapshot-test").expect("create fixture root");
            let outside_root = FuzzRoot::create("symlink-snapshot-outside").expect("create outside root");
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

    /// Checks that saved replacement bytes and every published resource remain
    /// unchanged after later actions.
    fn assert_preserved(replacements: &[PathBuf], published: &[PublishedResource]) {
        for marker in replacements {
            assert_eq!(fs::read(marker).expect("replacement remains readable"), REPLACEMENT);
        }
        for resource in published {
            match resource {
                PublishedResource::File(path, bytes) => {
                    assert!(fs::symlink_metadata(path).expect("published file remains").is_file());
                    assert_eq!(&fs::read(path).expect("published bytes remain readable"), bytes);
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
    ///
    /// Shared by host and rooted lifecycle targets; generated assertion
    /// failures remain fuzz findings.
    pub(crate) fn run(data: &[u8], rooted: bool) {
        let _ = run_model(data, rooted);
    }

    /// Executes the bounded lifecycle model and returns fixture counters.
    /// Returns `None` when host setup prevents the model from starting.
    fn run_model(data: &[u8], rooted: bool) -> Option<LifecycleWitness> {
        let Some(root) = FuzzRoot::create(if rooted { "rooted-lifecycle" } else { "lifecycle" }) else {
            return None;
        };
        let filesystem = if rooted {
            LocalFileSystem::rooted(root.path())
        } else {
            LocalFileSystem::host()
        };
        let Ok(filesystem) = filesystem else {
            return None;
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
        let mut witness = LifecycleWitness::default();

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
                        let resource = require_created(
                            filesystem.create_temp_file_with_options(
                                &LocalTempFileOptions::new().with_parent(parent).with_max_attempts(4),
                            ),
                            "file",
                        );
                        let mut resource = resource;
                        Output::write_fully(&mut resource, PAYLOAD).expect("write original fuzz file");
                        witness.created += 1;
                        Some(Resource::File(resource))
                    } else {
                        let resource = require_created(
                            filesystem.create_temp_directory_with_options(
                                &LocalTempDirectoryOptions::new()
                                    .with_parent(parent)
                                    .with_max_attempts(4)
                                    .with_cleanup_limits(cleanup_limits()),
                            ),
                            "directory",
                        );
                        fs::write(host_path(root.path(), resource.path(), rooted).join("payload"), PAYLOAD)
                            .expect("write original directory marker");
                        witness.created += 1;
                        Some(Resource::Directory(resource))
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
                    let full_cleanup_budget = match &resource {
                        Resource::File(_) => true,
                        Resource::Directory(directory) => {
                            directory.cleanup_limits().max_entries() == Some(MAX_ENTRIES)
                                && directory.cleanup_limits().max_depth() == Some(MAX_DEPTH)
                                && directory.cleanup_limits().max_pending_path_bytes() == Some(128 * 1024)
                        }
                    };
                    let expected_success = opcode == 2
                        && selector % 3 == 0
                        && previous == LocalTempSourceState::Owned
                        && !source_replaced
                        && !conflict
                        && full_cleanup_budget;
                    let directory_snapshot = if !is_file
                        && previous != LocalTempSourceState::Indeterminate
                        && !source_replaced
                        && source_path.is_dir()
                    {
                        Some(DirectorySnapshot::capture(&source_path))
                    } else {
                        None
                    };
                    let result = resource.publish(parent, &target, opcode, selector, rooted);
                    if expected_success && let Err(error) = &result {
                        eprintln!(
                            "expected publication success for opcode={opcode}, selector={selector}; got publication={:?}, stage={:?}, source={:?}, target={:?}",
                            error.publication, error.stage, error.source, error.target
                        );
                    }
                    assert_expected_publication_success(expected_success, result.is_ok());
                    match result {
                        Ok(outcome) => {
                            witness.successful_publications += 1;
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
                                    directory_snapshot.expect("owned directory has a publication snapshot"),
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
                                        directory_snapshot.expect("owned directory has a publication snapshot"),
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
                        witness.explicit_cleanups += 1;
                        witness.released += 1;
                        witness.artifacts_absent += usize::from(!source_path.exists());
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
        Some(witness)
    }

    #[cfg(test)]
    mod lifecycle_fixture_tests {
        use qubit_local_files::LocalFileError;
        use qubit_local_files::error::LocalFileErrorKind;
        use qubit_local_files::error::LocalFileOperation;

        use super::assert_expected_publication_success;
        use super::require_created;
        use super::run_model;

        #[test]
        fn test_expected_publication_success_rejects_failure_without_rejecting_other_cases() {
            assert!(
                std::panic::catch_unwind(|| {
                    assert_expected_publication_success(true, false);
                })
                .is_err()
            );

            assert_expected_publication_success(true, true);
            assert_expected_publication_success(false, false);
            assert_expected_publication_success(false, true);
        }

        #[test]
        fn test_require_created_panics_for_fixture_creation_errors_and_returns_success() {
            for kind in [LocalFileErrorKind::Unsupported, LocalFileErrorKind::InvalidPath] {
                let error = LocalFileError::new(kind, LocalFileOperation::CreateTempFile);
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        require_created::<()>(Err(error), "file")
                    }))
                    .is_err()
                );
            }
            assert_eq!(require_created(Ok(17), "file"), 17);
        }

        #[test]
        fn test_fixed_file_lifecycle_creates_cleans_releases_and_removes_artifact() {
            let witness = run_model(&[0, 0, 11, 0, 4, 0, 5, 0], false).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.explicit_cleanups, 1);
            assert_eq!(witness.released, 1);
            assert_eq!(witness.artifacts_absent, 1);
        }

        #[test]
        fn test_fixed_directory_lifecycle_creates_cleans_releases_and_removes_artifact() {
            let witness = run_model(&[0, 1, 11, 0, 4, 0, 5, 0], true).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.explicit_cleanups, 1);
            assert_eq!(witness.released, 1);
            assert_eq!(witness.artifacts_absent, 1);
        }

        #[test]
        fn test_fixed_host_directory_lifecycle_creates_cleans_releases_and_removes_artifact() {
            let witness = run_model(&[0, 1, 11, 0, 4, 0, 5, 0], false).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.explicit_cleanups, 1);
            assert_eq!(witness.released, 1);
            assert_eq!(witness.artifacts_absent, 1);
        }

        #[test]
        fn test_fixed_rooted_file_lifecycle_creates_cleans_releases_and_removes_artifact() {
            let witness = run_model(&[0, 0, 11, 0, 4, 0, 5, 0], true).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.explicit_cleanups, 1);
            assert_eq!(witness.released, 1);
            assert_eq!(witness.artifacts_absent, 1);
        }

        #[test]
        fn test_fixed_host_file_publication_succeeds() {
            let witness = run_model(&[0, 0, 2, 0], false).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.successful_publications, 1);
        }

        #[test]
        fn test_fixed_rooted_file_publication_succeeds() {
            let witness = run_model(&[0, 0, 2, 0], true).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.successful_publications, 1);
        }

        #[test]
        fn test_fixed_host_directory_publication_succeeds() {
            let witness = run_model(&[0, 1, 2, 0], false).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.successful_publications, 1);
        }

        #[test]
        fn test_fixed_rooted_directory_publication_succeeds() {
            let witness = run_model(&[0, 1, 2, 0], true).expect("fixture environment");
            assert_eq!(witness.created, 1);
            assert_eq!(witness.successful_publications, 1);
        }
    }
}
