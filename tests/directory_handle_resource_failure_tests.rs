// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::error::LocalFileError;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::observation::LocalSourceVerification;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::outcome::LocalTreeMutationState;
use qubit_local_files::policy::LocalPrivacyRequirement;

const CHILD_SCENARIO: &str = "QUBIT_DIRECTORY_FD_FAILURE_SCENARIO";

struct DescriptorExhaustion {
    original: libc::rlimit,
    files: Vec<fs::File>,
}

impl DescriptorExhaustion {
    fn release_slots(&mut self, count: usize) {
        assert!(
            self.files.len() >= count,
            "fixture should own enough filler descriptors"
        );
        for _ in 0..count {
            drop(self.files.pop().expect("reserved descriptor should exist"));
        }
    }

    fn new() -> Self {
        let mut original = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: The pointer names a valid, writable rlimit value.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut original) }, 0);
        let limited = libc::rlimit {
            rlim_cur: original.rlim_cur.min(128),
            rlim_max: original.rlim_max,
        };
        // SAFETY: Only this isolated child's soft limit changes; the hard limit is
        // preserved.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limited) }, 0);
        let mut exhaustion = Self {
            original,
            files: Vec::new(),
        };
        loop {
            match fs::File::open("/dev/null") {
                Ok(file) => exhaustion.files.push(file),
                Err(error) => {
                    assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
                    break;
                }
            }
        }
        exhaustion
    }
}

impl Drop for DescriptorExhaustion {
    fn drop(&mut self) {
        // SAFETY: Restoring the saved soft limit leaves the unchanged hard limit
        // intact.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &self.original) }, 0);
    }
}

fn assert_emfile(error: &LocalFileError, operation: LocalFileOperation, path: &Path) {
    assert_eq!(error.kind(), LocalFileErrorKind::Io, "{error:?}");
    assert_eq!(error.operation(), operation, "{error:?}");
    assert_eq!(error.path(), Some(path), "{error:?}");
    let mut source: Option<&(dyn std::error::Error + 'static)> =
        Some(error.io_error().expect("native source must be retained"));
    let mut native_emfile = false;
    while let Some(current) = source {
        if current
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.raw_os_error() == Some(libc::EMFILE))
        {
            native_emfile = true;
            break;
        }
        source = current.source();
    }
    assert!(
        native_emfile,
        "native EMFILE must survive error context wrapping: {error:?}"
    );
}

