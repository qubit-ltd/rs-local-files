// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod native_token;

use std::ffi::OsString;
use std::path::Path;

pub(crate) use native_token::NativeToken;

#[cfg(any(windows, test))]
use super::internal::unc_segment::UncSegment;
use super::session::ResolutionSession;
use crate::LocalResult;
use crate::directory::LocalResolutionOrigin;
use crate::error::LocalFileError;
use crate::error::LocalFileOperation;
use crate::error::LocalResourceKind;

/// Measures in constant space, checks the session budget, then builds native
/// tokens.
///
/// # Parameters
///
/// * `path` is the native path to tokenize; its lexical components are
///   preserved, including explicit dot and parent components.
/// * `origin` is attached to every emitted component token.
/// * `session` enforces pending component and native-byte limits before token
///   allocation.
///
/// # Returns
///
/// Returns the ordered tokens, including a directory requirement when the
/// input ends in a separator.
///
/// # Errors
///
/// Returns a resource-limit or allocation error if measuring, budget checking,
/// capacity calculation, or token allocation fails.
pub(crate) fn tokenize(
    path: &Path,
    origin: LocalResolutionOrigin,
    session: &ResolutionSession,
) -> LocalResult<Vec<NativeToken>> {
    let (components, bytes) =
        measure_path(path).ok_or_else(|| super::session::overflow_error(LocalResourceKind::PendingResolutionBytes))?;
    session.check_pending(components, bytes)?;
    let trailing_directory_marker = has_trailing_separator(path);
    let capacity = token_capacity(components, trailing_directory_marker)
        .ok_or_else(|| super::session::overflow_error(LocalResourceKind::PendingResolutionComponents))?;
    tokens_after_check(path, origin, capacity)
}

/// Computes the reservation capacity including an optional directory marker.
///
/// # Parameters
///
/// * `components` is the measured number of lexical components.
/// * `trailing_directory_marker` indicates whether one marker token is needed.
///
/// # Returns
///
/// Returns the required capacity, or `None` if adding the marker overflows.
#[inline]
fn token_capacity(components: usize, trailing_directory_marker: bool) -> Option<usize> {
    components.checked_add(usize::from(trailing_directory_marker))
}

