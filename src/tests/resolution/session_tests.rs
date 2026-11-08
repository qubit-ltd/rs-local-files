// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResourceKind;
use crate::directory::LocalResolutionScope;
use crate::options::LocalResolutionLimits;
use crate::resolution::ResolutionContract;
use crate::resolution::resolution_session::ResolutionSession;
use crate::resolution::session::overflow_error;

#[test]
fn test_resolution_session_zero_forbids_following_links() {
    let contract = ResolutionContract::unrestricted().tightened(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let mut session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
    let error = session.before_follow_link().unwrap_err();
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    let facts = error.resource_limit_error().unwrap();
    assert_eq!(facts.resource(), LocalResourceKind::SymlinkHops);
    assert_eq!(facts.limit(), 0);
    assert_eq!(facts.remaining(), 0);
    assert_eq!(facts.requested(), 1);
    assert_eq!(facts.observed(), Some(1));
    assert!(!facts.overflowed());
}

#[test]
fn test_resolution_session_accepts_exact_hop_limit_and_rejects_next_hop() {
    let contract = ResolutionContract::unrestricted().tightened(LocalResolutionLimits::new().with_max_symlink_hops(1));
    let mut session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
    session.before_follow_link().unwrap();
    assert_eq!(
        session
            .before_follow_link()
            .unwrap_err()
            .resource_limit_error()
            .unwrap()
            .observed(),
        Some(2)
    );
}

#[test]
fn test_resolution_session_checks_pending_and_resolved_snapshots_independently() {
    let contract = ResolutionContract::unrestricted().tightened(
        LocalResolutionLimits::new()
            .with_max_actual_components(2)
            .with_max_resolved_path_bytes(3),
    );
    let session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
    session.check_pending(2, 3).unwrap();
    session.check_resolved(2, 3).unwrap();
    assert_eq!(
        session
            .check_pending(3, 3)
            .unwrap_err()
            .resource_limit_error()
            .unwrap()
            .resource(),
        LocalResourceKind::PendingResolutionComponents
    );
    assert_eq!(
        session
            .check_resolved(2, 4)
            .unwrap_err()
            .resource_limit_error()
            .unwrap()
            .resource(),
        LocalResourceKind::ResolvedPathBytes
    );
}

#[cfg(unix)]
#[test]
fn test_resolution_session_target_byte_limit_accepts_l_and_rejects_l_plus_one() {
    let contract =
        ResolutionContract::unrestricted().tightened(LocalResolutionLimits::new().with_max_symlink_target_bytes(3));
    let session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
    session.validate_target(Path::new("abc")).unwrap();
    let smaller =
        ResolutionContract::unrestricted().tightened(LocalResolutionLimits::new().with_max_symlink_target_bytes(2));
    let error = ResolutionSession::new(&smaller, LocalResolutionScope::Descendant)
        .validate_target(Path::new("abc"))
        .unwrap_err();
    let facts = error.resource_limit_error().unwrap();
    assert_eq!(facts.resource(), LocalResourceKind::SymlinkTargetBytes);
    assert_eq!(facts.observed(), Some(3));
}

#[test]
fn test_unrepresentable_resolution_snapshot_retains_facts_when_contextualized() {
    let path = Path::new("requested/link");
    let error = overflow_error(LocalResourceKind::PendingResolutionBytes)
        .with_operation(LocalFileOperation::List)
        .with_path(path.to_path_buf());

    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.operation(), LocalFileOperation::List);
    assert_eq!(error.path(), Some(path));
    let facts = error
        .resource_limit_error()
        .expect("context must preserve overflow facts");
    assert_eq!(facts.resource(), LocalResourceKind::PendingResolutionBytes);
    assert_eq!(facts.limit(), usize::MAX);
    assert_eq!(facts.remaining(), 0);
    assert_eq!(facts.requested(), 1);
    assert_eq!(facts.observed(), None);
    assert!(facts.overflowed());
}