#[test]
fn test_directory_resource_failures_are_isolated_and_recoverable() {
    let binary = std::env::current_exe().expect("test binary should be locatable");
    for scenario in [
        "observe",
        "security",
        "child",
        "rooted-open",
        "host-open",
        "open-reader",
        "binding",
        "reopen",
        "ancestors",
        "create-stage",
        "bound-dot-observation-slot1",
        "bound-dot-anchor-slot2",
        "plain-dot-cursor-slot1",
        "private-stage-retained-slot3",
        "private-stage-unproven-slot2",
    ] {
        let output = Command::new(&binary)
            .args([
                "--exact",
                "test_directory_resource_failure_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_SCENARIO, scenario)
            .output()
            .expect("resource-limit scenario should start in its own process");
        assert!(
            output.status.success(),
            "{scenario} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_directory_resource_failure_child() {
    let Ok(scenario) = std::env::var(CHILD_SCENARIO) else {
        return;
    };
    let scratch = tempfile::tempdir().expect("fixture should exist before exhausting descriptors");
    let native_root = fs::canonicalize(scratch.path()).expect("fixture root should resolve before exhaustion");
    let parent_path = native_root.join("parent");
    fs::create_dir_all(parent_path.join("child")).expect("child fixture should exist");
    fs::write(parent_path.join("payload"), b"protected user data").expect("user payload should exist");
    let filesystem = LocalFileSystem::rooted(&native_root).expect("filesystem should open");
    let host = LocalFileSystem::host().expect("host namespace should be established before exhaustion");
    let parent = filesystem
        .open_directory_with_options(
            Path::new("parent"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound parent should open before descriptor exhaustion");
    let child_identity = parent
        .observe_entry(Path::new("child"))
        .expect("child should be observable")
        .identity()
        .expect("identity should exist")
        .clone();
    let mut reader = parent
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("reader should open before descriptor exhaustion");
    let retained_identity = parent
        .observe()
        .expect("parent should be observable")
        .identity()
        .expect("identity should exist")
        .clone();
    let mut exhaustion = DescriptorExhaustion::new();
    let available = match scenario.as_str() {
        "bound-dot-observation-slot1" | "plain-dot-cursor-slot1" => 1,
        "bound-dot-anchor-slot2" | "private-stage-unproven-slot2" => 2,
        "private-stage-retained-slot3" => 3,
        _ => 0,
    };
    exhaustion.release_slots(available);
    match scenario.as_str() {
        "observe" => assert_emfile(
            &parent.observe().expect_err("observation must clone the authority"),
            LocalFileOperation::Metadata,
            &parent_path,
        ),
        "security" => assert_emfile(
            &parent
                .security_observation(&LocalSecurityObservationOptions::new())
                .expect_err("security observation must clone the authority"),
            LocalFileOperation::Metadata,
            &parent_path,
        ),
        "child" => assert_emfile(
            &parent
                .open_directory(Path::new("child"), &LocalDirectoryOpenOptions::new())
                .expect_err("child opening must clone the parent authority"),
            LocalFileOperation::OpenRoot,
            Path::new("child"),
        ),
        "rooted-open" => assert_emfile(
            &filesystem
                .open_directory_with_options(Path::new("parent"), &LocalDirectoryOpenOptions::new())
                .expect_err("rooted opening must clone its retained namespace authority"),
            LocalFileOperation::OpenRoot,
            Path::new("parent"),
        ),
        "host-open" => assert_emfile(
            &host
                .open_directory_with_options(&parent_path, &LocalDirectoryOpenOptions::new())
                .expect_err("host opening must obtain a native root authority"),
            LocalFileOperation::OpenRoot,
            &parent_path,
        ),
        "open-reader" => assert_emfile(
            &parent
                .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
                .expect_err("reader resolution must duplicate its authority"),
            LocalFileOperation::OpenReader,
            &parent_path.join("payload"),
        ),
        "binding" => assert_emfile(
            &reader
                .verify_path_binding()
                .expect_err("binding verification must clone its retained anchor"),
            LocalFileOperation::Metadata,
            &native_root,
        ),
        "reopen" => assert_emfile(
            &parent
                .open_staged_directory(Path::new("child"), &child_identity, &LocalTreeValidationOptions::new())
                .expect_err("stage reopen must propagate authority cloning failure"),
            LocalFileOperation::OpenRoot,
            Path::new("child"),
        ),
        "ancestors" => {
            let observations = parent
                .ancestor_observations(&LocalListOptions::new())
                .expect("retained observations need no new native descriptors");
            assert_eq!(observations.len(), 2);
            assert_eq!(observations[1].identity(), Some(&retained_identity));
        }
        "create-stage" => {
            let error = parent
                .create_staged_directory(
                    &LocalTempDirectoryOptions::new()
                        .with_privacy(LocalPrivacyRequirement::Required)
                        .with_max_attempts(1),
                )
                .expect_err("stage creation cannot clone its parent authority");
            let stage_path = error
                .cause()
                .path()
                .expect("creation failure should identify its generated stage");
            assert_eq!(stage_path.parent(), Some(parent_path.as_path()));
            assert_emfile(error.cause(), LocalFileOperation::CreateTempDirectory, stage_path);
            assert_eq!(error.mutation_state(), LocalTreeMutationState::NotStarted);
            assert!(error.retained_resources().is_empty());
        }
        "bound-dot-observation-slot1" | "bound-dot-anchor-slot2" | "plain-dot-cursor-slot1" => {
            let mut options = LocalDirectoryOpenOptions::new();
            let operation = if scenario == "bound-dot-observation-slot1" {
                LocalFileOperation::Metadata
            } else {
                LocalFileOperation::OpenRoot
            };
            if scenario != "plain-dot-cursor-slot1" {
                options = options.with_path_binding(LocalPathBindingOptions::new());
            }
            let error = parent
                .open_directory(Path::new("."), &options)
                .expect_err("opening must propagate the later authority duplication failure");
            assert_emfile(&error, operation, &parent_path);
        }
        "private-stage-retained-slot3" => {
            let error = parent
                .create_staged_directory(
                    &LocalTempDirectoryOptions::new()
                        .with_privacy(LocalPrivacyRequirement::Required)
                        .with_max_attempts(1),
                )
                .expect_err("reopening the newly created stage must exhaust the remaining descriptors");
            assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
            assert_eq!(error.retained_resources().len(), 1);
            let stage_path = error.retained_resources()[0].diagnostic_path().to_path_buf();
            assert_eq!(stage_path.parent(), Some(parent_path.as_path()));
            assert!(error.retained_resources()[0].identity().is_some());
            assert_emfile(error.cause(), LocalFileOperation::OpenRoot, &stage_path);
            drop(exhaustion);
            assert!(stage_path.is_dir(), "a known-identity stage must remain recoverable");
            let (_cause, stage, resources) = error.into_parts();
            assert!(stage.is_none(), "no fully validated stage was returned");
            for resource in resources {
                resource
                    .cleanup()
                    .expect("known-identity retained stage should clean up after descriptor recovery");
            }
            assert!(!stage_path.exists());
            assert_eq!(
                fs::read(parent_path.join("payload")).expect("user payload must survive stage recovery"),
                b"protected user data"
            );
            assert!(parent_path.join("child").is_dir());
            return;
        }
        "private-stage-unproven-slot2" => {
            let error = parent
                .create_staged_directory(
                    &LocalTempDirectoryOptions::new()
                        .with_privacy(LocalPrivacyRequirement::Required)
                        .with_max_attempts(1),
                )
                .expect_err("opening the stage after mkdir must exhaust descriptors");
            assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
            assert!(
                error.retained_resources().is_empty(),
                "the failed open cannot establish a tree owner identity"
            );
            let stage_path = error
                .cause()
                .path()
                .expect("partial stage pathname must be retained")
                .to_path_buf();
            assert_eq!(stage_path.parent(), Some(parent_path.as_path()));
            assert_emfile(error.cause(), LocalFileOperation::CreateTempDirectory, &stage_path);
            let (cause, stage, resources) = error.into_parts();
            assert!(stage.is_none());
            assert!(resources.is_empty());
            let (_cause, recovery) = cause.into_temp_recovery_parts();
            let mut recovery = recovery.expect("native recovery authority must survive failure after mkdir");
            assert_eq!(recovery.path(), stage_path.as_path());
            assert!(recovery.identity().is_none());
            drop(exhaustion);
            let cleanup = recovery
                .cleanup()
                .expect_err("unproven identity must never authorize path-only deletion");
            assert_eq!(cleanup.kind(), LocalFileErrorKind::RequirementNotMet);
            assert_eq!(cleanup.operation(), LocalFileOperation::Cleanup);
            assert_eq!(cleanup.path(), Some(stage_path.as_path()));
            assert!(stage_path.is_dir(), "failed cleanup must preserve the unproven object");
            // The fixture now explicitly observes the named object before authorizing
            // cleanup.
            let relative = Path::new(stage_path.file_name().expect("stage should have a basename"));
            let identity = parent
                .observe_entry(relative)
                .expect("restored descriptors permit independent observation")
                .identity()
                .expect("created directory should have a native identity")
                .clone();
            let reopened = parent
                .open_staged_directory(
                    relative,
                    &identity,
                    &LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required),
                )
                .expect("explicitly observed stage should reopen safely");
            reopened
                .cleanup()
                .expect("independently verified stage should clean up");
            assert!(!stage_path.exists());
            assert_eq!(
                fs::read(parent_path.join("payload")).expect("user payload must survive conservative recovery"),
                b"protected user data"
            );
            assert!(parent_path.join("child").is_dir());
            return;
        }
        _ => panic!("unexpected resource scenario: {scenario}"),
    }
    drop(exhaustion);
    assert_eq!(
        fs::read(parent_path.join("payload")).expect("user data should remain readable after recovery"),
        b"protected user data"
    );
    assert!(parent_path.join("child").is_dir());
    assert_eq!(
        fs::read_dir(&parent_path)
            .expect("fixture parent should remain intact")
            .count(),
        2
    );
    assert_eq!(
        parent
            .observe()
            .expect("authority should recover once descriptor capacity returns")
            .identity(),
        Some(&retained_identity)
    );
    assert_eq!(
        reader
            .verify_path_binding()
            .expect("binding should recover without replacement"),
        LocalSourceVerification::Unchanged
    );
    assert_eq!(
        reader
            .read_to_end_limited(64)
            .expect("retained reader should remain usable"),
        b"protected user data"
    );
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage creation should recover");
    let stage_path = parent_path.join(stage.relative_name());
    assert!(stage_path.is_dir());
    stage.cleanup().expect("recovered stage should clean up normally");
    assert!(!stage_path.exists());
    assert_eq!(
        fs::read(parent_path.join("payload")).expect("cleanup must preserve user data"),
        b"protected user data"
    );
}

const SCAN_SCENARIO: &str = "QUBIT_DIRECTORY_RESOURCE_SCAN";
const SCAN_SLOTS: &str = "QUBIT_DIRECTORY_RESOURCE_SCAN_SLOTS";

#[test]
fn test_directory_operations_across_descriptor_windows() {
    let binary = std::env::current_exe().expect("test binary should exist");
    let mut operations = vec!["bound-reader", "sync-tree"];
    if cfg!(target_os = "linux") {
        operations.push("owner-policy");
    }
    for operation in operations {
        for slots in 0..=16 {
            let output = Command::new(&binary)
                .args([
                    "--exact",
                    "test_directory_resource_scan_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(SCAN_SCENARIO, operation)
                .env(SCAN_SLOTS, slots.to_string())
                .output()
                .expect("each resource window should have an isolated process");
            assert!(
                output.status.success(),
                "{operation} slots={slots}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

fn assert_window_emfile(error: &LocalFileError, root: &Path, known: &[std::path::PathBuf], operation: &str) {
    let allowed_operations: &[LocalFileOperation] = match operation {
        "bound-reader" => &[
            LocalFileOperation::BindPath,
            LocalFileOperation::OpenReader,
            LocalFileOperation::Metadata,
            LocalFileOperation::OpenRoot,
        ],
        "sync-tree" => &[
            LocalFileOperation::BindPath,
            LocalFileOperation::Metadata,
            LocalFileOperation::OpenRoot,
            LocalFileOperation::List,
            LocalFileOperation::Commit,
        ],
        "owner-policy" => &[
            LocalFileOperation::BindPath,
            LocalFileOperation::Metadata,
            LocalFileOperation::OpenRoot,
            LocalFileOperation::List,
        ],
        _ => panic!("unexpected operation"),
    };
    assert!(allowed_operations.contains(&error.operation()), "{error:?}");
    if let Some(path) = error.path() {
        let known_path = path == root
            || path == Path::new("/")
            || known.iter().any(|relative| {
                path == relative || path == root.join(relative) || path == Path::new("/").join(relative)
            });
        assert!(known_path, "error must name a known fixture entry: {error:?}");
        assert_emfile(error, error.operation(), path);
    } else {
        // Same-handle root policy verification intentionally has no pathname.
        assert_eq!(
            operation, "owner-policy",
            "only root policy cloning may omit path: {error:?}"
        );
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(
            error
                .io_error()
                .expect("root clone should retain its native error")
                .raw_os_error(),
            Some(libc::EMFILE)
        );
    }
}

#[test]
fn test_directory_resource_scan_child() {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::outcome::LocalTreeAccessState;
    use qubit_local_files::policy::LocalOwnerAccessPolicy;

    let Ok(operation) = std::env::var(SCAN_SCENARIO) else {
        return;
    };
    let slots: usize = std::env::var(SCAN_SLOTS)
        .expect("slot budget should be supplied")
        .parse()
        .expect("slot budget should be numeric");
    assert!(slots <= 16);
    let scratch = tempfile::tempdir().expect("fixture should exist");
    let native_root = fs::canonicalize(scratch.path()).expect("fixture path should be canonical");
    let parent = LocalFileSystem::rooted(&native_root)
        .expect("filesystem should open")
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("parent should retain its authority and opening binding");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created before exhaustion");
    let stage_relative = std::path::PathBuf::from(stage.relative_name());
    let stage_path = native_root.join(&stage_relative);
    fs::create_dir_all(stage_path.join("nested/deeper")).expect("deep tree should exist");
    fs::write(stage_path.join("nested/deeper/payload"), b"deep source payload").expect("deep file should exist");
    fs::write(stage_path.join("top"), b"top source payload").expect("top file should exist");
    fs::write(native_root.join("user-data"), b"unrelated user data").expect("user sentinel should exist");
    let entries = ["", "nested", "nested/deeper", "nested/deeper/payload", "top"];
    for relative in entries {
        let path = stage_path.join(relative);
        let mode = if path.is_dir() { 0o750 } else { 0o640 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("initial access modes should be explicit");
    }
    let before: Vec<_> = entries
        .iter()
        .map(|relative| {
            let metadata = fs::metadata(stage_path.join(relative)).expect("entry metadata should exist");
            (
                metadata.dev(),
                metadata.ino(),
                metadata.mode() & 0o777,
                metadata.is_dir(),
            )
        })
        .collect();
    let mut known = vec![std::path::PathBuf::from("user-data"), stage_relative.clone()];
    for relative in entries {
        known.push(stage_relative.join(relative));
        known.push(std::path::PathBuf::from(relative));
    }
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut access_result = None;
    let mut encountered_emfile = false;
    let mut exhaustion = DescriptorExhaustion::new();
    exhaustion.release_slots(slots);
    match operation.as_str() {
        "bound-reader" => {
            let relative = stage_relative.join("nested/deeper/payload");
            match parent.open_observed_reader(
                &relative,
                &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
            ) {
                Ok(mut reader) => {
                    assert_eq!(
                        reader
                            .read_to_end_limited(64)
                            .expect("retained file reads require no further descriptor"),
                        b"deep source payload"
                    );
                    match reader.verify_path_binding() {
                        Ok(verification) => {
                            assert_eq!(verification, LocalSourceVerification::Unchanged)
                        }
                        Err(error) => {
                            encountered_emfile = true;
                            assert_window_emfile(&error, &native_root, &known, &operation)
                        }
                    }
                }
                Err(error) => {
                    encountered_emfile = true;
                    assert_window_emfile(&error, &native_root, &known, &operation);
                }
            }
        }
        "sync-tree" => {
            if let Err(error) = stage.sync_tree_with_options(&parent, &LocalListOptions::new()) {
                encountered_emfile = true;
                assert_window_emfile(&error, &native_root, &known, &operation);
            }
        }
        "owner-policy" => {
            if !cfg!(target_os = "linux") {
                panic!("complete owner ACL verification is Linux-specific here");
            }
            match stage.apply_owner_access_policy_with_options(&policy, &LocalListOptions::new()) {
                Ok(outcome) => {
                    assert_eq!(outcome.access_state(), LocalTreeAccessState::OwnerPolicyApplied);
                    assert_eq!(outcome.owner_access_policy(), Some(&policy));
                    access_result = Some(LocalTreeAccessState::OwnerPolicyApplied);
                }
                Err(error) => {
                    encountered_emfile = true;
                    assert_window_emfile(error.cause(), &native_root, &known, &operation);
                    assert!(
                        error.retained_resources().is_empty(),
                        "borrowed stage retains cleanup ownership"
                    );
                    assert!(matches!(
                        error.access_state(),
                        LocalTreeAccessState::Preserved
                            | LocalTreeAccessState::Mixed
                            | LocalTreeAccessState::Indeterminate
                    ));
                    access_result = Some(error.access_state());
                }
            }
        }
        _ => panic!("unexpected scan operation: {operation}"),
    }
    drop(exhaustion);
    if slots == 0 {
        assert!(encountered_emfile, "zero slots must exercise a real resource failure");
    }
    if slots == 16 {
        assert!(
            !encountered_emfile,
            "this small fixture must succeed with sixteen free descriptors"
        );
    }
    let mut original_modes = 0;
    let mut target_modes = 0;
    for (relative, (device, inode, mode, directory)) in entries.iter().zip(&before) {
        let current = fs::metadata(stage_path.join(relative)).expect("every original entry must remain named");
        assert_eq!((current.dev(), current.ino()), (*device, *inode), "{relative}");
        let current_mode = current.mode() & 0o777;
        let target_mode = if *directory { 0o700 } else { 0o600 };
        if current_mode == *mode {
            original_modes += 1;
        }
        if current_mode == target_mode {
            target_modes += 1;
        }
        if operation == "owner-policy" {
            assert!(
                current_mode == *mode || current_mode == target_mode,
                "policy failure must not invent a third permission state: {relative}"
            );
        } else {
            assert_eq!(
                current_mode, *mode,
                "read and sync must preserve permissions: {relative}"
            );
        }
    }
    match access_result {
        Some(LocalTreeAccessState::Preserved) => assert_eq!(original_modes, entries.len()),
        Some(LocalTreeAccessState::OwnerPolicyApplied) => assert_eq!(target_modes, entries.len()),
        Some(LocalTreeAccessState::Mixed) => {
            assert!(original_modes > 0);
            assert!(target_modes > 0);
        }
        Some(LocalTreeAccessState::Indeterminate) | None => {}
        state => panic!("unexpected policy state: {state:?}"),
    }
    assert_eq!(
        fs::read(stage_path.join("nested/deeper/payload")).expect("source must survive every descriptor window"),
        b"deep source payload"
    );
    assert_eq!(
        fs::read(stage_path.join("top")).expect("top source must survive"),
        b"top source payload"
    );
    assert_eq!(
        fs::read(native_root.join("user-data")).expect("unrelated user payload must survive"),
        b"unrelated user data"
    );
    assert!(
        !native_root.join("published").exists(),
        "scanned operations must never publish the stage"
    );
    if operation == "owner-policy" {
        let outcome = stage
            .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
            .expect("same retained owner should permit retry after descriptor recovery");
        assert_eq!(outcome.access_state(), LocalTreeAccessState::OwnerPolicyApplied);
        for relative in entries {
            let metadata = fs::metadata(stage_path.join(relative)).expect("verified policy entry should exist");
            assert_eq!(metadata.mode() & 0o777, if metadata.is_dir() { 0o700 } else { 0o600 });
        }
    }
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("sync should retry successfully once descriptor capacity returns");
    let mut recovered = parent
        .open_observed_reader(
            &stage_relative.join("nested/deeper/payload"),
            &LocalObservedReadOptions::new(),
        )
        .expect("read should recover from any earlier failure");
    assert_eq!(
        recovered
            .read_to_end_limited(64)
            .expect("recovered read should complete"),
        b"deep source payload"
    );
    assert_eq!(
        recovered
            .verify_path_binding()
            .expect("recovered binding should verify"),
        LocalSourceVerification::Unchanged
    );
    drop(recovered);
    stage
        .cleanup()
        .expect("retained stage ownership must support safe cleanup after recovery");
    assert!(!stage_path.exists());
    assert_eq!(
        fs::read(native_root.join("user-data")).expect("stage cleanup must preserve unrelated data"),
        b"unrelated user data"
    );
}