/// Allocates and constructs tokens after the caller has charged the session.
///
/// The caller-provided capacity is reserved before walking the path so
/// allocation failure is reported through the filesystem error type.
///
/// # Parameters
///
/// * `path` is the path whose lexical components are converted.
/// * `origin` is retained on each component token.
/// * `capacity` is the checked reservation size computed by `token_capacity`.
///
/// # Returns
///
/// Returns the allocated token sequence, preserving a trailing directory
/// marker when present.
///
/// # Errors
///
/// Returns an I/O-shaped allocation error if reserving the token vector or a
/// native component buffer fails.
fn tokens_after_check(path: &Path, origin: LocalResolutionOrigin, capacity: usize) -> LocalResult<Vec<NativeToken>> {
    let mut tokens = Vec::new();
    tokens.try_reserve_exact(capacity).map_err(allocation_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::ffi::OsStringExt;
        for part in path
            .as_os_str()
            .as_bytes()
            .split(|byte| *byte == b'/')
            .filter(|part| !part.is_empty())
        {
            let token = match part {
                b"." => NativeToken::Current(origin),
                b".." => NativeToken::Parent(origin),
                _ => {
                    let mut component = Vec::new();
                    component.try_reserve_exact(part.len()).map_err(allocation_error)?;
                    component.extend_from_slice(part);
                    NativeToken::Normal(OsString::from_vec(component), origin)
                }
            };
            tokens.push(token);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::ffi::OsStringExt;
        let mut units = path
            .as_os_str()
            .encode_wide()
            .skip(
                windows_prefix_units(path)
                    .ok_or_else(|| super::session::overflow_error(LocalResourceKind::PendingResolutionBytes))?,
            )
            .peekable();
        while units.peek().is_some() {
            while matches!(units.peek(), Some(unit) if is_separator(*unit)) {
                units.next();
            }
            let mut component = Vec::<u16>::new();
            while matches!(units.peek(), Some(unit) if !is_separator(*unit)) {
                component.try_reserve(1).map_err(allocation_error)?;
                component.push(units.next().expect("peeked code unit"));
            }
            if component.is_empty() {
                continue;
            }
            let token = match component.as_slice() {
                [dot] if *dot == b'.' as u16 => NativeToken::Current(origin),
                [dot, dot2] if *dot == b'.' as u16 && *dot2 == b'.' as u16 => NativeToken::Parent(origin),
                _ => NativeToken::Normal(OsString::from_wide(&component), origin),
            };
            tokens.push(token);
            while matches!(units.peek(), Some(unit) if is_separator(*unit)) {
                units.next();
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        for component in path.components() {
            use std::path::Component;
            tokens.push(match component {
                Component::CurDir => NativeToken::Current(origin),
                Component::ParentDir => NativeToken::Parent(origin),
                Component::Normal(value) => NativeToken::Normal(value.to_os_string(), origin),
                Component::RootDir | Component::Prefix(_) => continue,
            });
        }
    }
    if has_trailing_separator(path) {
        tokens.push(NativeToken::RequireDirectory);
    }
    Ok(tokens)
}

/// Reports whether the native path ends in a platform separator.
///
/// The check uses native path units on Unix and Windows, preserving non-UTF-8
/// and non-BMP names without lossy conversion.
///
/// # Parameters
///
/// * `path` is the native path to inspect.
///
/// # Returns
///
/// Returns `true` only when the final native unit is a platform separator.
#[must_use]
#[inline]
fn has_trailing_separator(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().last() == Some(&b'/')
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().last().is_some_and(is_separator)
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.to_string_lossy().ends_with(std::path::MAIN_SEPARATOR)
    }
}

/// Computes the complete pending component and byte snapshot without
/// allocating.
///
/// # Parameters
///
/// * `path` is measured using native encoding units; separators do not count as
///   components.
///
/// # Returns
///
/// Returns component and byte counts, or `None` if the path cannot be
/// represented by the platform measurement or a count overflows.
pub(crate) fn measure_path(path: &Path) -> Option<(usize, usize)> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut components = 0usize;
        let mut bytes = 0usize;
        for part in path
            .as_os_str()
            .as_bytes()
            .split(|byte| *byte == b'/')
            .filter(|part| !part.is_empty())
        {
            components = checked_sum(components, 1)?;
            bytes = checked_sum(bytes, part.len())?;
            bytes = checked_sum(bytes, 1)?;
        }
        Some((components, bytes))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        measure_windows_units(path.as_os_str().encode_wide())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let mut components = 0usize;
        let mut bytes = 0usize;
        for component in path.components() {
            use std::path::Component;
            let value = match component {
                Component::CurDir => Some("."),
                Component::ParentDir => Some(".."),
                Component::Normal(value) => Some(value.to_str()?),
                Component::RootDir | Component::Prefix(_) => None,
            };
            if let Some(value) = value {
                components = checked_sum(components, 1)?;
                bytes = checked_sum(bytes, value.len())?;
                bytes = checked_sum(bytes, 1)?;
            }
        }
        Some((components, bytes))
    }
}

/// Counts the Windows prefix units omitted from descendant tokenization.
///
/// Server and share names form the UNC anchor; extended UNC markers and the
/// anchor are also excluded from the pending descendant budget.
///
/// # Parameters
///
/// * `path` is the native Windows path being tokenized.
///
/// # Returns
///
/// Returns the number of encoded units to skip, or `None` if prefix measurement
/// overflows or the UNC anchor is incomplete.
#[cfg(windows)]
fn windows_prefix_units(path: &Path) -> Option<usize> {
    use std::os::windows::ffi::OsStrExt;
    prefix_units_from_wide(path.as_os_str().encode_wide())
}

/// Counts drive, root, and UNC anchor units from an encoded Windows path.
///
/// Returns `None` if counting the prefix overflows or a required UNC segment
/// is absent.
///
/// # Type Parameters
///
/// * `I` is an iterator over encoded UTF-16 code units.
///
/// # Parameters
///
/// * `units` supplies the encoded path in native order.
///
/// # Returns
///
/// Returns the number of prefix units to skip, or `None` for overflow or an
/// incomplete UNC anchor.
#[cfg(any(windows, test))]
fn prefix_units_from_wide<I>(mut units: I) -> Option<usize>
where
    I: Iterator<Item = u16>,
{
    let first = match units.next() {
        Some(unit) => unit,
        None => return Some(0),
    };
    let second = units.next();
    if second == Some(b':' as u16) {
        return Some(2);
    }
    let mut all_units = std::iter::once(first).chain(second).chain(units).peekable();
    if is_separator(first) && second.is_some_and(is_separator) {
        let mut skipped = 0usize;
        while matches!(all_units.peek(), Some(unit) if is_separator(*unit)) {
            all_units.next();
            skipped = checked_sum(skipped, 1)?;
        }
        let first_segment = consume_unc_segment(&mut all_units, &mut skipped)?;
        let second_segment = consume_unc_segment(&mut all_units, &mut skipped)?;
        if first_segment.is_extended_marker && second_segment.is_unc_marker {
            consume_unc_segment(&mut all_units, &mut skipped)?;
            consume_unc_segment(&mut all_units, &mut skipped)?;
        }
        return Some(skipped);
    }
    if is_separator(first) {
        let mut skipped = 0usize;
        while matches!(all_units.peek(), Some(unit) if is_separator(*unit)) {
            all_units.next();
            skipped = checked_sum(skipped, 1)?;
        }
        return Some(skipped);
    }
    Some(0)
}

