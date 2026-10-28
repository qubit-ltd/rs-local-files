// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::cell::RefCell;

use super::super::MAX_OWNER_DESCRIPTOR_BYTES;
use super::super::decode_dacl_descriptor;
use super::super::query_security_descriptor_with;
use crate::error::LocalFileErrorKind;
use crate::error::LocalResourceKind;

const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;

#[test]
fn test_review_acl_budget_zero_accepts_null_dacl_without_acl_payload() {
    let mut descriptor = vec![0; 20];
    descriptor[0] = 1;
    descriptor[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
    let capacities = RefCell::new(Vec::new());
    let result = query_security_descriptor_with(4, Some(0), Some(0), |_, output, capacity, needed| {
        capacities.borrow_mut().push(capacity);
        if capacity == 0 {
            // SAFETY: the probe's output argument is the writable `needed` scalar.
            unsafe { *needed = 20 };
            return STATUS_BUFFER_TOO_SMALL;
        }
        // SAFETY: the buffer has the exact capacity reported to this callable.
        unsafe { std::ptr::copy_nonoverlapping(descriptor.as_ptr(), output.cast(), descriptor.len()) };
        0
    })
    .expect("null DACL descriptor fits its fixed header budget");
    assert_eq!(result, descriptor);
    assert_eq!(*capacities.borrow(), [0, 20]);
}

#[test]
fn test_review_acl_budget_zero_rejects_nonempty_acl_after_probe_only() {
    let capacities = RefCell::new(Vec::new());
    let error = query_security_descriptor_with(4, Some(0), Some(0), |_, _, capacity, needed| {
        capacities.borrow_mut().push(capacity);
        // DACL-only descriptor is 20-byte header + 8-byte ACL.
        // SAFETY: `needed` points to writable storage supplied by the caller.
        unsafe { *needed = 28 };
        STATUS_BUFFER_TOO_SMALL
    })
    .expect_err("nonempty ACL must exceed a zero payload budget");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(
        error.resource_limit_error().map(|limit| limit.resource()),
        Some(LocalResourceKind::AclBytes)
    );
    assert_eq!(*capacities.borrow(), [0]);
}

#[test]
fn test_review_acl_budget_exact_limit_allows_payload_and_bounded_growth_retry() {
    let descriptor = vec![1; 28];
    let capacities = RefCell::new(Vec::new());
    let calls = RefCell::new(0usize);
    let result = query_security_descriptor_with(4, Some(8), Some(8), |_, output, capacity, needed| {
        capacities.borrow_mut().push(capacity);
        let mut calls = calls.borrow_mut();
        *calls += 1;
        if *calls == 1 {
            // Initial probe advertises a shorter result; payload query reports
            // the stable length on the bounded retry.
            // SAFETY: `needed` points to writable storage supplied by the caller.
            unsafe { *needed = 24 };
            return STATUS_BUFFER_TOO_SMALL;
        }
        if *calls == 2 {
            // SAFETY: `needed` points to writable storage supplied by the caller.
            unsafe { *needed = 28 };
            return STATUS_BUFFER_TOO_SMALL;
        }
        assert_eq!(capacity, 28);
        // SAFETY: the buffer has the exact capacity reported to this callable.
        unsafe { std::ptr::copy_nonoverlapping(descriptor.as_ptr(), output.cast(), descriptor.len()) };
        0
    })
    .expect("descriptor at the exact ACL budget should succeed");
    assert_eq!(result, descriptor);
    assert_eq!(*capacities.borrow(), [0, 24, 28]);
}

#[test]
fn test_review_acl_budget_owner_query_uses_separate_fixed_internal_cap() {
    let capacities = RefCell::new(Vec::new());
    let result = query_security_descriptor_with(1, Some(MAX_OWNER_DESCRIPTOR_BYTES), None, |_, _, capacity, needed| {
        capacities.borrow_mut().push(capacity);
        // SAFETY: `needed` points to writable storage supplied by the caller.
        unsafe { *needed = (MAX_OWNER_DESCRIPTOR_BYTES + 1) as u32 };
        STATUS_BUFFER_TOO_SMALL
    })
    .expect_err("owner descriptor beyond its internal cap must be rejected before allocation");
    assert_eq!(result.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(capacities.borrow().as_slice(), [0]);
}

#[test]
fn test_review_acl_budget_decoder_rejects_bad_offset_and_acl_size() {
    let owner = "S-1-5-32-545";
    let mut descriptor = vec![0; 28];
    descriptor[0] = 1;
    descriptor[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
    descriptor[16..20].copy_from_slice(&20u32.to_le_bytes());
    descriptor[20] = 2;
    descriptor[22..24].copy_from_slice(&8u16.to_le_bytes());

    let mut bad_offset = descriptor.clone();
    bad_offset[16..20].copy_from_slice(&19u32.to_le_bytes());
    assert!(decode_dacl_descriptor(&bad_offset, None, Some(8), owner, owner).is_none());

    let mut bad_acl_size = descriptor;
    bad_acl_size[22..24].copy_from_slice(&9u16.to_le_bytes());
    assert!(decode_dacl_descriptor(&bad_acl_size, None, Some(8), owner, owner).is_none());
}
