// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fixed directory authorities and native component policies.

pub(crate) mod binding;
mod binding_anchor;
#[path = "local_component_policy.rs"]
mod component_policy;
mod conditional;
mod directory_path_binding;
#[path = "local_directory_handle.rs"]
mod handle;
mod internal;
mod local_resolution_origin;
mod local_resolution_policy;
mod local_resolution_scope;
mod local_tree_entry;
mod local_tree_entry_observation;
mod local_tree_validation_options;
mod path_binding;
#[path = "local_staged_directory.rs"]
mod staged;
#[cfg(all(test, unix))]
pub(crate) use handle::install_component_policy_checkpoint;
#[cfg(test)]
pub(crate) use staged::fail_tree_sync_after;
#[cfg(test)]
pub(crate) use staged::install_stage_created_hook;
#[path = "local_tree_observer.rs"]
mod tree_observer;
#[cfg(test)]
pub(crate) use tree_observer::reset_test_observation_count;
#[cfg(test)]
pub(crate) use tree_observer::test_observation_count;
mod tree_recovery;
mod tree_validation;

pub use component_policy::LocalComponentPolicy;
pub use handle::LocalDirectoryHandle;
pub(crate) use handle::retain_directory_chain;
pub use local_resolution_origin::LocalResolutionOrigin;
pub use local_resolution_policy::LocalResolutionPolicy;
pub use local_resolution_scope::LocalResolutionScope;
pub use staged::LocalStagedDirectory;
pub use tree_observer::LocalTreeObserver;
pub use tree_recovery::LocalTreeAccessError;
pub use tree_recovery::LocalTreeCleanupError;
pub use tree_recovery::LocalTreeCreateError;
pub use tree_recovery::LocalTreePublishError;
pub use tree_recovery::LocalTreeRecoveryLocation;
pub use tree_recovery::LocalTreeRetainedResource;
pub use tree_validation::LocalTreeEntry;
pub use tree_validation::LocalTreeEntryObservation;
pub use tree_validation::LocalTreeValidationOptions;
