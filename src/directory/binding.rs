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

#[derive(Debug)]
pub(crate) struct PathBinding {
    filesystem: LocalFileSystem,
    name: OsString,
    identity: crate::observation::LocalFileIdentity,
    deadline: Option<Duration>,
}

impl PathBinding {
    pub(crate) fn new(
        filesystem: LocalFileSystem,
        name: OsString,
        identity: crate::observation::LocalFileIdentity,
        deadline: Option<Duration>,
    ) -> Self {
        Self {
            filesystem,
            name,
            identity,
            deadline,
        }
    }
}

impl ReaderPathBinding for PathBinding {
    fn verify(&self) -> LocalResult<LocalSourceVerification> {
        let started_at = Instant::now();
        match self.filesystem.open_observed_reader(
            Path::new(&self.name),
            &crate::options::LocalObservedReadOptions::new(),
        ) {
            Ok(reader) => {
                if self
                    .deadline
                    .is_some_and(|limit| started_at.elapsed() >= limit)
                {
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
    fn verify(&self) -> LocalResult<LocalSourceVerification> {
        let started_at = Instant::now();
        for anchor in &self.anchors {
            if self
                .deadline
                .is_some_and(|limit| started_at.elapsed() >= limit)
            {
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
            if retained_observation.identity() != Some(&anchor.identity) {
                return Ok(LocalSourceVerification::Changed);
            }
            let root = match crate::rooted::Root::open(&anchor.path) {
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
            if observation.identity() != Some(&anchor.identity) {
                return Ok(LocalSourceVerification::Changed);
            }
        }
        if self
            .deadline
            .is_some_and(|limit| started_at.elapsed() >= limit)
        {
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
        Ok(LocalSourceVerification::Unchanged)
    }
}
