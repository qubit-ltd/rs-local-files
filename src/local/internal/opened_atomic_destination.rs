// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix atomic destination identity with optional metadata-copy handles.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ffi::CString;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use super::rooted_file_io::open_file_at;
use super::unix_nonblocking::clear_nonblocking;
use super::unix_nonblocking::open_with_nonblocking_retry;
use super::unix_stat::is_regular_file_mode;
use crate::options::LocalWriteMetadataPolicy;

/// Commit-time Unix identity and an optional metadata-copy handle.
#[must_use = "the destination handle and captured identity must remain authoritative until commit"]
pub(crate) struct OpenedAtomicDestination {
    /// Readable handle only when strict metadata preservation is requested.
    file: Option<File>,
    /// Device identifier captured from the open handle.
    device: u64,
    /// Inode identifier captured from the open handle.
    inode: u64,
}

impl OpenedAtomicDestination {
    /// Constructs and validates destination identity from an open file.
    ///
    /// Takes ownership of `file` and clears its nonblocking flag. Returns
    /// `InvalidInput` for a non-regular handle, or propagates metadata and
    /// descriptor-configuration errors; failure closes the owned handle.
    pub(crate) fn from_file(file: File) -> Result<Self> {
        let metadata_result = file.metadata();
        #[cfg(feature = "test-support")]
        let metadata_result = if super::test_support::is_enabled("atomic-destination-stat") {
            Err(crate::local::test_fault_error())
        } else {
            metadata_result
        };
        let metadata = metadata_result?;
        if !metadata.is_file() || test_support_enabled("atomic-destination-type") {
            return Err(invalid_atomic_destination());
        }
        clear_nonblocking(file.as_raw_fd())?;
        Ok(Self {
            file: Some(file),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// Returns the metadata-copy handle, or `None` for identity-only
    /// observation.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn file(&self) -> Option<&File> {
        self.file.as_ref()
    }

    /// Returns the captured device identifier.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) const fn device(&self) -> u64 {
        self.device
    }

    /// Returns the captured inode identifier.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) const fn inode(&self) -> u64 {
        self.inode
    }
}

/// Observes the current destination without following its final component.
///
/// `UseStaging` captures no-follow identity without opening for read access;
/// `PreserveExisting` retains a readable handle for copying metadata.
///
/// Returns `None` when absent. A positive `open_retry_timeout` permits
/// nonblocking-open retries. Links and unsupported resource kinds produce
/// `InvalidInput`; other open or handle-validation errors are preserved.
pub(crate) fn open_atomic_destination(
    path: &Path,
    open_retry_timeout: Option<Duration>,
    metadata_policy: LocalWriteMetadataPolicy,
) -> Result<Option<OpenedAtomicDestination>> {
    if metadata_policy == LocalWriteMetadataPolicy::UseStaging {
        return match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(OpenedAtomicDestination {
                file: None,
                device: metadata.dev(),
                inode: metadata.ino(),
            })),
            Ok(_) => Err(invalid_atomic_destination()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        };
    }
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("atomic-destination-open") {
        return Err(crate::local::test_fault_error());
    }
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    open_destination_with_retry(open_retry_timeout, || {
        let result = options.open(path);
        #[cfg(feature = "test-support")]
        let result = inject_destination_open_result(
            result,
            "atomic-destination-would-block",
            "atomic-destination-invalid",
            "atomic-destination-native",
        );
        result
    })
}

