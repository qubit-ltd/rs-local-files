// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private random file-name generation.

use std::io::Error;
use std::io::Result;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::file_name_validation::validate_file_name_fragment;
use super::file_name_validation::validate_portable_generated_component;

/// Random payload length used by generated file names.
const RANDOM_NAME_BYTES: usize = 16;

/// Builds a file-name component from caller fragments and process-local data.
///
/// The generated component combines the selected prefix and suffix with the
/// current Unix timestamp, process ID, and 128 bits of operating-system
/// randomness. Both fragments and the completed component are validated
/// before it is returned.
///
/// # Parameters
/// - `default_prefix`: Prefix used when `prefix` is `None`.
/// - `prefix`: Optional caller-provided prefix.
/// - `suffix`: Optional caller-provided suffix.
///
/// # Returns
/// A validated component containing the selected fragments and generated
/// uniqueness data.
///
/// # Errors
/// Returns [`std::io::ErrorKind::InvalidInput`] when either fragment can
/// behave like a path or the completed component violates the portable
/// filename policy. Returns [`std::io::ErrorKind::Other`] when operating-system
/// randomness is unavailable.
pub(crate) fn try_random_file_name(default_prefix: &str, prefix: Option<&str>, suffix: Option<&str>) -> Result<String> {
    let prefix = prefix.unwrap_or(default_prefix);
    let suffix = suffix.unwrap_or("");
    validate_file_name_fragment("prefix", prefix)?;
    validate_file_name_fragment("suffix", suffix)?;
    let timestamp = unix_timestamp_nanos();
    let process_id = std::process::id();
    let random = try_random_hex()?;
    let component = format!("{prefix}{timestamp:x}-{process_id:x}-{random}{suffix}");
    validate_portable_generated_component(&component)?;
    Ok(component)
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
#[inline]
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
    #[cfg(test)]
    let random_result = crate::tests::native_io_fault_tests::random_fill(bytes);
    #[cfg(not(test))]
    let random_result = getrandom::fill(bytes);
    random_result.map_err(Error::other)
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
