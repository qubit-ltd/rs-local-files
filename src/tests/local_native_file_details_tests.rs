// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::observation::LocalNativeFileDetails;
use crate::observation::LocalWindowsFileDetails;

#[test]
fn test_windows_native_details_preserve_full_ids_and_special_timestamps() {
    let file_id = [0x80, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0xff];
    let details = LocalWindowsFileDetails::new(u64::MAX, file_id, i64::MIN, i64::MAX);
    let native = LocalNativeFileDetails::from_windows(details);
    assert!(native.unix().is_none());
    let windows = native
        .windows()
        .expect("Windows details remain readable on any platform");
    assert_eq!(windows.volume_serial(), u64::MAX);
    assert_eq!(windows.file_id(), &file_id);
    assert_eq!(windows.last_write_time(), i64::MIN);
    assert_eq!(windows.change_time(), i64::MAX);
}
