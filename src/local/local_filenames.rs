// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Random, portable, and lexical file-name helpers.

use std::convert::Infallible;
use std::ffi::OsStr;
use std::io::Result;
use std::path::Path;

use super::internal::{
    file_name_from_path,
    file_name_from_url,
    normalize_extension,
    try_random_file_name,
    validate_portable_file_name_impl,
};

/// File-name utility namespace.
///
/// This type is an uninstantiable namespace for random and lexical file-name
/// helpers. The path-based methods follow [`Path`] semantics, including Rust's
/// handling of dotfiles. Public methods that return file-name data return
/// UTF-8 strings (`&str` or `String`) instead of [`OsStr`]; invalid UTF-8 path
/// components are reported as `None`.
///
/// # Examples
/// ```
/// use qubit_local_files::LocalFilenames;
/// use std::path::Path;
///
/// let path = Path::new("/tmp/archive.tar.gz");
///
/// assert!(LocalFilenames::random().starts_with(LocalFilenames::DEFAULT_RANDOM_PREFIX));
/// assert_eq!(Some("archive.tar"), LocalFilenames::file_stem(path));
/// assert_eq!(Some("gz"), LocalFilenames::extension(path));
/// assert!(LocalFilenames::has_extension(path, ".gz"));
/// ```
pub struct LocalFilenames {
    /// Uninhabited field that prevents construction of this namespace type.
    _private: Infallible,
}

impl LocalFilenames {
    /// Default prefix used by random file-name generation.
    pub const DEFAULT_RANDOM_PREFIX: &str = "qubit-local-files-";

    /// Builds a random file-name component using the default prefix.
    ///
    /// The generated name contains a timestamp, process id, and random
    /// hexadecimal payload. It is only a file-name component; it is not joined
    /// to any directory and does not create anything on the filesystem.
    ///
    /// # Returns
    /// A random file-name component.
    ///
    /// # Panics
    /// Panics if the operating system random source cannot provide bytes.
    #[must_use]
    #[inline(always)]
    pub fn random() -> String {
        Self::try_random().expect("failed to build random file name")
    }

    /// Builds a random file-name component from an optional prefix and suffix.
    ///
    /// The caller-provided prefix and suffix must be file-name fragments, not
    /// paths. Path separators, root components, parent directory components,
    /// platform prefixes, and NUL bytes are rejected by
    /// [`LocalFilenames::try_random_with`].
    ///
    /// # Parameters
    /// - `prefix`: Optional file-name prefix. The default is
    ///   [`LocalFilenames::DEFAULT_RANDOM_PREFIX`].
    /// - `suffix`: Optional file-name suffix. The default is empty.
    ///
    /// # Returns
    /// A random file-name component.
    ///
    /// # Panics
    /// Panics if `prefix` or `suffix` is not a safe file-name fragment, or if
    /// the operating system random source cannot provide bytes.
    #[must_use]
    #[inline(always)]
    pub fn random_with(prefix: Option<&str>, suffix: Option<&str>) -> String {
        Self::try_random_with(prefix, suffix)
            .expect("failed to build random file name")
    }

    /// Tries to build a random file-name component using the default prefix.
    ///
    /// # Returns
    /// A random file-name component.
    ///
    /// # Errors
    /// Returns [`std::io::ErrorKind::Other`] when the operating system random
    /// source cannot provide bytes.
    #[inline(always)]
    pub fn try_random() -> Result<String> {
        Self::try_random_with(None, None)
    }

    /// Tries to build a random file-name component from a prefix and suffix.
    ///
    /// The generated name contains a timestamp, process id, and random
    /// hexadecimal payload. The caller-provided prefix and suffix must be file
    /// name fragments, not paths. Path separators, root components, parent
    /// directory components, platform prefixes, and NUL bytes are rejected.
    ///
    /// # Parameters
    /// - `prefix`: Optional file-name prefix. The default is
    ///   [`LocalFilenames::DEFAULT_RANDOM_PREFIX`].
    /// - `suffix`: Optional file-name suffix. The default is empty.
    ///
    /// # Returns
    /// A random file-name component.
    ///
    /// # Errors
    /// Returns [`std::io::ErrorKind::InvalidInput`] when `prefix` or `suffix`
    /// is not a safe file-name fragment. Returns
    /// [`std::io::ErrorKind::Other`] when the operating system random source
    /// cannot provide bytes.
    pub fn try_random_with(
        prefix: Option<&str>,
        suffix: Option<&str>,
    ) -> Result<String> {
        try_random_file_name(Self::DEFAULT_RANDOM_PREFIX, prefix, suffix)
    }

    /// Validates that `name` is a portable single-component file name.
    ///
    /// This is a lexical, conservative validation helper for names that should
    /// be safe to use as one file-name component across common platforms. It
    /// does not check whether the current filesystem can actually create the
    /// file, and it does not inspect permissions, existing paths, mount
    /// options, Unicode normalization, or filesystem-specific limits beyond
    /// a conservative 255-byte UTF-8 length cap.
    ///
    /// A portable file name must:
    /// - be non-empty;
    /// - not be `.` or `..`;
    /// - be at most 255 UTF-8 bytes;
    /// - not contain NUL, path separators, control characters, or Windows
    ///   reserved file-name characters;
    /// - not end with a space or dot;
    /// - not use a Windows reserved device name such as `CON`, `NUL`, `COM1`,
    ///   `LPT1`, `COM¹`, or `LPT³`, including names with extensions such as
    ///   `CON.txt`. Windows treats the ISO/IEC 8859-1 superscript digits `¹`,
    ///   `²`, and `³` as device-name digits, as documented by [Microsoft's
    ///   file-naming rules].
    ///
    /// [Microsoft's file-naming rules]: https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file
    ///
    /// # Parameters
    /// - `name`: File-name component to validate.
    ///
    /// # Errors
    /// Returns [`std::io::ErrorKind::InvalidInput`] when `name` is not a
    /// portable file-name component.
    pub fn validate_portable_file_name(name: &str) -> Result<()> {
        validate_portable_file_name_impl(name)
    }

