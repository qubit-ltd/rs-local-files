// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private random file-name generation.
// qubit-style: allow source-test-pair

use std::io::{
    Error,
    Result,
};
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use super::file_name_validation::validate_file_name_fragment;

/// Random payload length used by generated file names.
const RANDOM_NAME_BYTES: usize = 16;

/// Tries to build a random file-name component.
///
/// # Parameters
/// - `default_prefix`: Prefix used when `prefix` is `None`.
/// - `prefix`: Optional caller-provided prefix.
/// - `suffix`: Optional caller-provided suffix.
///
/// # Returns
/// A random file-name component.
///
/// # Errors
/// Returns [`std::io::ErrorKind::InvalidInput`] when a caller-provided
/// fragment can behave like a path. Returns
/// [`std::io::ErrorKind::Other`] when operating-system randomness is
/// unavailable.
pub(crate) fn try_random_file_name(
    default_prefix: &str,
    prefix: Option<&str>,
    suffix: Option<&str>,
) -> Result<String> {
    let prefix = prefix.unwrap_or(default_prefix);
    let suffix = suffix.unwrap_or("");
    validate_file_name_fragment("prefix", prefix)?;
    validate_file_name_fragment("suffix", suffix)?;
    let timestamp = unix_timestamp_nanos();
    let process_id = std::process::id();
    let random = try_random_hex()?;
    Ok(format!(
        "{prefix}{timestamp:x}-{process_id:x}-{random}{suffix}"
    ))
}

/// Returns the current Unix timestamp in nanoseconds.
///
/// # Returns
/// Nanoseconds since the Unix epoch, or zero if the system clock is earlier
/// than the epoch.
#[inline]
#[must_use]
fn unix_timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

/// Tries to return random bytes encoded as lowercase hexadecimal.
///
/// # Returns
/// A hexadecimal string derived from operating-system randomness.
///
/// # Errors
/// Returns [`std::io::ErrorKind::Other`] if the operating system random
/// source cannot provide bytes.
fn try_random_hex() -> Result<String> {
    let mut bytes = [0_u8; RANDOM_NAME_BYTES];
    fill_random_bytes(&mut bytes)?;
    Ok(hex_encode(&bytes))
}

/// Fills a byte slice with random bytes.
///
/// # Parameters
/// - `bytes`: Destination buffer.
///
/// # Errors
/// Returns [`std::io::ErrorKind::Other`] if the operating system random
/// source cannot provide bytes.
#[inline]
fn fill_random_bytes(bytes: &mut [u8]) -> Result<()> {
    getrandom::fill(bytes).map_err(Error::other)
}

/// Encodes bytes as lowercase hexadecimal.
///
/// # Parameters
/// - `bytes`: Bytes to encode.
///
/// # Returns
/// Lowercase hexadecimal string.
#[must_use]
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}
