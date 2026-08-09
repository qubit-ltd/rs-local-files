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
/// * `retries` - The maximum number of generated names to attempt.
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
    retries: usize,
    mut generate: G,
    mut open: O,
) -> Result<(String, CString, File)>
where
    G: FnMut() -> Result<String>,
    O: FnMut(&str) -> Result<(CString, File)>,
{
    for _ in 0..retries {
        let name = generate()?;
        match open(&name) {
            Ok((native_name, file)) => {
                return Ok((name, native_name, file));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::new(
        ErrorKind::AlreadyExists,
        "failed to create a unique rooted atomic staging file",
    ))
}
