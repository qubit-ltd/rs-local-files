// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by structured error integration tests.

/// Local filesystem operation that produced an error.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalFileOperation {
    /// Querying filesystem capabilities.
    Capabilities,
    /// Validating a native or portable filename.
    ValidateName,
    /// Generating a random filename.
    GenerateName,
    /// Binding a host path to the current working directory.
    BindPath,
    /// Composing or classifying native paths.
    ComposePath,
    /// Reading entry metadata.
    Metadata,
    /// Opening a file reader.
    OpenReader,
    /// Opening a file writer.
    OpenWriter,
    /// Copying an entry.
    Copy,
    /// Listing a directory.
    List,
    /// Creating a directory.
    CreateDirectory,
    /// Deleting a file.
    DeleteFile,
    /// Deleting a directory.
    DeleteDirectory,
    /// Renaming an entry.
    Rename,
    /// Creating a temporary file.
    CreateTempFile,
    /// Creating a temporary directory.
    CreateTempDirectory,
    /// Committing a writer or temporary resource.
    Commit,
    /// Aborting a writer.
    Abort,
    /// Cleaning a temporary resource.
    Cleanup,
}
