// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy options.
// qubit-style: allow source-test-pair

use std::time::Duration;

use crate::{
    LocalCopyConflictPolicy,
    LocalCopyTypeConflictPolicy,
    LocalDurabilityRequirement,
    LocalSymlinkPolicy,
};

/// Options controlling recursive directory copy behavior.
///
/// The default is conservative: existing destination entries are not
/// overwritten, directory symbolic links are not followed, and source
/// permissions are not copied to destination entries. Final symbolic-link
/// entries are copied as links. On Unix, newly created files therefore keep
/// the private staging mode `0o600` and newly created directories use `0o700`,
/// subject to a more restrictive process umask.
///
/// Regular-file validation and preserved file permissions are read from the
/// same opened source handle that supplies copied bytes. Disabling symbolic
/// links requests a no-follow open on Unix and rejects Windows name-surrogate
/// reparse handles. Directory traversal and destination mutation remain
/// path-based and require external containment when the tree is adversarial.
///
/// File commits using [`LocalCopyConflictPolicy::Fail`] or
/// [`LocalCopyConflictPolicy::Skip`] require native no-replace installation,
/// which is available on Linux, macOS, and Windows. Other targets return
/// [`std::io::ErrorKind::Unsupported`] at the file-commit stage. The
/// [`LocalCopyConflictPolicy::Overwrite`] policy uses ordinary replacement and
/// is not subject to that no-replace matrix. Destination directories created
/// before a failed file commit are not rolled back.
///
/// Construct this non-exhaustive type through [`Self::new`] and its builders.
#[must_use = "directory copy options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCopyDirOptions {
    /// Policy for existing destination file entries.
    conflict: LocalCopyConflictPolicy,

    /// Policy for source and destination entry type mismatches.
    type_conflict: LocalCopyTypeConflictPolicy,

    /// Symbolic-link policy for the source tree.
    ///
    /// `Reject` copies directory symbolic links as final link entries instead
    /// of traversing them. File symbolic links are always copied as link
    /// entries rather than dereferenced.
    ///
    /// File type is verified from the opened source handle. Directory
    /// traversal, destination reinspection, and destructive replacement remain
    /// separate path-based operations, so this policy is not a sandbox
    /// boundary when an untrusted actor can mutate either tree
    /// concurrently.
    symlink_policy: LocalSymlinkPolicy,

    /// Whether to copy source permissions to destination entries after
    /// copying.
    ///
    /// File permissions come from metadata for the same opened handle used to
    /// copy the bytes. Directory permissions come from traversal metadata.
    /// This uses `std::fs::set_permissions` and therefore only preserves the
    /// portable permission bits exposed by the Rust standard library. When
    /// this is `false`, new or replaced files retain the copy staging mode and
    /// new directories retain the private directory mode; on Unix these are
    /// `0o600` and `0o700`, respectively, subject to the process umask.
    preserve_permissions: bool,

    /// Optional limit for retrying a nonblocking source open.
    open_retry_timeout: Option<Duration>,

    /// Synchronization policy for staged regular files.
    durability: LocalDurabilityRequirement,
}

impl LocalCopyDirOptions {
    /// Returns conservative directory copy options.
    ///
    /// # Returns
    /// Options that fail on destination conflicts, do not follow symbolic
    /// links, and do not preserve source permissions.
    #[inline]
    pub const fn new() -> Self {
        Self {
            conflict: LocalCopyConflictPolicy::Fail,
            type_conflict: LocalCopyTypeConflictPolicy::Fail,
            symlink_policy: LocalSymlinkPolicy::Reject,
            preserve_permissions: false,
            open_retry_timeout: None,
            durability: LocalDurabilityRequirement::NotRequired,
        }
    }

    /// Sets the synchronization policy for staged regular files.
    #[inline(always)]
    pub(crate) const fn with_durability(
        mut self,
        durability: LocalDurabilityRequirement,
    ) -> Self {
        self.durability = durability;
        self
    }

    /// Returns the staged-file synchronization policy.
    #[inline(always)]
    pub(crate) const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Returns the destination file conflict policy.
    ///
    /// # Returns
    /// Policy applied to existing destination file entries.
    #[inline(always)]
    pub const fn conflict_policy(&self) -> LocalCopyConflictPolicy {
        self.conflict
    }

    /// Sets the policy for existing destination file entries.
    ///
    /// # Parameters
    /// - `conflict`: Conflict policy to use.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn with_conflict(
        mut self,
        conflict: LocalCopyConflictPolicy,
    ) -> Self {
        self.conflict = conflict;
        self
    }

    /// Returns the entry type-conflict policy.
    ///
    /// # Returns
    /// Policy applied to source and destination type mismatches.
    #[inline(always)]
    pub const fn type_conflict_policy(&self) -> LocalCopyTypeConflictPolicy {
        self.type_conflict
    }

    /// Sets the policy for source and destination type mismatches.
    ///
    /// # Parameters
    /// - `type_conflict`: Type-conflict policy to use.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn with_type_conflict(
        mut self,
        type_conflict: LocalCopyTypeConflictPolicy,
    ) -> Self {
        self.type_conflict = type_conflict;
        self
    }

    /// Returns the symbolic-link policy used while traversing the source tree.
    ///
    /// # Returns
    /// The configured source-tree symbolic-link policy.
    #[inline(always)]
    pub(crate) const fn symlink_policy(&self) -> LocalSymlinkPolicy {
        self.symlink_policy
    }

    /// Sets the symbolic-link policy used while traversing the source tree.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub(crate) const fn with_symlink_policy(
        mut self,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Self {
        self.symlink_policy = symlink_policy;
        self
    }

    /// Returns whether source permissions will be preserved.
    ///
    /// # Returns
    /// `true` when destination permissions are copied from the source.
    #[must_use]
    #[inline(always)]
    pub const fn preserves_permissions(&self) -> bool {
        self.preserve_permissions
    }

    /// Enables preservation of source permissions.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn preserve_permissions(mut self) -> Self {
        self.preserve_permissions = true;
        self
    }

    /// Returns the configured nonblocking-open retry timeout.
    ///
    /// On Unix, this limits how long a copy waits for a regular source whose
    /// active file lease makes a nonblocking open return
    /// [`std::io::ErrorKind::WouldBlock`]. `None` preserves the default
    /// unbounded wait.
    ///
    /// # Returns
    /// The configured timeout, or `None` when retries are unbounded.
    #[must_use]
    #[inline(always)]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Sets the nonblocking-open retry timeout.
    ///
    /// On Unix, [`Duration::ZERO`] returns
    /// [`std::io::ErrorKind::TimedOut`] after the first lease-conflicting open
    /// attempt. Other open errors are never retried.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time to retry a lease-conflicting open.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    #[allow(dead_code)]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }
}

impl Default for LocalCopyDirOptions {
    /// Returns conservative directory copy options.
    ///
    /// # Returns
    /// Options that do not overwrite existing destination entries, do not
    /// follow symbolic links, and do not preserve source permissions.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    fn default() -> Self {
        Self::new()
    }
}
