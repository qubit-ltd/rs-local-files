//! Host-bound temporary-resource storage.

/// Marks a temporary resource whose path is already bound to the host namespace.
#[derive(Debug)]
pub(crate) struct HostTempResourceBackend;
