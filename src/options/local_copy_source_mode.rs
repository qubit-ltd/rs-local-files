// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native copy source interpretation mode.

/// Selects the source kind accepted by a unified local copy.
///
/// Selection uses metadata without following the final link. `Tree` rejects
/// links even when their targets are directories; `Entry` copies the link
/// itself. Unsupported special files are rejected before destination mutation.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalCopyOptions;
/// use qubit_local_files::options::LocalCopySourceMode;
///
/// let options = LocalCopyOptions::new().with_entry_source();
/// assert_eq!(options.source_mode(), LocalCopySourceMode::Entry);
/// let automatic = options.with_source_mode(LocalCopySourceMode::Auto);
/// assert_eq!(automatic.source_mode(), LocalCopySourceMode::Auto);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub enum LocalCopySourceMode {
    /// Copy one regular file or symbolic-link entry, never a directory.
    ///
    /// Final links are copied without opening their targets, including dangling
    /// links.
    Entry,
    /// Require a directory-tree source.
    Tree,
    /// Select entry or tree copying from the source metadata.
    ///
    /// Special files are unsupported in every mode.
    #[default]
    Auto,
}
