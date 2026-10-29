// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::LocalFileSystemScope;
use crate::LocalWriteOptions;
use crate::capability::LocalConditionalKind;

/// Reports whether this build contains a backend for the requested protocol.
pub(crate) fn conditional_protocol_compiled(
    scope: LocalFileSystemScope,
    kind: LocalConditionalKind,
    options: &LocalWriteOptions,
) -> bool {
    match kind {
        LocalConditionalKind::Content => {
            #[cfg(windows)]
            {
                scope == LocalFileSystemScope::Host
                    && options.metadata_policy() == crate::options::LocalWriteMetadataPolicy::PreserveExisting
            }
            #[cfg(unix)]
            {
                let _ = (scope, options);
                unix_content_protocol_compiled()
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = (scope, options);
                false
            }
        }
        LocalConditionalKind::Absent => {
            #[cfg(windows)]
            {
                scope == LocalFileSystemScope::Host
                    || options.durability() != crate::policy::LocalDurabilityRequirement::Required
            }
            #[cfg(unix)]
            {
                let _ = (scope, options);
                true
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = (scope, options);
                false
            }
        }
    }
}

/// Reports whether this Unix build includes the Content publication protocol.
#[cfg(unix)]
pub(crate) const fn unix_content_protocol_compiled() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::unix_content_protocol_compiled;

    #[cfg(unix)]
    #[test]
    fn unix_content_protocol_matches_compiled_platform_matrix() {
        assert_eq!(
            unix_content_protocol_compiled(),
            cfg!(any(target_os = "linux", target_os = "macos"))
        );
    }
}
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
// =============================================================================
