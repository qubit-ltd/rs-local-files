// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owned Windows security descriptor and ACL storage.

use std::io;
use std::mem::size_of;
use std::mem::zeroed;

use windows_sys::Win32::Security::ACL;
use windows_sys::Win32::Security::ACL_REVISION;
use windows_sys::Win32::Security::AddAccessAllowedAce;
use windows_sys::Win32::Security::InitializeAcl;
use windows_sys::Win32::Security::InitializeSecurityDescriptor;
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;
use windows_sys::Win32::Security::SE_DACL_PROTECTED;
use windows_sys::Win32::Security::SECURITY_DESCRIPTOR;
use windows_sys::Win32::Security::SetSecurityDescriptorDacl;

use super::CurrentUser;
use super::default_access_mask;
use super::last_os_error;

const ACL_BUFFER_SIZE: usize = 1024;

/// Owns an absolute security descriptor and the ACL storage it references.
///
/// The descriptor contains a pointer to `acl`'s heap allocation. Keep this
/// value alive and do not mutate or replace that allocation while a native
/// call uses [`SecurityDescriptor::as_ptr`]. Moving this struct preserves the
/// vector's heap allocation; dropping it or reallocating the vector does not.
pub(super) struct SecurityDescriptor {
    /// Absolute descriptor whose DACL pointer refers to the owned `acl` buffer.
    descriptor: SECURITY_DESCRIPTOR,
    /// Owns the initialized ACL and ACE bytes for the descriptor's lifetime.
    pub(super) acl: Vec<usize>,
}

impl SecurityDescriptor {
    /// Builds the default current-user descriptor for a file or directory.
    ///
    /// `directory` selects the directory or file mask, and `read_only` applies
    /// the removals documented by [`default_access_mask`]. Token lookup and
    /// native ACL/descriptor initialization failures are returned as I/O
    /// errors.
    ///
    /// # Parameters
    ///
    /// * `directory` selects file or directory access bits.
    /// * `read_only` removes the default write bits when true.
    ///
    /// # Returns
    ///
    /// A protected descriptor containing the default mask for the requested
    /// entry kind.
    ///
    /// # Errors
    ///
    /// Returns an error if native access, validation, or verification fails.
    pub(super) fn for_current_user(directory: bool, read_only: bool) -> io::Result<Self> {
        let access = default_access_mask(directory, read_only);
        Self::for_current_user_with_mask(access)
    }

    /// Builds a protected DACL granting `access` to the current token user.
    ///
    /// The mask is placed in one allow ACE; this constructor does not validate
    /// it against the owner-policy mask whitelist. `AddAccessAllowedAce` copies
    /// the SID into the ACE, so the temporary token-information buffer need not
    /// outlive this function. The returned value owns both the ACL and the
    /// absolute descriptor that points to it. Native API failures are returned
    /// as I/O errors.
    ///
    /// # Parameters
    ///
    /// * `access` is the exact mask stored in the current user allow ACE.
    ///
    /// # Returns
    ///
    /// A protected descriptor containing the exact requested access mask.
    ///
    /// # Errors
    ///
    /// Returns an error if native access, validation, or verification fails.
    pub(super) fn for_current_user_with_mask(access: u32) -> io::Result<Self> {
        #[cfg(test)]
        super::fail_if(super::FailurePoint::Descriptor)?;
        let current_user = CurrentUser::acquire()?;
        let current_sid = current_user.sid()?;

        let mut acl = vec![0_usize; ACL_BUFFER_SIZE.div_ceil(size_of::<usize>())];
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        // SAFETY: `acl_ptr` points to the writable, word-aligned allocation
        // owned by `acl`, and the supplied size is its full byte capacity.
        if unsafe { InitializeAcl(acl_ptr, (acl.len() * size_of::<usize>()) as u32, ACL_REVISION) } == 0 {
            return Err(last_os_error());
        }
        // SAFETY: `acl_ptr` is an initialized ACL with writable capacity; the
        // borrowed SID remains valid through this synchronous call, which adds
        // the SID data to the ACL's ACE storage.
        if unsafe { AddAccessAllowedAce(acl_ptr, ACL_REVISION, access, current_sid) } == 0 {
            return Err(last_os_error());
        }

        // SAFETY: SECURITY_DESCRIPTOR is a plain FFI output structure; it is
        // immediately initialized by InitializeSecurityDescriptor below.
        let mut descriptor = unsafe { zeroed::<SECURITY_DESCRIPTOR>() };
        // SAFETY: `descriptor` is writable storage for one SECURITY_DESCRIPTOR.
        if unsafe { InitializeSecurityDescriptor((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1) } == 0 {
            return Err(last_os_error());
        }
        // SAFETY: `descriptor` was initialized above and `acl_ptr` refers to
        // the live ACL allocation moved into this returned struct; the
        // descriptor stores this pointer, so `acl` must remain alive with it.
        if unsafe { SetSecurityDescriptorDacl((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1, acl_ptr, 0) }
            == 0
        {
            return Err(last_os_error());
        }
        // NtCreateFile with this descriptor creates an explicit DACL without inheriting
        // ACEs.
        descriptor.Control |= SE_DACL_PROTECTED;
        Ok(Self { descriptor, acl })
    }

    /// Returns a mutable-typed raw pointer borrowed from this descriptor.
    ///
    /// The pointer is valid only while `self` and its ACL allocation remain
    /// alive and unchanged. It does not transfer ownership or authorize the
    /// caller to free the descriptor. Do not move `self` while the pointer is
    /// retained. Keep the value alive for the complete native call that
    /// consumes the pointer.
    ///
    /// # Returns
    ///
    /// A raw pointer borrowed from the descriptor and its owned ACL storage.
    #[must_use]
    #[inline]
    pub(super) fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        (&self.descriptor as *const SECURITY_DESCRIPTOR).cast_mut().cast()
    }
}
