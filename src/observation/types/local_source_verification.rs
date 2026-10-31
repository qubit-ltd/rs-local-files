// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Result of checking whether a pathname still names its observed object.
///
/// Obtain this value from
/// [`crate::observation::LocalObservedReader::verify_path_binding`].
/// `Unchanged`, `Changed`, and `Missing` are successful verification results;
/// query, policy, and timeout failures are returned as errors. A successful
/// `Unchanged` result covers only the check that just completed and does not
/// lock the namespace against later changes.
///
/// ```
/// use std::path::Path;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::LocalSourceVerification;
/// use qubit_local_files::options::{LocalObservedReadOptions, LocalPathBindingOptions};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// std::fs::write(fixture.path().join("item"), b"data")?;
/// let filesystem = LocalFileSystem::rooted(fixture.path())?;
/// let reader = filesystem.open_observed_reader(
///     Path::new("item"),
///     &LocalObservedReadOptions::new()
///         .with_path_binding(LocalPathBindingOptions::new()),
/// )?;
/// match reader.verify_path_binding()? {
///     LocalSourceVerification::Unchanged => {}
///     LocalSourceVerification::Changed | LocalSourceVerification::Missing => {}
///     _ => {},
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalSourceVerification {
    /// The name still resolves to the same observed object.
    Unchanged,
    /// The name or an observed binding now identifies a different object.
    Changed,
    /// The observed name no longer exists.
    Missing,
}