/// Checks whether a path still names the opened destination identity.
///
/// Returns `false` for absence, a different identity, or a non-regular entry.
/// Other no-follow metadata errors are propagated.
pub(crate) fn destination_identity_matches(path: &Path, destination: &OpenedAtomicDestination) -> Result<bool> {
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("atomic-identity-mismatch") {
        return Ok(false);
    }
    let result = fs::symlink_metadata(path);
    #[cfg(feature = "test-support")]
    let result = if super::test_support::is_enabled("atomic-identity-missing") {
        Err(Error::from(ErrorKind::NotFound))
    } else if super::test_support::is_enabled("atomic-identity-inspect") {
        Err(crate::local::test_fault_error())
    } else {
        result
    };
    match result {
        Ok(metadata) => Ok(metadata.file_type().is_file()
            && metadata.dev() == destination.device
            && metadata.ino() == destination.inode),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Opens the current rooted destination without following its final entry.
///
/// `name` is one child of the opened `parent`. Absence returns `None`; retry
/// behavior and errors match [`open_atomic_destination`], using descriptor
/// authority throughout.
pub(in crate::local) fn open_rooted_atomic_destination(
    parent: &File,
    name: &CString,
    open_retry_timeout: Option<Duration>,
    metadata_policy: LocalWriteMetadataPolicy,
) -> Result<Option<OpenedAtomicDestination>> {
    if metadata_policy == LocalWriteMetadataPolicy::UseStaging {
        let Some(status) = rooted_destination_status(parent, name)? else {
            return Ok(None);
        };
        if !is_regular_file_mode(status.st_mode) {
            return Err(invalid_atomic_destination());
        }
        return Ok(Some(OpenedAtomicDestination {
            file: None,
            device: native_identity_component(status.st_dev)?,
            inode: native_identity_component(status.st_ino)?,
        }));
    }
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("rooted-destination-open") {
        return Err(crate::local::test_fault_error());
    } else if super::test_support::is_enabled("rooted-destination-missing") {
        return Ok(None);
    }
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    open_destination_with_retry(open_retry_timeout, || {
        let result = open_file_at(parent, name, flags, 0);
        #[cfg(feature = "test-support")]
        let result = inject_destination_open_result(
            result,
            "rooted-destination-would-block",
            "rooted-destination-invalid",
            "rooted-destination-native",
        );
        result
    })
}

/// Repeats a nonblocking destination open until it succeeds or is classified.
///
/// # Parameters
/// - `open_retry_timeout`: Positive elapsed retry budget, or `None`/zero for
///   one native attempt.
/// - `open`: Native path-based or descriptor-relative open attempt.
///
/// # Returns
/// An authoritative destination handle, or `None` when the entry is missing.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for symbolic links and other forbidden
/// resource types, or preserves any other native open or inspection error.
fn open_destination_with_retry<F>(
    open_retry_timeout: Option<Duration>,
    open: F,
) -> Result<Option<OpenedAtomicDestination>>
where
    F: FnMut() -> Result<File>,
{
    match open_with_nonblocking_retry(open_retry_timeout, open) {
        Ok(file) => OpenedAtomicDestination::from_file(file).map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENXIO | libc::ENODEV)) => {
            Err(invalid_atomic_destination())
        }
        Err(error) => Err(error),
    }
}

/// Applies test-support-only failures to one native destination-open result.
///
/// # Parameters
/// - `result`: Native open result before fault injection.
/// - `would_block_fault`: One-shot retry fault name.
/// - `invalid_fault`: Invalid-resource fault name.
/// - `native_fault`: Unclassified native failure name.
///
/// # Returns
/// The original result or the selected injected failure.
///
/// # Errors
/// Returns the selected retry, invalid-resource, or native test fault, or
/// preserves the native error in `result`.
#[cfg(feature = "test-support")]
fn inject_destination_open_result(
    result: Result<File>,
    would_block_fault: &str,
    invalid_fault: &str,
    native_fault: &str,
) -> Result<File> {
    if super::test_support::take(would_block_fault) {
        Err(Error::from(ErrorKind::WouldBlock))
    } else if super::test_support::is_enabled(invalid_fault) {
        Err(Error::from_raw_os_error(libc::ELOOP))
    } else if super::test_support::is_enabled(native_fault) {
        Err(crate::local::test_fault_error())
    } else {
        result
    }
}

/// Checks whether a rooted entry still names an opened destination identity.
///
/// Absence, a different identity, or a non-regular entry returns `false`.
/// Inspection failures propagate; an identity outside `u64` returns
/// `InvalidData` instead of truncating it.
pub(in crate::local) fn rooted_destination_identity_matches(
    parent: &File,
    name: &CString,
    destination: &OpenedAtomicDestination,
) -> Result<bool> {
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("rooted-identity-mismatch")
        || super::test_support::is_enabled("rooted-identity-missing")
    {
        return Ok(false);
    } else if super::test_support::is_enabled("rooted-identity-inspect") {
        return Err(crate::local::test_fault_error());
    }
    let Some(status) = rooted_destination_status(parent, name)? else {
        return Ok(false);
    };
    if !is_regular_file_mode(status.st_mode) || test_support_enabled("rooted-status-type") {
        return Ok(false);
    }
    let device = native_identity_component(status.st_dev)?;
    let inode = native_identity_component(status.st_ino)?;
    Ok(device == destination.device() && inode == destination.inode())
}

