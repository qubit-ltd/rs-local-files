// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Same-handle file observations and source verification values.

pub(crate) mod query;
#[path = "local_observed_reader.rs"]
pub(crate) mod reader;
#[path = "local_file_snapshot.rs"]
pub(crate) mod snapshot;
mod types;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub use reader::LocalObservedReader;
pub use snapshot::LocalFileSnapshot;
pub use types::LocalFileIdentity;
pub use types::LocalFileObservation;
pub use types::LocalNativeFileDetails;
pub use types::LocalObservationRequirement;
pub use types::LocalSourceVerification;
pub use types::LocalUnixFileDetails;
pub use types::LocalWindowsFileDetails;
