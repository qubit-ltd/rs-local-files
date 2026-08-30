// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scope-bound native path and filename values.

mod local_file_names;
mod local_namespace_path;
mod local_path_codec;
mod local_path_resolver;
mod local_paths;

#[cfg(test)]
mod local_path_resolver_tests;

pub use local_file_names::LocalFileNames;
pub(crate) use local_namespace_path::LocalNamespacePath;
pub use local_path_codec::LocalPathCodec;
pub(crate) use local_path_resolver::LocalPathResolver;
pub use local_paths::LocalPaths;
