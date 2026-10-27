use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalCommitSyncOutcome {
    pub(crate) durable: bool,
    pub(crate) file_synced: bool,
}

#[derive(Debug, Default)]
pub(crate) struct LocalCommitSyncObserver {
    file_sync_calls: AtomicUsize,
    parent_sync_calls: AtomicUsize,
}

impl LocalCommitSyncObserver {
    pub(crate) fn record_file_sync(&self) {
        self.file_sync_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_parent_sync(&self) {
        self.parent_sync_calls.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn file_sync_calls(&self) -> usize {
        self.file_sync_calls.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn parent_sync_calls(&self) -> usize {
        self.parent_sync_calls.load(Ordering::Relaxed)
    }
}
