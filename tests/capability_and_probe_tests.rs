// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::process::Child;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use std::time::SystemTime;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::capability::LocalPathLengthUnit;
use qubit_local_files::capability::SizeLimit;
#[cfg(unix)]
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::path::LocalFileNames;
use tempfile::tempdir;

#[cfg(unix)]
struct ProbeChild {
    child: Child,
    reaped: bool,
}

#[cfg(unix)]
impl ProbeChild {
    fn terminate_and_wait(&mut self) -> (std::io::Result<()>, std::io::Result<std::process::ExitStatus>) {
        let killed = self.child.kill();
        let waited = self.child.wait();
        self.reaped = waited.is_ok();
        (killed, waited)
    }
}

#[cfg(unix)]
impl Drop for ProbeChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }
}

/// Verifies capabilities do not misrepresent a compile-time path bound as a
/// filesystem-specific limit.
#[test]
fn test_host_file_system_limits_vary_by_path() {
    let limits = LocalFileSystem::host().expect("Host filesystem should open").limits();

    assert_eq!(SizeLimit::VariesByPath, limits.max_path_length());
    assert_eq!(SizeLimit::VariesByPath, limits.max_component_length());
    #[cfg(unix)]
    assert_eq!(LocalPathLengthUnit::Bytes, limits.length_unit());
    #[cfg(windows)]
    assert_eq!(LocalPathLengthUnit::Utf16CodeUnits, limits.length_unit());
}

