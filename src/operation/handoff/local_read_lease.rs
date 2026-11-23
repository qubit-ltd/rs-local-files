// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::super::executor::LocalIoExecutor;
use super::super::temp::LocalTempFileAccess;
use super::super::temp::LocalTempNative;
use super::LocalReadExport;
use super::LocalReadResource;
use super::LocalStageFailure;
use super::command_error;
use super::enqueue_resource_command;

/// A clonable lease. Each clone keeps the same single registered resource
/// alive. Repeated exported handles are native file clones that share the
/// open file's cursor; callers account for each extra descriptor and keep one
/// lease alive for every user for the full duration of access.
#[derive(Clone)]
pub struct LocalReadLease {
    /// Shared identity and export policy for the registered native file.
    resource: Arc<LocalReadResource>,
}

impl LocalReadLease {
    /// Builds a lease around the same registered temp resource, without
    /// creating a new resource entry.
    ///
    /// # Parameters
    ///
    /// * `access` — registered access token whose resource identity is
    ///   retained.
    /// * `export_rooted` — explicit permission to export a Rooted read handle.
    pub(crate) fn from_temp_access(access: &LocalTempFileAccess, export_rooted: bool) -> Self {
        Self {
            resource: Arc::new(LocalReadResource {
                reference: access.clone_access().clone_resource_ref(),
                export_read_handle: export_rooted,
            }),
        }
    }

    /// Returns the original registered resource identity.
    ///
    /// # Parameters
    ///
    /// * `self` — lease whose stable resource identity is returned.
    pub fn resource_id(&self) -> qubit_operation::ResourceId {
        self.resource.reference.id()
    }

    /// Exports a clone of the already-open sealed handle and retains this
    /// lease. Rooted exports require the explicit option supplied at
    /// staging time.
    ///
    /// # Parameters
    ///
    /// * `self` — lease retaining the sealed native file.
    /// * `executor` — worker queue used to clone the native handle without
    ///   blocking the caller.
    pub async fn export_read_handle(&self, executor: &LocalIoExecutor) -> Result<LocalReadExport, LocalStageFailure> {
        let reference = self.resource.reference.clone();
        let allow_rooted = self.resource.export_read_handle;
        let result = enqueue_resource_command(executor, reference, move |native| match native {
            LocalTempNative::Sealed(file) => {
                if file.native_path().is_none() && !allow_rooted {
                    return Err(LocalStageFailure::RootedExportDenied);
                }
                file.try_clone_readonly_file().map_err(LocalStageFailure::Local)
            }
            _ => Err(LocalStageFailure::NotSealed),
        })
        .map_err(command_error)?
        .await
        .map_err(command_error)?;
        Ok(LocalReadExport {
            file: result?,
            lease: self.clone(),
        })
    }
}
