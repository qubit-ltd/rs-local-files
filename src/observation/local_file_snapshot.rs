// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Same-handle snapshot value and comparison.

use super::LocalFileObservation;
use super::LocalNativeFileDetails;

#[cfg(test)]
thread_local! {
    static READ_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn install_read_hook(hook: impl FnOnce() + 'static) -> impl Drop {
    READ_HOOK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "snapshot hook already installed on this thread"
        );
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    struct HookGuard;
    impl Drop for HookGuard {
        fn drop(&mut self) {
            READ_HOOK.with(|slot| *slot.borrow_mut() = None);
        }
    }
    HookGuard
}

#[cfg(test)]
pub(crate) fn run_read_hook() {
    READ_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

/// Bytes bracketed by required observations of one opened file.
///
/// The before and after observations use the same opened handle. They must
/// agree on object identity, length, and available native change timestamps,
/// and the byte count must match that length. This detects many concurrent
/// changes, but the bytes and observations are not an atomic filesystem
/// snapshot; an identity is not a content version and same-size writes can
/// evade timestamp granularity or races.
///
/// # Example
///
/// ```no_run
/// use qubit_io::Output;
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalSnapshotOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut source = filesystem.create_temp_file()?;
/// source.write_fully(b"snapshot data")?;
/// let path = source.path().to_path_buf();
/// let options = LocalSnapshotOptions::builder(1024).build();
/// let snapshot = filesystem.read_consistent_with_options(&path, &options)?;
/// assert_eq!(snapshot.bytes(), b"snapshot data");
/// source.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct LocalFileSnapshot {
    bytes: Vec<u8>,
    before: LocalFileObservation,
    after: LocalFileObservation,
}

impl LocalFileSnapshot {
    pub(crate) const fn new(bytes: Vec<u8>, before: LocalFileObservation, after: LocalFileObservation) -> Self {
        Self { bytes, before, after }
    }

    /// Returns the collected bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Consumes the snapshot and returns its bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    /// Returns the observation taken before reading.
    pub const fn before(&self) -> &LocalFileObservation {
        &self.before
    }
    /// Returns the observation taken after reading.
    pub const fn after(&self) -> &LocalFileObservation {
        &self.after
    }
}

pub(crate) fn observations_match(before: &LocalFileObservation, after: &LocalFileObservation, count: u64) -> bool {
    if before.identity() != after.identity()
        || before.metadata().len() != after.metadata().len()
        || before.metadata().len() != count
    {
        return false;
    }
    match (before.native_details(), after.native_details()) {
        (Some(left), Some(right)) => native_match(left, right),
        _ => false,
    }
}

fn native_match(left: &LocalNativeFileDetails, right: &LocalNativeFileDetails) -> bool {
    if let (Some(a), Some(b)) = (left.unix(), right.unix()) {
        return a.modified_seconds() == b.modified_seconds()
            && a.modified_nanos() == b.modified_nanos()
            && a.changed_seconds() == b.changed_seconds()
            && a.changed_nanos() == b.changed_nanos();
    }
    if let (Some(a), Some(b)) = (left.windows(), right.windows()) {
        return a.last_write_time() == b.last_write_time() && a.change_time() == b.change_time();
    }
    false
}