    /// Returns the final file-name component of `path` as UTF-8.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    ///
    /// # Returns
    /// The final file-name component as `&str`, or `None` when `path` has no
    /// file-name component or when the component is not valid UTF-8.
    #[inline(always)]
    pub fn file_name(path: &Path) -> Option<&str> {
        path.file_name().and_then(OsStr::to_str)
    }

    /// Returns the file stem of `path` as UTF-8.
    ///
    /// The stem follows [`Path::file_stem`] semantics. For example,
    /// `archive.tar.gz` has stem `archive.tar`.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    ///
    /// # Returns
    /// The file stem as `&str`, or `None` when there is no stem or when the
    /// stem is not valid UTF-8.
    #[inline(always)]
    pub fn file_stem(path: &Path) -> Option<&str> {
        path.file_stem().and_then(OsStr::to_str)
    }

    /// Returns the file prefix of `path` as UTF-8.
    ///
    /// The prefix follows [`Path::file_prefix`] semantics. For example,
    /// `archive.tar.gz` has prefix `archive`.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    ///
    /// # Returns
    /// The file prefix as `&str`, or `None` when there is no prefix or when the
    /// prefix is not valid UTF-8.
    #[inline(always)]
    pub fn file_prefix(path: &Path) -> Option<&str> {
        path.file_prefix().and_then(OsStr::to_str)
    }

    /// Returns the final extension of `path` as UTF-8.
    ///
    /// The extension follows [`Path::extension`] semantics. Dotfiles such as
    /// `.env` do not have an extension unless they contain another dot.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    ///
    /// # Returns
    /// The extension without the leading dot, or `None` when there is no
    /// extension or when the extension is not valid UTF-8.
    #[inline(always)]
    pub fn extension(path: &Path) -> Option<&str> {
        path.extension().and_then(OsStr::to_str)
    }

    /// Returns the final extension of `path` with a leading dot.
    ///
    /// This method follows [`Path::extension`] semantics. If the path has an
    /// empty extension, such as `name.`, it returns an empty string.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    ///
    /// # Returns
    /// The extension with a leading dot, or `None` when there is no extension
    /// or when the extension is not valid UTF-8.
    #[inline]
    pub fn dot_extension(path: &Path) -> Option<String> {
        Self::extension(path).map(|extension| {
            if extension.is_empty() {
                String::new()
            } else {
                format!(".{extension}")
            }
        })
    }

    /// Tests whether `path` has the specified final extension.
    ///
    /// The `extension` argument may be written with or without a leading dot.
    /// The comparison is case-sensitive.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    /// - `extension`: Expected final extension.
    ///
    /// # Returns
    /// `true` when `path` has `extension` as its final extension.
    #[must_use]
    #[inline]
    pub fn has_extension(path: &Path, extension: &str) -> bool {
        Self::extension(path) == Some(normalize_extension(extension))
    }

    /// Tests whether `path` has the specified final extension, ignoring ASCII
    /// case.
    ///
    /// The `extension` argument may be written with or without a leading dot.
    /// Only ASCII case is folded; non-ASCII characters are compared exactly.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    /// - `extension`: Expected final extension.
    ///
    /// # Returns
    /// `true` when `path` has `extension` as its final extension ignoring ASCII
    /// case.
    #[must_use]
    #[inline]
    pub fn has_extension_ignore_ascii_case(
        path: &Path,
        extension: &str,
    ) -> bool {
        Self::extension(path)
            .map(|actual| {
                actual.eq_ignore_ascii_case(normalize_extension(extension))
            })
            .unwrap_or(false)
    }

    /// Returns the final file-name segment from a path-like string.
    ///
    /// This is a lexical helper for strings that may contain `/` or `\`
    /// separators. It does not touch the filesystem and does not normalize the
    /// input.
    ///
    /// # Parameters
    /// - `path`: Path-like string to inspect.
    ///
    /// # Returns
    /// The substring after the final slash or backslash. If no separator is
    /// present, the original string is returned.
    #[must_use]
    #[inline]
    pub fn file_name_from_path(path: &str) -> &str {
        file_name_from_path(path)
    }

    /// Returns the final decoded file-name segment from a URL-like string.
    ///
    /// Query strings, fragments, schemes, and hierarchical URL authorities are
    /// excluded before the final slash-delimited path segment is selected. An
    /// authority-only URL therefore returns an empty string. Opaque URLs such
    /// as `mailto:user@example.com` use the scheme-specific part as their
    /// lexical path.
    ///
    /// Percent-encoded UTF-8 sequences are decoded when the decoded result
    /// remains a single safe file-name fragment. If the selected segment
    /// contains invalid percent encoding, invalid UTF-8, or encoded path
    /// separators, parent-directory components, dot segments, or NUL bytes,
    /// the original selected segment is returned unchanged. This helper is
    /// lexical; it does not validate or normalize a complete URL.
    ///
    /// # Parameters
    /// - `url`: URL-like string to inspect.
    ///
    /// # Returns
    /// The decoded final URL path segment, or an empty string when no path
    /// segment exists.
    #[must_use]
    pub fn file_name_from_url(url: &str) -> String {
        file_name_from_url(url)
    }
}
