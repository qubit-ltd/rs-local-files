use std::path::Path;

use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::policy::LocalFileSyncRequirement;

#[test]
fn staged_directory_exposes_bounded_file_synchronization() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("entry"), &mut &b"payload"[..], 16)
        .expect("file should be written");

    assert!(
        stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::Required)
            .expect("required file synchronization should succeed")
    );
    assert!(
        !stage
            .sync_files_with_options(
                &LocalListOptions::new(),
                LocalFileSyncRequirement::NotRequired
            )
            .expect("not-required synchronization should succeed")
    );
}
