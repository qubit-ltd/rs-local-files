// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Current process user-token information used to build private DACLs.

use std::io;
use std::mem::size_of;
use std::ptr;

use windows_sys::Win32::Security::GetLengthSid;
use windows_sys::Win32::Security::GetTokenInformation;
use windows_sys::Win32::Security::PSID;
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::Security::TOKEN_USER;
use windows_sys::Win32::Security::TokenUser;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::OpenProcessToken;

use super::last_os_error;
use super::token::Token;

/// Owns the current process token and its `TokenUser` information buffer.
///
/// The SID exposed by [`CurrentUser::sid`] points into `information`; it is
/// borrowed from this value and must not outlive it or be freed separately.
#[derive(Debug)]
pub(super) struct CurrentUser {
    /// Owns the process-token handle used to obtain `information`.
    _token: Token,
    /// Owns the aligned storage filled by `GetTokenInformation` for
    /// `TokenUser`.
    information: Vec<usize>,
}

impl CurrentUser {
    /// Opens the current process token and retrieves its `TokenUser` record.
    ///
    /// Returns an I/O error if opening the token, sizing the information
    /// buffer, or retrieving the record fails. The token and returned data
    /// are owned by the resulting value.
    ///
    /// # Returns
    ///
    /// The current process user token and its populated `TokenUser` buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if opening the token or either token-information query
    /// fails.
    pub(super) fn acquire() -> io::Result<Self> {
        #[cfg(test)]
        super::fail_if(super::FailurePoint::Token)?;
        let mut token = ptr::null_mut();
        // GetCurrentProcess is a pseudo-handle; OpenProcessToken returns an owned
        // handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_os_error());
        }
        let token = Token(token);

        let mut required = 0_u32;
        // The first call obtains the required buffer size.
        // SAFETY: this is the documented size-query form: the output buffer is
        // null with a zero length, and `required` is writable output storage.
        unsafe {
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required);
        }
        if required == 0 {
            return Err(last_os_error());
        }
        let words = (required as usize).div_ceil(size_of::<usize>());
        let mut information = vec![0_usize; words];
        // SAFETY: `information` is writable and word-aligned, and its rounded
        // byte capacity is at least the `required` length supplied to the API.
        // `required` is writable for the returned length.
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                information.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        Ok(Self {
            _token: token,
            information,
        })
    }

    /// Returns the user SID pointer stored in this token-information buffer.
    ///
    /// The pointer is borrowed from `self` and is valid only while `self` and
    /// its `information` allocation remain alive and unchanged. It must not be
    /// freed independently. An error is returned for a null SID or one whose
    /// length cannot be read.
    ///
    /// # Returns
    ///
    /// The current user SID borrowed from this object's information buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if native access, validation, or verification fails.
    pub(super) fn sid(&self) -> io::Result<PSID> {
        #[cfg(test)]
        super::fail_if(super::FailurePoint::Sid)?;
        // GetTokenInformation returns TOKEN_USER at the start of this owned buffer.
        // SAFETY: `information` is allocated in machine-word units (and thus
        // aligned for `TOKEN_USER`), and the successful TokenUser query wrote
        // that record into this live buffer. The SID pointer is borrowed from it.
        let sid = unsafe { (*(self.information.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        // SAFETY: short-circuit evaluation excludes null; otherwise `sid`
        // points into the still-live TokenUser result buffer.
        if sid.is_null() || unsafe { GetLengthSid(sid) } == 0 {
            return Err(last_os_error());
        }
        Ok(sid)
    }
}
