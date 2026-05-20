/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
/// Options controlling temporary file persistence behavior.
///
/// The default is conservative: existing destination paths are not overwritten.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalPersistOptions {
    /// Whether an existing target path may be overwritten.
    pub overwrite: bool,
}

impl Default for LocalPersistOptions {
    /// Returns conservative persistence options.
    ///
    /// # Returns
    /// Options that reject existing destination paths.
    #[inline]
    fn default() -> Self {
        Self { overwrite: false }
    }
}
