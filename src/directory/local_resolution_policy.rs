// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::path::Path;

use crate::LocalResult;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Identifies whether resolution is opening the authority root or a descendant.
pub enum LocalResolutionScope {
    /// The native path is being resolved to establish the root authority.
    OpeningRoot,
    /// The path is being resolved beneath an established authority.
    Descendant,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Identifies whether a path component came from caller input or a link target.
pub enum LocalResolutionOrigin {
    /// The component came from the operation's input path.
    Input,
    /// The component came from the native target of a followed symlink.
    SymlinkTarget,
}
/// Pure validation hooks applied before the resolver accesses a component or
/// target.
///
/// Implementations should be deterministic and must not perform filesystem
/// operations. The library may invoke hooks more than once while resolving or
/// rechecking a binding; it does not promise a particular invocation count.
pub trait LocalResolutionPolicy: std::fmt::Debug + Send + Sync {
    /// Validates a normal component before the resolver performs metadata or
    /// open calls.
    fn validate_component(
        &self,
        component: &OsStr,
        scope: LocalResolutionScope,
        origin: LocalResolutionOrigin,
    ) -> LocalResult<()>;
    /// Validates the unnormalized native target before the resolver interprets
    /// its components.
    fn validate_symlink_target(&self, target: &Path, scope: LocalResolutionScope) -> LocalResult<()>;
}
