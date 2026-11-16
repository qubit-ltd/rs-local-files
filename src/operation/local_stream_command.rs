// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::SeekFrom;
use std::sync::Arc;
/// An owned command sent to a stream's serial worker.
///
/// Each command carries only owned data so a worker can execute it after the caller yields.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalStreamCommand;
/// use std::sync::Arc;
///
/// let command = LocalStreamCommand::Write {
///     bytes: Arc::from(&b"payload"[..]),
/// };
/// assert!(matches!(command, LocalStreamCommand::Write { .. }));
/// ```
#[derive(Debug, Clone)]
pub enum LocalStreamCommand {
    /// Reads at most the requested number of bytes.
    Read {
        /// Maximum number of bytes to return.
        max_bytes: usize,
    },
    /// Writes an immutable, shared payload.
    Write {
        /// Bytes retained by the worker until the write completes.
        bytes: Arc<[u8]>,
    },
    /// Flushes buffered bytes to the underlying stream.
    Flush,
    /// Moves the reader to an absolute or relative position.
    Seek(#[doc = "Target position applied before subsequent reads."] SeekFrom),
    /// Publishes the writer contents and returns the publication outcome.
    Commit,
    /// Closes the stream while preserving already completed operations.
    Close,
    /// Aborts and releases the native owner when the public port is dropped.
    Abort,
}
