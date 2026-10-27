use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};

use qubit_io::ByteInput;

use crate::directory::LocalDirectoryHandle;
use crate::error::LocalResult;
use crate::observation::{LocalFileIdentity, LocalFileObservation};
use crate::options::{
    LocalCreateDirectoryOptions, LocalDirectoryOpenOptions, LocalListOptions,
    LocalTempCleanupLimits, LocalTempDirectoryOptions, LocalWriteMode, LocalWriteOptions,
};
use crate::outcome::LocalTreeMutationState;
use crate::policy::LocalFileSyncRequirement;
use crate::{LocalFileError, LocalFileErrorKind, LocalFileOperation};

use super::tree_observer::LocalTreeObserver;
use super::tree_recovery::{
    LocalTreeCleanupError, LocalTreeCreateError, LocalTreeRecoveryLocation,
    LocalTreeRetainedResource,
};
use super::tree_validation::{LocalTreeEntry, LocalTreeValidationOptions};

/// A temporary directory retained under a fixed parent authority.
#[derive(Debug)]
pub struct LocalStagedDirectory {
    owner: Option<LocalTreeRetainedResource>,
    directory: Option<LocalDirectoryHandle>,
    relative_name: PathBuf,
    identity: Option<LocalFileIdentity>,
    private: bool,
}

