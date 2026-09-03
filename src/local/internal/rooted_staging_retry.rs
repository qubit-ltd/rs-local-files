// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Retry control for cryptographically random rooted-staging collisions.
// qubit-style: allow source-test-pair
// Finite fixtures cannot deterministically exhaust random filename retries.

use std::ffi::CString;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;

/// Retries generated names until a rooted staging entry is opened.
///
/// # Type Parameters
///
/// * `G` - The filename generator.
/// * `O` - The operation that converts a generated name and opens its entry.
///
/// # Parameters
///
/// * `retries` - Optional maximum number of generated names to attempt.
/// * `generate` - The fallible filename generator called once per attempt.
/// * `open` - The fallible entry opener called with each generated name.
///
/// # Returns
///
/// The successful generated name, its native C representation, and the opened
/// staging handle.
///
/// # Errors
///
/// Returns a generator or non-collision open error immediately, or
/// `AlreadyExists` after all attempts collide.
pub(super) fn retry_rooted_staging_entry<G, O>(
    retries: Option<usize>,
    mut generate: G,
    mut open: O,
) -> Result<(String, CString, File)>
where
    G: FnMut() -> Result<String>,
    O: FnMut(&str) -> Result<(CString, File)>,
{
    if retries == Some(0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "rooted staging retry count must be greater than zero",
        ));
    }
    let mut attempt = 0_usize;
    loop {
        attempt = attempt.saturating_add(1);
        let name = generate()?;
        match open(&name) {
            Ok((native_name, file)) => {
                return Ok((name, native_name, file));
            }
            Err(error)
                if error.kind() == ErrorKind::AlreadyExists && retries.is_none_or(|retries| attempt < retries) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    "failed to create a unique rooted atomic staging file",
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::fs::File;
    use std::io::Error;
    use std::io::ErrorKind;

    use super::retry_rooted_staging_entry;

    #[test]
    fn test_retry_rooted_staging_entry_retries_collisions_and_returns_open_handle() {
        let fixture = tempfile::NamedTempFile::new().expect("fixture file should be created");
        let mut generated = 0_usize;
        let mut opened = 0_usize;

        let (name, native_name, file) = retry_rooted_staging_entry(
            Some(3),
            || {
                generated += 1;
                Ok(format!("staging-{generated}"))
            },
            |candidate| {
                opened += 1;
                if opened == 1 {
                    return Err(Error::from(ErrorKind::AlreadyExists));
                }
                Ok((
                    CString::new(candidate).expect("candidate has no NUL"),
                    File::open(fixture.path())?,
                ))
            },
        )
        .expect("a later open attempt should succeed");

        assert_eq!("staging-2", name);
        assert_eq!(CString::new("staging-2").expect("literal has no NUL"), native_name);
        assert!(file.metadata().expect("opened fixture should be queryable").is_file());
        assert_eq!(2, generated);
        assert_eq!(2, opened);
    }

    #[test]
    fn test_retry_rooted_staging_entry_rejects_zero_and_exhausted_retry_budgets() {
        let zero = retry_rooted_staging_entry(Some(0), || Ok("unused".to_owned()), |_| unreachable!())
            .expect_err("zero retries must be rejected");
        assert_eq!(ErrorKind::InvalidInput, zero.kind());

        let exhausted = retry_rooted_staging_entry(
            Some(2),
            || Ok("collision".to_owned()),
            |_| Err(Error::from(ErrorKind::AlreadyExists)),
        )
        .expect_err("bounded collisions must exhaust the retry budget");
        assert_eq!(ErrorKind::AlreadyExists, exhausted.kind());
    }

    #[test]
    fn test_retry_rooted_staging_entry_propagates_generation_and_open_errors() {
        let generated = retry_rooted_staging_entry::<_, fn(&str) -> _>(
            None,
            || Err(Error::from(ErrorKind::InvalidData)),
            |_| unreachable!(),
        )
        .expect_err("generator errors must not be retried");
        assert_eq!(ErrorKind::InvalidData, generated.kind());

        let opened = retry_rooted_staging_entry(
            None,
            || Ok("candidate".to_owned()),
            |_| Err(Error::from(ErrorKind::PermissionDenied)),
        )
        .expect_err("non-collision open errors must not be retried");
        assert_eq!(ErrorKind::PermissionDenied, opened.kind());
    }
}