/// Identifies slash and backslash as Windows path separators.
///
/// # Parameters
///
/// * `unit` is one UTF-16 code unit from the path.
///
/// # Returns
///
/// Returns `true` for either Windows path separator.
#[must_use]
#[inline]
#[cfg(any(windows, test))]
const fn is_separator(unit: u16) -> bool {
    unit == b'/' as u16 || unit == b'\\' as u16
}

/// Consumes one UNC segment and following separators while updating the prefix
/// length. The returned flags identify the extended-path markers.
///
/// # Type Parameters
///
/// * `I` is an iterator over UTF-16 code units.
///
/// # Parameters
///
/// * `units` supplies the unconsumed path suffix.
/// * `skipped` accumulates the number of consumed prefix units.
///
/// # Returns
///
/// Returns marker flags for the consumed segment, or `None` if the prefix
/// length overflows.
#[cfg(any(windows, test))]
fn consume_unc_segment<I>(units: &mut std::iter::Peekable<I>, skipped: &mut usize) -> Option<UncSegment>
where
    I: Iterator<Item = u16>,
{
    let mut length = 0usize;
    let mut is_question = true;
    let expected_unc = [b'U' as u16, b'N' as u16, b'C' as u16];
    let mut is_unc = true;
    while matches!(units.peek(), Some(unit) if !is_separator(*unit)) {
        let unit = units.next()?;
        is_question &= length == 0 && unit == b'?' as u16;
        is_unc &= expected_unc
            .get(length)
            .is_some_and(|expected| ascii_upper(unit) == *expected);
        length = checked_sum(length, 1)?;
        *skipped = checked_sum(*skipped, 1)?;
    }
    while matches!(units.peek(), Some(unit) if is_separator(*unit)) {
        units.next();
        *skipped = checked_sum(*skipped, 1)?;
    }
    Some(UncSegment {
        is_extended_marker: is_question && length == 1,
        is_unc_marker: is_unc && length == expected_unc.len(),
    })
}

/// Adds a path measurement quantity, returning `None` on integer overflow.
///
/// # Parameters
///
/// * `current` is the accumulated count.
/// * `added` is the next quantity to include.
///
/// # Returns
///
/// Returns the sum when it fits in `usize`.
#[inline]
fn checked_sum(current: usize, added: usize) -> Option<usize> {
    current.checked_add(added)
}

/// Measures Windows descendant path units using UTF-16 code units.
///
/// Each component charges its encoded bytes plus a separator budget. Prefix
/// anchors are excluded before measurement.
///
/// # Type Parameters
///
/// * `I` is a cloneable iterator over UTF-16 code units.
///
/// # Parameters
///
/// * `units` is the encoded native path.
///
/// # Returns
///
/// Returns descendant component and byte counts, or `None` on arithmetic
/// overflow or an invalid prefix measurement.
#[cfg(any(windows, test))]
fn measure_windows_units<I>(units: I) -> Option<(usize, usize)>
where
    I: Iterator<Item = u16> + Clone,
{
    let skip = prefix_units_from_wide(units.clone())?;
    let mut components = 0usize;
    let mut bytes = 0usize;
    let mut current_units = 0usize;
    for unit in units.skip(skip) {
        if is_separator(unit) {
            if current_units != 0 {
                components = checked_sum(components, 1)?;
                bytes = checked_sum(bytes, current_units.checked_mul(2)?)?;
                bytes = checked_sum(bytes, 2)?;
                current_units = 0;
            }
        } else {
            current_units = checked_sum(current_units, 1)?;
        }
    }
    if current_units != 0 {
        components = checked_sum(components, 1)?;
        bytes = checked_sum(bytes, current_units.checked_mul(2)?)?;
        bytes = checked_sum(bytes, 2)?;
    }
    Some((components, bytes))
}

