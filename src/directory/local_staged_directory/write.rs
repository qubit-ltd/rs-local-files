// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use qubit_io::ByteInput;

use super::LocalStagedDirectory;
use super::cleanup_partial_private_file;
use super::create_stage_file;
use super::resource_limit;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::error::LocalResult;
use crate::observation::LocalFileObservation;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::policy::LocalFileSyncRequirement;

impl LocalStagedDirectory {
    /// Creates a new file beneath the stage and streams at most `maximum`
    /// bytes.
    ///
    /// The destination is never replaced. Input bytes consumed before an I/O or
    /// limit error cannot be restored; a successful result carries the
    /// created file's observation. On failure, the stage remains owned by the
    /// caller. Any `cleanup_error` reports a failed attempt to remove this
    /// write's staging file; it is distinct from private temporary recovery.
    /// Cleaning the whole stage may remove other files already created there.
    pub fn write_file_new<S: ByteInput + ?Sized>(
        &mut self,
        relative: &Path,
        input: &mut S,
        maximum: u64,
    ) -> LocalResult<LocalFileObservation> {
        let observation = if self.private || self.owner_access_policy.is_some() {
            self.write_private_file_new(relative, input, maximum)?
        } else {
            let mut writer = self.directory().open_writer_with_options(
                relative,
                &LocalWriteOptions::new(LocalWriteMode::CreateNew)
                    .with_atomicity(crate::LocalAtomicityRequirement::Required)
                    .with_file_sync(LocalFileSyncRequirement::NotRequired),
            )?;
            let mut total = 0_u64;
            let mut block = [0_u8; 8192];
            loop {
                let count = match qubit_io::Input::read(input, &mut block) {
                    Ok(count) => count,
                    Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(source) => {
                        let cause = LocalFileError::from_io(
                            LocalFileOperation::Read,
                            Some(writer.path().to_path_buf()),
                            None,
                            source,
                        );
                        return Err(abort_failed_stage_write(&mut writer, cause));
                    }
                };
                if count == 0 {
                    break;
                }
                total = match total.checked_add(count as u64) {
                    Some(total) => total,
                    None => {
                        let cause = resource_limit().with_path(writer.path().to_path_buf());
                        return Err(abort_failed_stage_write(&mut writer, cause));
                    }
                };
                if total > maximum {
                    let cause = resource_limit().with_path(writer.path().to_path_buf());
                    return Err(abort_failed_stage_write(&mut writer, cause));
                }
                let mut offset = 0;
                while offset < count {
                    let written = match qubit_io::Output::write(&mut writer, &block[offset..count]) {
                        Ok(written) => written,
                        Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(source) => {
                            let cause = LocalFileError::from_io(
                                LocalFileOperation::Commit,
                                Some(writer.path().to_path_buf()),
                                None,
                                source,
                            );
                            return Err(abort_failed_stage_write(&mut writer, cause));
                        }
                    };
                    if written == 0 {
                        let cause = LocalFileError::new(LocalFileErrorKind::DataCorruption, LocalFileOperation::Commit)
                            .with_path(writer.path().to_path_buf());
                        return Err(abort_failed_stage_write(&mut writer, cause));
                    }
                    offset += written;
                }
            }
            if let Err(error) = writer.commit() {
                return Err(abort_failed_stage_commit(error));
            }
            self.directory().observe_entry(relative)?
        };
        let identity = observation
            .identity()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata))?;
        self.owner
            .as_mut()
            .expect("stage cleanup owner is present")
            .record_entry(relative, identity)?;
        Ok(observation)
    }

    fn write_private_file_new<S: ByteInput + ?Sized>(
        &self,
        relative: &Path,
        input: &mut S,
        maximum: u64,
    ) -> LocalResult<LocalFileObservation> {
        self.directory().validate_direct_input(relative)?;
        let name = relative
            .file_name()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::ValidateName))?;
        let parent_relative = relative
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if self.owner_access_policy.is_some() && parent_relative != Path::new(".") {
            self.ensure_owner_parent_directories(parent_relative)?;
        }
        let containing = if parent_relative == Path::new(".") {
            self.directory().clone()
        } else {
            self.directory()
                .open_directory(parent_relative, &LocalDirectoryOpenOptions::new())?
        };
        let parent_file = containing.root_authority()?.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(containing.diagnostic_path().to_path_buf()),
                None,
                source,
            )
        })?;
        let file = create_stage_file(&parent_file, name, self.owner_access_policy.as_ref()).map_err(|error| {
            let path = containing.diagnostic_path().join(name);
            let recovery = error
                .retained
                .map(|entry| crate::LocalTempRecovery::from_native(path.clone(), entry));
            let mut cause = LocalFileError::from_io(LocalFileOperation::OpenWriter, Some(path), None, error.source);
            if let Some(recovery) = recovery {
                cause = cause.with_temp_recovery(recovery);
            }
            cause
        })?;
        let path = containing.diagnostic_path().join(name);
        let identity = match crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Required,
        ) {
            Ok(observation) => observation.identity().cloned(),
            Err(cause) => {
                return Err(cleanup_partial_private_file(
                    cause,
                    &parent_file,
                    name,
                    file,
                    None,
                    &path,
                ));
            }
        };
        let Some(identity) = identity else {
            return Err(cleanup_partial_private_file(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
                    .with_path(path.clone()),
                &parent_file,
                name,
                file,
                None,
                &path,
            ));
        };
        let mut file = file;
        let mut total = 0_u64;
        let mut block = [0_u8; 8192];
        loop {
            let count = match qubit_io::Input::read(input, &mut block) {
                Ok(count) => count,
                Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(source) => {
                    let cause = LocalFileError::from_io(LocalFileOperation::Read, Some(path.clone()), None, source);
                    return Err(cleanup_partial_private_file(
                        cause,
                        &parent_file,
                        name,
                        file,
                        Some(identity),
                        &path,
                    ));
                }
            };
            if count == 0 {
                break;
            }
            total = match total.checked_add(count as u64) {
                Some(total) => total,
                None => {
                    return Err(cleanup_partial_private_file(
                        resource_limit(),
                        &parent_file,
                        name,
                        file,
                        Some(identity),
                        &path,
                    ));
                }
            };
            if total > maximum {
                return Err(cleanup_partial_private_file(
                    resource_limit(),
                    &parent_file,
                    name,
                    file,
                    Some(identity),
                    &path,
                ));
            }
            #[cfg(test)]
            let write_result = crate::tests::native_io_fault_tests::check(
                crate::tests::native_io_fault_tests::NativeIoPoint::FileWrite,
            )
            .and_then(|()| std::io::Write::write_all(&mut file, &block[..count]));
            #[cfg(not(test))]
            let write_result = std::io::Write::write_all(&mut file, &block[..count]);
            if let Err(source) = write_result {
                let cause = LocalFileError::from_io(LocalFileOperation::Commit, Some(path.clone()), None, source);
                return Err(cleanup_partial_private_file(
                    cause,
                    &parent_file,
                    name,
                    file,
                    Some(identity),
                    &path,
                ));
            }
        }
        crate::observation::query::query_observation(&file, crate::observation::LocalObservationRequirement::Required)
    }

    fn ensure_owner_parent_directories(&self, parent_relative: &Path) -> LocalResult<()> {
        let policy = self.owner_access_policy.as_ref().expect("owner policy checked");
        let mut path = PathBuf::new();
        for component in parent_relative.components() {
            let std::path::Component::Normal(name) = component else {
                continue;
            };
            let parent_file = if path.as_os_str().is_empty() {
                self.directory().root_authority()?.try_clone_authority()
            } else {
                self.directory()
                    .open_directory(&path, &LocalDirectoryOpenOptions::new())?
                    .root_authority()?
                    .try_clone_authority()
            }
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenRoot,
                    Some(self.directory().diagnostic_path().join(&path)),
                    None,
                    source,
                )
            })?;
            path.push(name);
            let created = {
                #[cfg(unix)]
                {
                    crate::temp::internal::privacy_unix::create_owner_entry_at(&parent_file, name, true, policy)
                }
                #[cfg(windows)]
                {
                    crate::temp::internal::privacy_windows::create_owner_entry_at(&parent_file, name, true, policy)
                }
                #[cfg(not(any(unix, windows)))]
                {
                    let _ = (parent_file, name, policy);
                    Err(crate::temp::types::NativePrivateCreateError {
                        source: std::io::Error::new(std::io::ErrorKind::Unsupported, "owner policies are unsupported"),
                        retained: None,
                    })
                }
            };
            if let Err(error) = created {
                if error.source.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::CreateTempDirectory,
                        Some(self.directory().diagnostic_path().join(&path)),
                        None,
                        error.source,
                    ));
                }
                let existing = self
                    .directory()
                    .open_directory(&path, &LocalDirectoryOpenOptions::new())?
                    .root_authority()?
                    .try_clone_authority()
                    .map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenRoot,
                            Some(self.directory().diagnostic_path().join(&path)),
                            None,
                            source,
                        )
                    })?;
                #[cfg(unix)]
                crate::temp::internal::privacy_unix::verify_owner_access(&existing, true, policy).map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(self.directory().diagnostic_path().join(&path)),
                            None,
                            source,
                        )
                    },
                )?;
                #[cfg(windows)]
                crate::temp::internal::privacy_windows::verify_owner_access(&existing, true, policy).map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(self.directory().diagnostic_path().join(&path)),
                            None,
                            source,
                        )
                    },
                )?;
            }
        }
        Ok(())
    }
}

