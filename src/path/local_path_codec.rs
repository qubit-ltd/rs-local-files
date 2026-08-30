// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Canonical escaped-byte conversion for native path components.

use std::ffi::OsStr;
use std::ffi::OsString;

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalPathCodecError;
use crate::LocalResult;

/// Reversible canonical conversion for one native path component.
pub struct LocalPathCodec;

impl LocalPathCodec {
    /// Converts one native path component to canonical escaped-byte text.
    ///
    /// # Parameters
    ///
    /// - `component`: Native component to encode.
    ///
    /// # Returns
    ///
    /// Canonical text that preserves all representable native bytes or code
    /// units.
    ///
    /// # Errors
    ///
    /// Returns a structured path error when the component contains native NUL
    /// or the current platform has no reversible native encoding.
    pub fn encode_component(component: &OsStr) -> LocalResult<String> {
        platform_codec::encode_native_text(component)
            .map(|encoded| encoded.into_owned())
            .map_err(path_codec_error)
    }

    /// Converts canonical escaped-byte text to one native path component.
    ///
    /// # Parameters
    ///
    /// - `component`: Canonical text to decode.
    ///
    /// # Returns
    ///
    /// The owned native component.
    ///
    /// # Errors
    ///
    /// Returns a structured path error when an escape is malformed, the text
    /// is non-canonical, contains native NUL, or cannot represent a native
    /// component on the current platform.
    pub fn decode_component(component: &str) -> LocalResult<OsString> {
        let native = platform_codec::decode_canonical_text(component).map_err(path_codec_error)?;
        let canonical = platform_codec::encode_native_text(&native).map_err(path_codec_error)?;
        if canonical != component {
            return Err(path_codec_error(LocalPathCodecError::NonCanonicalText));
        }
        Ok(native)
    }
}

/// Converts a codec error to the public structured path error.
///
/// # Parameters
///
/// - `error`: Native path codec failure.
///
/// # Returns
///
/// A compose-path error retaining the codec failure as its typed source.
#[must_use]
#[inline]
fn path_codec_error(error: LocalPathCodecError) -> LocalFileError {
    LocalFileError::from_path_codec(LocalFileOperation::ComposePath, None, error)
}

/// Platform-specific native path representation operations.
mod platform_codec {
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
        use std::borrow::Cow;
        use std::ffi::OsStr;
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::ffi::OsStringExt;

        use super::decode_escaped_bytes;
        use super::push_escaped_byte;
        use super::push_scalar;
        use crate::LocalPathCodecError;

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
        pub(crate) fn encode_native_text<'a>(native: &'a OsStr) -> Result<Cow<'a, str>, LocalPathCodecError> {
            let bytes = native.as_bytes();
            if bytes.contains(&0) {
                return Err(LocalPathCodecError::NativeNul);
            }
            if let Ok(valid) = std::str::from_utf8(bytes)
                && !valid.contains('%')
                && !valid.chars().any(char::is_control)
            {
                return Ok(Cow::Borrowed(valid));
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
        use std::borrow::Cow;
        use std::char::decode_utf16;
        use std::ffi::OsStr;
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::ffi::OsStringExt;

        use super::decode_escaped_bytes;
        use super::push_escaped_byte;
        use super::push_scalar;
        use crate::LocalPathCodecError;

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
        pub(crate) fn encode_native_text<'a>(native: &'a OsStr) -> Result<Cow<'a, str>, LocalPathCodecError> {
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
            if first != 0xED || !(0xA0..=0xBF).contains(&second) || !(0x80..=0xBF).contains(&third) {
                return None;
            }
            let surrogate = (u16::from(first & 0x0F) << 12) | (u16::from(second & 0x3F) << 6) | u16::from(third & 0x3F);
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
        use std::borrow::Cow;
        use std::ffi::OsStr;
        use std::ffi::OsString;

        use crate::LocalPathCodecError;

        /// Reports that this platform has no supported reversible codec.
        #[inline(always)]
        pub(crate) fn decode_canonical_text(_text: &str) -> Result<OsString, LocalPathCodecError> {
            Err(LocalPathCodecError::UnsupportedNativeEncoding)
        }

        /// Reports that this platform has no supported reversible codec.
        #[inline(always)]
        pub(crate) fn encode_native_text<'a>(_native: &'a OsStr) -> Result<Cow<'a, str>, LocalPathCodecError> {
            Err(LocalPathCodecError::UnsupportedNativeEncoding)
        }
    }

    pub(crate) use native::decode_canonical_text;
    pub(crate) use native::encode_native_text;
}