/// Converts an ASCII lowercase UTF-16 unit to uppercase for UNC marker checks.
///
/// # Parameters
///
/// * `unit` is the encoded UTF-16 code unit to normalize.
///
/// # Returns
///
/// Returns its uppercase ASCII equivalent, or the unchanged unit otherwise.
#[must_use]
#[inline]
#[cfg(any(windows, test))]
fn ascii_upper(unit: u16) -> u16 {
    if (b'a' as u16..=b'z' as u16).contains(&unit) {
        unit - (b'a' as u16 - b'A' as u16)
    } else {
        unit
    }
}

/// Converts a reservation failure into the operation's structured I/O error.
///
/// # Parameters
///
/// * `error` is the allocation failure returned by a fallible reservation.
///
/// # Returns
///
/// Returns a metadata-operation error retaining the allocation failure as its
/// I/O source.
fn allocation_error(error: std::collections::TryReserveError) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::Metadata,
        None,
        None,
        std::io::Error::new(std::io::ErrorKind::OutOfMemory, error),
    )
}

#[cfg(test)]
mod tests {
    use super::prefix_units_from_wide;

    #[test]
    fn test_native_measurement_reports_overflow() {
        assert_eq!(
            super::checked_sum(usize::MAX, 1),
            None,
            "path measurement must reject overflowing additions"
        );
        assert_eq!(
            super::token_capacity(usize::MAX, true),
            None,
            "token capacity must reject a trailing marker overflow"
        );
    }

    #[test]
    fn test_windows_unc_prefix_measurement_excludes_server_and_share() {
        let standard = r"\\server\share\foo";
        let standard_anchor = r"\\server\share\";
        assert_eq!(
            prefix_units_from_wide(standard.encode_utf16()),
            Some(standard_anchor.encode_utf16().count()),
            "the standard UNC anchor must include its server and share"
        );
        assert_eq!(
            super::measure_windows_units(standard.encode_utf16().collect::<Vec<_>>().into_iter()),
            Some((1, 8)),
            "the standard UNC budget must include only the descendant"
        );

        let extended = r"\\?\UNC\server\share\foo";
        let extended_anchor = r"\\?\UNC\server\share\";
        assert_eq!(
            prefix_units_from_wide(extended.encode_utf16()),
            Some(extended_anchor.encode_utf16().count()),
            "the extended UNC anchor must include its server and share"
        );
        assert_eq!(
            super::measure_windows_units(extended.encode_utf16().collect::<Vec<_>>().into_iter()),
            Some((1, 8)),
            "the extended UNC budget must include only the descendant"
        );
    }
}

#[cfg(test)]
mod allocation_contract_tests {
    use std::io::ErrorKind;
    use std::path::Path;

    use crate::directory::LocalResolutionOrigin;
    use crate::error::LocalFileOperation;

    #[test]
    fn test_unrepresentable_token_capacity_reports_allocation_failure() {
        let error = super::tokens_after_check(Path::new("child"), LocalResolutionOrigin::Input, usize::MAX)
            .expect_err("an unrepresentable token allocation must return an error");

        assert_eq!(
            error.operation(),
            LocalFileOperation::Metadata,
            "allocation failure must retain the metadata operation"
        );
        assert_eq!(
            error.io_error_kind(),
            ErrorKind::OutOfMemory,
            "allocation failure must retain its out-of-memory classification"
        );
        assert!(
            error
                .io_error()
                .expect("allocation failure must retain its I/O source")
                .get_ref()
                .is_some(),
            "allocation failure must retain the original reservation error"
        );
    }
}

#[cfg(test)]
mod windows_measurement_boundary_tests {
    #[test]
    fn test_windows_native_measurement_handles_roots_drives_empty_and_trailing_separators() {
        let cases = [
            ("", (0, 0)),
            (r"C:\", (0, 0)),
            (r"\a\b", (2, 8)),
            (r"a\b\", (2, 8)),
            (r"C:\a\..\😀", (3, 16)),
            (r"\\?\uNc\server\share\a", (1, 4)),
        ];
        for (path, expected) in cases {
            let units = path.encode_utf16().collect::<Vec<_>>();
            assert_eq!(
                super::measure_windows_units(units.into_iter()),
                Some(expected),
                "native path {path:?}"
            );
        }
    }
}
