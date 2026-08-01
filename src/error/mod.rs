// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured errors for native local filesystem operations.

mod local_file_error;
mod local_file_error_kind;
mod local_file_error_source;
mod local_file_operation;
mod local_path_codec_error;
mod local_result;

pub use local_file_error::LocalFileError;
pub use local_file_error_kind::LocalFileErrorKind;
pub use local_file_error_source::LocalFileErrorSource;
pub use local_file_operation::LocalFileOperation;
pub use local_path_codec_error::LocalPathCodecError;
pub use local_result::LocalResult;
