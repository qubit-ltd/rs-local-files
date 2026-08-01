// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Namespace state known after a temporary-resource persistence failure.

use std::io;

use crate::LocalPersistStage;

/// Strongest namespace state known after persistence fails.
///
/// [`Self::NotPublished`] means the temporary resource remains owned and can
/// be cleaned up or retried. [`Self::Indeterminate`] means a native install
/// attempt may have changed the namespace, so cleanup is unsafe.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPersistFailureState {
    /// The target was not published and the temporary resource remains owned.
    NotPublished,
    /// The target was published, but cleanup of the source was not completed.
    PublishedSourceRetained,
    /// A native install attempt left publication state unknown.
    Indeterminate,
}

impl LocalPersistFailureState {
    /// Classifies a failure from its stage and native error kind.
    pub(crate) const fn from_error(stage: LocalPersistStage, kind: io::ErrorKind) -> Self {
        match stage {
            LocalPersistStage::ResolveTarget | LocalPersistStage::PrepareParent => {
                Self::NotPublished
            }
            LocalPersistStage::InstallDestination
                if matches!(
                    kind,
                    io::ErrorKind::AlreadyExists
                        | io::ErrorKind::CrossesDevices
                        | io::ErrorKind::InvalidInput
                        | io::ErrorKind::IsADirectory
                        | io::ErrorKind::NotADirectory
                        | io::ErrorKind::DirectoryNotEmpty
                        | io::ErrorKind::Unsupported
                ) =>
            {
                Self::NotPublished
            }
            LocalPersistStage::InstallDestination => Self::Indeterminate,
        }
    }
}
