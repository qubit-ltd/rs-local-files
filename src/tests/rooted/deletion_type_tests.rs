use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::local::LocalRelativePath;
use crate::rooted::EntryKind;
use crate::rooted::Root;

#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_remove_file_does_not_delete_replacement_directory() {
    let directory = tempdir().expect("root fixture should be created");
    let root = Root::open(directory.path()).expect("root should open");
    let path = LocalRelativePath::new(Path::new("entry")).expect("path should be valid");
    fs::write(directory.path().join("entry"), b"original").expect("file should be written");
    let fault = crate::local::install_test_fault("temp-observed-file-becomes-directory")
        .expect("replacement fault should install");

    let result = root.remove_file(&path);
    drop(fault);

    assert!(result.is_err(), "file removal must reject a replacement directory");
    assert_eq!(
        EntryKind::Directory,
        root.symlink_metadata(&path).expect("replacement should remain").kind(),
    );
}
#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_remove_empty_dir_does_not_delete_replacement_leaf() {
    for fault_name in [
        "temp-observed-directory-becomes-file",
        "temp-observed-directory-becomes-symlink",
    ] {
        let directory = tempdir().expect("root fixture should be created");
        let root = Root::open(directory.path()).expect("root should open");
        let path = LocalRelativePath::new(Path::new("entry")).expect("path should be valid");
        fs::create_dir(directory.path().join("entry")).expect("directory should be created");
        let fault = crate::local::install_test_fault(fault_name).expect("replacement fault should install");

        let result = root.remove_empty_dir(&path);
        drop(fault);

        assert!(result.is_err(), "directory removal must reject a replaced leaf");
        assert_ne!(
            EntryKind::Directory,
            root.symlink_metadata(&path).expect("replacement should remain").kind(),
        );
    }
}
