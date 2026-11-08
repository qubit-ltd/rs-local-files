// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource guards and retained publication failures for the lifecycle model.

use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalResult;
use qubit_local_files::LocalTempDirectory;
use qubit_local_files::LocalTempFile;
use qubit_local_files::error::LocalPersistError;
use qubit_local_files::error::LocalPersistErrorParts;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalPersistOutcome;
use qubit_local_files::outcome::LocalPersistStage;
use qubit_local_files::outcome::LocalTempSourceState;
use qubit_local_files::policy::LocalDurabilityRequirement;

/// The public resource retained between independently decoded operations.
#[derive(Debug)]
pub(super) enum Resource {
    /// A temporary file tracked through lifecycle operations.
    File(LocalTempFile),
    /// A temporary directory tracked through lifecycle operations.
    Directory(LocalTempDirectory),
}

/// Publication errors keep ownership and both independent state axes.
pub(super) struct Retained {
    /// The guard returned by a failed publication attempt.
    pub(super) resource: Box<Resource>,
    /// Whether publication occurred before the failure.
    pub(super) publication: LocalPersistFailureState,
    /// The operation stage at which publication failed.
    pub(super) stage: LocalPersistStage,
    /// The retained source's authority after the failure.
    pub(super) source: LocalTempSourceState,
    /// The resolved target when target resolution was reached.
    pub(super) target: Option<PathBuf>,
}

/// Extracts named publication state while preserving the returned resource
/// guard.
fn retain<T>(error: LocalPersistError<T>, wrap: fn(T) -> Resource) -> Retained {
    let LocalPersistErrorParts {
        resource,
        state,
        source_state,
        stage,
        resolved_target,
        ..
    } = error.into_parts();
    Retained {
        resource: Box::new(wrap(resource)),
        publication: state,
        stage,
        source: source_state,
        target: resolved_target,
    }
}

impl Resource {
    /// Returns the creating authority's namespace path without host
    /// conversion.
    #[must_use]
    #[inline]
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::File(resource) => resource.path(),
            Self::Directory(resource) => resource.path(),
        }
    }

    /// Reads the guard's current source authority after preceding
    /// operations.
    #[inline]
    pub(super) fn source_state(&self) -> LocalTempSourceState {
        match self {
            Self::File(resource) => resource.source_state(),
            Self::Directory(resource) => resource.source_state(),
        }
    }

    /// Runs public cleanup, preserving the concrete error for assertions.
    pub(super) fn cleanup(&mut self) -> LocalResult<()> {
        match self {
            Self::File(resource) => resource.cleanup(),
            Self::Directory(resource) => resource.cleanup(),
        }
    }

    /// Publishes, keeps, or retries at an explicit base; failures retain
    /// the resource.
    pub(super) fn publish(
        self,
        base: &Path,
        target: &Path,
        opcode: u8,
        selector: u8,
        rooted: bool,
    ) -> Result<LocalPersistOutcome, Retained> {
        let durability = match selector % 3 {
            0 => LocalDurabilityRequirement::NotRequired,
            1 => LocalDurabilityRequirement::Preferred,
            _ => LocalDurabilityRequirement::Required,
        };
        let options = LocalPersistOptions::new().with_durability(durability);
        let (invalid_base, invalid_target) = match selector % 6 {
            0 => (Path::new("/"), Path::new("../../outside")),
            1 => (Path::new("relative"), Path::new("published")),
            2 => (Path::new("/."), Path::new("published")),
            3 => (Path::new("/"), Path::new("/absolute")),
            4 => (Path::new("/"), Path::new("")),
            _ => (Path::new("/missing"), Path::new("published")),
        };
        let relative_base = base.join("scratch");
        let relative_target = Path::new("..").join(target.file_name().unwrap_or_default());
        let (explicit_base, explicit_target) = if selector & 1 == 0 {
            (base, Path::new(target.file_name().unwrap_or_default()))
        } else {
            (relative_base.as_path(), relative_target.as_path())
        };
        match self {
            Self::File(resource) => {
                let result = match opcode {
                    3 => resource.keep(),
                    10 if rooted => resource.persist_at(invalid_base, invalid_target, options),
                    9 => resource.persist_at(explicit_base, explicit_target, options),
                    _ => resource.persist_with(target, options),
                };
                result.map_err(|error| retain(error, Self::File))
            }
            Self::Directory(resource) => {
                let result = match opcode {
                    3 => resource.keep(),
                    10 if rooted => resource.persist_at(invalid_base, invalid_target, options),
                    9 => resource.persist_at(explicit_base, explicit_target, options),
                    _ => resource.persist_with(target, options),
                };
                result.map_err(|error| retain(error, Self::Directory))
            }
        }
    }
}
