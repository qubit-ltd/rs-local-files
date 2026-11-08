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

/// Revalidates directory path bindings against their retained file identities.
///
/// Each anchor is checked independently during a verification pass. A
/// successful pass does not reserve namespace entries or prevent them from
/// changing later.
#[derive(Debug)]
pub(crate) struct DirectoryPathBinding {
    /// Paths and retained authorities whose identities must remain consistent.
    anchors: Vec<BindingAnchor>,
    /// Maximum duration allowed for one verification pass, when configured.
    deadline: Option<Duration>,
}

impl DirectoryPathBinding {
    /// Creates a binding verifier for the supplied path anchors.
    ///
    /// # Parameters
    ///
    /// * `anchors` — retained path and identity data to verify.
    /// * `deadline` — optional duration limit applied to verification.
    ///
    /// # Returns
    ///
    /// A verifier that checks each anchor against its retained identity.
    #[inline]
    pub(crate) fn new(anchors: Vec<BindingAnchor>, deadline: Option<Duration>) -> Self {
        Self { anchors, deadline }
    }
}

impl ReaderPathBinding for DirectoryPathBinding {
    /// Returns the configured duration limit for verification.
    ///
    /// # Returns
    ///
    /// `Some(duration)` when a verification limit was configured, or `None`
    /// when verification has no duration limit.
    #[inline]
    fn verification_deadline(&self) -> Option<Duration> {
        self.deadline
    }

    /// Reopens each anchored pathname and compares its identity with the
    /// retained object. A missing name is a successful `Missing` result;
    /// non-NotFound resolution/query failures and deadline expiration are
    /// errors. This check describes the verification pass only and does not
    /// reserve a name or prevent a subsequent namespace change.
    // Each retained anchor proves only that the path resolved to the same
    // identity during this verification pass. Reopening and checking a path
    // does not pin its namespace entry against changes after this method ends.
    ///
    /// # Parameters
    ///
    /// * `deadline` — optional absolute instant after which verification fails.
    ///
    /// # Returns
    ///
    /// `Unchanged` if every path still resolves to its retained identity,
    /// `Changed` if an identity differs, or `Missing` if an anchored path no
    /// longer exists.
    ///
    /// # Errors
    ///
    /// Returns an error if authority cloning, metadata observation, or path
    /// reopening fails for a reason other than a missing path, or if the
    /// deadline expires during verification.
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
            let root_result = crate::rooted_local_file_system::RootedLocalFileSystem::open_with_contract_and_deadline(
                &anchor.opening_recipe,
                anchor.contract.clone(),
                anchor.symlink_policy,
                deadline,
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

/// Checks whether a verification pass has reached its optional deadline.
///
/// # Parameters
///
/// * `deadline` — absolute instant that bounds the verification pass.
/// * `path` — path to associate with a deadline error.
///
/// # Errors
///
/// Returns a metadata-operation error with `TimedOut` when the deadline has
/// expired; otherwise, returns success.
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
