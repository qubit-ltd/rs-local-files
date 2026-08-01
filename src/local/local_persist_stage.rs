// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Temporary-resource persistence failure stages.
// qubit-style: allow source-test-pair

/// Stage at which temporary-resource persistence failed.
///
/// Additional stages may be added as persistence behavior evolves. Downstream
/// matches must retain a wildcard arm.
///
/// ```compile_fail
/// use qubit_local_files::LocalPersistStage;
///
/// fn classify(stage: LocalPersistStage) {
///     match stage {
///         LocalPersistStage::ResolveTarget => {}
///         LocalPersistStage::PrepareParent => {}
///         LocalPersistStage::InstallDestination => {}
///     }
/// }
/// ```
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::LocalPersistStage;
///
/// LocalPersistStage::ResolveTarget.clone();
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPersistStage {
    /// Resolving the requested target to an absolute path failed.
    ResolveTarget,
    /// Preparing the target's parent directory failed.
    PrepareParent,
    /// Installing the temporary resource at the target failed.
    InstallDestination,
}