impl LocalStagedDirectory {
    pub(crate) fn new(
        owner: LocalTreeRetainedResource,
        directory: LocalDirectoryHandle,
        relative_name: PathBuf,
        identity: Option<LocalFileIdentity>,
        private: bool,
    ) -> Self {
        Self {
            owner: Some(owner),
            directory: Some(directory),
            relative_name,
            identity,
            private,
        }
    }
    /// Returns the fixed authority for this stage.
    pub fn directory(&self) -> &LocalDirectoryHandle {
        self.directory.as_ref().expect("stage authority is open")
    }
    /// Returns the stage name relative to its creating parent authority.
    pub fn relative_name(&self) -> &OsStr {
        self.relative_name.as_os_str()
    }
    /// Returns the captured native identity, when available.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Sets the limits used by explicit cleanup and the final Drop attempt.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        if let Some(owner) = &mut self.owner {
            owner.set_cleanup_limits(limits);
        }
    }
    /// Observes the stage directory through its retained handle.
    pub fn observation(&self) -> LocalResult<LocalFileObservation> {
        self.directory().observe()
    }

    /// Creates a new file beneath the stage and streams at most `maximum` bytes.
    ///
    /// The destination is never replaced. Input bytes consumed before an I/O or limit
    /// error cannot be restored; a successful result carries the created file's observation.
    pub fn write_file_new<S: ByteInput + ?Sized>(
        &mut self,
        relative: &Path,
        input: &mut S,
        maximum: u64,
    ) -> LocalResult<LocalFileObservation> {
        let observation = if self.private {
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
                    let written = qubit_io::Output::write(&mut writer, &block[offset..count])
                        .map_err(|source| {
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
        let identity = observation.identity().ok_or_else(|| {
            LocalFileError::new(
                LocalFileErrorKind::Unsupported,
                LocalFileOperation::Metadata,
            )
        })?;
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
        let name = relative.file_name().ok_or_else(|| {
            LocalFileError::new(
                LocalFileErrorKind::InvalidPath,
                LocalFileOperation::ValidateName,
            )
        })?;
        let parent_relative = relative
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let containing = if parent_relative == Path::new(".") {
            self.directory().clone()
        } else {
            self.directory()
                .open_directory(parent_relative, &LocalDirectoryOpenOptions::new())?
        };
        let parent_file = containing
            .root_authority()?
            .try_clone_authority()
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenWriter,
                    Some(containing.diagnostic_path().to_path_buf()),
                    None,
                    source,
                )
            })?;
        let file = create_private_file(&parent_file, name).map_err(|error| {
            let path = containing.diagnostic_path().join(name);
            let recovery = error
                .retained
                .map(|entry| crate::LocalTempRecovery::from_native(path.clone(), entry));
            let mut cause = LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(path),
                None,
                error.source,
            );
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
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Metadata,
                )
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
                    let cause = LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(path.clone()),
                        None,
                        source,
                    );
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
                let cause = LocalFileError::from_io(
                    LocalFileOperation::Commit,
                    Some(path.clone()),
                    None,
                    source,
                );
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
        crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Required,
        )
    }

    /// Creates a lazy, recursive observer using the supplied resource ceilings.
    pub fn observe_tree_with_options(
        &self,
        options: &LocalListOptions,
    ) -> LocalResult<LocalTreeObserver> {
        let recursive = options.clone().with_recursive();
        let walker = self
            .directory()
            .list_with_options(Path::new("."), &recursive)?;
        Ok(LocalTreeObserver::new(self.directory().clone(), walker))
    }

    /// Validates the complete tree against an expected entry set and optional constraints.
    ///
    /// Returns `ResourceLimit` if traversal or exact-set indexing exceeds its budgets,
    /// and `RequirementNotMet` when an enabled entry rule or expected set is violated.
    pub fn verify_entries(
        &self,
        expected: &[LocalTreeEntry],
        options: &LocalTreeValidationOptions,
    ) -> LocalResult<()> {
        if options
            .exact_entries()
            .is_some_and(|configured| configured != expected)
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::ValidateName,
            ));
        }
        if options.privacy() == crate::policy::LocalPrivacyRequirement::Required {
            if !self.private {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Metadata,
                ));
            }
            #[cfg(unix)]
            {
                let mode = self
                    .observation()?
                    .metadata()
                    .permissions()
                    .unix_mode()
                    .ok_or_else(|| {
                        LocalFileError::new(
                            LocalFileErrorKind::Unsupported,
                            LocalFileOperation::Metadata,
                        )
                    })?;
                if mode & 0o077 != 0 {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Metadata,
                    ));
                }
            }
            #[cfg(windows)]
            return Err(LocalFileError::new(
                LocalFileErrorKind::Unsupported,
                LocalFileOperation::Metadata,
            ));
        }
        if options.limits().max_depth().is_some() {
            return Err(resource_limit());
        }
        if options
            .limits()
            .max_entries()
            .is_some_and(|maximum| expected.len() > maximum)
        {
            return Err(resource_limit());
        }
        let expected_name_bytes = expected.iter().try_fold(0_usize, |total, entry| {
            total
                .checked_add(entry.relative_path().as_os_str().as_encoded_bytes().len())
                .ok_or_else(resource_limit)
        })?;
        if options
            .limits()
            .max_seen_name_bytes()
            .is_some_and(|maximum| expected_name_bytes > maximum)
        {
            return Err(resource_limit());
        }
        let mut seen = Vec::new();
        seen.try_reserve_exact(expected.len())
            .map_err(|_| resource_limit())?;
        seen.resize(expected.len(), false);
        let mut name_bytes = expected_name_bytes;
        let mut count = 0_usize;
        let mut link_counts: Vec<(LocalFileIdentity, u64, usize)> = Vec::new();
        for item in self.observe_tree_with_options(options.limits())? {
            let item = item?;
            let kind = item.observation().metadata().kind();
            if (options.rejects_symlinks() && kind == crate::LocalFileKind::Symlink)
                || (options.rejects_special_files()
                    && !matches!(
                        kind,
                        crate::LocalFileKind::File
                            | crate::LocalFileKind::Directory
                            | crate::LocalFileKind::Symlink
                    ))
            {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Metadata,
                ));
            }
            if options.privacy() == crate::policy::LocalPrivacyRequirement::Required {
                #[cfg(unix)]
                {
                    let mode = item
                        .observation()
                        .metadata()
                        .permissions()
                        .unix_mode()
                        .ok_or_else(|| {
                            LocalFileError::new(
                                LocalFileErrorKind::Unsupported,
                                LocalFileOperation::Metadata,
                            )
                        })?;
                    if mode & 0o077 != 0 {
                        return Err(LocalFileError::new(
                            LocalFileErrorKind::RequirementNotMet,
                            LocalFileOperation::Metadata,
                        ));
                    }
                }
            }
            count = count.checked_add(1).ok_or_else(resource_limit)?;
            name_bytes = name_bytes
                .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                .ok_or_else(resource_limit)?;
            if options
                .limits()
                .max_seen_name_bytes()
                .is_some_and(|maximum| name_bytes > maximum)
            {
                return Err(resource_limit());
            }
            let Some(index) = expected.iter().position(|entry| {
                entry.relative_path() == item.relative_path() && entry.kind() == kind
            }) else {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Metadata,
                ));
            };
            if seen[index] {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Metadata,
                ));
            }
            seen[index] = true;
            if options.rejects_external_hardlinks() && kind == crate::LocalFileKind::File {
                let (current, native_count) = self
                    .directory()
                    .observe_entry_with_link_count(item.relative_path())?;
                if current.identity() != item.observation().identity() {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    ));
                }
                let native_count = native_count.ok_or_else(|| {
                    LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Metadata,
                    )
                })?;
                let identity = current.identity().cloned().ok_or_else(|| {
                    LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Metadata,
                    )
                })?;
                if let Some((_, _, occurrences)) = link_counts
                    .iter_mut()
                    .find(|(observed, _, _)| *observed == identity)
                {
                    *occurrences = occurrences.checked_add(1).ok_or_else(resource_limit)?;
                } else {
                    link_counts.try_reserve(1).map_err(|_| resource_limit())?;
                    link_counts.push((identity, native_count, 1));
                }
            }
        }
        if link_counts.iter().any(|(_, native_count, occurrences)| {
            u64::try_from(*occurrences).is_ok_and(|count| native_count > &count)
        }) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Metadata,
            ));
        }
        if count != expected.len() || seen.iter().any(|was_seen| !was_seen) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Metadata,
            ));
        }
        Ok(())
    }

    /// Removes the stage after identity checks and returns cleanup failures with an owner.
    pub fn cleanup(mut self) -> Result<(), LocalTreeCleanupError> {
        drop(self.directory.take());
        self.owner
            .take()
            .expect("staged directory retains its cleanup owner")
            .cleanup()
    }

    /// Explicitly gives up automatic cleanup and returns a path/identity recovery record.
    pub fn retain_in_place(mut self) -> LocalTreeRecoveryLocation {
        drop(self.directory.take());
        self.owner
            .take()
            .expect("staged directory retains its cleanup owner")
            .retain_in_place()
    }
}