/// Verifies Host path-limit probing resolves a missing descendant through its
/// nearest existing authority instead of requiring the final entry to exist.
#[test]
fn test_host_file_system_limits_probe_missing_descendant() {
    let root = tempdir().expect("temporary root should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let expected = filesystem
        .limits_at(root.path())
        .expect("existing Host path limits should be probeable");
    let limits = filesystem
        .limits_at(&root.path().join("missing/child"))
        .expect("missing Host descendants should be probeable");

    assert_eq!(expected, limits);
}

/// Verifies space observations are available without caching host limits.
#[test]
fn test_host_file_system_space_observes_existing_directory() {
    let space = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .space_at(std::env::temp_dir().as_path())
        .expect("an existing host directory should be queryable");

    #[cfg(any(unix, windows))]
    assert!(space.capacity_bytes().is_some());
    #[cfg(not(any(unix, windows)))]
    let _ = &space;
}

/// Ensures capability probes reject FIFOs without blocking on a reader open.
#[cfg(unix)]
#[test]
fn test_host_and_rooted_special_file_probe_does_not_block() {
    if let Ok(operation) = std::env::var("QUBIT_FIFO_PROBE_CHILD") {
        let path = std::env::var_os("QUBIT_FIFO_PROBE_PATH").expect("child path");
        let path = std::path::PathBuf::from(path);
        if operation == "host-space" {
            assert!(
                LocalFileSystem::host()
                    .expect("Host filesystem")
                    .space_at(&path)
                    .is_err()
            );
        } else if operation == "host-limits" {
            assert!(
                LocalFileSystem::host()
                    .expect("Host filesystem")
                    .limits_at(&path)
                    .is_err()
            );
        } else {
            let root = path.parent().expect("FIFO parent");
            let filesystem = LocalFileSystem::rooted(root).expect("root authority");
            let operand = path.file_name().expect("FIFO name");
            let operand = std::path::Path::new(operand);
            if operation == "rooted-space" {
                assert!(filesystem.space_at(operand).is_err());
            } else {
                assert!(filesystem.limits_at(operand).is_err());
            }
        }
        return;
    }

    for operation in ["host-space", "host-limits", "rooted-space", "rooted-limits"] {
        let directory = tempdir().expect("temporary directory");
        let fifo = directory.path().join("probe.fifo");
        let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path has no NUL");
        assert_eq!(0, unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) });
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("test_host_and_rooted_special_file_probe_does_not_block")
            .arg("--nocapture")
            .env("QUBIT_FIFO_PROBE_CHILD", operation)
            .env("QUBIT_FIFO_PROBE_PATH", &fifo)
            .spawn()
            .expect("probe child should start");
        let mut child = ProbeChild { child, reaped: false };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.child.try_wait().expect("child status") {
                child.reaped = true;
                assert!(status.success(), "probe failed for {operation}: {status}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                let (kill_result, wait_result) = child.terminate_and_wait();
                panic!("probe blocked for {operation}; kill={kill_result:?}, wait={wait_result:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Verifies rooted space observations use the opened authority for missing
/// descendants instead of the diagnostic root path.
#[test]
fn test_rooted_file_system_space_observes_nearest_existing_ancestor() {
    let root = tempdir().expect("temporary root should be created");
    std::fs::create_dir(root.path().join("nested")).expect("nested directory should be created");
    let filesystem = LocalFileSystem::rooted(root.path()).expect("root authority should open");
    let space = filesystem
        .space_at(std::path::Path::new("nested/missing/child"))
        .expect("missing rooted descendants should be probeable");

    #[cfg(any(unix, windows))]
    assert!(space.capacity_bytes().is_some());
    #[cfg(not(any(unix, windows)))]
    let _ = &space;
}

/// Verifies capability snapshots expose independent operation guarantees.
#[test]
fn test_host_capabilities_report_operation_support() {
    let capabilities = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .capabilities();

    assert_eq!(cfg!(any(unix, windows)), capabilities.supports_rooted_operations(),);
    assert_eq!(
        cfg!(any(target_os = "linux", target_os = "macos", windows)),
        capabilities.supports_atomic_rename(),
    );
    assert_eq!(cfg!(any(unix, windows)), capabilities.supports_atomic_replace(),);
    assert_eq!(
        cfg!(any(target_os = "linux", target_os = "macos", windows)),
        capabilities.can_attempt_atomic_temp_persist(),
    );
    assert_eq!(cfg!(unix), capabilities.supports_durable_rename());
    assert_eq!(cfg!(unix), capabilities.supports_durable_file_copy());
    assert_eq!(cfg!(unix), capabilities.supports_durable_write());
    assert_eq!(cfg!(unix), capabilities.supports_durable_temp_file_persist());
}

/// Verifies Rooted capabilities follow the separately implemented target
/// contract.
#[test]
fn test_rooted_capabilities_report_operation_support() {
    #[cfg(any(unix, windows))]
    {
        let rooted = tempdir().expect("root should be created");
        let capabilities = LocalFileSystem::rooted(rooted.path())
            .expect("root authority should open")
            .capabilities();
        assert!(capabilities.supports_rooted_operations());
        assert_eq!(
            cfg!(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios",
                windows
            )),
            capabilities.supports_atomic_rename(),
        );
        assert_eq!(cfg!(any(unix, windows)), capabilities.supports_atomic_replace(),);
        assert_eq!(
            cfg!(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios",
                windows
            )),
            capabilities.can_attempt_atomic_temp_persist(),
        );
        assert_eq!(cfg!(unix), capabilities.supports_durable_rename());
        assert_eq!(cfg!(unix), capabilities.supports_durable_file_copy());
        assert_eq!(cfg!(unix), capabilities.supports_durable_write());
        assert_eq!(cfg!(unix), capabilities.supports_durable_temp_file_persist());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let rooted = tempdir().expect("root should be created");
        assert!(LocalFileSystem::rooted(rooted.path()).is_err());
    }
}

/// Verifies the host snapshot only advertises native no-replace operations
/// implemented by the host backend on this target.
#[test]
fn test_host_capabilities_match_host_no_replace_backend() {
    let capabilities = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .capabilities();
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    assert!(capabilities.supports_atomic_rename());
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    assert!(!capabilities.supports_atomic_rename());
}

/// Verifies that generated names are portable single path components.
#[test]
fn test_local_file_names_generate_random_portable_components() {
    let names = LocalFileNames::portable();
    let first = names.random_name().expect("a random filename should be generated");
    let second = names
        .random_name_with(
            Some(std::ffi::OsStr::new("prefix-")),
            Some(std::ffi::OsStr::new(".tmp")),
        )
        .expect("a random filename with affixes should be generated");

    assert_ne!(first, second);
    let second_text = second.to_str().expect("a portable random filename should be UTF-8");
    assert!(second_text.starts_with("prefix-"));
    assert!(second_text.ends_with(".tmp"));
    names
        .validate(first.as_os_str())
        .expect("the default random filename should be portable");
    names
        .validate(second.as_os_str())
        .expect("the affixed random filename should be portable");
}

/// Verifies that metadata observes a final symbolic link without following it.
#[cfg(unix)]
#[test]
fn test_local_file_system_metadata_does_not_follow_final_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    fs::write(&target, b"payload").expect("target should be written");
    symlink(&target, &link).expect("symbolic link should be created");

    let metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&link)
        .expect("symbolic-link metadata should be available");

    assert_eq!(LocalFileKind::Symlink, metadata.kind());
    assert_eq!(
        fs::symlink_metadata(&link)
            .expect("native symlink metadata should be available")
            .len(),
        metadata.len(),
    );
    assert!(metadata.modified_at().is_some());
    let _: Option<SystemTime> = metadata.created_at();
}
