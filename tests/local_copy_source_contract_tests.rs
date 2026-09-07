// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Source-mode preflight must reject unsupported entries without publication.

use std::fs;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopySourceMode;
use qubit_local_files::outcome::LocalCopyFailureState;
use tempfile::tempdir;

/// Socket sources must fail before creating a requested destination parent.
#[cfg(unix)]
#[test]
fn test_special_source_is_unsupported_before_parent_creation() {
    for rooted in [false, true] {
        for options in [
            LocalCopyOptions::new(),
            LocalCopyOptions::new().with_entry_source(),
            LocalCopyOptions::new().with_tree_source(),
        ] {
            let directory = tempdir().expect("fixture should exist");
            let _listener = UnixListener::bind(directory.path().join("socket")).expect("socket source should exist");
            let (filesystem, source, target) = if rooted {
                (
                    LocalFileSystem::rooted(directory.path()).expect("root should open"),
                    PathBuf::from("socket"),
                    PathBuf::from("missing/target"),
                )
            } else {
                (
                    LocalFileSystem::host().expect("Host should open"),
                    directory.path().join("socket"),
                    directory.path().join("missing/target"),
                )
            };
            let failure = filesystem
                .copy_with_options(&source, &target, &options.with_create_parent())
                .expect_err("special sources must be rejected");
            assert_eq!(LocalFileErrorKind::Unsupported, failure.error().kind());
            assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
            assert!(!directory.path().join("missing").exists());
            assert!(fs::symlink_metadata(directory.path().join("socket")).is_ok());
        }
    }
}

/// All ordinary entry and tree modes agree between Host and Rooted backends.
#[test]
fn test_copy_source_modes_and_preflight_side_effects() {
    for rooted in [false, true] {
        for directory_source in [false, true] {
            for mode in [
                LocalCopySourceMode::Entry,
                LocalCopySourceMode::Tree,
                LocalCopySourceMode::Auto,
            ] {
                for existing_target in [false, true] {
                    let directory = tempdir().expect("fixture should exist");
                    if directory_source {
                        fs::create_dir(directory.path().join("source")).expect("source directory should exist");
                        fs::write(directory.path().join("source/payload"), b"payload").expect("payload should exist");
                    } else {
                        fs::write(directory.path().join("source"), b"payload").expect("source should exist");
                    }
                    if existing_target {
                        fs::create_dir(directory.path().join("parent")).expect("target parent should exist");
                        if directory_source {
                            fs::create_dir(directory.path().join("parent/target"))
                                .expect("target directory should exist");
                            fs::write(directory.path().join("parent/target/sentinel"), b"unchanged")
                                .expect("sentinel should exist");
                        } else {
                            fs::write(directory.path().join("parent/target"), b"unchanged")
                                .expect("target should exist");
                        }
                    }
                    let filesystem = if rooted {
                        LocalFileSystem::rooted(directory.path())
                    } else {
                        LocalFileSystem::host()
                    }
                    .expect("filesystem should open");
                    let source = if rooted {
                        PathBuf::from("source")
                    } else {
                        directory.path().join("source")
                    };
                    let target = if rooted {
                        PathBuf::from("parent/target")
                    } else {
                        directory.path().join("parent/target")
                    };
                    let options = LocalCopyOptions::new()
                        .with_source_mode(mode)
                        .with_create_parent()
                        .with_conflict(LocalCopyConflictPolicy::Overwrite);
                    let result = filesystem.copy_with_options(&source, &target, &options);
                    let mismatch = (directory_source && mode == LocalCopySourceMode::Entry)
                        || (!directory_source && mode == LocalCopySourceMode::Tree);
                    if mismatch {
                        let failure = result.expect_err("source mode must reject a different kind");
                        assert_eq!(LocalFileErrorKind::RequirementNotMet, failure.error().kind());
                        assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
                        if existing_target {
                            let sentinel = if directory_source {
                                "parent/target/sentinel"
                            } else {
                                "parent/target"
                            };
                            assert_eq!(
                                b"unchanged",
                                fs::read(directory.path().join(sentinel))
                                    .expect("target should remain")
                                    .as_slice()
                            );
                        } else {
                            assert!(!directory.path().join("parent").exists());
                        }
                    } else {
                        let _ = result.expect("compatible mode should copy");
                        let payload = if directory_source {
                            "parent/target/payload"
                        } else {
                            "parent/target"
                        };
                        assert_eq!(
                            b"payload",
                            fs::read(directory.path().join(payload))
                                .expect("copied payload should exist")
                                .as_slice()
                        );
                    }
                }
            }
        }
    }
}

