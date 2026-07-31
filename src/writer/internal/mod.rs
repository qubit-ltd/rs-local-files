// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private writer backend state.

mod local_file_writer_backend;
mod local_staged_commit_error;

pub(crate) use local_file_writer_backend::LocalFileWriterBackend;
pub(crate) use local_staged_commit_error::LocalStagedCommitError;
