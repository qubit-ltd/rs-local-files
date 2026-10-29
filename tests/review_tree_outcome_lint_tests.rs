// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn copy_fixture(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("consumer directory should be created");
    for entry in fs::read_dir(source).expect("fixture should be readable") {
        let entry = entry.expect("fixture entry should be readable");
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("fixture entry type should be readable")
            .is_dir()
        {
            copy_fixture(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("fixture file should be copied");
        }
    }
}

#[test]
fn test_tree_publish_outcome_must_use_in_external_consumer() {
    let temporary = tempdir().expect("isolated consumer workspace");
    let consumer = temporary.path().join("consumer");
    copy_fixture(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tree_outcome_lint")
            .as_path(),
        &consumer,
    );
    let manifest = consumer.join("Cargo.toml");
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_text = fs::read_to_string(&manifest).expect("consumer manifest should be readable");
    let normalized_crate_path = crate_path.display().to_string().replace('\\', "/");
    assert!(!normalized_crate_path.contains('\\'));
    fs::write(&manifest, manifest_text.replace("../../..", &normalized_crate_path))
        .expect("consumer manifest should point at this crate");
    let target = temporary.path().join("target");

    let accepted = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--no-default-features")
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .output()
        .expect("positive consumer check should start");
    assert!(
        accepted.status.success(),
        "explicitly inspecting the outcome should compile:\n{}\n{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );

    let rejected = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--no-default-features")
        .arg("--features")
        .arg("drop-result")
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .output()
        .expect("negative consumer check should start");
    let diagnostic = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        !rejected.status.success() && diagnostic.contains("unused_must_use"),
        "discarding the outcome should fail specifically for unused_must_use:\n{}\n{}",
        String::from_utf8_lossy(&rejected.stdout),
        diagnostic
    );
}