/// Reads rooted destination status without following the final entry.
///
/// Returns `None` for absence and propagates other `fstatat` errors.
fn rooted_destination_status(parent: &File, name: &CString) -> Result<Option<libc::stat>> {
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("rooted-status-missing") {
        return Ok(None);
    } else if super::test_support::is_enabled("rooted-status-error") {
        return Err(crate::local::test_fault_error());
    }
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `status` is writable storage and the parent descriptor and name
    // remain live for this non-retaining lookup.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        let error = Error::last_os_error();
        return if error.kind() == ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: successful `fstatat` initialized the complete status value.
    Ok(Some(unsafe { status.assume_init() }))
}

/// Converts a platform-native stat identity component to the public width.
///
/// Returns `InvalidData` when the value cannot be represented as `u64`.
fn native_identity_component<T>(value: T) -> Result<u64>
where
    u64: TryFrom<T>,
{
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("rooted-identity-overflow") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "injected atomic destination identity overflow",
        ));
    }
    match u64::try_from(value) {
        Ok(value) => Ok(value),
        Err(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "atomic destination identity is outside the supported range",
        )),
    }
}

/// Returns whether a test-support-only atomic destination fault is selected.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn test_support_enabled(name: &str) -> bool {
    #[cfg(feature = "test-support")]
    return super::test_support::is_enabled(name);
    #[cfg(not(feature = "test-support"))]
    {
        let _ = name;
        false
    }
}

/// Creates the stable type error for atomic destinations.
#[must_use]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn invalid_atomic_destination() -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        "atomic write destination must be absent or a regular file",
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::fs;
    use std::fs::File;
    use std::os::unix::fs::symlink;

    use super::destination_identity_matches;
    use super::open_atomic_destination;
    use super::open_rooted_atomic_destination;
    use super::rooted_destination_identity_matches;
    use crate::options::LocalWriteMetadataPolicy;

    /// Real entry replacement between observation and validation is rejected in
    /// both namespaces.
    #[test]
    fn test_destination_identity_rejects_real_replacement_under_both_metadata_policies() {
        for policy in [
            LocalWriteMetadataPolicy::PreserveExisting,
            LocalWriteMetadataPolicy::UseStaging,
        ] {
            for replacement in ["file", "symlink", "directory"] {
                let fixture = tempfile::tempdir().expect("isolated identity fixture");
                let path = fixture.path().join("destination");
                fs::write(&path, b"observed").expect("original destination");
                let parent = File::open(fixture.path()).expect("opened parent authority");
                let name = CString::new("destination").expect("entry name");
                let host = open_atomic_destination(&path, None, policy)
                    .expect("host observation")
                    .expect("existing host destination");
                let rooted = open_rooted_atomic_destination(&parent, &name, None, policy)
                    .expect("rooted observation")
                    .expect("existing rooted destination");
                assert_eq!(
                    host.file().is_some(),
                    policy == LocalWriteMetadataPolicy::PreserveExisting
                );
                assert_eq!(
                    rooted.file().is_some(),
                    policy == LocalWriteMetadataPolicy::PreserveExisting
                );
                assert!(destination_identity_matches(&path, &host).expect("unchanged host identity"));
                assert!(
                    rooted_destination_identity_matches(&parent, &name, &rooted).expect("unchanged rooted identity")
                );

                // Retain the original inode so the filesystem cannot recycle its identity.
                let observed = fixture.path().join("observed");
                fs::rename(&path, &observed).expect("replace after identity observation");
                match replacement {
                    "file" => fs::write(&path, b"replacement").expect("different inode"),
                    // A following lookup would see the original inode; no-follow must reject this link.
                    "symlink" => symlink("observed", &path).expect("link to original inode"),
                    _ => fs::create_dir(&path).expect("directory replacement"),
                }
                assert!(!destination_identity_matches(&path, &host).expect("host identity check"));
                assert!(!rooted_destination_identity_matches(&parent, &name, &rooted).expect("rooted identity check"));
                assert_eq!(fs::read(observed).expect("retained original content"), b"observed");
            }
        }
    }
}