/// Final links are entries even when their targets are directories or absent.
#[cfg(any(unix, windows))]
#[test]
fn test_final_link_source_modes_do_not_follow_targets() {
    for rooted in [false, true] {
        for referent in ["file", "directory", "missing", "missing-directory"] {
            for mode in [
                LocalCopySourceMode::Entry,
                LocalCopySourceMode::Tree,
                LocalCopySourceMode::Auto,
            ] {
                let directory = tempdir().expect("fixture should exist");
                fs::write(directory.path().join("file"), b"payload").expect("referent should exist");
                fs::create_dir(directory.path().join("directory")).expect("directory referent should exist");
                #[cfg(unix)]
                std::os::unix::fs::symlink(referent, directory.path().join("source"))
                    .expect("source link should exist");
                #[cfg(windows)]
                if matches!(referent, "directory" | "missing-directory") {
                    std::os::windows::fs::symlink_dir(referent, directory.path().join("source"))
                        .expect("directory link fixture requires symlink privilege");
                } else {
                    std::os::windows::fs::symlink_file(referent, directory.path().join("source"))
                        .expect("file link fixture requires symlink privilege");
                }
                let filesystem = if rooted {
                    LocalFileSystem::rooted(directory.path())
                } else {
                    LocalFileSystem::host()
                }
                .expect("filesystem should open");
                let source = if rooted {
                    PathBuf::from("source")
                } else {
                    directory.path().join("source")
                };
                let target = if rooted {
                    PathBuf::from("parent/target")
                } else {
                    directory.path().join("parent/target")
                };
                let options = LocalCopyOptions::new().with_source_mode(mode).with_create_parent();
                let result = filesystem.copy_with_options(&source, &target, &options);
                if mode == LocalCopySourceMode::Tree {
                    let failure = result.expect_err("tree requires an actual directory");
                    assert_eq!(LocalFileErrorKind::RequirementNotMet, failure.error().kind());
                    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
                    assert!(!directory.path().join("parent").exists());
                } else {
                    let _ = result.expect("link entry should copy");
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::FileTypeExt;

                        let kind = fs::symlink_metadata(directory.path().join("parent/target"))
                            .expect("copied link should exist")
                            .file_type();
                        assert_eq!(
                            matches!(referent, "directory" | "missing-directory"),
                            kind.is_symlink_dir()
                        );
                    }
                    assert_eq!(
                        PathBuf::from(referent),
                        fs::read_link(directory.path().join("parent/target")).expect("target should be a link")
                    );
                }
            }
        }
    }
}

