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

const GUIDE: &str = include_str!("../doc/user_guide.md");
const START: &str = "// BEGIN executable conditional write example";
const END: &str = "// END executable conditional write example";
const SENTINEL: &str = "QUBIT_LOCAL_FILES_DOCUMENTATION_CONSUMER_SENTINEL";

#[test]
fn documented_conditional_write_runs_as_a_consumer_without_a_digest_feature() {
    let start = GUIDE.find(START).expect("consumer example start marker exists") + START.len();
    let end = GUIDE[start..]
        .find(END)
        .map(|offset| start + offset)
        .expect("consumer example end marker exists");
    let visible_code = GUIDE[start..end]
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("# ") && line != "#"
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(visible_code.contains("open_conditional_writer"));
    assert!(visible_code.contains("candidate.open_reader()"));
    assert!(visible_code.contains("assert_eq!(std::fs::read(&path)?, b\"C\")"));

    let workspace = tempdir().expect("consumer workspace is created");
    let project = workspace.path().join("consumer");
    fs::create_dir_all(project.join("src")).expect("consumer source directory is created");
    let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("the tested crate path is canonicalized");
    let dependency_path = crate_path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    fs::write(
        project.join("Cargo.toml"),
        format!(
            "[package]\nname = \"documentation-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nqubit-local-files = {{ path = \"{dependency_path}\", features = [\"digest\"] }}\nqubit-io = \"0.19\"\nsha2 = \"0.11\"\ntempfile = \"^3.27\"\n"
        ),
    )
    .expect("consumer manifest is written");

    let source = format!(
        "fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{visible_code}\nstd::fs::write(std::env::var(\"{SENTINEL}\")?, b\"conditional example completed\")?;\nOk(())\n}}\n"
    );
    fs::write(project.join("src/main.rs"), source).expect("consumer source is written");

    let sentinel = workspace.path().join("completed.sentinel");
    let status = Command::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg(project.join("Cargo.toml"))
        .current_dir(&project)
        .env("CARGO_TARGET_DIR", project.join("target"))
        .env(SENTINEL, &sentinel)
        .output()
        .expect("consumer cargo process starts");

    assert!(
        status.status.success(),
        "consumer failed with {}\nstdout:\n{}\nstderr:\n{}",
        status.status,
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(
        fs::read(&sentinel).expect("consumer completion sentinel exists"),
        b"conditional example completed"
    );
}
