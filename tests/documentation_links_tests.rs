// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

const DOCUMENTS: &[&str] = &[
    "README.md",
    "README.zh_CN.md",
    "doc/user_guide.md",
    "doc/user_guide.zh_CN.md",
    "doc/local_file_system_design.md",
    "doc/local_file_system_design.zh_CN.md",
    "doc/design-0.6.0.zh_CN.md",
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

fn heading_anchors(source: &str) -> Vec<String> {
    let mut anchors = Vec::new();
    let mut counts = HashMap::<String, usize>::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let title = trimmed[hashes..].trim().trim_end_matches('#').trim();
            let mut slug = String::new();
            let mut separator = false;
            for ch in title.chars().filter(|ch| *ch != '`' && *ch != '*') {
                if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                    if separator && !slug.is_empty() && !slug.ends_with('-') {
                        slug.push('-');
                    }
                    separator = false;
                    slug.push(ch.to_lowercase().next().unwrap_or(ch));
                } else {
                    separator = true;
                }
            }
            let count = counts.entry(slug.clone()).or_default();
            anchors.push(if *count == 0 {
                slug
            } else {
                format!("{slug}-{}", *count)
            });
            *count += 1;
        }
        let mut rest = line;
        while let Some((_, after_id)) = rest.split_once("id=\"") {
            if let Some((id, after)) = after_id.split_once('"') {
                anchors.push(id.to_owned());
                rest = after;
            } else {
                break;
            }
        }
        let mut rest = line;
        while let Some((_, after_name)) = rest.split_once("name=\"") {
            if let Some((name, after)) = after_name.split_once('"') {
                anchors.push(name.to_owned());
                rest = after;
            } else {
                break;
            }
        }
    }
    anchors
}

fn decode_fragment(fragment: &str) -> Option<String> {
    let bytes = fragment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn missing_anchors(source_path: &Path, source: &str) -> Vec<(usize, String)> {
    let mut missing = Vec::new();
    let mut cache = HashMap::<PathBuf, Vec<String>>::new();
    for (line, target) in relative_markdown_links(source) {
        let Some((path, fragment)) = target.split_once('#') else {
            continue;
        };
        if fragment.is_empty() {
            continue;
        }
        let target_path = if path.is_empty() {
            source_path.to_path_buf()
        } else {
            source_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(path)
        };
        if !path.is_empty() && !target_path.is_file() {
            continue;
        }
        let anchors = cache.entry(target_path.clone()).or_insert_with(|| {
            if path.is_empty() {
                heading_anchors(source)
            } else {
                std::fs::read_to_string(&target_path)
                    .map(|text| heading_anchors(&text))
                    .unwrap_or_default()
            }
        });
        match decode_fragment(fragment) {
            Some(fragment) if anchors.contains(&fragment) => {}
            _ => missing.push((line, target)),
        }
    }
    missing
}

#[test]
fn test_reader_document_relative_links_exist() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut missing = Vec::new();
    for document in DOCUMENTS {
        let source_path = root.join(document);
        let source = std::fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("cannot read {document}: {error}"));
        for (line, target) in missing_links(source_path.parent().unwrap(), &source) {
            missing.push(format!("{document}:{line}: {target}"));
        }
        for (line, target) in missing_anchors(&source_path, &source) {
            missing.push(format!("{document}:{line}: missing anchor {target}"));
        }
    }
    assert!(
        missing.is_empty(),
        "missing Markdown links or anchors:\n{}",
        missing.join("\n")
    );
}

#[test]
fn test_scanner_fixtures_cover_relative_missing_anchor_and_url_links() {
    let directory = tempfile::tempdir().expect("create fixture directory");
    let root = directory.path();
    std::fs::write(root.join("exists.md"), "fixture").expect("create linked file");
    std::fs::write(
        root.join("anchors.md"),
        "# English Heading\n## 中文标题\n## Repeated! Heading\n## Repeated Heading\n<a id=\"custom-anchor\"></a>\n",
    )
    .expect("create anchor fixture");

    assert_eq!(missing_links(root, "[ok](exists.md)"), Vec::new());
    assert_eq!(
        missing_links(root, "[missing](absent.md)"),
        vec![(1, "absent.md".into())]
    );
    assert_eq!(missing_links(root, "[anchor](#section)"), Vec::new());
    assert_eq!(
        missing_links(root, "[web](https://example.com/page)"),
        Vec::new()
    );
    assert_eq!(
        relative_markdown_links("[anchor](#section)"),
        vec![(1, "#section".into())]
    );
    assert_eq!(
        relative_markdown_links("[web](https://example.com/page)"),
        Vec::new()
    );

    let source_path = root.join("source.md");
    let fixture = "[english](anchors.md#english-heading) [中文](anchors.md#%E4%B8%AD%E6%96%87%E6%A0%87%E9%A2%98) [explicit](anchors.md#custom-anchor) [missing local](#not-here) [missing remote](anchors.md#absent) [missing file](absent.md#section)";
    assert_eq!(
        missing_anchors(&source_path, fixture),
        vec![(1, "#not-here".into()), (1, "anchors.md#absent".into())]
    );
    assert_eq!(
        missing_links(root, fixture),
        vec![(1, "absent.md#section".into())]
    );
}
