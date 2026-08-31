// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scope-compatible native and portable filename policies.

use std::ffi::OsStr;
use std::ffi::OsString;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;
use crate::LocalResult;
use crate::path::internal::LocalFileNamePolicy;

/// Number of random bytes encoded into a generated filename.
const RANDOM_NAME_BYTES: usize = 16;

/// Configured validation and generation policy for filename components.
///
/// # Examples
///
/// ```
/// use std::ffi::OsStr;
///
/// use qubit_local_files::LocalFileNames;
///
/// let names = LocalFileNames::portable().with_max_component_bytes(32)?;
/// names.validate(OsStr::new("report.csv"))?;
/// # Ok::<(), qubit_local_files::LocalFileError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileNames {
    /// Rules applied to components.
    policy: LocalFileNamePolicy,
    /// Maximum native or portable encoded component size.
    max_component_bytes: Option<usize>,
}

impl LocalFileNames {
    /// Creates the conservative cross-platform filename policy.
    #[inline(always)]
    pub const fn portable() -> Self {
        Self {
            policy: LocalFileNamePolicy::Portable,
            max_component_bytes: None,
        }
    }

    /// Creates the lossless current-platform filename policy.
    #[inline(always)]
    pub(crate) const fn native() -> Self {
        Self {
            policy: LocalFileNamePolicy::Native,
            max_component_bytes: None,
        }
    }

    /// Returns the caller-configured component-size budget.
    #[must_use]
    pub const fn max_component_bytes(&self) -> Option<usize> {
        self.max_component_bytes
    }

    /// Reconfigures the maximum encoded size of one component.
    ///
    /// # Parameters
    ///
    /// - `maximum`: Positive maximum number of encoded bytes.
    ///
    /// # Returns
    ///
    /// A copy of this policy with the requested maximum.
    ///
    /// # Errors
    ///
    /// Returns a structured resource-limit error when `maximum` is zero.
    pub fn with_max_component_bytes(mut self, maximum: usize) -> LocalResult<Self> {
        if maximum == 0 {
            return Err(component_limit_error(1, maximum));
        }
        self.max_component_bytes = Some(maximum);
        Ok(self)
    }

    /// Removes the caller-configured component-size budget.
    pub const fn without_max_component_bytes(mut self) -> Self {
        self.max_component_bytes = None;
        self
    }

    /// Validates one filename component according to this policy.
    ///
    /// # Parameters
    ///
    /// - `name`: Native filename component to validate.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when `name` violates the selected policy,
    /// or a resource-limit error when its encoded size exceeds the configured
    /// maximum.
    pub fn validate(&self, name: &OsStr) -> LocalResult<()> {
        let encoded_bytes = match self.policy {
            LocalFileNamePolicy::Portable => {
                let text = name.to_str().ok_or_else(invalid_name_error)?;
                validate_portable_component(text)?;
                text.len()
            }
            LocalFileNamePolicy::Native => {
                validate_native_component(name)?;
                native_component_bytes(name)?
            }
        };
        if let Some(maximum) = self.max_component_bytes
            && encoded_bytes > maximum
        {
            return Err(component_limit_error(encoded_bytes, maximum));
        }
        Ok(())
    }

    /// Generates a cryptographically random filename component.
    ///
    /// # Errors
    ///
    /// Returns a structured generation error when operating-system randomness
    /// is unavailable or the generated name violates this policy.
    #[inline(always)]
    pub fn random_name(&self) -> LocalResult<OsString> {
        self.random_name_with(None, None)
    }

    /// Generates a cryptographically random filename with native affixes.
    ///
    /// # Parameters
    ///
    /// - `prefix`: Optional native prefix.
    /// - `suffix`: Optional native suffix.
    ///
    /// # Errors
    ///
    /// Returns a structured generation error when operating-system randomness
    /// is unavailable, or a validation error when the resulting component
    /// violates this policy.
    pub fn random_name_with(&self, prefix: Option<&OsStr>, suffix: Option<&OsStr>) -> LocalResult<OsString> {
        let mut random = [0_u8; RANDOM_NAME_BYTES];
        getrandom::fill(&mut random).map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::GenerateName,
                None,
                None,
                std::io::Error::other(source),
            )
        })?;
        let random = encode_hex(&random);
        let mut name = prefix.map_or_else(|| OsString::from("qubit-local-files-"), OsStr::to_os_string);
        name.push(random);
        if let Some(suffix) = suffix {
            name.push(suffix);
        }
        self.validate(&name)?;
        Ok(name)
    }
}

/// Encodes random bytes as lowercase hexadecimal text.
///
/// # Parameters
///
/// - `bytes`: Random bytes to encode.
///
/// # Returns
///
/// Lowercase hexadecimal text containing two characters per input byte.
#[must_use]
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
    encoded
}

