// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Selects the destination condition and verification protocol for a
/// conditional write.
///
/// `Absent` uses a publication protocol that does not overwrite an existing
/// destination. `Content` checks the expected digest before publication and
/// verifies the displaced content afterward; it is not a strict
/// compare-and-swap. A concurrent change may be detected after the candidate
/// becomes visible, and rollback or recovery may fail. To determine the actual
/// effect, inspect the result of [`LocalConditionalWriter::commit`] or the
/// error's [`LocalConditionalPublicationState`]. `Restored` means
/// publication occurred and the previous destination was restored; it does not
/// mean the candidate was never visible.
///
/// [`LocalConditionalWriter::commit`]: crate::writer::LocalConditionalWriter::commit
/// [`LocalConditionalPublicationState`]: crate::outcome::LocalConditionalPublicationState
/// [`LocalConditionalCommitError`]: crate::error::LocalConditionalCommitError
///
/// # Type Parameters
///
/// - `T`: The expected content digest when using [`Self::Content`].
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalWritePrecondition;
/// use sha2::{Digest, Sha256};
///
/// let expected = Sha256::digest(b"current content");
/// let precondition = LocalWritePrecondition::Content(expected);
/// assert!(matches!(precondition, LocalWritePrecondition::Content(_)));
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalWritePrecondition<T> {
    /// Publish only when the destination does not exist.
    Absent,
    /// Requests digest-based verification of the destination content around
    /// publication; this is not a strict compare-and-swap.
    Content(
        /// Expected digest used by the `Content` verification protocol.
        T,
    ),
}

#[cfg(test)]
mod conditional_types_tests {
    use sha2::Digest;
    use sha2::Sha256;

    use super::LocalWritePrecondition;

    #[test]
    fn test_content_precondition_keeps_full_digest() {
        let expected = Sha256::digest(b"A");
        let condition = LocalWritePrecondition::Content(expected);
        assert!(matches!(condition, LocalWritePrecondition::Content(value) if value == Sha256::digest(b"A")));
    }
}
