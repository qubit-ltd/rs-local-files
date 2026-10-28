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
    /// created file's observation.
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
                let count = qubit_io::Input::read(input, &mut block).map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(writer.path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                if count == 0 {
                    break;
                }
                total = total.checked_add(count as u64).ok_or_else(resource_limit)?;
                if total > maximum {
                    let _ = writer.abort();
                    return Err(resource_limit());
                }
                let mut offset = 0;
                while offset < count {
                    let written = qubit_io::Output::write(&mut writer, &block[offset..count]).map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Commit,
                            Some(writer.path().to_path_buf()),
                            None,
                            source,
                        )
                    })?;
                    if written == 0 {
                        return Err(LocalFileError::new(
                            LocalFileErrorKind::DataCorruption,
                            LocalFileOperation::Commit,
                        ));
                    }
                    offset += written;
                }
            }
            if let Err(error) = writer.commit() {
                let (cause, _, retry) = error.into_parts();
                drop(retry);
                return Err(cause);
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
            if let Err(source) = std::io::Write::write_all(&mut file, &block[..count]) {
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
