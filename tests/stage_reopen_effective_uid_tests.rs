// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Staged-directory reopen uses the effective Unix user for privacy checks.

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    #[cfg(target_os = "linux")]
    use std::path::Path;
    #[cfg(target_os = "linux")]
    use std::process::Command;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::options::LocalDirectoryOpenOptions;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreeValidationOptions;
    use qubit_local_files::policy::LocalPrivacyRequirement;
    use tempfile::TempDir;

    #[cfg(target_os = "linux")]
    const CHILD_SENTINEL: &str = "QUBIT_STAGE_REOPEN_EFFECTIVE_UID_CHILD";
    #[cfg(target_os = "linux")]
    const CHILD_MARKER: &str = "QUBIT_STAGE_REOPEN_EFFECTIVE_UID_MARKER";

    fn required_privacy() -> LocalTreeValidationOptions {
        LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required)
    }

    fn parent_authority(scratch: &TempDir) -> qubit_local_files::directory::LocalDirectoryHandle {
        LocalFileSystem::host()
            .expect("host filesystem")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent authority")
    }

    #[test]
    fn test_private_stage_retain_reopen_and_cleanup() {
        let scratch = tempfile::tempdir().expect("scratch parent");
        let parent = parent_authority(&scratch);
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
            .expect("private stage");
        let name = std::path::PathBuf::from(stage.relative_name());
        let identity = stage.identity().expect("stage identity").clone();
        drop(stage.retain_in_place());

        let reopened = parent
            .open_staged_directory(&name, &identity, &required_privacy())
            .expect("default private mode should reopen");
        assert_eq!(Some(&identity), reopened.identity());
        drop(reopened.retain_in_place());

        parent
            .open_staged_directory(&name, &identity, &required_privacy())
            .expect("reopen before cleanup")
            .cleanup()
            .expect("cleanup reopened stage");
    }

    #[test]
    fn test_reopen_rejects_replacement_identity_and_group_or_other_permissions() {
        let scratch = tempfile::tempdir().expect("scratch parent");
        let parent = parent_authority(&scratch);
        let first = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
            .expect("first private stage");
        let first_name = std::path::PathBuf::from(first.relative_name());
        let first_identity = first.identity().expect("first identity").clone();
        drop(first.retain_in_place());
        let replacement = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("replacement stage");
        let replacement_identity = replacement.identity().expect("replacement identity").clone();
        replacement.cleanup().expect("cleanup replacement stage");

        let error = parent
            .open_staged_directory(&first_name, &replacement_identity, &required_privacy())
            .expect_err("a replacement identity must be rejected");
        assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());

        fs::set_permissions(scratch.path().join(&first_name), fs::Permissions::from_mode(0o750))
            .expect("add group permission to retained stage");
        let error = parent
            .open_staged_directory(&first_name, &first_identity, &required_privacy())
            .expect_err("group/other permissions must be rejected");
        assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());

        fs::set_permissions(scratch.path().join(&first_name), fs::Permissions::from_mode(0o700))
            .expect("restore private mode");
        parent
            .open_staged_directory(&first_name, &first_identity, &required_privacy())
            .expect("restore valid stage")
            .cleanup()
            .expect("cleanup original stage");
    }

    #[test]
    fn test_reopen_uses_effective_uid_child() {
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os(CHILD_SENTINEL).is_some() {
                run_effective_uid_child();
                return;
            }

            if unsafe { libc::geteuid() } != 0 {
                eprintln!("UID-difference scenario NOT EXECUTED: test runner is not privileged");
                return;
            }

            let marker = tempfile::tempdir().expect("marker directory");
            let marker_path = marker.path().join("child-completed");
            let output = Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "unix::test_reopen_uses_effective_uid_child", "--nocapture"])
                .env(CHILD_SENTINEL, "run")
                .env(CHILD_MARKER, &marker_path)
                .output()
                .expect("launch isolated UID child");
            assert!(
                output.status.success(),
                "UID child failed: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(marker_path.exists(), "child completion marker must exist");
            eprintln!("UID-difference scenario EXECUTED: 1 isolated Linux child completed");
        }

        #[cfg(not(target_os = "linux"))]
        eprintln!("UID-difference scenario NOT EXECUTED: isolated child is Linux-only");
    }

    #[cfg(target_os = "linux")]
    fn run_effective_uid_child() {
        let marker = std::env::var_os(CHILD_MARKER).expect("child marker path");
        let scratch = tempfile::tempdir().expect("child scratch parent");
        fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o711))
            .expect("make fixture parent traversable after UID switch");

        // Keep saved UID 0 so the isolated child can restore its original identity for
        // cleanup.
        assert_eq!(
            0,
            unsafe { libc::setresuid(0, 65_534, 0) },
            "set distinct real/effective UIDs in child"
        );
        assert_eq!(0, unsafe { libc::getuid() }, "real UID should differ");
        assert_eq!(65_534, unsafe { libc::geteuid() }, "effective UID should own stage");

        let parent = LocalFileSystem::host()
            .expect("child host filesystem")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("open fixture parent as effective UID");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
            .expect("create stage as effective UID");
        let name = std::path::PathBuf::from(stage.relative_name());
        let identity = stage.identity().expect("stage identity").clone();
        drop(stage.retain_in_place());
        let reopened = parent
            .open_staged_directory(&name, &identity, &required_privacy())
            .expect("reopen must compare owner with effective UID");
        assert_eq!(Some(&identity), reopened.identity());
        reopened.cleanup().expect("cleanup stage as effective UID");

        assert_eq!(0, unsafe { libc::setresuid(0, 0, 0) }, "restore child root identity");
        fs::write(Path::new(&marker), b"completed").expect("write child completion marker");
    }
}
