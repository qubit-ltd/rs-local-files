// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Same-handle file observations and source verification values.

mod local_access_observation;
mod local_acl_presence;
mod local_file_system_locality;
mod local_file_system_observation;
mod local_security_observation;
mod local_security_observation_options;
mod local_unix_access_observation;
mod local_unix_file_system_observation;
mod local_unix_platform;
mod local_windows_access_observation;
mod local_windows_ace_observation;
mod local_windows_file_system_observation;
pub(crate) mod query;
#[path = "local_observed_reader.rs"]
pub(crate) mod reader;
pub(crate) mod security;
#[path = "local_file_snapshot.rs"]
pub(crate) mod snapshot;
mod types;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub use local_access_observation::LocalAccessObservation;
pub use local_acl_presence::LocalAclPresence;
pub use local_file_system_locality::LocalFileSystemLocality;
pub use local_file_system_observation::LocalFileSystemObservation;
pub use local_security_observation::LocalSecurityObservation;
pub use local_security_observation_options::LocalSecurityObservationOptions;
pub use local_unix_access_observation::LocalUnixAccessObservation;
pub use local_unix_file_system_observation::LocalUnixFileSystemObservation;
pub use local_unix_platform::LocalUnixPlatform;
pub use local_windows_access_observation::LocalWindowsAccessObservation;
pub use local_windows_ace_observation::LocalWindowsAceObservation;
pub use local_windows_file_system_observation::LocalWindowsFileSystemObservation;
pub use reader::LocalObservedReader;
pub use snapshot::LocalFileSnapshot;
pub use types::LocalFileIdentity;
pub use types::LocalFileObservation;
pub use types::LocalNativeFileDetails;
pub use types::LocalObservationRequirement;
pub use types::LocalSourceVerification;
pub use types::LocalUnixFileDetails;
pub use types::LocalWindowsFileDetails;
