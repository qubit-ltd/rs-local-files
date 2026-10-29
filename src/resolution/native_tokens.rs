// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;
use std::path::Path;

use super::session::ResolutionSession;
use crate::LocalResult;
use crate::directory::LocalResolutionOrigin;
use crate::error::LocalFileError;
use crate::error::LocalFileOperation;
use crate::error::LocalResourceKind;

/// A lexical resolution step retaining dot and parent components.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeToken {
    Normal(OsString, LocalResolutionOrigin),
    Current(LocalResolutionOrigin),
    Parent(LocalResolutionOrigin),
}

/// Measures in constant space, checks the session budget, then builds native
/// tokens.
pub(crate) fn tokenize(
    path: &Path,
    origin: LocalResolutionOrigin,
    session: &ResolutionSession,
) -> LocalResult<Vec<NativeToken>> {
    let (components, bytes) =
        measure_path(path).ok_or_else(|| super::session::overflow_error(LocalResourceKind::PendingResolutionBytes))?;
    session.check_pending(components, bytes)?;
    tokens_after_check(path, origin, components)
}

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
    Ok(tokens)
}

/// Computes the complete pending component and byte snapshot without
/// allocating.
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

#[cfg(windows)]
fn windows_prefix_units(path: &Path) -> Option<usize> {
    use std::os::windows::ffi::OsStrExt;
    prefix_units_from_wide(path.as_os_str().encode_wide())
}

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

#[cfg(any(windows, test))]
const fn is_separator(unit: u16) -> bool {
    unit == b'/' as u16 || unit == b'\\' as u16
}

#[cfg(any(windows, test))]
struct UncSegment {
    is_extended_marker: bool,
    is_unc_marker: bool,
}

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

fn checked_sum(current: usize, added: usize) -> Option<usize> {
    current.checked_add(added)
}

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

#[cfg(any(windows, test))]
fn ascii_upper(unit: u16) -> u16 {
    if (b'a' as u16..=b'z' as u16).contains(&unit) {
        unit - (b'a' as u16 - b'A' as u16)
    } else {
        unit
    }
}

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
    use std::path::Path;

    use super::NativeToken;
    use super::measure_path;
    use super::prefix_units_from_wide;
    use super::tokenize;
    use crate::directory::LocalResolutionOrigin;
    use crate::directory::LocalResolutionScope;
    use crate::options::LocalResolutionLimits;
    use crate::resolution::ResolutionContract;
    use crate::resolution::session::ResolutionSession;

    fn session(max_components: usize, max_bytes: usize) -> ResolutionSession {
        let contract = ResolutionContract::unrestricted().tightened(
            LocalResolutionLimits::new()
                .with_max_actual_components(max_components)
                .with_max_resolved_path_bytes(max_bytes),
        );
        ResolutionSession::new(&contract, LocalResolutionScope::Descendant)
    }

    #[test]
    fn resolution_session_tokenizer_preserves_dot_parent_and_origin() {
        let tokens = tokenize(
            Path::new("a/./../b"),
            LocalResolutionOrigin::SymlinkTarget,
            &session(4, 9),
        )
        .unwrap();
        assert_eq!(
            tokens,
            vec![
                NativeToken::Normal("a".into(), LocalResolutionOrigin::SymlinkTarget),
                NativeToken::Current(LocalResolutionOrigin::SymlinkTarget),
                NativeToken::Parent(LocalResolutionOrigin::SymlinkTarget),
                NativeToken::Normal("b".into(), LocalResolutionOrigin::SymlinkTarget),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolution_session_tokenizer_preserves_non_utf8_names() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::ffi::OsStringExt;
        let value = std::ffi::OsString::from_vec(vec![b'x', b'/', 0xff]);
        let tokens = tokenize(Path::new(&value), LocalResolutionOrigin::Input, &session(2, 4)).unwrap();
        assert!(matches!(&tokens[1], NativeToken::Normal(name, _) if name.as_bytes() == [0xff]));
    }

    #[cfg(unix)]
    #[test]
    fn resolution_session_tokenizer_checks_exact_limit_before_building_tokens() {
        assert_eq!(measure_path(Path::new("a/b")), Some((2, 4)));
        assert_eq!(
            tokenize(Path::new("a/b"), LocalResolutionOrigin::Input, &session(2, 4))
                .unwrap()
                .len(),
            2
        );
        let error = tokenize(Path::new("a/b"), LocalResolutionOrigin::Input, &session(2, 3)).unwrap_err();
        assert_eq!(
            error.resource_limit_error().unwrap().resource(),
            crate::error::LocalResourceKind::PendingResolutionBytes
        );
        let error = tokenize(Path::new("a/b"), LocalResolutionOrigin::Input, &session(1, 4)).unwrap_err();
        assert_eq!(
            error.resource_limit_error().unwrap().resource(),
            crate::error::LocalResourceKind::PendingResolutionComponents
        );
    }

    #[test]
    fn resolution_session_native_measurement_reports_overflow() {
        assert_eq!(super::checked_sum(usize::MAX, 1), None);
    }

    #[test]
    fn resolution_session_windows_unc_prefixes_exclude_server_and_share() {
        let standard = r"\\server\share\foo";
        let standard_anchor = r"\\server\share\";
        assert_eq!(
            prefix_units_from_wide(standard.encode_utf16()),
            Some(standard_anchor.encode_utf16().count())
        );
        assert_eq!(
            super::measure_windows_units(standard.encode_utf16().collect::<Vec<_>>().into_iter()),
            Some((1, 8))
        );

        let extended = r"\\?\UNC\server\share\foo";
        let extended_anchor = r"\\?\UNC\server\share\";
        assert_eq!(
            prefix_units_from_wide(extended.encode_utf16()),
            Some(extended_anchor.encode_utf16().count())
        );
        assert_eq!(
            super::measure_windows_units(extended.encode_utf16().collect::<Vec<_>>().into_iter()),
            Some((1, 8))
        );
    }

    #[cfg(windows)]
    #[test]
    fn resolution_session_windows_unc_tokenization_only_emits_descendants() {
        for value in [r"\\server\share\foo", r"\\?\UNC\server\share\foo"] {
            let path = Path::new(value);
            assert_eq!(measure_path(path), Some((1, 8)));
            assert_eq!(
                tokenize(path, LocalResolutionOrigin::Input, &session(1, 8)).unwrap(),
                vec![NativeToken::Normal("foo".into(), LocalResolutionOrigin::Input)]
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn resolution_session_windows_non_bmp_pending_bytes_use_utf16_units() {
        assert_eq!(measure_path(Path::new("a/😀")), Some((2, 10)));
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

        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.io_error_kind(), ErrorKind::OutOfMemory);
        assert!(
            error
                .io_error()
                .expect("allocation failure must retain its I/O source")
                .get_ref()
                .is_some()
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
