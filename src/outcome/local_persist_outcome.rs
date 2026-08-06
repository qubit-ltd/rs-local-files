// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured successful temporary-resource persistence outcomes.

use std::path::{
    Path,
    PathBuf,
};

use crate::LocalPersistMethod;

/// Guarantees actually achieved while persisting a temporary resource.
#[must_use]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPersistOutcome {
    /// Authority-local path at which the resource was published.
    path: PathBuf,
    /// Native publication method.
    method: LocalPersistMethod,
    /// Whether publication was atomic.
    atomic: bool,
    /// Whether persistence durability was synchronized.
    durable: bool,
}

impl LocalPersistOutcome {
    /// Creates a temporary-resource persistence outcome.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    pub(crate) const fn new(
        path: PathBuf,
        method: LocalPersistMethod,
        atomic: bool,
        durable: bool,
    ) -> Self {
        Self {
            path,
            method,
            atomic,
            durable,
        }
    }

    /// Returns the authority-local published path.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the native publication method.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn method(&self) -> LocalPersistMethod {
        self.method
    }

    /// Reports whether publication was atomic.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn atomic(&self) -> bool {
        self.atomic
    }

    /// Reports whether persistence durability was synchronized.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn durable(&self) -> bool {
        self.durable
    }

    /// Returns the owned authority-local published path.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}
