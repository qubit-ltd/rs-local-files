// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Keep path package dependency graphs in sync with the parent lock while
// adopting registry source/checksum data for the same pinned version.
#[must_use]
pub(crate) fn lock_with_parent_sources(consumer: &str, parent: &str) -> String {
    const PACKAGE: &str = "\n[[package]]\n";
    let mut entries = consumer.split(PACKAGE).map(str::to_owned).collect::<Vec<_>>();
    for entry in entries.iter_mut().skip(1) {
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
        if let (Some(consumer_dependencies), Some(parent_dependencies)) = (
            lock_array(entry, "dependencies"),
            lock_array(parent_entry, "dependencies"),
        ) {
            *entry = entry.replacen(consumer_dependencies, parent_dependencies, 1);
        }
        let source = parent_entry
            .lines()
            .filter(|line| line.starts_with("source = ") || line.starts_with("checksum = "))
            .collect::<Vec<_>>();
        if !source.is_empty() {
            *entry = entry.replacen(&header, &format!("{header}{}\n", source.join("\n")), 1);
        }
    }
    loop {
        let dependency_references = entries
            .iter()
            .filter_map(|entry| lock_array(entry, "dependencies"))
            .flat_map(|dependencies| {
                let start = dependencies.find('[').expect("lock array starts") + 1;
                let end = dependencies.rfind(']').expect("lock array ends");
                dependencies[start..end]
                    .split(',')
                    .map(str::trim)
                    .filter_map(|dependency| dependency.strip_prefix('"'))
                    .filter_map(|dependency| dependency.strip_suffix('"'))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let previous_len = entries.len();
        entries.retain(|entry| {
            let mut lines = entry.lines();
            let Some(name) = lines
                .next()
                .and_then(|line| line.strip_prefix("name = \""))
                .and_then(|line| line.strip_suffix('"'))
            else {
                return true;
            };
            let Some(version) = lines
                .next()
                .and_then(|line| line.strip_prefix("version = \""))
                .and_then(|line| line.strip_suffix('"'))
            else {
                return true;
            };
            if !entry.lines().any(|line| line.starts_with("source = ")) {
                return true;
            }
            let same_name_exists_in_parent = parent
                .split(PACKAGE)
                .skip(1)
                .any(|parent_entry| parent_entry.starts_with(&format!("name = \"{name}\"\n")));
            let exact_package_exists_in_parent = parent
                .split(PACKAGE)
                .skip(1)
                .any(|parent_entry| parent_entry.starts_with(&format!("name = \"{name}\"\nversion = \"{version}\"\n")));
            exact_package_exists_in_parent
                || !same_name_exists_in_parent
                || dependency_references.iter().any(|dependency| {
                    dependency == &format!("{name} {version}")
                        || dependency == name
                            && parent.split(PACKAGE).skip(1).any(|parent_entry| {
                                parent_entry.starts_with(&format!("name = \"{name}\"\nversion = \"{version}\"\n"))
                            })
                })
        });
        if entries.len() == previous_len {
            break;
        }
    }
    entries.join(PACKAGE)
}

fn lock_array<'a>(entry: &'a str, name: &str) -> Option<&'a str> {
    let start = entry.find(&format!("{name} = ["))?;
    let tail = &entry[start..];
    let end = tail.find(']')? + 1;
    Some(&tail[..end])
}

#[test]
fn test_consumer_lock_adopts_path_dependencies_and_packaged_sources_for_same_versions() {
    let base_consumer = "version = 4\n\n[[package]]\nname = \"qubit-local-files\"\nversion = \"0.5.0\"\ndependencies = [\"qubit-redact\"]\n\n[[package]]\nname = \"qubit-redact\"\nversion = \"0.10.0\"\ndependencies = [\"qubit-budget 0.9.0\"]\n";
    let source =
        "source = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"reviewed-checksum\"\n";
    let base_packaged = format!(
        "version = 4\n\n[[package]]\nname = \"qubit-local-files\"\nversion = \"0.5.0\"\n{source}dependencies = [\"criterion\"]\n\n[[package]]\nname = \"qubit-redact\"\nversion = \"0.10.0\"\n{source}dependencies = [\"qubit-budget\"]\n"
    );
    let obsolete_consumer = format!(
        "\n[[package]]\nname = \"obsolete-a\"\nversion = \"0.1.0\"\n{source}dependencies = [\"obsolete-b 0.1.0\"]\n\n[[package]]\nname = \"obsolete-b\"\nversion = \"0.1.0\"\n{source}dependencies = []\n"
    );
    let obsolete_parent = format!(
        "\n[[package]]\nname = \"obsolete-a\"\nversion = \"0.2.0\"\n{source}dependencies = []\n\n[[package]]\nname = \"obsolete-b\"\nversion = \"0.2.0\"\n{source}dependencies = []\n"
    );
    let consumer = format!("{base_consumer}{obsolete_consumer}");
    let packaged = format!("{base_packaged}{obsolete_parent}");
    let expected = base_consumer
        .replace(
            "name = \"qubit-redact\"\nversion = \"0.10.0\"\n",
            &format!("name = \"qubit-redact\"\nversion = \"0.10.0\"\n{source}"),
        )
        .replace(
            "dependencies = [\"qubit-budget 0.9.0\"]",
            "dependencies = [\"qubit-budget\"]",
        );
    assert_eq!(lock_with_parent_sources(&consumer, &packaged), expected);
    assert_eq!(lock_with_parent_sources(&consumer, &consumer), consumer);
    let different_version = base_packaged.replace("version = \"0.10.0\"", "version = \"0.11.0\"");
    assert_eq!(
        lock_with_parent_sources(base_consumer, &different_version),
        base_consumer
    );
    let different_source = base_packaged.replace("github.com/rust-lang/crates.io-index", "example.invalid/index");
    assert_eq!(lock_with_parent_sources(&expected, &different_source), expected);
}