fn abort_failed_stage_write(writer: &mut crate::writer::LocalFileWriter, cause: LocalFileError) -> LocalFileError {
    match writer.abort() {
        Ok(_) => cause,
        Err(cleanup) => cause.with_additional_cleanup_error(cleanup),
    }
}

fn abort_failed_stage_commit(error: crate::writer::LocalFileCommitError) -> LocalFileError {
    let (cause, _, retry) = error.into_parts();
    match retry {
        Some(mut retry) => abort_failed_stage_write(&mut retry, cause),
        None => cause,
    }
}

#[cfg(all(test, feature = "test-support", unix))]
mod commit_cleanup_tests {
    use std::fs;
    use std::path::Path;

    use qubit_io::Output;
    use tempfile::tempdir;

    use super::abort_failed_stage_commit;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalWriteMode;
    use crate::options::LocalWriteOptions;
    use crate::policy::LocalFileSyncRequirement;
    use crate::test_support::install_test_fault;

    #[test]
    fn test_failed_commit_aborts_retry_writer_and_preserves_both_errors() {
        let directory = tempdir().expect("fixture should be created");
        let parent = LocalFileSystem::host()
            .expect("host filesystem should open")
            .open_directory_with_options(directory.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent directory should open");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage should be created");
        let stage_path = stage.directory().diagnostic_path().to_path_buf();
        let target = stage_path.join("payload");
        fs::write(&target, b"existing").expect("existing destination should be written");
        let mut writer = stage
            .directory()
            .open_writer_with_options(
                Path::new("payload"),
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                    .with_file_sync(LocalFileSyncRequirement::Required),
            )
            .expect("writer should stage target");
        Output::write_fully(&mut writer, b"staged").expect("staged write should succeed");

        let commit_error = {
            let _fault = install_test_fault("writer-staged-file-sync").expect("sync failure should install");
            writer
                .commit()
                .expect_err("pre-install sync failure should be retryable")
        };
        assert!(commit_error.writer().is_some(), "commit should return Some(retry)");
        let primary_message = commit_error.error().io_error().unwrap().to_string();
        let error = {
            let _fault = install_test_fault("atomic-install-unlink-persistent")
                .expect("retry abort cleanup fault should install");
            abort_failed_stage_commit(commit_error)
        };

        assert_eq!(primary_message, error.io_error().unwrap().to_string());
        assert_eq!(crate::LocalFileOperation::Commit, error.operation());
        assert_eq!(
            Some(crate::LocalFileOperation::Abort),
            error.cleanup_error().map(crate::LocalFileError::operation),
            "abort failure should be attached structurally",
        );
        assert_eq!(
            b"existing",
            fs::read(&target).expect("original target should remain").as_slice()
        );
        let entries: Vec<_> = fs::read_dir(&stage_path)
            .expect("directory should be readable")
            .map(|entry| entry.expect("directory entry should be readable").path())
            .collect();
        assert_eq!(
            2,
            entries.len(),
            "failed abort should retain staging beside the original target"
        );
        let staging = entries
            .into_iter()
            .find(|path| path != &target)
            .expect("retry abort should retain its staging entry");
        assert!(
            staging.exists(),
            "retained staging should remain available to the owner"
        );
        stage
            .cleanup()
            .expect("stage owner should recover and remove retained staging");
        assert!(!stage_path.exists(), "owner cleanup should remove the complete stage");
    }
}

