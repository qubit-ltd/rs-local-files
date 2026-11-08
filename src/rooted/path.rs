// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Validated rooted paths.

/// A validated relative path accepted by rooted filesystem operations.
///
/// The path must be non-empty and contain only normal relative components.
/// Use [`Path::new`] to validate candidate input before passing it to a
/// [`super::Root`].
pub type Path = crate::local::LocalRelativePath;
