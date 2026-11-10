// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::MutexGuard;

use tempfile::tempdir;

mod support {
    pub(crate) mod consumer_lock;
}

use support::consumer_lock;

const GUIDES: [(&str, &str); 2] = [
    ("English", include_str!("../doc/user_guide.md")),
    ("中文", include_str!("../doc/user_guide.zh_CN.md")),
];
const MANIFEST_START: &str = "# BEGIN executable application dependencies";
const MANIFEST_END: &str = "# END executable application dependencies";
const START: &str = "// BEGIN executable conditional write example";
const END: &str = "// END executable conditional write example";
const SEAL_START: &str = "// BEGIN executable seal cleanup helper";
const SEAL_END: &str = "// END executable seal cleanup helper";
const INSPECT_START: &str = "// BEGIN executable inspection cleanup helper";
const INSPECT_END: &str = "// END executable inspection cleanup helper";
const INTRO_DEPS_START: &str = "# BEGIN guide introductory dependencies";
const INTRO_DEPS_END: &str = "# END guide introductory dependencies";
const TREE_SYNC_START: &str = "// BEGIN executable tree sync example";
const TREE_SYNC_END: &str = "// END executable tree sync example";
const EXPORT_START: &str = "// BEGIN executable export example";
const EXPORT_END: &str = "// END executable export example";
#[cfg(target_os = "linux")]
const READONLY_RECOVERY_START: &str = "// BEGIN executable readonly stage recovery example";
#[cfg(target_os = "linux")]
const READONLY_RECOVERY_END: &str = "// END executable readonly stage recovery example";
const SENTINEL: &str = "QUBIT_LOCAL_FILES_DOCUMENTATION_CONSUMER_SENTINEL";
// Regenerate this fixture from the documented dependencies when they change.
// Both consumers have the same package identity and use this reviewed graph.
const CONSUMER_LOCK: &str = include_str!("fixtures/documentation_consumer/Cargo.lock");
const INTRO_CONSUMER_LOCK: &str = include_str!("fixtures/documentation_consumer/introductory.Cargo.lock");

// These tests invoke nested Cargo builds. Keeping them in one process-wide
// critical section avoids competing consumer builds and their temporary output
// trees when the test harness runs this integration binary in parallel.
static DOCUMENTATION_CONSUMER_LOCK: Mutex<()> = Mutex::new(());