/// Tree copying must preserve the Windows directory-link flag without targets.
#[cfg(windows)]
#[test]
fn test_tree_copy_preserves_dangling_directory_link_kind() {
    use std::os::windows::fs::FileTypeExt;

    use qubit_local_files::policy::LocalSymlinkPolicy;

    for rooted in [false, true] {
        let directory = tempdir().expect("fixture should exist");
        fs::create_dir(directory.path().join("source")).expect("source directory should exist");
        std::os::windows::fs::symlink_dir("missing", directory.path().join("source/link"))
            .expect("directory-link fixture requires symlink privilege");
        let filesystem = if rooted {
            LocalFileSystem::rooted(directory.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem should open");
        let source = if rooted {
            PathBuf::from("source")
        } else {
            directory.path().join("source")
        };
        let target = if rooted {
            PathBuf::from("target")
        } else {
            directory.path().join("target")
        };
        let _ = filesystem
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::new()
                    .with_tree_source()
                    .with_symlink_policy(LocalSymlinkPolicy::Reject),
            )
            .expect("tree should copy dangling link as an entry");
        let link = directory.path().join("target/link");
        assert!(
            fs::symlink_metadata(&link)
                .expect("link should exist")
                .file_type()
                .is_symlink_dir()
        );
        assert_eq!(
            PathBuf::from("missing"),
            fs::read_link(link).expect("target must remain a link")
        );
    }
}

/// Resetting a configured source mode restores automatic dispatch and keeps
/// budgets.
#[test]
fn test_auto_reset_copies_opposite_source_kind_and_retains_byte_limit() {
    let select = std::hint::black_box(
        LocalCopyOptions::with_source_mode as fn(LocalCopyOptions, LocalCopySourceMode) -> LocalCopyOptions,
    );
    for rooted in [false, true] {
        for initial in [LocalCopySourceMode::Entry, LocalCopySourceMode::Tree] {
            let directory = tempdir().expect("fixture should exist");
            let tree = initial == LocalCopySourceMode::Entry;
            let physical_source = directory.path().join("source");
            if tree {
                fs::create_dir(&physical_source).expect("tree source should exist");
            }
            fs::write(
                if tree {
                    physical_source.join("payload")
                } else {
                    physical_source.clone()
                },
                b"payload",
            )
            .expect("source content should exist");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let source = if rooted {
                PathBuf::from("source")
            } else {
                physical_source
            };
            let target = if rooted {
                PathBuf::from("target")
            } else {
                directory.path().join("target")
            };
            let configured = select(LocalCopyOptions::new().with_max_bytes(7), initial);
            let mismatch = filesystem
                .copy_with_options(&source, &target, &configured)
                .expect_err("configured mode rejects the opposite source kind");
            assert_eq!(LocalFileErrorKind::RequirementNotMet, mismatch.error().kind());
            let automatic = select(configured, LocalCopySourceMode::Auto);
            assert_eq!(Some(7), automatic.max_bytes());
            let outcome = filesystem
                .copy_with_options(&source, &target, &automatic)
                .expect("Auto reset should select the actual source kind");
            assert_eq!(1, outcome.stats().files());
            assert_eq!(7, outcome.stats().bytes());
        }
    }
}

/// Replacing a directory-link destination preserves its referent's contents.
#[cfg(any(unix, windows))]
#[test]
fn test_link_copy_overwrites_directory_link_without_removing_referent() {
    use qubit_local_files::policy::LocalSymlinkPolicy;

    for rooted in [false, true] {
        for tree in [false, true] {
            let directory = tempdir().expect("fixture should exist");
            // macOS temp paths may traverse /var -> /private/var. Resolve only
            // the fixture parent, before creating the links under test.
            #[cfg(target_os = "macos")]
            let parent = fs::canonicalize(directory.path()).expect("fixture parent should resolve");
            #[cfg(not(target_os = "macos"))]
            let parent = directory.path().to_path_buf();
            let source = parent.join("source");
            let target = parent.join("target");
            fs::create_dir(directory.path().join("old-referent")).expect("old referent should exist");
            fs::write(directory.path().join("old-referent/sentinel"), b"retained")
                .expect("referent sentinel should exist");
            fs::write(directory.path().join("new-referent"), b"new").expect("new referent should exist");
            let (source_link, target_link, new_referent, old_referent) = if tree {
                fs::create_dir(&source).expect("source tree should exist");
                fs::create_dir(&target).expect("target tree should exist");
                (
                    source.join("link"),
                    target.join("link"),
                    "../new-referent",
                    "../old-referent",
                )
            } else {
                (source.clone(), target.clone(), "new-referent", "old-referent")
            };
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(new_referent, &source_link).expect("source link should exist");
                std::os::unix::fs::symlink(old_referent, &target_link).expect("target link should exist");
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(new_referent, &source_link)
                    .expect("source link fixture requires symlink privilege");
                std::os::windows::fs::symlink_dir(old_referent, &target_link)
                    .expect("target link fixture requires symlink privilege");
            }
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let source_operand = if rooted { PathBuf::from("source") } else { source };
            let target_operand = if rooted { PathBuf::from("target") } else { target };
            let _ = filesystem
                .copy_with_options(
                    &source_operand,
                    &target_operand,
                    &LocalCopyOptions::new()
                        .with_source_mode(if tree {
                            LocalCopySourceMode::Tree
                        } else {
                            LocalCopySourceMode::Entry
                        })
                        .with_symlink_policy(LocalSymlinkPolicy::Reject)
                        .with_conflict(LocalCopyConflictPolicy::Overwrite),
                )
                .expect("copy should replace the directory-link entry");
            assert_eq!(
                PathBuf::from(new_referent),
                fs::read_link(&target_link).expect("target remains a link")
            );
            assert_eq!(
                b"retained",
                fs::read(directory.path().join("old-referent/sentinel"))
                    .expect("referent remains")
                    .as_slice()
            );
            #[cfg(windows)]
            {
                use std::os::windows::fs::FileTypeExt;

                assert!(
                    !fs::symlink_metadata(target_link)
                        .expect("link should exist")
                        .file_type()
                        .is_symlink_dir()
                );
            }
        }
    }
}

