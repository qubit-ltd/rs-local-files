// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Canonical escaped-byte conversion for native path components.

use std::{borrow::Cow, ffi::OsStr};

use crate::LocalPathCodecError;

/// Reversible canonical conversion for one native path component.
pub struct LocalPathCodec {
    /// Prevents construction of this stateless codec type.
    _private: (),
}

impl LocalPathCodec {
    /// Converts canonical escaped-byte text to one native path component.
    ///
    /// # Parameters
    ///
    /// - `text`: Canonical text to decode.
    ///
    /// # Returns
    ///
    /// The owned native component, or `LocalPathCodecError` when an escape is
    /// malformed, the text is non-canonical, or it cannot represent a native
    /// component on the current platform.
    pub fn encode<'a>(text: &'a str) -> Result<Cow<'a, OsStr>, LocalPathCodecError> {
        let native = platform::decode_canonical_text(text)?;
        let canonical = platform::encode_native_text(&native)
            .expect("decoded canonical text cannot contain a native NUL byte");
        if canonical != text {
            return Err(LocalPathCodecError::NonCanonicalText);
        }
        Ok(Cow::Owned(native))
    }

    /// Converts one native path component to canonical escaped-byte text.
    ///
    /// # Parameters
    ///
    /// - `native`: Native component to encode.
    ///
    /// # Returns
    ///
    /// Canonical text that preserves all representable native bytes or code
    /// units, or `LocalPathCodecError::NativeNul` when `native` contains NUL.
    #[inline(always)]
    pub fn decode<'a>(native: &'a OsStr) -> Result<Cow<'a, str>, LocalPathCodecError> {
        platform::encode_native_text(native)
    }

    /// Decodes one raw URI path component into canonical local path text.
    ///
    /// URI percent escapes encode bytes, while canonical local path text keeps
    /// Unicode scalars literal and escapes percent signs, controls, and invalid
    /// UTF-8 bytes. Callers remain responsible for rejecting decoded native
    /// separators, roots, and prefixes when assembling hierarchical paths.
    ///
    /// # Parameters
    ///
    /// - `component`: Raw URI path component without a slash separator.
    ///
    /// # Returns
    ///
    /// Canonical local path text representing the decoded URI bytes.
    ///
    /// # Errors
    ///
    /// Returns [`LocalPathCodecError::InvalidEscape`] when `component`
    /// contains a truncated or non-hexadecimal percent escape.
    pub fn decode_uri_component(component: &str) -> Result<String, LocalPathCodecError> {
        let bytes = decode_uri_bytes(component)?;
        if bytes.contains(&0) {
            return Err(LocalPathCodecError::NativeNul);
        }
        Ok(canonicalize_uri_bytes(&bytes))
    }
}

/// Strictly percent-decodes a URI component without treating `+` as a space.
fn decode_uri_bytes(component: &str) -> Result<Vec<u8>, LocalPathCodecError> {
    let bytes = component.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = bytes
            .get(index + 1)
            .copied()
            .ok_or(LocalPathCodecError::InvalidEscape { offset: index })?;
        let low = bytes
            .get(index + 2)
            .copied()
            .ok_or(LocalPathCodecError::InvalidEscape { offset: index })?;
        let high = hex_value(high).ok_or(LocalPathCodecError::InvalidEscape { offset: index })?;
        let low = hex_value(low).ok_or(LocalPathCodecError::InvalidEscape { offset: index })?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    Ok(decoded)
}

/// Converts one ASCII hexadecimal digit to its numeric value.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Canonicalizes decoded URI bytes without requiring a native path value.
fn canonicalize_uri_bytes(bytes: &[u8]) -> String {
    let mut canonical = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                push_uri_scalars(&mut canonical, valid);
                break;
            }
            Err(error) => {
                let valid_end = error.valid_up_to();
                let valid = std::str::from_utf8(&remaining[..valid_end])
                    .expect("valid UTF-8 prefix must decode");
                push_uri_scalars(&mut canonical, valid);
                let invalid_len = error.error_len().unwrap_or(1);
                for byte in &remaining[valid_end..valid_end + invalid_len] {
                    push_uri_escaped_byte(&mut canonical, *byte);
                }
                remaining = &remaining[valid_end + invalid_len..];
            }
        }
    }
    canonical
}