fn lock_documentation_consumer() -> MutexGuard<'static, ()> {
    DOCUMENTATION_CONSUMER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn cargo_manifest_path(path: &str) -> String {
    let path = path
        .strip_prefix(r"\\?\UNC\")
        .map(|unc_path| format!(r"\\{unc_path}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(str::to_owned))
        .unwrap_or_else(|| path.to_owned());
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

fn diagnose_consumer_lock_update(project: &Path) -> String {
    fn entries(lock: &str) -> Vec<String> {
        lock.split("\n[[package]]\n")
            .skip(1)
            .map(|entry| {
                let identity = entry.lines().take(3).collect::<Vec<_>>().join(" ");
                let dependencies = entry
                    .find("dependencies = [")
                    .and_then(|start| entry[start..].find(']').map(|end| &entry[start..start + end + 1]))
                    .unwrap_or("dependencies = []");
                format!("{identity} {dependencies}")
            })
            .collect()
    }

    let lock_path = project.join("Cargo.lock");
    let before = fs::read_to_string(&lock_path).unwrap_or_default();
    let result = Command::new("cargo")
        .arg("metadata")
        .arg("--offline")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(project.join("Cargo.toml"))
        .current_dir(project)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET")
        .output();
    let after = fs::read_to_string(&lock_path).unwrap_or_default();
    let result = match result {
        Ok(result) => result,
        Err(error) => return format!("cargo metadata diagnostic could not start: {error}"),
    };
    let before_entries = entries(&before);
    let after_entries = entries(&after);
    let changes = before_entries
        .iter()
        .filter(|entry| !after_entries.contains(entry))
        .map(|entry| format!("- {entry}"))
        .chain(
            after_entries
                .iter()
                .filter(|entry| !before_entries.contains(entry))
                .map(|entry| format!("+ {entry}")),
        )
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "cargo metadata --offline exited {}; lock entry changes:\n{}\nstderr:\n{}",
        result.status,
        if changes.is_empty() { "<none>" } else { &changes },
        String::from_utf8_lossy(&result.stderr)
    )
}

#[test]
fn test_cargo_manifest_path_normalizes_windows_extended_paths() {
    assert_eq!(cargo_manifest_path(r"\\?\D:\repo\crate"), r"D:\\repo\\crate");
    assert_eq!(
        cargo_manifest_path(r"\\?\UNC\server\share\crate"),
        r"\\\\server\\share\\crate"
    );
    assert_eq!(cargo_manifest_path("/workspace/crate"), "/workspace/crate");
}

#[test]
fn test_documented_conditional_write_runs_as_a_consumer_without_a_digest_feature() {
    let _guard = lock_documentation_consumer();
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let workspace = tempdir().expect("consumer workspace is created");
    let target = workspace.path().join("target");
    let parent_lock = fs::read_to_string(crate_path.join("Cargo.lock")).expect("the tested crate has a lockfile");
    let consumer_lock = consumer_lock::lock_with_parent_sources(CONSUMER_LOCK, &parent_lock);
    for (index, (language, guide)) in GUIDES.into_iter().enumerate() {
        let dependencies = extract_marked(guide, MANIFEST_START, MANIFEST_END, language);
        let dependency_line = dependencies
            .lines()
            .find(|line| line.trim_start().starts_with("qubit-local-files ="))
            .expect("marked dependencies contain qubit-local-files");
        assert_eq!(
            dependencies
                .lines()
                .filter(|line| line.trim_start().starts_with("qubit-local-files ="))
                .count(),
            1,
            "{language} dependency block has one qubit-local-files entry"
        );
        assert!(dependency_line.contains("features = [\"digest\"]"));
        let dependency_path = cargo_manifest_path(&crate_path.to_string_lossy());
        let dependencies = dependencies.replace(
            dependency_line,
            &dependency_line.replace("version = \"0.5.0\"", &format!("path = \"{dependency_path}\"")),
        );
        let visible_code = extract_marked(guide, START, END, language)
            .lines()
            .filter(|line| {
                let line = line.trim_start();
                !line.starts_with("# ") && line != "#"
            })
            .filter(|line| !line.starts_with("// BEGIN ") && !line.starts_with("// END "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible_code.contains("open_conditional_writer"));
        assert!(visible_code.contains("candidate.open_reader()"));
        assert!(visible_code.contains("assert_eq!(std::fs::read(&path)?, b\"C\")"));

        let project = workspace.path().join(format!("consumer-{index}"));
        fs::create_dir_all(project.join("src")).expect("consumer source directory is created");
        let manifest = format!(
            "[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"
        );
        fs::write(project.join("Cargo.toml"), manifest).expect("consumer manifest is written");
        fs::write(project.join("Cargo.lock"), &consumer_lock).expect("reviewed consumer lock is written");
        let source = format!(
            "fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{visible_code}\nstd::fs::write(std::env::var(\"{SENTINEL}\")?, b\"conditional example completed\")?;\nOk(())\n}}\n"
        );
        fs::write(project.join("src/main.rs"), source).expect("consumer source is written");

        let sentinel = workspace.path().join(format!("completed-{index}.sentinel"));
        let status = Command::new("cargo")
            .arg("run")
            .arg("--frozen")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            // CI sanitizer suites set rustc flags for the parent target. The
            // consumer is a fresh host build and must not inherit them.
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env(SENTINEL, &sentinel)
            .output()
            .expect("consumer cargo process starts");
        if !status.status.success() {
            panic!(
                "{language} consumer failed with {}\nstdout:\n{}\nstderr:\n{}\nLock resolution diagnostic:\n{}",
                status.status,
                String::from_utf8_lossy(&status.stdout),
                String::from_utf8_lossy(&status.stderr),
                diagnose_consumer_lock_update(&project)
            );
        }
        assert_eq!(
            fs::read_to_string(project.join("Cargo.lock")).expect("consumer lock remains readable"),
            consumer_lock,
            "{language} consumer must not change the reviewed dependency graph"
        );
        assert_eq!(
            fs::read(&sentinel).expect("consumer completion sentinel exists"),
            b"conditional example completed"
        );
    }
}

#[test]
fn test_documented_failed_seal_cleanup_retains_owner_for_retry() {
    let _guard = lock_documentation_consumer();
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let workspace = tempdir().expect("consumer workspace is created");
    let target = workspace.path().join("target");
    let parent_lock = fs::read_to_string(crate_path.join("Cargo.lock")).expect("the tested crate has a lockfile");
    let consumer_lock = consumer_lock::lock_with_parent_sources(CONSUMER_LOCK, &parent_lock);

    for (index, (language, guide)) in GUIDES.into_iter().enumerate() {
        let dependencies = extract_marked(guide, MANIFEST_START, MANIFEST_END, language);
        let dependency_line = dependencies
            .lines()
            .find(|line| line.trim_start().starts_with("qubit-local-files ="))
            .expect("marked dependencies contain qubit-local-files");
        let dependency_path = cargo_manifest_path(&crate_path.to_string_lossy());
        let dependencies = dependencies.replace(
            dependency_line,
            &dependency_line
                .replace("version = \"0.5.0\"", &format!("path = \"{dependency_path}\""))
                .replace("features = [\"digest\"]", "features = [\"digest\", \"test-support\"]"),
        );
        let helper = extract_marked(guide, SEAL_START, SEAL_END, language);
        let project = workspace.path().join(format!("seal-consumer-{index}"));
        fs::create_dir_all(project.join("src")).expect("consumer source directory is created");
        let manifest = format!(
            "[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"
        );
        fs::write(project.join("Cargo.toml"), manifest).expect("consumer manifest is written");
        fs::write(project.join("Cargo.lock"), &consumer_lock).expect("reviewed consumer lock is written");
        let source = format!(
            r#"{helper}
fn main() -> Result<(), Box<dyn std::error::Error>> {{
    use std::path::Path;
    use std::path::PathBuf;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::outcome::LocalTempSourceState;
    use qubit_local_files::test_support::install_test_fault;
    use tempfile::tempdir;

    for rooted in [false, true] {{
        let parent = tempdir()?;
        let filesystem = if rooted {{ LocalFileSystem::rooted(parent.path())? }} else {{ LocalFileSystem::host()? }};
        let requested_parent = if rooted {{ Path::new("") }} else {{ parent.path() }};
        let mut temporary = filesystem.create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(requested_parent))?;
        let resource_native = if rooted {{
            parent.path().join(temporary.path().strip_prefix(std::path::MAIN_SEPARATOR_STR)?)
        }} else {{ temporary.path().to_path_buf() }};
        let native_sandbox: PathBuf = resource_native.parent().expect("sandbox parent exists").to_path_buf();
        temporary.close();
        let failure = temporary.seal_with_options(&qubit_local_files::LocalSealOptions::new())
            .expect_err("a closed writer must fail to seal");

        cleanup_failed_seal(failure).expect("cleanup succeeds without a fault");
        assert!(!native_sandbox.exists());

        let mut temporary = filesystem.create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(requested_parent))?;
        let resource_native = if rooted {{
            parent.path().join(temporary.path().strip_prefix(std::path::MAIN_SEPARATOR_STR)?)
        }} else {{ temporary.path().to_path_buf() }};
        let native_sandbox: PathBuf = resource_native.parent().expect("sandbox parent exists").to_path_buf();
        temporary.close();
        let failure = temporary.seal_with_options(&qubit_local_files::LocalSealOptions::new())
            .expect_err("a closed writer must fail to seal");
        let fault = install_test_fault("temp-file-sandbox-remove")?;
        let pending = cleanup_failed_seal(failure).expect_err("sandbox removal should fail");
        assert_eq!(pending.cause.operation(), qubit_local_files::error::LocalFileOperation::Cleanup);
        assert_eq!(pending.resource.source_state(), LocalTempSourceState::CleanupRequired);
        assert!(native_sandbox.exists());
        assert!(
            std::error::Error::source(&*pending)
                .expect("the application error exposes its cleanup cause")
                .is::<qubit_local_files::LocalFileError>()
        );
        drop(fault);
        let fault = install_test_fault("temp-file-sandbox-remove")?;
        let pending = retry_pending_seal_cleanup(pending as Box<dyn std::error::Error>)
            .expect_err("the second sandbox removal should fail");
        let mut pending = pending
            .downcast::<PendingSealCleanup>()
            .expect("a failed retry must still own the resource");
        assert_eq!(pending.cause.operation(), qubit_local_files::error::LocalFileOperation::Cleanup);
        assert_eq!(pending.resource.source_state(), LocalTempSourceState::CleanupRequired);
        assert!(native_sandbox.exists());
        assert!(
            std::error::Error::source(&*pending)
                .expect("the retry error exposes its updated cleanup cause")
                .is::<qubit_local_files::LocalFileError>()
        );
        drop(fault);
        pending.resource.cleanup()?;
        assert!(!native_sandbox.exists());
        assert_eq!(pending.resource.source_state(), LocalTempSourceState::Released);
    }}
    std::fs::write(std::env::var("{SENTINEL}")?, b"seal cleanup helper completed")?;
    Ok(())
}}
"#
        );
        fs::write(project.join("src/main.rs"), source).expect("consumer source is written");
        let sentinel = workspace.path().join(format!("seal-completed-{index}.sentinel"));
        let status = Command::new("cargo")
            .arg("run")
            .arg("--frozen")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env(SENTINEL, &sentinel)
            .output()
            .expect("consumer cargo process starts");
        assert!(
            status.status.success(),
            "{language} seal consumer failed with {}\nstdout:\n{}\nstderr:\n{}",
            status.status,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(
            fs::read_to_string(project.join("Cargo.lock")).expect("consumer lock remains readable"),
            consumer_lock,
            "{language} consumer must not change the reviewed dependency graph"
        );
        assert_eq!(
            fs::read(&sentinel).expect("consumer completion sentinel exists"),
            b"seal cleanup helper completed"
        );
    }
}

#[test]
fn test_documented_inspection_cleanup_retains_owner_for_retry() {
    let _guard = lock_documentation_consumer();
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let workspace = tempdir().expect("consumer workspace is created");
    let target = workspace.path().join("target");
    let parent_lock = fs::read_to_string(crate_path.join("Cargo.lock")).expect("the tested crate has a lockfile");
    let consumer_lock = consumer_lock::lock_with_parent_sources(CONSUMER_LOCK, &parent_lock);

    for (index, (language, guide)) in GUIDES.into_iter().enumerate() {
        let dependencies = extract_marked(guide, MANIFEST_START, MANIFEST_END, language);
        let dependency_line = dependencies
            .lines()
            .find(|line| line.trim_start().starts_with("qubit-local-files ="))
            .expect("marked dependencies contain qubit-local-files");
        let dependency_path = cargo_manifest_path(&crate_path.to_string_lossy());
        let dependencies = dependencies.replace(
            dependency_line,
            &dependency_line
                .replace("version = \"0.5.0\"", &format!("path = \"{dependency_path}\""))
                .replace("features = [\"digest\"]", "features = [\"digest\", \"test-support\"]"),
        );
        let helper = extract_marked(guide, INSPECT_START, INSPECT_END, language);
        let project = workspace.path().join(format!("inspect-consumer-{index}"));
        fs::create_dir_all(project.join("src")).expect("consumer source directory is created");
        let manifest = format!(
            "[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"
        );
        fs::write(project.join("Cargo.toml"), manifest).expect("consumer manifest is written");
        fs::write(project.join("Cargo.lock"), &consumer_lock).expect("reviewed consumer lock is written");
        let source = format!(
            r#"{helper}
fn main() -> Result<(), Box<dyn std::error::Error>> {{
    use std::io;

    use qubit_local_files::test_support::install_test_fault;

    let mut inspection_path_was_present = false;
    assert_eq!(inspect_staged(b"payload", |path| {{
        inspection_path_was_present = path.exists();
        std::fs::read(path)
    }}).expect("successful inspection"), b"payload");
    assert!(inspection_path_was_present);

    let mut failure = inspect_staged::<()>(b"payload", |_| Err(io::Error::other("inspect failed")))
        .expect_err("inspection failure is retained");
    assert_eq!(failure.primary.as_ref().unwrap().to_string(), "inspect failed");
    assert!(failure.pending.is_none());

    let mut native_sandbox = None;
    let mut failure = {{
        let fault = install_test_fault("temp-file-sandbox-remove")?;
        let result = inspect_staged(b"payload", |path| {{
            native_sandbox = path.parent().map(std::path::Path::to_path_buf);
            Ok(())
        }}).expect_err("cleanup fault is retained after successful inspection");
        drop(fault);
        result
    }};
    assert!(failure.primary.is_none());
    assert_eq!(failure.local.as_ref().unwrap().operation(), qubit_local_files::error::LocalFileOperation::Cleanup);
    assert!(failure.pending.is_some());
    let sandbox = native_sandbox.expect("sandbox path observed");
    assert!(sandbox.exists());
    assert!(failure.retry_cleanup());
    assert!(!sandbox.exists());

    let mut native_sandbox = None;
    let mut failure = {{
        let fault = install_test_fault("temp-file-sandbox-remove")?;
        let result = inspect_staged::<()>(b"payload", |path| {{
            native_sandbox = path.parent().map(std::path::Path::to_path_buf);
            Err(io::Error::other("inspect primary"))
        }}).expect_err("cleanup fault retains its owner");
        drop(fault);
        result
    }};
    assert_eq!(failure.primary.as_ref().unwrap().to_string(), "inspect primary");
    assert_eq!(failure.local.as_ref().unwrap().operation(), qubit_local_files::error::LocalFileOperation::Cleanup);
    assert!(failure.pending.is_some());
    let sandbox = native_sandbox.expect("sandbox path observed");
    assert!(sandbox.exists());
    {{
        let fault = install_test_fault("temp-file-sandbox-remove")?;
        assert!(!failure.retry_cleanup());
        drop(fault);
    }}
    assert!(failure.pending.is_some());
    assert_eq!(failure.primary.as_ref().unwrap().to_string(), "inspect primary");
    assert!(failure.retry_cleanup());
    assert!(failure.pending.is_none());
    assert!(!sandbox.exists());
    assert_eq!(failure.primary.as_ref().unwrap().to_string(), "inspect primary");

    let mut inspection_ran = false;
    let mut failure = {{
        let fault = install_test_fault("temp-identity-file-cleanup")?;
        let result = inspect_staged::<()>(b"payload", |_| {{
            inspection_ran = true;
            Ok(())
        }}).expect_err("creation failure retains its recovery owner");
        drop(fault);
        result
    }};
    assert!(!inspection_ran);
    let creation_error = failure.local.as_ref().expect("creation error is retained");
    assert_eq!(creation_error.operation(), qubit_local_files::error::LocalFileOperation::CreateTempFile);
    let creation_error_text = creation_error.to_string();
    let historical_cleanup = creation_error.cleanup_error().expect("creation error retains its historical cleanup error").to_string();
    assert!(failure.pending.is_none());
    assert!(failure.creation_cleanup_error.is_none());
    let recovery = failure.creation_recovery.as_ref().expect("creation recovery owner is retained");
    let recovery_path = recovery.path().parent().expect("recovery file has a sandbox").to_path_buf();
    let blocker = recovery_path.join("inspection-cleanup-test-blocker");
    std::fs::write(&blocker, b"owned by this test")?;
    assert!(!failure.retry_cleanup(), "a nonempty sandbox still needs cleanup");
    assert!(failure.creation_recovery.is_some(), "failed retry retains its owner");
    assert_eq!(failure.creation_cleanup_error.as_ref().unwrap().operation(), qubit_local_files::error::LocalFileOperation::Cleanup);
    assert_ne!(failure.creation_cleanup_error.as_ref().unwrap().to_string(), historical_cleanup);
    assert_eq!(failure.local.as_ref().unwrap().operation(), qubit_local_files::error::LocalFileOperation::CreateTempFile);
    assert_eq!(failure.local.as_ref().unwrap().to_string(), creation_error_text);
    assert_eq!(failure.local.as_ref().unwrap().cleanup_error().unwrap().to_string(), historical_cleanup);
    assert!(blocker.exists());
    std::fs::remove_file(&blocker)?;
    assert!(failure.retry_cleanup());
    assert!(failure.creation_recovery.is_none());
    assert!(failure.creation_cleanup_error.is_none());
    assert_eq!(failure.local.as_ref().unwrap().operation(), qubit_local_files::error::LocalFileOperation::CreateTempFile);
    assert_eq!(failure.local.as_ref().unwrap().to_string(), creation_error_text);
    assert_eq!(failure.local.as_ref().unwrap().cleanup_error().unwrap().to_string(), historical_cleanup);
    assert!(!recovery_path.exists());
    assert!(failure.retry_cleanup(), "retry is idempotent after recovery");
    std::fs::write(std::env::var("{SENTINEL}")?, b"inspection cleanup completed")?;
    Ok(())
}}
"#
        );
        fs::write(project.join("src/main.rs"), source).expect("consumer source is written");
        let sentinel = workspace.path().join(format!("inspect-completed-{index}.sentinel"));
        let status = Command::new("cargo")
            .arg("run")
            .arg("--frozen")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env(SENTINEL, &sentinel)
            .output()
            .expect("consumer cargo process starts");
        assert!(
            status.status.success(),
            "{language} inspection consumer failed with {}\nstdout:\n{}\nstderr:\n{}",
            status.status,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(
            fs::read_to_string(project.join("Cargo.lock")).expect("consumer lock remains readable"),
            consumer_lock
        );
        assert_eq!(
            fs::read(&sentinel).expect("consumer completion sentinel exists"),
            b"inspection cleanup completed"
        );
    }
}

#[test]
fn test_documented_tree_sync_compiles_with_introductory_dependencies() {
    let _guard = lock_documentation_consumer();
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let workspace = tempdir().expect("consumer workspace is created");
    let target = workspace.path().join("target");
    let parent_lock = fs::read_to_string(crate_path.join("Cargo.lock")).expect("the tested crate has a lockfile");
    let consumer_lock = consumer_lock::lock_with_parent_sources(INTRO_CONSUMER_LOCK, &parent_lock);

    for (index, (language, guide)) in GUIDES.into_iter().enumerate() {
        let dependencies = extract_marked(guide, INTRO_DEPS_START, INTRO_DEPS_END, language);
        let dependency_line = dependencies
            .lines()
            .find(|line| line.trim_start().starts_with("qubit-local-files ="))
            .expect("introductory dependencies contain qubit-local-files");
        let dependency_path = cargo_manifest_path(&crate_path.to_string_lossy());
        let dependencies = dependencies.replace(
            dependency_line,
            &format!("qubit-local-files = {{ path = \"{dependency_path}\" }}"),
        );
        let code = extract_marked(guide, TREE_SYNC_START, TREE_SYNC_END, language);
        assert!(code.contains("sync_tree_with_options"));
        let project = workspace.path().join(format!("tree-sync-consumer-{index}"));
        fs::create_dir_all(project.join("src")).expect("consumer source directory is created");
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"),
        )
        .expect("consumer manifest is written");
        fs::write(project.join("Cargo.lock"), &consumer_lock).expect("reviewed consumer lock is written");
        fs::write(
            project.join("src/main.rs"),
            format!("fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{code}\nOk(())\n}}\n"),
        )
        .expect("consumer source is written");
        let status = Command::new("cargo")
            .arg("check")
            .arg("--frozen")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .output()
            .expect("consumer cargo process starts");
        if !status.status.success() {
            panic!(
                "{language} tree sync consumer failed with {}\nstdout:\n{}\nstderr:\n{}",
                status.status,
                String::from_utf8_lossy(&status.stdout),
                String::from_utf8_lossy(&status.stderr)
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn test_documented_readonly_stage_recovery_preserves_destination() {
    let _guard = lock_documentation_consumer();
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let workspace = tempdir().expect("consumer workspace is created");
    let target = workspace.path().join("target");
    let parent_lock = fs::read_to_string(crate_path.join("Cargo.lock")).expect("the tested crate has a lockfile");
    let consumer_lock = consumer_lock::lock_with_parent_sources(INTRO_CONSUMER_LOCK, &parent_lock);

    for (index, (language, guide)) in GUIDES.into_iter().enumerate() {
        let dependencies = extract_marked(guide, INTRO_DEPS_START, INTRO_DEPS_END, language);
        let dependency_line = dependencies
            .lines()
            .find(|line| line.trim_start().starts_with("qubit-local-files ="))
            .expect("introductory dependencies contain qubit-local-files");
        let dependency_path = cargo_manifest_path(&crate_path.to_string_lossy());
        let dependencies = dependencies.replace(
            dependency_line,
            &format!("qubit-local-files = {{ path = \"{dependency_path}\" }}"),
        );
        let code = extract_marked(guide, READONLY_RECOVERY_START, READONLY_RECOVERY_END, language)
            .lines()
            .filter(|line| !line.trim_start().starts_with("# "))
            .collect::<Vec<_>>()
            .join("\n");
        let project = workspace.path().join(format!("readonly-recovery-consumer-{index}"));
        let sentinel = project.join("consumer.sentinel");
        let sentinel_path = sentinel.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        fs::create_dir_all(project.join("src")).expect("consumer source directory is created");
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"),
        )
        .expect("consumer manifest is written");
        fs::write(project.join("Cargo.lock"), &consumer_lock).expect("reviewed consumer lock is written");
        fs::write(
            project.join("src/main.rs"),
            format!("fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{code}\nstd::fs::write(\"{sentinel_path}\", b\"readonly stage recovery completed\")?;\nOk(())\n}}\n"),
        )
        .expect("consumer source is written");
        let status = Command::new("cargo")
            .arg("run")
            .arg("--frozen")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove(SENTINEL)
            .output()
            .expect("consumer cargo process starts");
        assert!(
            status.status.success(),
            "{language} read-only recovery consumer failed with {}\nstdout:\n{}\nstderr:\n{}",
            status.status,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(
            fs::read(&sentinel).expect("consumer completion sentinel exists"),
            b"readonly stage recovery completed"
        );
    }
}

#[test]
fn test_documented_export_runs_twice_in_existing_output_directory() {
    let _guard = lock_documentation_consumer();
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let workspace = tempdir().expect("consumer workspace is created");
    let target = workspace.path().join("target");
    let parent_lock = fs::read_to_string(crate_path.join("Cargo.lock")).expect("the tested crate has a lockfile");
    let consumer_lock = consumer_lock::lock_with_parent_sources(INTRO_CONSUMER_LOCK, &parent_lock);
    let mut failures = Vec::new();

    for (index, (language, guide)) in GUIDES.into_iter().enumerate() {
        let dependencies = extract_marked(guide, INTRO_DEPS_START, INTRO_DEPS_END, language);
        let dependency_line = dependencies
            .lines()
            .find(|line| line.trim_start().starts_with("qubit-local-files ="))
            .expect("introductory dependencies contain qubit-local-files");
        let dependency_path = cargo_manifest_path(&crate_path.to_string_lossy());
        let dependencies = dependencies.replace(
            dependency_line,
            &format!("qubit-local-files = {{ path = \"{dependency_path}\" }}"),
        );
        let code = extract_marked(guide, EXPORT_START, EXPORT_END, language)
            .lines()
            .filter(|line| {
                let line = line.trim_start();
                !line.starts_with("# ") && line != "#"
            })
            .filter(|line| !line.starts_with("// BEGIN ") && !line.starts_with("// END "))
            .collect::<Vec<_>>()
            .join("\n");
        let project = workspace.path().join(format!("export-consumer-{index}"));
        let source_directory = project.join("src");
        fs::create_dir_all(&source_directory).expect("consumer source directory is created");
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"),
        )
        .expect("consumer manifest is written");
        fs::write(project.join("Cargo.lock"), &consumer_lock).expect("reviewed consumer lock is written");
        fs::write(
            source_directory.join("main.rs"),
            format!("fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{code}\nOk(())\n}}\n"),
        )
        .expect("consumer source is written");
        let status = Command::new("cargo")
            .arg("build")
            .arg("--frozen")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .output()
            .expect("consumer cargo process starts");
        if !status.status.success() {
            failures.push(format!(
                "{language} export consumer failed to build with {}\nstdout:\n{}\nstderr:\n{}",
                status.status,
                String::from_utf8_lossy(&status.stdout),
                String::from_utf8_lossy(&status.stderr)
            ));
            continue;
        }
        let executable = target
            .join("debug")
            .join(format!("documentation-consumer{}", std::env::consts::EXE_SUFFIX));
        let run_directory = workspace.path().join(format!("export-run-{index}"));
        fs::create_dir_all(&run_directory).expect("export run directory is created");
        for run in 0..2 {
            let manifest = run_directory.join("build/output/manifest.json");
            if run == 1 {
                fs::write(&manifest, b"old export").expect("previous export is seeded");
            }
            let output = Command::new(&executable)
                .current_dir(&run_directory)
                .output()
                .expect("documented export starts");
            if !output.status.success() {
                failures.push(format!(
                    "{language} export run {run} failed with {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            match fs::read(&manifest) {
                Ok(contents) if contents == br#"{"complete":true}"# => {}
                Ok(contents) => failures.push(format!(
                    "{language} export run {run} published unexpected manifest: {contents:?}"
                )),
                Err(error) => failures.push(format!(
                    "{language} export run {run} did not publish a manifest: {error}"
                )),
            }
        }
        assert_eq!(
            fs::read_to_string(project.join("Cargo.lock")).expect("consumer lock remains readable"),
            consumer_lock,
            "{language} consumer must not change the reviewed dependency graph"
        );
    }
    assert!(
        failures.is_empty(),
        "documented export failures:\n{}",
        failures.join("\n")
    );
}

fn extract_marked<'a>(text: &'a str, start: &str, end: &str, language: &str) -> &'a str {
    let start = text
        .find(start)
        .unwrap_or_else(|| panic!("{language} guide start marker `{start}` exists"))
        + start.len();
    let end = text[start..]
        .find(end)
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("{language} guide end marker `{end}` exists"));
    text[start..end].trim()
}