/// Validates a conservative portable UTF-8 filename component.
///
/// # Parameters
///
/// - `name`: Portable UTF-8 component.
///
/// # Errors
///
/// Returns an invalid-path error for empty names, dot segments, trailing
/// spaces or dots, forbidden characters, or Windows reserved device names.
fn validate_portable_component(name: &str) -> LocalResult<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || name.chars().any(|character| {
            character.is_control() || matches!(character, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
        || is_windows_reserved_file_name(name)
    {
        return Err(invalid_name_error());
    }
    Ok(())
}

/// Validates one filename component in the current native representation.
///
/// # Parameters
///
/// - `name`: Native component to validate.
///
/// # Errors
///
/// Returns an invalid-path error when `name` is empty, is a dot segment,
/// contains NUL or a native path separator, or is not exactly one normal path
/// component.
fn validate_native_component(name: &OsStr) -> LocalResult<()> {
    if name.is_empty() || contains_native_nul(name) || has_native_separator(name) {
        return Err(invalid_name_error());
    }
    let path = std::path::Path::new(name);
    let mut components = path.components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_))) || components.next().is_some() {
        return Err(invalid_name_error());
    }
    Ok(())
}

/// Returns the byte size used for a native component limit.
///
/// # Parameters
///
/// - `name`: Validated native component.
///
/// # Returns
///
/// The raw byte count on Unix or UTF-16 byte count on Windows.
///
/// # Errors
///
/// Returns an unsupported error on targets without a lossless native path
/// representation supported by this crate.
#[cfg(unix)]
fn native_component_bytes(name: &OsStr) -> LocalResult<usize> {
    use std::os::unix::ffi::OsStrExt;

    Ok(name.as_bytes().len())
}

/// Returns the UTF-16 byte size used for a Windows native component limit.
#[cfg(windows)]
fn native_component_bytes(name: &OsStr) -> LocalResult<usize> {
    use std::os::windows::ffi::OsStrExt;

    Ok(name.encode_wide().count() * std::mem::size_of::<u16>())
}

/// Rejects native component sizing on unsupported targets.
#[cfg(not(any(unix, windows)))]
fn native_component_bytes(_name: &OsStr) -> LocalResult<usize> {
    Err(LocalFileError::new(
        LocalFileErrorKind::Unsupported,
        LocalFileOperation::ValidateName,
    ))
}

/// Reports whether a Unix component contains native NUL.
#[cfg(unix)]
#[must_use]
fn contains_native_nul(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    name.as_bytes().contains(&0)
}

/// Reports whether a Windows component contains native NUL.
#[cfg(windows)]
#[must_use]
fn contains_native_nul(name: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    name.encode_wide().any(|unit| unit == 0)
}

/// Rejects native components on unsupported targets.
#[cfg(not(any(unix, windows)))]
#[must_use]
const fn contains_native_nul(_name: &OsStr) -> bool {
    true
}

/// Reports whether a Unix component contains its native separator.
#[cfg(unix)]
#[must_use]
fn has_native_separator(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    name.as_bytes().contains(&b'/')
}

/// Reports whether a Windows component contains a native separator.
#[cfg(windows)]
#[must_use]
fn has_native_separator(name: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    name.encode_wide()
        .any(|unit| unit == u16::from(b'/') || unit == u16::from(b'\\'))
}

/// Rejects native components on unsupported targets.
#[cfg(not(any(unix, windows)))]
#[must_use]
const fn has_native_separator(_name: &OsStr) -> bool {
    true
}

/// Tests whether a component uses a Windows reserved device name.
///
/// # Parameters
///
/// - `name`: Portable name to inspect.
///
/// # Returns
///
/// `true` for reserved base names, including those followed by an extension.
#[must_use]
fn is_windows_reserved_file_name(name: &str) -> bool {
    let base_name = name
        .split_once('.')
        .map_or(name, |(base_name, _)| base_name)
        .trim_end_matches([' ', '.']);
    if base_name.eq_ignore_ascii_case("CON")
        || base_name.eq_ignore_ascii_case("PRN")
        || base_name.eq_ignore_ascii_case("AUX")
        || base_name.eq_ignore_ascii_case("NUL")
        || base_name.eq_ignore_ascii_case("CONIN$")
        || base_name.eq_ignore_ascii_case("CONOUT$")
    {
        return true;
    }
    let Some((suffix_index, suffix)) = base_name.char_indices().next_back() else {
        return false;
    };
    let prefix = &base_name[..suffix_index];
    (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
        && matches!(suffix, '1'..='9' | '¹' | '²' | '³')
}

/// Creates a structured invalid filename error.
#[must_use]
#[inline(always)]
fn invalid_name_error() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::ValidateName)
}

/// Creates a structured component-size limit error.
///
/// # Parameters
///
/// - `requested`: Encoded bytes required by the component.
/// - `limit`: Configured maximum component size.
///
/// # Returns
///
/// A validation error retaining complete resource-limit facts.
#[must_use]
fn component_limit_error(requested: usize, limit: usize) -> LocalFileError {
    LocalFileError::from_resource_limit(
        LocalFileOperation::ValidateName,
        None,
        LocalResourceLimitError::new(LocalResourceKind::PathComponentBytes, limit, limit, requested),
    )
}
