//! Same-handle snapshot value and comparison.

use super::{LocalFileObservation, LocalNativeFileDetails};

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
#[derive(Clone, Debug)]
pub struct LocalFileSnapshot {
    bytes: Vec<u8>,
    before: LocalFileObservation,
    after: LocalFileObservation,
}

impl LocalFileSnapshot {
    pub(crate) const fn new(
        bytes: Vec<u8>,
        before: LocalFileObservation,
        after: LocalFileObservation,
    ) -> Self {
        Self {
            bytes,
            before,
            after,
        }
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

pub(crate) fn observations_match(
    before: &LocalFileObservation,
    after: &LocalFileObservation,
    count: u64,
) -> bool {
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
