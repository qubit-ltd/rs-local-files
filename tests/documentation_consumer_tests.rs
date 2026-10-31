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
const SENTINEL: &str = "QUBIT_LOCAL_FILES_DOCUMENTATION_CONSUMER_SENTINEL";
// Regenerate this fixture from the documented dependencies when they change.
// Both consumers have the same package identity and use this reviewed graph.
const CONSUMER_LOCK: &str = include_str!("fixtures/documentation_consumer/Cargo.lock");

#[test]
fn documented_conditional_write_runs_as_a_consumer_without_a_digest_feature() {
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
        let dependency_path = crate_path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
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
        assert!(
            status.status.success(),
            "{language} consumer failed with {}\nstdout:\n{}\nstderr:\n{}",
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
            b"conditional example completed"
        );
    }
}

#[test]
fn documented_failed_seal_cleanup_retains_owner_for_retry() {
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
        let dependency_path = crate_path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
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
