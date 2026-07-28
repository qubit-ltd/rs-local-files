// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Integration tests for provider-facing native backend primitives.

use qubit_local_files::backend::{
    atomic,
    copy,
    directory,
    read,
    remove,
    rename,
    rooted,
    write,
};

/// Verifies that provider adapters can depend on the supported native backend
/// namespace without importing legacy crate-root modules.
#[test]
fn test_backend_exposes_provider_primitives() {
    let _ = atomic::Options::new();
    let _ = copy::Options::new();
    let _ = read::OpenOptions::default();
    let _ = rooted::Path::new(std::path::Path::new("provider-file"));
    let _ = write::OpenOptions::new;
    let _ = directory::read;
    let _ = remove::any;
    let _ = rename::move_path;
}
