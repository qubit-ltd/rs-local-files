// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use super::binding_anchor::BindingAnchor;
use crate::LocalResult;
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;

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
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
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
            let root_result = crate::rooted_local_file_system::RootedLocalFileSystem::open_with_contract(
                &anchor.opening_recipe,
                anchor.contract.clone(),
                anchor.symlink_policy,
            );
            check_verification_deadline(deadline, &anchor.path)?;
            let root = match root_result {
                Ok(root) => root,
                Err(error) if error.kind() == crate::error::LocalFileErrorKind::NotFound => {
                    return Ok(LocalSourceVerification::Missing);
                }
                Err(error) => {
                    return Err(error
                        .with_operation(crate::LocalFileOperation::Metadata)
                        .with_path(anchor.path.clone()));
                }
            };
            check_verification_deadline(deadline, &anchor.path)?;
            let file = root.root_authority().try_clone_authority().map_err(|error| {
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
