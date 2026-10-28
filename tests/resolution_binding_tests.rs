use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::observation::LocalSourceVerification;
use qubit_local_files::options::{
    LocalDirectoryOpenOptions, LocalObservedReadOptions, LocalPathBindingOptions, LocalResolutionLimits,
};

#[cfg(unix)]
#[test]
fn binding_reopen_replays_the_root_contract_from_its_original_name() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("dir")).expect("bound directory should be created");
    std::fs::create_dir(tmp.path().join("other")).expect("replacement target should be created");
    std::fs::write(tmp.path().join("dir/source"), b"source").expect("source should be created");
    std::fs::write(tmp.path().join("other/source"), b"replacement").expect("replacement should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let binding = LocalPathBindingOptions::new().with_resolution_limits(
        LocalResolutionLimits::new().with_max_symlink_hops(0),
    );
    let reader_options = LocalObservedReadOptions::new().with_path_binding(binding);
    let reader = filesystem
        .open_observed_reader(std::path::Path::new("dir/source"), &reader_options)
        .expect("reader and binding should open");

    std::fs::rename(tmp.path().join("dir"), tmp.path().join("saved"))
        .expect("original bound directory should be renamed");
    symlink("other", tmp.path().join("dir")).expect("replacement directory symlink should be created");

    let error = reader
        .verify_path_binding()
        .expect_err("reopening a replaced ancestor must replay the zero-hop contract");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn binding_reports_unchanged_before_namespace_replacement() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("dir")).expect("directory should be created");
    std::fs::write(tmp.path().join("dir/source"), b"source").expect("source should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let reader = filesystem
        .open_observed_reader(
            std::path::Path::new("dir/source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader and binding should open");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Unchanged,
    );
}

#[cfg(unix)]
#[test]
fn rooted_directory_binding_reopens_the_original_symlink_recipe() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("fixture directory should exist");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should exist");
    std::fs::create_dir(tmp.path().join("other")).expect("replacement directory should exist");
    std::fs::write(tmp.path().join("real/source"), b"source").expect("source should exist");
    std::fs::write(tmp.path().join("other/source"), b"replacement").expect("replacement should exist");
    symlink("real", tmp.path().join("alias")).expect("directory alias should exist");

    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(
            std::path::Path::new("alias"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound alias directory should open");
    let reader = directory
        .open_observed_reader(
            std::path::Path::new("source"),
            &LocalObservedReadOptions::new(),
        )
        .expect("reader should open");

    std::fs::remove_file(tmp.path().join("alias")).expect("old alias should be removed");
    symlink("other", tmp.path().join("alias")).expect("replacement alias should be installed");

    assert_eq!(
        reader.verify_path_binding().expect("binding should be rechecked"),
        LocalSourceVerification::Changed,
    );
}
