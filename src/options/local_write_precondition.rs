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
#[must_use]
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalWritePrecondition<T> {
    /// Publish only when the destination does not exist.
    Absent,
    /// Publish only when the destination content has the expected digest.
    Content(T),
}

#[cfg(test)]
mod conditional_types_tests {
    use sha2::Digest;
    use sha2::Sha256;

    use super::LocalWritePrecondition;

    #[test]
    fn conditional_types_keep_full_digest() {
        let expected = Sha256::digest(b"A");
        let condition = LocalWritePrecondition::Content(expected);
        assert!(matches!(condition, LocalWritePrecondition::Content(value) if value == Sha256::digest(b"A")));
    }
}
