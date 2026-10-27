use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::LocalFileSystem;
use crate::LocalResult;
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;

/// Shared byte budget for path resolution and retained ancestor anchors.
#[derive(Debug)]
pub(crate) struct PathBindingBudget {
    limit: Option<usize>,
    used: usize,
}

impl PathBindingBudget {
    pub(crate) const fn new(limit: Option<usize>) -> Self {
        Self { limit, used: 0 }
    }

    pub(crate) fn debit(
        &mut self,
        bytes: usize,
        operation: crate::LocalFileOperation,
        path: &Path,
    ) -> LocalResult<()> {
        self.used = self.used.checked_add(bytes).ok_or_else(|| {
            crate::LocalFileError::new(crate::LocalFileErrorKind::ResourceLimit, operation)
                .with_path(path.to_path_buf())
        })?;
        if self.limit.is_some_and(|limit| self.used > limit) {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                operation,
            )
            .with_path(path.to_path_buf()));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct PathBinding {
    filesystem: LocalFileSystem,
    name: OsString,
    identity: crate::observation::LocalFileIdentity,
    options: crate::options::LocalPathBindingOptions,
}

impl PathBinding {
    pub(crate) fn new(
        filesystem: LocalFileSystem,
        name: OsString,
        identity: crate::observation::LocalFileIdentity,
        options: crate::options::LocalPathBindingOptions,
    ) -> Self {
        Self {
            filesystem,
            name,
            identity,
            options,
        }
    }
}

impl ReaderPathBinding for PathBinding {
    fn verification_deadline(&self) -> Option<Duration> {
        self.options.deadline()
    }

    fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        let result = self.filesystem.open_observed_reader_with_binding_deadline(
            Path::new(&self.name),
            &crate::options::LocalObservedReadOptions::new(),
            self.options,
            deadline,
        );
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::Metadata,
                None,
                None,
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "path binding deadline exceeded",
                ),
            ));
        }
        match result {
            Ok(reader) => {
                Ok(if reader.observation().identity() == Some(&self.identity) {
                    LocalSourceVerification::Unchanged
                } else {
                    LocalSourceVerification::Changed
                })
            }
            Err(error) if error.kind() == crate::error::LocalFileErrorKind::NotFound => {
                Ok(LocalSourceVerification::Missing)
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BindingAnchor {
    pub(crate) path: PathBuf,
    pub(crate) identity: crate::observation::LocalFileIdentity,
    pub(crate) authority: Arc<crate::rooted::Root>,
}

#[derive(Debug)]
pub(crate) struct DirectoryPathBinding {
    anchors: Vec<BindingAnchor>,
    deadline: Option<Duration>,
}

impl DirectoryPathBinding {
    pub(crate) fn new(anchors: Vec<BindingAnchor>, deadline: Option<Duration>) -> Self {
        Self { anchors, deadline }
    }
}

impl ReaderPathBinding for DirectoryPathBinding {
    fn verification_deadline(&self) -> Option<Duration> {
        self.deadline
    }

    fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        for anchor in &self.anchors {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(crate::LocalFileError::from_io(
                    crate::LocalFileOperation::Metadata,
                    Some(anchor.path.clone()),
                    None,
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "path binding deadline exceeded",
                    ),
                ));
            }
            let retained = anchor.authority.try_clone_authority().map_err(|error| {
                crate::LocalFileError::from_io(
                    crate::LocalFileOperation::Metadata,
                    Some(anchor.path.clone()),
                    None,
                    error,
                )
            })?;
            let retained_observation = crate::observation::query::query_observation(
                &retained,
                crate::observation::LocalObservationRequirement::Optional,
            )?;
            check_verification_deadline(deadline, &anchor.path)?;
            if retained_observation.identity() != Some(&anchor.identity) {
                return Ok(LocalSourceVerification::Changed);
            }
            let root_result = crate::rooted::Root::open(&anchor.path);
            check_verification_deadline(deadline, &anchor.path)?;
            let root = match root_result {
                Ok(root) => root,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(LocalSourceVerification::Missing);
                }
                Err(error) => {
                    return Err(crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Metadata,
                        Some(anchor.path.clone()),
                        None,
                        error,
                    ));
                }
            };
            check_verification_deadline(deadline, &anchor.path)?;
            let file = root.try_clone_authority().map_err(|error| {
                crate::LocalFileError::from_io(
                    crate::LocalFileOperation::Metadata,
                    Some(anchor.path.clone()),
                    None,
                    error,
                )
            })?;
            let observation = crate::observation::query::query_observation(
                &file,
                crate::observation::LocalObservationRequirement::Optional,
            )?;
            check_verification_deadline(deadline, &anchor.path)?;
            if observation.identity() != Some(&anchor.identity) {
                return Ok(LocalSourceVerification::Changed);
            }
        }
        check_verification_deadline(deadline, Path::new(""))?;
        Ok(LocalSourceVerification::Unchanged)
    }
}

fn check_verification_deadline(deadline: Option<Instant>, path: &Path) -> LocalResult<()> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(crate::LocalFileError::from_io(
            crate::LocalFileOperation::Metadata,
            Some(path.to_path_buf()),
            None,
            std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PathBinding;
    use crate::LocalFileSystem;
    use crate::observation::reader::ReaderPathBinding;
    use crate::options::LocalObservedReadOptions;
    use crate::options::LocalPathBindingOptions;

    #[test]
    fn path_binding_reopen_obeys_verification_deadline() {
        let directory = tempfile::tempdir().expect("fixture should exist");
        std::fs::write(directory.path().join("source"), b"source").expect("source should exist");
        let filesystem = LocalFileSystem::rooted(directory.path()).expect("root should open");
        let reader = filesystem
            .open_observed_reader(std::path::Path::new("source"), &LocalObservedReadOptions::new())
            .expect("reader should open");
        let binding = PathBinding::new(
            filesystem,
            "source".into(),
            reader.observation().identity().cloned().expect("identity should be available"),
            LocalPathBindingOptions::new().with_deadline(std::time::Duration::ZERO),
        );
        let error = binding.verify().unwrap_err();
        assert_eq!(error.io_error().unwrap().kind(), std::io::ErrorKind::TimedOut);
    }
}
