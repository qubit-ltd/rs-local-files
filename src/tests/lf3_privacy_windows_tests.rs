#[cfg(windows)]
#[test]
fn lf3_privacy_windows_creates_protected_file_and_directory() {
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;

    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = File::open(fixture.path()).unwrap();
    let file =
        privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("private")).unwrap();
    privacy_windows::verify_current_user_access(&file, false, false).unwrap();
    assert_ne!(file.as_raw_handle(), parent.as_raw_handle());

    let directory =
        privacy_windows::create_private_directory_at(&parent, std::ffi::OsStr::new("private-dir"))
            .unwrap();
    privacy_windows::verify_current_user_access(&directory, true, false).unwrap();
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_read_only_removes_data_write_access() {
    use std::fs::File;

    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = File::open(fixture.path()).unwrap();
    let file =
        privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("readonly")).unwrap();
    privacy_windows::apply_current_user_access(&file, false, true).unwrap();
    privacy_windows::verify_current_user_access(&file, false, true).unwrap();
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_refuses_existing_name_without_touching_it() {
    use std::fs::File;

    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("existing");
    std::fs::write(&path, b"preserve").unwrap();
    let parent = File::open(fixture.path()).unwrap();
    assert!(
        privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("existing")).is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"preserve");
}