/// Appends UTF-8 scalars using local canonical escaped-byte text.
fn push_uri_scalars(canonical: &mut String, text: &str) {
    for scalar in text.chars() {
        if scalar == '%' || scalar.is_control() {
            for byte in scalar.to_string().bytes() {
                push_uri_escaped_byte(canonical, byte);
            }
        } else {
            canonical.push(scalar);
        }
    }
}

/// Appends one uppercase percent escape.
fn push_uri_escaped_byte(canonical: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    canonical.push('%');
    canonical.push(char::from(HEX[usize::from(byte >> 4)]));
    canonical.push(char::from(HEX[usize::from(byte & 0x0F)]));
}

/// Platform-specific native path representation operations.
mod platform {
    use crate::LocalPathCodecError;

    /// Decodes uppercase percent escapes and literal UTF-8 into raw bytes.
    fn decode_escaped_bytes(text: &str) -> Result<Vec<u8>, LocalPathCodecError> {
        let bytes = text.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            }
            let high = bytes.get(index + 1).copied();
            let low = bytes.get(index + 2).copied();
            let (Some(high), Some(low)) = (high, low) else {
                return Err(LocalPathCodecError::InvalidEscape { offset: index });
            };
            let (Some(high), Some(low)) = (uppercase_hex(high), uppercase_hex(low)) else {
                return Err(LocalPathCodecError::InvalidEscape { offset: index });
            };
            decoded.push((high << 4) | low);
            index += 3;
        }
        Ok(decoded)
    }

    /// Converts one ASCII hexadecimal digit to its nibble value.
    #[inline]
    fn uppercase_hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }

    /// Appends an uppercase percent escape for one native byte.
    #[inline]
    fn push_escaped_byte(text: &mut String, byte: u8) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        text.push('%');
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }

    /// Appends a scalar in canonical form, escaping percent and controls.
    fn push_scalar(text: &mut String, scalar: char) {
        if scalar == '%' || scalar.is_control() {
            for byte in scalar.to_string().bytes() {
                push_escaped_byte(text, byte);
            }
        } else {
            text.push(scalar);
        }
    }

    #[cfg(unix)]
    mod native {
        use std::{
            borrow::Cow,
            ffi::{OsStr, OsString},
            os::unix::ffi::{OsStrExt, OsStringExt},
        };

        use crate::LocalPathCodecError;

        use super::{decode_escaped_bytes, push_escaped_byte, push_scalar};

        /// Decodes canonical bytes to a Unix native component.
        pub(crate) fn decode_canonical_text(text: &str) -> Result<OsString, LocalPathCodecError> {
            let bytes = decode_escaped_bytes(text)?;
            if bytes.contains(&0) {
                return Err(LocalPathCodecError::NativeNul);
            }
            Ok(OsString::from_vec(bytes))
        }

        /// Encodes a Unix native component while preserving invalid UTF-8
        /// bytes.
        pub(crate) fn encode_native_text<'a>(
            native: &'a OsStr,
        ) -> Result<Cow<'a, str>, LocalPathCodecError> {
            let bytes = native.as_bytes();
            if bytes.contains(&0) {
                return Err(LocalPathCodecError::NativeNul);
            }
            let mut encoded = String::with_capacity(bytes.len());
            let mut remaining = bytes;
            while !remaining.is_empty() {
                match std::str::from_utf8(remaining) {
                    Ok(valid) => {
                        for scalar in valid.chars() {
                            push_scalar(&mut encoded, scalar);
                        }
                        break;
                    }
                    Err(error) => {
                        let valid_end = error.valid_up_to();
                        let valid = std::str::from_utf8(&remaining[..valid_end])
                            .expect("valid_up_to always identifies a valid UTF-8 prefix");
                        for scalar in valid.chars() {
                            push_scalar(&mut encoded, scalar);
                        }
                        let invalid_len = error.error_len().unwrap_or(1);
                        for byte in &remaining[valid_end..valid_end + invalid_len] {
                            push_escaped_byte(&mut encoded, *byte);
                        }
                        remaining = &remaining[valid_end + invalid_len..];
                    }
                }
            }
            Ok(Cow::Owned(encoded))
        }
    }

    #[cfg(windows)]
    mod native {
        use std::{
            borrow::Cow,
            char::decode_utf16,
            ffi::{OsStr, OsString},
            os::windows::ffi::{OsStrExt, OsStringExt},
        };

        use crate::LocalPathCodecError;

        use super::{decode_escaped_bytes, push_escaped_byte, push_scalar};

        /// Decodes canonical WTF-8 bytes to a Windows native component.
        pub(crate) fn decode_canonical_text(text: &str) -> Result<OsString, LocalPathCodecError> {
            let bytes = decode_escaped_bytes(text)?;
            let units = decode_wtf8(&bytes)?;
            if units.contains(&0) {
                return Err(LocalPathCodecError::NativeNul);
            }
            Ok(OsString::from_wide(&units))
        }

        /// Encodes a Windows native component while preserving unpaired
        /// surrogate code units as escaped WTF-8 bytes.
        pub(crate) fn encode_native_text<'a>(
            native: &'a OsStr,
        ) -> Result<Cow<'a, str>, LocalPathCodecError> {
            let units = native.encode_wide().collect::<Vec<_>>();
            if units.contains(&0) {
                return Err(LocalPathCodecError::NativeNul);
            }
            let mut encoded = String::new();
            for result in decode_utf16(units) {
                match result {
                    Ok(scalar) => push_scalar(&mut encoded, scalar),
                    Err(error) => {
                        for byte in wtf8_surrogate_bytes(error.unpaired_surrogate()) {
                            push_escaped_byte(&mut encoded, byte);
                        }
                    }
                }
            }
            Ok(Cow::Owned(encoded))
        }

        /// Decodes UTF-8 plus WTF-8 surrogate sequences into UTF-16 units.
        fn decode_wtf8(bytes: &[u8]) -> Result<Vec<u16>, LocalPathCodecError> {
            let mut units = Vec::with_capacity(bytes.len());
            let mut index = 0;
            while index < bytes.len() {
                if let Some((surrogate, width)) = wtf8_surrogate_at(bytes, index) {
                    units.push(surrogate);
                    index += width;
                    continue;
                }
                match std::str::from_utf8(&bytes[index..]) {
                    Ok(valid) => {
                        units.extend(valid.encode_utf16());
                        break;
                    }
                    Err(error) => {
                        let valid_end = error.valid_up_to();
                        if valid_end == 0 {
                            return Err(LocalPathCodecError::UnrepresentableNativeValue);
                        }
                        let valid = std::str::from_utf8(&bytes[index..index + valid_end])
                            .map_err(|_| LocalPathCodecError::UnrepresentableNativeValue)?;
                        units.extend(valid.encode_utf16());
                        index += valid_end;
                    }
                }
            }
            Ok(units)
        }

        /// Returns a surrogate represented by a WTF-8 sequence at `index`.
        fn wtf8_surrogate_at(bytes: &[u8], index: usize) -> Option<(u16, usize)> {
            let first = *bytes.get(index)?;
            let second = *bytes.get(index + 1)?;
            let third = *bytes.get(index + 2)?;
            if first != 0xED || !(0xA0..=0xBF).contains(&second) || !(0x80..=0xBF).contains(&third)
            {
                return None;
            }
            let surrogate = (u16::from(first & 0x0F) << 12)
                | (u16::from(second & 0x3F) << 6)
                | u16::from(third & 0x3F);
            Some((surrogate, 3))
        }

        /// Encodes one unpaired UTF-16 surrogate as its three WTF-8 bytes.
        #[must_use]
        #[inline]
        fn wtf8_surrogate_bytes(surrogate: u16) -> [u8; 3] {
            [
                0xE0 | ((surrogate >> 12) as u8),
                0x80 | (((surrogate >> 6) & 0x3F) as u8),
                0x80 | ((surrogate & 0x3F) as u8),
            ]
        }
    }

    #[cfg(not(any(unix, windows)))]
    mod native {
        use std::{
            borrow::Cow,
            ffi::{OsStr, OsString},
        };

        use crate::LocalPathCodecError;

        /// Reports that this platform has no supported reversible codec.
        #[inline(always)]
        pub(crate) fn decode_canonical_text(_text: &str) -> Result<OsString, LocalPathCodecError> {
            Err(LocalPathCodecError::UnsupportedNativeEncoding)
        }

        /// Reports that this platform has no supported reversible codec.
        #[inline(always)]
        pub(crate) fn encode_native_text<'a>(
            _native: &'a OsStr,
        ) -> Result<Cow<'a, str>, LocalPathCodecError> {
            Err(LocalPathCodecError::UnsupportedNativeEncoding)
        }
    }

    pub(crate) use native::{decode_canonical_text, encode_native_text};
}
