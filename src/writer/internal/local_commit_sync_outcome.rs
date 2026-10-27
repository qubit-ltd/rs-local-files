#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalCommitSyncOutcome {
    pub(crate) durable: bool,
    pub(crate) file_synced: bool,
}
