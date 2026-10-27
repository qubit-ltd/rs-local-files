// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured errors for native local filesystem operations.

#[cfg(feature = "digest")]
mod local_conditional_commit_error;
#[cfg(feature = "digest")]
mod local_conditional_failure_reason;
#[cfg(feature = "digest")]
mod local_conditional_write_stage;
mod local_file_effect_state;
mod local_file_error;
mod local_file_error_kind;
mod local_file_error_source;
mod local_file_operation;
mod local_path_codec_error;
mod local_resource_kind;
mod local_resource_limit_error;
mod local_result;
mod local_temp_seal_error;

#[cfg(feature = "digest")]
pub use local_conditional_commit_error::LocalConditionalCommitError;
#[cfg(feature = "digest")]
pub use local_conditional_failure_reason::LocalConditionalFailureReason;
#[cfg(feature = "digest")]
pub use local_conditional_write_stage::LocalConditionalWriteStage;
pub use local_file_effect_state::LocalFileEffectState;
pub use local_file_error::LocalFileError;
pub use local_file_error_kind::LocalFileErrorKind;
pub use local_file_error_source::LocalFileErrorSource;
pub use local_file_operation::LocalFileOperation;
pub use local_path_codec_error::LocalPathCodecError;
pub use local_resource_kind::LocalResourceKind;
pub use local_resource_limit_error::LocalResourceLimitError;
pub use local_result::LocalResult;
pub use local_temp_seal_error::LocalTempSealError;

pub use crate::local::LocalPersistError;
pub use crate::local::LocalPersistErrorParts;
pub use crate::writer::LocalFileCommitError;
