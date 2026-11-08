// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::path::PathBuf;

const DOCUMENTS: &[&str] = &[
    "README.md",
    "README.zh_CN.md",
    "doc/user_guide.md",
    "doc/user_guide.zh_CN.md",
    "doc/local_file_system_design.md",
    "doc/local_file_system_design.zh_CN.md",
    "doc/review-rlf3-windows-evidence.md",
    "doc/review-rlf4-windows-staging-cleanup-evidence.md",
];

fn relative_markdown_links(source: &str) -> Vec<(usize, String)> {
    source
        .lines()
        .enumerate()
        .flat_map(|(index, line)| {
            let mut rest = line;
            let mut links = Vec::new();
            while let Some((_, after_open)) = rest.split_once("](") {
                let Some((target, after_close)) = after_open.split_once(')') else {
                    break;
                };
                let target = target.trim();
                if !target.is_empty()
                    && !target.starts_with('#')
                    && !target.chars().any(char::is_whitespace)
                    && !target.split_once(':').is_some_and(|(scheme, _)| {
                        !scheme.is_empty()
                            && scheme
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                    })
                {
                    links.push((index + 1, target.to_owned()));
                }
                rest = after_close;
            }
            links
        })
        .collect()
}

fn missing_links(base: &Path, source: &str) -> Vec<(usize, String)> {
    relative_markdown_links(source)
        .into_iter()
        .filter(|(_, target)| {
            let path = target.split('#').next().unwrap_or_default();
            !path.is_empty() && !base.join(path).is_file()
        })
        .collect()
}

#[test]
fn test_reader_document_relative_links_exist() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut missing = Vec::new();
    for document in DOCUMENTS {
        let source_path = root.join(document);
        let source =
            std::fs::read_to_string(&source_path).unwrap_or_else(|error| panic!("cannot read {document}: {error}"));
        for (line, target) in missing_links(source_path.parent().unwrap(), &source) {
            missing.push(format!("{document}:{line}: {target}"));
        }
    }
    assert!(missing.is_empty(), "missing Markdown links:\n{}", missing.join("\n"));
}

#[test]
fn test_scanner_fixtures_cover_relative_missing_anchor_and_url_links() {
    let directory = tempfile::tempdir().expect("create fixture directory");
    let root = directory.path();
    std::fs::write(root.join("exists.md"), "fixture").expect("create linked file");

    assert_eq!(missing_links(root, "[ok](exists.md)"), Vec::new());
    assert_eq!(
        missing_links(root, "[missing](absent.md)"),
        vec![(1, "absent.md".into())]
    );
    assert_eq!(missing_links(root, "[anchor](#section)"), Vec::new());
    assert_eq!(missing_links(root, "[web](https://example.com/page)"), Vec::new());
    assert_eq!(relative_markdown_links("[anchor](#section)"), Vec::new());
    assert_eq!(relative_markdown_links("[web](https://example.com/page)"), Vec::new());
}
