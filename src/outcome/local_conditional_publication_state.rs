// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Observed state of the destination when a conditional commit terminates.
///
/// `Indeterminate` means the backend could not verify whether publication
/// completed; callers must inspect the destination before retrying.
///
/// # Examples
///
/// ```
/// use qubit_io::Output;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalWriteMode;
/// use qubit_local_files::options::LocalWriteOptions;
/// use qubit_local_files::options::LocalWritePrecondition;
/// use qubit_local_files::outcome::LocalConditionalPublicationState;
/// use qubit_local_files::policy::LocalSymlinkPolicy;
/// use sha2::Sha256;
/// use tempfile::tempdir;
///
/// let directory = tempdir()?;
/// let target = directory.path().canonicalize()?.join("model");
/// std::fs::write(&target, b"existing")?;
/// let mut filesystem = LocalFileSystem::host()?;
/// filesystem.set_symlink_policy(LocalSymlinkPolicy::Reject)?;
/// let mut writer = filesystem.open_conditional_writer::<Sha256>(
///     &target,
///     LocalWritePrecondition::Absent,
///     &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
/// )?;
/// writer.write_fully(b"candidate bytes")?;
/// let error = writer.commit().expect_err("the target already exists");
/// assert_eq!(error.publication(), LocalConditionalPublicationState::NotPublished);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalConditionalPublicationState {
    /// No publication operation changed the destination.
    NotPublished,
    /// The candidate was published at the destination.
    Published,
    /// A publication occurred and the previous destination was restored.
    Restored,
    /// The backend cannot determine the final destination state.
    Indeterminate,
}