#[cfg(all(test, unix))]
mod native_write_failure_tests {
    use std::path::Path;

    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::policy::LocalPrivacyRequirement;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn test_private_write_initial_observation_failure_preserves_unknown_identity_owner() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
            .expect("private stage");
        let path = stage.directory().diagnostic_path().join("payload");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 1);
        let error = stage
            .write_private_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
            .expect_err("initial observation fails before consuming input");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(
            error.io_error().expect("native cause").to_string(),
            "injected native I/O contract failure"
        );
        let (_, mut recovery) = error.into_temp_recovery_parts();
        let recovery = recovery
            .as_mut()
            .expect("unknown identity requires an explicit recovery owner");
        assert_eq!(recovery.path(), path);
        assert!(recovery.identity().is_none());
        assert_eq!(
            recovery
                .cleanup()
                .expect_err("unknown identity must never delete by name")
                .kind(),
            LocalFileErrorKind::RequirementNotMet
        );
        assert_eq!(std::fs::metadata(&path).expect("partial file remains").len(), 0);
        drop(guard);
        stage
            .cleanup()
            .expect("stage owner can observe and remove its partial child");
        assert!(!path.exists());
    }

    #[test]
    fn test_private_write_final_observation_failure_keeps_stage_cleanup_responsibility() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
            .expect("private stage");
        let path = stage.directory().diagnostic_path().join("payload");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 2);
        let error = stage
            .write_private_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
            .expect_err("final observation fails after writing");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert!(error.temp_recovery().is_none());
        assert_eq!(
            std::fs::read(&path).expect("complete partial child remains stage-owned"),
            b"payload"
        );
        drop(guard);
        stage
            .cleanup()
            .expect("stage retains cleanup responsibility after post-write failure");
        assert!(!path.exists());
    }

    #[test]
    fn test_private_write_parent_clone_failure_precedes_file_creation() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let path = stage.directory().diagnostic_path().join("payload");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 1);
        let error = stage
            .write_private_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
            .expect_err("cannot create a child without a retained parent authority");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::OpenWriter);
        assert_eq!(error.path(), Some(stage.directory().diagnostic_path()));
        assert!(!path.exists());
        drop(guard);
        stage.cleanup().expect("unmodified stage remains cleanup-owned");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn test_owner_parent_clone_failure_does_not_create_intermediate_directories() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let policy = crate::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
            .expect("owner-policy stage");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 1);
        let error = stage
            .ensure_owner_parent_directories(Path::new("nested"))
            .expect_err("parent authority cloning precedes intermediate mkdir");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
        assert!(!stage.directory().diagnostic_path().join("nested").exists());
        drop(guard);
        stage
            .ensure_owner_parent_directories(Path::new("nested"))
            .expect("retry creates owner-only directory");
        stage.cleanup().expect("owner-policy stage cleanup");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_existing_owner_directory_open_failure_preserves_existing_directory() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let policy = crate::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
            .expect("owner-policy stage");
        stage
            .ensure_owner_parent_directories(Path::new("nested"))
            .expect("existing owner directory");
        let path = stage.directory().diagnostic_path().join("nested");
        let before = std::fs::metadata(&path).expect("existing metadata");
        // The second authority clone opens the already-existing child directory.
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 2);
        let error = stage
            .ensure_owner_parent_directories(Path::new("nested"))
            .expect_err("existing directory verification requires its own authority handle");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
        assert_eq!(error.path(), Some(Path::new("nested")));
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(&path).expect("unchanged directory").ino(),
            before.ino()
        );
        drop(guard);
        stage
            .ensure_owner_parent_directories(Path::new("nested"))
            .expect("retry verifies existing owner directory");
        stage.cleanup().expect("cleanup existing owner directory");
    }
    #[test]
    fn test_stage_write_native_failure_cleans_candidates_and_keeps_stage_retryable() {
        for private in [false, true] {
            let fixture = tempfile::tempdir().expect("fixture");
            let parent = LocalFileSystem::host()
                .expect("host")
                .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
                .expect("parent");
            let options = if private {
                LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required)
            } else {
                LocalTempDirectoryOptions::new()
            };
            let mut stage = parent.create_staged_directory(&options).expect("stage");
            let directory = stage.directory().diagnostic_path().to_path_buf();
            let path = directory.join("payload");
            let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::FileWrite, 1);
            let error = stage
                .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
                .expect_err("native write must fail before file publication");
            assert!(guard.fired());
            assert_eq!(error.kind(), LocalFileErrorKind::Io);
            assert_eq!(error.operation(), LocalFileOperation::Commit);
            assert_eq!(
                error.path(),
                Some(if private { path.as_path() } else { Path::new("/payload") })
            );
            assert!(
                error
                    .io_error()
                    .expect("native cause")
                    .to_string()
                    .contains("injected native I/O contract failure")
            );
            assert!(
                error.temp_recovery().is_none(),
                "identity-aware candidate cleanup completed"
            );
            assert!(!path.exists());
            assert_eq!(
                std::fs::read_dir(&directory).expect("inspect stage").count(),
                0,
                "failed candidate must be removed"
            );
            drop(guard);
            stage
                .write_file_new(Path::new("payload"), &mut &b"retry"[..], 32)
                .expect("same destination is reusable after write failure");
            assert_eq!(std::fs::read(&path).expect("retry contents"), b"retry");
            stage.cleanup().expect("stage still owns successful retry");
        }
    }
}