/// A copied link must not silently downgrade required atomic publication.
#[cfg(any(unix, windows))]
#[test]
fn test_link_copy_rejects_required_atomicity_before_mutation() {
    use qubit_local_files::policy::LocalAtomicityRequirement;

    for rooted in [false, true] {
        for mode in [LocalCopySourceMode::Entry, LocalCopySourceMode::Auto] {
            let options = LocalCopyOptions::new()
                .with_source_mode(mode)
                .with_atomicity(LocalAtomicityRequirement::Required);
            let directory = tempdir().expect("fixture should exist");
            #[cfg(unix)]
            std::os::unix::fs::symlink("missing", directory.path().join("source")).expect("source link should exist");
            #[cfg(windows)]
            std::os::windows::fs::symlink_file("missing", directory.path().join("source"))
                .expect("source link fixture requires symlink privilege");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let source = if rooted {
                PathBuf::from("source")
            } else {
                directory.path().join("source")
            };
            let target = if rooted {
                PathBuf::from("parent/target")
            } else {
                directory.path().join("parent/target")
            };
            let failure = filesystem
                .copy_with_options(&source, &target, &options.with_create_parent())
                .expect_err("link copying cannot satisfy the required guarantee");
            assert_eq!(LocalFileErrorKind::RequirementNotMet, failure.error().kind());
            assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
            assert!(!directory.path().join("parent").exists());
        }
    }
}

/// Link durability covers namespace publication even without a regular file.
#[cfg(unix)]
#[test]
fn test_link_copy_reports_required_namespace_durability() {
    use qubit_local_files::policy::LocalDurabilityRequirement;

    for rooted in [false, true] {
        let directory = tempdir().expect("fixture should exist");
        std::os::unix::fs::symlink("missing", directory.path().join("source"))
            .expect("dangling source link should exist");
        let filesystem = if rooted {
            LocalFileSystem::rooted(directory.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem should open");
        let source = if rooted {
            PathBuf::from("source")
        } else {
            directory.path().join("source")
        };
        let target = if rooted {
            PathBuf::from("new/nested/target")
        } else {
            directory.path().join("new/nested/target")
        };
        let outcome = filesystem
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::new()
                    .with_entry_source()
                    .with_create_parent()
                    .with_durability(LocalDurabilityRequirement::Required),
            )
            .expect("link namespace should synchronize on Unix");
        assert!(outcome.durable());
        assert!(!outcome.atomic());
        assert_eq!(1, outcome.stats().files());
        assert_eq!(0, outcome.stats().bytes());
        assert_eq!(
            PathBuf::from("missing"),
            fs::read_link(directory.path().join("new/nested/target")).expect("published link remains dangling")
        );
    }
}
