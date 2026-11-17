// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Native reader or writer owned by a local serial worker.
pub(crate) enum LocalNativeStream {
    /// The worker-owned read handle.
    Reader(#[doc = "Native reader retained exclusively by its serial worker."] crate::LocalFileReader),
    /// The worker-owned write handle.
    Writer(#[doc = "Native writer retained exclusively until commit or abort."] Box<crate::LocalFileWriter>),
}
