// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Condition that must hold for a conditional write to be published.
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
    /// Publish only when the destination content has the expected digest.
    Content(
        /// Digest the destination must match before the write can be published.
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
