// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private URL-like file-name parsing.
// qubit-style: allow source-test-pair

use super::file_name_validation::validate_file_name_fragment;

/// Returns the final decoded file-name segment from a URL-like string.
///
/// # Parameters
/// - `url`: URL-like string to inspect.
///
/// # Returns
/// The decoded final URL path segment, or an empty string when no path segment
/// exists.
#[must_use]
pub(crate) fn file_name_from_url(url: &str) -> String {
    let path = lexical_url_path(url);
    let name = match path.rfind('/') {
        Some(index) => &path[index + 1..],
        None => path,
    };
    match percent_decode_utf8(name) {
        Some(decoded) if is_safe_decoded_url_file_name(&decoded) => decoded,
        _ => name.to_owned(),
    }
}

/// Tests whether a decoded URL segment is still a safe file-name fragment.
///
/// # Parameters
/// - `name`: Decoded URL path segment.
///
/// # Returns
/// `true` when the decoded segment cannot behave as a path after decoding.
#[must_use]
fn is_safe_decoded_url_file_name(name: &str) -> bool {
    if name == "." || name == ".." {
        return false;
    }
    validate_file_name_fragment("URL file name", name).is_ok()
}

/// Removes query and fragment suffixes from a URL-like string.
///
/// # Parameters
/// - `url`: URL-like string to inspect.
///
/// # Returns
/// The prefix before the first `?` or `#`, or the full input when neither is
/// present.
#[inline]
#[must_use]
fn strip_query_and_fragment(url: &str) -> &str {
    match (url.find('?'), url.find('#')) {
        (Some(query), Some(fragment)) => &url[..query.min(fragment)],
        (Some(index), None) | (None, Some(index)) => &url[..index],
        (None, None) => url,
    }
}

/// Returns the lexical path portion of a URL-like string.
///
/// A syntactically valid scheme is removed first. When the remaining value
/// begins with `//`, its authority is excluded and only the following path is
/// returned. No URL validation, normalization, or percent decoding occurs.
///
/// # Parameters
/// - `url`: URL-like string to inspect.
///
/// # Returns
/// The path-like portion before any query or fragment, or an empty string for
/// an authority-only URL.
#[inline]
#[must_use]
fn lexical_url_path(url: &str) -> &str {
    let value = strip_query_and_fragment(url);
    let value = strip_url_scheme(value);
    let Some(authority_and_path) = value.strip_prefix("//") else {
        return value;
    };
    authority_and_path
        .find('/')
        .map_or("", |index| &authority_and_path[index..])
}

/// Removes a syntactically valid URL scheme and its colon.
///
/// # Parameters
/// - `value`: Query- and fragment-free URL-like value.
///
/// # Returns
/// The substring after a valid leading scheme, or `value` unchanged when no
/// valid scheme is present.
#[inline]
#[must_use]
fn strip_url_scheme(value: &str) -> &str {
    let Some((scheme, remainder)) = value.split_once(':') else {
        return value;
    };
    if is_url_scheme(scheme) {
        remainder
    } else {
        value
    }
}

/// Tests whether a string matches the lexical URL scheme grammar.
///
/// # Parameters
/// - `value`: Candidate scheme without its trailing colon.
///
/// # Returns
/// `true` when the candidate starts with an ASCII letter and every remaining
/// byte is an ASCII letter, digit, plus sign, hyphen, or period.
#[inline]
#[must_use]
fn is_url_scheme(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// Decodes percent-encoded UTF-8.
///
/// # Parameters
/// - `value`: Percent-encoded string.
///
/// # Returns
/// The decoded string, or `None` when the input contains malformed percent
/// encoding or decoded bytes are not valid UTF-8.
fn percent_decode_utf8(value: &str) -> Option<String> {
    if !value.as_bytes().contains(&b'%') {
        return Some(value.to_owned());
    }
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

/// Converts an ASCII hexadecimal digit to its numeric value.
///
/// # Parameters
/// - `byte`: ASCII byte to convert.
///
/// # Returns
/// The hexadecimal value, or `None` when `byte` is not an ASCII hexadecimal
/// digit.
#[inline]
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