impl Drop for LocalStagedDirectory {
    fn drop(&mut self) {
        drop(self.directory.take());
        if let Some(owner) = self.owner.take() {
            let _ = owner.cleanup();
        }
    }
}

impl LocalDirectoryHandle {
    /// Reopens a stage beneath this authority only if its identity still matches `expected`.
    pub fn open_staged_directory(
        &self,
        relative: &Path,
        expected: &LocalFileIdentity,
        options: &LocalTreeValidationOptions,
    ) -> LocalResult<LocalStagedDirectory> {
        let directory = self.open_directory(relative, &LocalDirectoryOpenOptions::new())?;
        let observation = directory.observe()?;
        if observation.identity() != Some(expected) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Metadata,
            )
            .with_path(self.diagnostic_path().join(relative)));
        }
        let private = options.privacy() == crate::policy::LocalPrivacyRequirement::Required;
        if private {
            let file = directory
                .root_authority()?
                .try_clone_authority()
                .map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = file.metadata().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::getuid() } {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Metadata,
                    ));
                }
            }
            #[cfg(windows)]
            crate::temp::internal::privacy_windows::verify_current_user_access(&file, true, false)
                .map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
        }
        let identity = observation.identity().cloned();
        let path = self.diagnostic_path().join(relative);
        let owner = LocalTreeRetainedResource::new(
            path,
            relative.to_path_buf(),
            identity.clone(),
            self.clone(),
            Some(directory.clone()),
        );
        let stage =
            LocalStagedDirectory::new(owner, directory, relative.to_path_buf(), identity, private);
        if let Some(entries) = options.exact_entries() {
            stage.verify_entries(entries, options)?;
        }
        Ok(stage)
    }

    /// Creates a uniquely named stage directly beneath this fixed directory authority.
    pub fn create_staged_directory(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> Result<LocalStagedDirectory, LocalTreeCreateError> {
        let parent = self.root_authority().map_err(|cause| {
            LocalTreeCreateError::new(cause, None, Vec::new(), LocalTreeMutationState::NotStarted)
        })?;
        let maximum_attempts = options.max_attempts().unwrap_or(64);
        if maximum_attempts == 0 {
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(
                    LocalFileErrorKind::InvalidOptions,
                    LocalFileOperation::GenerateName,
                ),
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        }
        let mut created_file = None;
        let mut relative = None;
        for _ in 0..maximum_attempts {
            let name = stage_name().map_err(|cause| {
                LocalTreeCreateError::new(
                    cause,
                    None,
                    Vec::new(),
                    LocalTreeMutationState::NotStarted,
                )
            })?;
            if options.privacy() == crate::policy::LocalPrivacyRequirement::Required {
                match create_private_directory(parent, &name) {
                    Ok(file) => {
                        created_file = Some(file);
                        relative = Some(PathBuf::from(name));
                        break;
                    }
                    Err(error)
                        if error.source.kind() == std::io::ErrorKind::AlreadyExists
                            && error.retained.is_none() =>
                    {
                        continue;
                    }
                    Err(error) => {
                        let path = self.diagnostic_path().join(&name);
                        let mutation_state = if error.retained.is_some() {
                            LocalTreeMutationState::Started
                        } else {
                            LocalTreeMutationState::NotStarted
                        };
                        let identity = error
                            .retained
                            .as_ref()
                            .and_then(|entry| entry.identity.clone());
                        let retained = identity
                            .clone()
                            .map(|identity| {
                                LocalTreeRetainedResource::new(
                                    path.clone(),
                                    PathBuf::from(&name),
                                    Some(identity),
                                    self.clone(),
                                    None,
                                )
                            })
                            .into_iter()
                            .collect();
                        let recovery = error.retained.map(|entry| {
                            crate::LocalTempRecovery::from_native(path.clone(), entry)
                        });
                        let mut cause = LocalFileError::from_io(
                            LocalFileOperation::CreateTempDirectory,
                            Some(path),
                            None,
                            error.source,
                        );
                        if let Some(recovery) = recovery {
                            cause = cause.with_temp_recovery(recovery);
                        }
                        return Err(LocalTreeCreateError::new(
                            cause,
                            None,
                            retained,
                            mutation_state,
                        ));
                    }
                }
            } else {
                let path = PathBuf::from(&name);
                match self
                    .filesystem()
                    .create_directory_with_options(&path, &LocalCreateDirectoryOptions::new())
                {
                    Ok(_outcome) => {
                        relative = Some(path);
                        break;
                    }
                    Err(error) if error.kind() == LocalFileErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        let state = match error.effect_state() {
                            Some(crate::error::LocalFileEffectState::Unchanged) => {
                                LocalTreeMutationState::NotStarted
                            }
                            Some(
                                crate::error::LocalFileEffectState::PartiallyApplied
                                | crate::error::LocalFileEffectState::Applied,
                            ) => LocalTreeMutationState::Started,
                            Some(crate::error::LocalFileEffectState::Indeterminate) | None => {
                                LocalTreeMutationState::Indeterminate
                            }
                        };
                        return Err(LocalTreeCreateError::new(error, None, Vec::new(), state));
                    }
                }
            }
        }
        let Some(relative) = relative else {
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(
                    LocalFileErrorKind::AlreadyExists,
                    LocalFileOperation::CreateTempDirectory,
                ),
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        };
        let path = self.diagnostic_path().join(&relative);
        let directory = match self.open_directory(&relative, &LocalDirectoryOpenOptions::new()) {
            Ok(directory) => directory,
            Err(cause) => {
                let identity = self
                    .observe_entry(&relative)
                    .ok()
                    .and_then(|observation| observation.identity().cloned());
                let resource =
                    LocalTreeRetainedResource::new(path, relative, identity, self.clone(), None);
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![resource],
                    LocalTreeMutationState::Started,
                ));
            }
        };
        let observation = match directory.observe() {
            Ok(observation) => observation,
            Err(cause) => {
                let identity = self
                    .observe_entry(&relative)
                    .ok()
                    .and_then(|observation| observation.identity().cloned());
                let resource = LocalTreeRetainedResource::new(
                    path,
                    relative,
                    identity,
                    self.clone(),
                    Some(directory.clone()),
                );
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![resource],
                    LocalTreeMutationState::Started,
                ));
            }
        };
        let identity = observation.identity().cloned();
        if let Some(file) = created_file {
            let native_observation = match crate::observation::query::query_observation(
                &file,
                crate::observation::LocalObservationRequirement::Required,
            ) {
                Ok(observation) => observation,
                Err(cause) => {
                    let owner = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        identity.clone(),
                        self.clone(),
                        Some(directory.clone()),
                    );
                    return Err(LocalTreeCreateError::new(
                        cause,
                        None,
                        vec![owner],
                        LocalTreeMutationState::Started,
                    ));
                }
            };
            if identity.as_ref() != native_observation.identity() {
                let owner = LocalTreeRetainedResource::new(
                    path.clone(),
                    relative.clone(),
                    identity.clone(),
                    self.clone(),
                    Some(directory.clone()),
                );
                return Err(LocalTreeCreateError::new(
                    LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    ),
                    None,
                    vec![owner],
                    LocalTreeMutationState::Indeterminate,
                ));
            }
        }
        let owner = LocalTreeRetainedResource::new(
            path,
            relative.clone(),
            identity.clone(),
            self.clone(),
            Some(directory.clone()),
        );
        Ok(LocalStagedDirectory::new(
            owner,
            directory,
            relative,
            identity,
            options.privacy() == crate::policy::LocalPrivacyRequirement::Required,
        ))
    }
}

fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

fn stage_name() -> LocalResult<std::ffi::OsString> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|source| {
        LocalFileError::from_io(
            LocalFileOperation::GenerateName,
            None,
            None,
            std::io::Error::other(source.to_string()),
        )
    })?;
    let mut name = String::from("qubit-stage-");
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut name, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(name.into())
}

fn create_private_directory(
    parent: &crate::rooted::Root,
    name: &OsStr,
) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    let parent_file = parent.try_clone_authority().map_err(|source| {
        crate::temp::types::NativePrivateCreateError {
            source,
            retained: None,
        }
    })?;
    let result = {
        #[cfg(unix)]
        {
            crate::temp::internal::privacy_unix::create_private_directory_at(&parent_file, name)
        }
        #[cfg(windows)]
        {
            crate::temp::internal::privacy_windows::create_private_directory_at(&parent_file, name)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = name;
            Err(crate::temp::types::NativePrivateCreateError {
                source: std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "private directories are unsupported",
                ),
                retained: None,
            })
        }
    }?;
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("lf3-tree-private-directory-after-open") {
        let identity = crate::observation::query::query_observation(
            &result,
            crate::observation::LocalObservationRequirement::Required,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        let retained =
            parent_file
                .try_clone()
                .ok()
                .map(|parent| crate::temp::types::NativePrivateEntry {
                    parent,
                    name: name.to_os_string(),
                    file: Some(result),
                    identity,
                    directory: true,
                });
        return Err(crate::temp::types::NativePrivateCreateError {
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected failure after private stage open",
            ),
            retained,
        });
    }
    Ok(result)
}

fn create_private_file(
    parent: &File,
    name: &OsStr,
) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    #[cfg(unix)]
    {
        crate::temp::internal::privacy_unix::create_private_file_at(parent, name)
    }
    #[cfg(windows)]
    {
        crate::temp::internal::privacy_windows::create_private_file_at(parent, name)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, name);
        Err(crate::temp::types::NativePrivateCreateError {
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "private files are unsupported",
            ),
            retained: None,
        })
    }
}

fn cleanup_partial_private_file(
    cause: LocalFileError,
    parent: &File,
    name: &OsStr,
    file: File,
    identity: Option<LocalFileIdentity>,
    path: &Path,
) -> LocalFileError {
    let Ok(parent) = parent.try_clone() else {
        return cause;
    };
    let entry = crate::temp::types::NativePrivateEntry {
        parent,
        name: name.to_os_string(),
        file: Some(file),
        identity,
        directory: false,
    };
    let mut recovery = crate::LocalTempRecovery::from_native(path.to_path_buf(), entry);
    match recovery.cleanup() {
        Ok(()) => cause,
        Err(_) => cause.with_temp_recovery(recovery),
    }
}
