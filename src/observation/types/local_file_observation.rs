// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalFileIdentity;
use super::LocalNativeFileDetails;
use crate::LocalFileMetadata;

/// Metadata and native identity gathered from one retained handle.
///
/// Obtain observations from public filesystem operations such as
/// [`crate::directory::LocalDirectoryHandle::observe_entry`]. When identity is
/// available, it can be compared across observations from the same fixed
/// authority to check whether a name still refers to the same object. It does
/// not identify a content version or make separate observations atomic.
///
/// Use [`crate::directory::LocalDirectoryHandle::observe_entry`] to obtain an
/// observation. The identity and native details are optional because some
/// targets cannot provide them; metadata remains available when the operation
/// succeeds.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::LocalFileObservation;
///
/// fn observed_length(observation: &LocalFileObservation) -> u64 {
///     observation.metadata().len()
/// }
/// ```
#[derive(Clone, Debug)]
pub struct LocalFileObservation {
    /// Normalized metadata collected from the retained handle.
    metadata: LocalFileMetadata,
    /// Native object identity, absent when the platform query could not supply
    /// it.
    identity: Option<LocalFileIdentity>,
    /// Native timestamps and platform facts from that handle, when available.
    native_details: Option<LocalNativeFileDetails>,
}

impl LocalFileObservation {
    /// Constructs an observation from facts collected from one native handle.
    pub(crate) const fn from_parts(
        metadata: LocalFileMetadata,
        identity: Option<LocalFileIdentity>,
        native_details: Option<LocalNativeFileDetails>,
    ) -> Self {
        Self {
            metadata,
            identity,
            native_details,
        }
    }

    /// Returns normalized metadata from the observed handle.
    ///
    /// Metadata is present for every successfully created observation, even
    /// when native identity or timestamp details are unavailable.
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }

    /// Returns opaque object identity when the platform supplied it.
    ///
    /// `None` means identity was unavailable; it is not evidence that the
    /// observed name was missing.
    pub const fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }

    /// Returns native details gathered from the same handle, when available.
    ///
    /// `None` means the native query did not supply platform-specific facts.
    pub const fn native_details(&self) -> Option<&LocalNativeFileDetails> {
        self.native_details.as_ref()
    }
}
