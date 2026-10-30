// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Packaging replaces path dependencies with their registry counterparts. Adopt
// only source/checksum from the parent lock for the same pinned version.
pub(crate) fn lock_with_parent_sources(consumer: &str, parent: &str) -> String {
    const PACKAGE: &str = "\n[[package]]\n";
    let mut result = consumer.to_owned();
    for entry in consumer.split(PACKAGE).skip(1) {
        let identity = entry.lines().take(2).collect::<Vec<_>>().join("\n");
        if entry.lines().any(|line| line.starts_with("source = "))
            || identity.starts_with("name = \"documentation-consumer\"\n")
            || identity.starts_with("name = \"tree-outcome-lint-consumer\"\n")
            || identity.starts_with("name = \"qubit-local-files\"\n")
        {
            continue;
        }
        let header = format!("{identity}\n");
        let Some(parent_entry) = parent.split(PACKAGE).skip(1).find(|entry| entry.starts_with(&header)) else {
            continue;
        };
        let source = parent_entry
            .lines()
            .filter(|line| line.starts_with("source = ") || line.starts_with("checksum = "))
            .collect::<Vec<_>>();
        if !source.is_empty() {
            result = result.replacen(&header, &format!("{header}{}\n", source.join("\n")), 1);
        }
    }
    result
}

#[test]
fn consumer_lock_adopts_packaged_sources_without_changing_versions_or_dependencies() {
    let consumer = "version = 4\n\n[[package]]\nname = \"qubit-local-files\"\nversion = \"0.5.0\"\ndependencies = [\"qubit-redact\"]\n\n[[package]]\nname = \"qubit-redact\"\nversion = \"0.10.0\"\ndependencies = [\"qubit-budget\"]\n";
    let source =
        "source = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"reviewed-checksum\"\n";
    let packaged = format!(
        "version = 4\n\n[[package]]\nname = \"qubit-local-files\"\nversion = \"0.5.0\"\n{source}dependencies = [\"criterion\"]\n\n[[package]]\nname = \"qubit-redact\"\nversion = \"0.10.0\"\n{source}dependencies = [\"qubit-budget\", \"other-feature\"]\n"
    );
    let expected = consumer.replace(
        "name = \"qubit-redact\"\nversion = \"0.10.0\"\n",
        &format!("name = \"qubit-redact\"\nversion = \"0.10.0\"\n{source}"),
    );
    assert_eq!(lock_with_parent_sources(consumer, &packaged), expected);
    assert_eq!(lock_with_parent_sources(consumer, consumer), consumer);
    let different_version = packaged.replace("version = \"0.10.0\"", "version = \"0.11.0\"");
    assert_eq!(lock_with_parent_sources(consumer, &different_version), consumer);
    let different_source = packaged.replace("github.com/rust-lang/crates.io-index", "example.invalid/index");
    assert_eq!(lock_with_parent_sources(&expected, &different_source), expected);
}
