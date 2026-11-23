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
    "doc/freebsd-compatibility.md",
    "doc/freebsd-compatibility.zh_CN.md",
    "doc/user_guide.md",
    "doc/user_guide.zh_CN.md",
    "doc/local_file_system_design.md",
    "doc/local_file_system_design.zh_CN.md",
    "doc/design-0.6.0.zh_CN.md",
    "doc/review-rlf3-windows-evidence.md",
    "doc/review-rlf4-windows-staging-cleanup-evidence.md",
    "doc/review-rlf5-windows-access-evidence.md",
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

fn missing_package_links(
    package_root: &Path,
    source_path: &Path,
    source: &str,
    package_files: &[PathBuf],
) -> Vec<(usize, String)> {
    fn normalize(path: &Path) -> Option<PathBuf> {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !normalized.pop() {
                        return None;
                    }
                }
                std::path::Component::Normal(part) => normalized.push(part),
                std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
            }
        }
        Some(normalized)
    }

    relative_markdown_links(source)
        .into_iter()
        .filter_map(|(line, target)| {
            let path = target.split('#').next().unwrap_or_default();
            if path.is_empty() {
                return None;
            }
            let resolved = source_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .strip_prefix(package_root)
                .ok()?
                .join(path);
            let Some(relative) = normalize(&resolved) else {
                return Some((line, target));
            };
            (!package_files.iter().any(|file| file == &relative)).then_some((line, target))
        })
        .collect()
}

fn missing_package_anchors(
    package_root: &Path,
    source_path: &Path,
    source: &str,
    package_files: &[PathBuf],
) -> Vec<(usize, String)> {
    fn normalize(path: &Path) -> Option<PathBuf> {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !normalized.pop() {
                        return None;
                    }
                }
                std::path::Component::Normal(part) => normalized.push(part),
                std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
            }
        }
        Some(normalized)
    }

    let Some(source_relative) = source_path.strip_prefix(package_root).ok() else {
        return Vec::new();
    };
    let mut cache = HashMap::<PathBuf, Vec<String>>::new();
    relative_markdown_links(source)
        .into_iter()
        .filter_map(|(line, target)| {
            let (path, fragment) = target.split_once('#')?;
            if fragment.is_empty() {
                return None;
            }
            let relative = if path.is_empty() {
                source_relative.to_path_buf()
            } else {
                let joined = source_relative.parent().unwrap_or_else(|| Path::new(".")).join(path);
                normalize(&joined)?
            };
            if !package_files.iter().any(|file| file == &relative) {
                return None;
            }
            let anchors = cache.entry(relative.clone()).or_insert_with(|| {
                std::fs::read_to_string(package_root.join(&relative))
                    .map(|text| heading_anchors(&text))
                    .unwrap_or_default()
            });
            match decode_fragment(fragment) {
                Some(fragment) if anchors.contains(&fragment) => None,
                _ => Some((line, target)),
            }
        })
        .collect()
}

fn heading_anchors(source: &str) -> Vec<String> {
    fn heading_slug(title: &str) -> String {
        let title = title.trim().trim_end_matches('#').trim();
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
        slug
    }

    fn fence_marker(line: &str) -> Option<(char, usize)> {
        let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
        if indentation > 3 {
            return None;
        }
        let rest = &line[indentation..];
        let marker = rest.chars().next()?;
        if !matches!(marker, '`' | '~') {
            return None;
        }
        let length = rest.chars().take_while(|ch| *ch == marker).count();
        (length >= 3).then_some((marker, length))
    }

    fn closes_fence(line: &str, marker: char, minimum_length: usize) -> bool {
        let Some((found_marker, length)) = fence_marker(line) else {
            return false;
        };
        if found_marker != marker || length < minimum_length {
            return false;
        }
        let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
        line[indentation + length..].trim().is_empty()
    }

    fn setext_level(line: &str) -> Option<char> {
        let trimmed = line.trim();
        let marker = trimmed.chars().next()?;
        if !matches!(marker, '=' | '-') || trimmed.chars().any(|ch| ch != marker) {
            return None;
        }
        (trimmed.len() >= 2).then_some(marker)
    }

    fn atx_title(line: &str) -> Option<&str> {
        let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
        if indentation > 3 {
            return None;
        }
        let trimmed = &line[indentation..];
        let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
        if !(1..=6).contains(&hashes) {
            return None;
        }
        let rest = &trimmed[hashes..];
        if !rest.is_empty() && !rest.chars().next().is_some_and(char::is_whitespace) {
            return None;
        }
        Some(rest)
    }

    fn without_inline_code(line: &str) -> String {
        let mut visible = String::with_capacity(line.len());
        let mut index = 0;
        let mut code_ticks = None;
        while index < line.len() {
            let rest = &line[index..];
            let tick_count = rest.bytes().take_while(|byte| *byte == b'`').count();
            if tick_count > 0 {
                if let Some(expected) = code_ticks {
                    if tick_count == expected {
                        code_ticks = None;
                    }
                } else {
                    code_ticks = Some(tick_count);
                }
                index += tick_count;
                continue;
            }
            let ch = rest.chars().next().expect("index stays on a character boundary");
            if code_ticks.is_none() {
                visible.push(ch);
            }
            index += ch.len_utf8();
        }
        visible
    }

    fn without_html_comments(line: &str, in_comment: &mut bool) -> String {
        let mut visible = String::with_capacity(line.len());
        let mut rest = line;
        while !rest.is_empty() {
            if *in_comment {
                if let Some((_, after)) = rest.split_once("-->") {
                    *in_comment = false;
                    rest = after;
                } else {
                    break;
                }
                continue;
            }
            let Some((before, after_open)) = rest.split_once('<') else {
                visible.push_str(rest);
                break;
            };
            visible.push_str(before);
            if let Some(after_comment) = after_open.strip_prefix("!--") {
                *in_comment = true;
                rest = after_comment;
                continue;
            }
            visible.push('<');
            rest = after_open;
        }
        visible
    }

    fn html_anchor_attributes(line: &str) -> Vec<String> {
        let mut anchors = Vec::new();
        let mut rest = line;
        while !rest.is_empty() {
            let Some((_, after_open)) = rest.split_once('<') else {
                break;
            };
            let Some((tag, after_tag)) = after_open.split_once('>') else {
                break;
            };
            if tag
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '/')
            {
                for attribute in tag.split_whitespace().skip(1) {
                    let Some((name, value)) = attribute.split_once('=') else {
                        continue;
                    };
                    if matches!(name, "id" | "name")
                        && let Some(value) = value.strip_prefix('"').and_then(|value| value.strip_suffix('"'))
                    {
                        anchors.push(value.to_owned());
                    }
                }
            }
            rest = after_tag;
        }
        anchors
    }

    let mut anchors = Vec::new();
    let mut counts = HashMap::<String, usize>::new();
    let lines: Vec<_> = source.lines().collect();
    let mut fence = None;
    let mut html_comment = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, minimum_length)) = fence {
            if closes_fence(line, marker, minimum_length) {
                fence = None;
            }
            index += 1;
            continue;
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            index += 1;
            continue;
        }
        if let Some(opening) = fence_marker(line) {
            fence = Some(opening);
            index += 1;
            continue;
        }

        let uncommented = without_html_comments(line, &mut html_comment);
        let visible = without_inline_code(&uncommented);
        let visible_next = lines.get(index + 1).map(|next| without_inline_code(next));
        let title = atx_title(&uncommented).or_else(|| {
            if visible_next.as_deref().is_some_and(|next| setext_level(next).is_some())
                && !uncommented.trim().is_empty()
            {
                Some(uncommented.trim())
            } else {
                None
            }
        });
        if let Some(title) = title {
            let slug = heading_slug(title);
            let count = counts.entry(slug.clone()).or_default();
            anchors.push(if *count == 0 {
                slug
            } else {
                format!("{slug}-{}", *count)
            });
            *count += 1;
        }
        anchors.extend(html_anchor_attributes(&visible));
        index += 1;
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
            source_path.parent().unwrap_or_else(|| Path::new(".")).join(path)
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
        let source =
            std::fs::read_to_string(&source_path).unwrap_or_else(|error| panic!("cannot read {document}: {error}"));
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
#[ignore = "run by the release package-link gate with cargo package --list output"]
fn test_packaged_documentation_links() {
    let list_path = std::env::var_os("RLF_PACKAGE_FILE_LIST")
        .map(PathBuf::from)
        .expect("release gate must provide cargo package --list output");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let package_files: Vec<_> = std::fs::read_to_string(&list_path)
        .expect("read actual package member list")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let package_markdown: Vec<_> = package_files
        .iter()
        .filter(|file| file.extension().is_some_and(|extension| extension == "md"))
        .cloned()
        .collect();
    let mut missing = Vec::new();
    for document in package_markdown {
        let source_path = root.join(&document);
        let source =
            std::fs::read_to_string(&source_path).unwrap_or_else(|error| panic!("cannot read {document:?}: {error}"));
        for (line, target) in missing_package_links(&root, &source_path, &source, &package_files) {
            missing.push(format!(
                "{}:{line}: linked file is absent from package: {target}",
                document.display()
            ));
        }
        for (line, target) in missing_package_anchors(&root, &source_path, &source, &package_files) {
            missing.push(format!(
                "{}:{line}: linked anchor is absent from packaged Markdown: {target}",
                document.display()
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "packaged Markdown links are missing from cargo package --list:\n{}",
        missing.join("\n")
    );
}

#[test]
fn test_package_link_scanner_detects_unshipped_targets() {
    let directory = tempfile::tempdir().expect("create package fixture");
    let root = directory.path();
    let source_path = root.join("guide.md");
    let shipped = PathBuf::from("doc/shipped.md");
    let package_files = vec![shipped.clone()];
    std::fs::create_dir_all(root.join("doc")).expect("create doc directory");
    std::fs::write(root.join(&shipped), "shipped").expect("create packaged document");
    std::fs::write(root.join("doc/unshipped.md"), "not shipped").expect("create unpackaged document");

    assert_eq!(
        missing_package_links(
            root,
            &source_path,
            "[shipped](doc/shipped.md) [unshipped](doc/unshipped.md) [escape](../../doc/shipped.md)",
            &package_files,
        ),
        vec![(1, "doc/unshipped.md".into()), (1, "../../doc/shipped.md".into())]
    );
}

#[test]
fn test_package_anchor_scanner_detects_missing_shipped_anchors() {
    let directory = tempfile::tempdir().expect("create package fixture");
    let root = directory.path();
    let source_path = root.join("README.md");
    let target_path = root.join("doc/guide.md");
    let package_files = vec![PathBuf::from("README.md"), PathBuf::from("doc/guide.md")];
    std::fs::create_dir_all(root.join("doc")).expect("create documentation directory");
    std::fs::write(&source_path, "# Start\n").expect("write packaged README");
    std::fs::write(&target_path, "# Real Heading\n").expect("write packaged guide");

    assert_eq!(
        missing_package_anchors(
            root,
            &source_path,
            "[ok](doc/guide.md#real-heading) [bad](doc/guide.md#missing-heading) [local](#start)",
            &package_files,
        ),
        vec![(1, "doc/guide.md#missing-heading".into())]
    );
}

#[test]
fn test_scanner_fixtures_cover_relative_missing_anchor_and_url_links() {
    let directory = tempfile::tempdir().expect("create fixture directory");
    let root = directory.path();
    std::fs::write(root.join("exists.md"), "fixture").expect("create linked file");
    std::fs::write(
        root.join("anchors.md"),
        "# English Heading\n## 中文标题\n## Repeated! Heading\n## Repeated Heading\n<a id=\"custom-anchor\"></a>\n\nSetext Heading\n===============\n\n~~~md\n# Fake Heading\n<a id=\"fake-id\"></a>\n~~~\n\n```\n## Another Fake Heading\n```\n\n    <a id=\"indented-fake-id\"></a>\n\nInline ` <a id=\"inline-fake-id\"> ` code.\n\n<!-- <a id=\"comment-fake-id\"></a>\n# comment-fake-heading\n<a id=\"comment-fake-id-2\"></a> -->\n",
    )
    .expect("create anchor fixture");

    assert_eq!(missing_links(root, "[ok](exists.md)"), Vec::new());
    assert_eq!(
        missing_links(root, "[missing](absent.md)"),
        vec![(1, "absent.md".into())]
    );
    assert_eq!(missing_links(root, "[anchor](#section)"), Vec::new());
    assert_eq!(missing_links(root, "[web](https://example.com/page)"), Vec::new());
    assert_eq!(
        relative_markdown_links("[anchor](#section)"),
        vec![(1, "#section".into())]
    );
    assert_eq!(relative_markdown_links("[web](https://example.com/page)"), Vec::new());

    let source_path = root.join("source.md");
    let fixture = "[english](anchors.md#english-heading) [中文](anchors.md#%E4%B8%AD%E6%96%87%E6%A0%87%E9%A2%98) [explicit](anchors.md#custom-anchor) [missing local](#not-here) [missing remote](anchors.md#absent) [missing file](absent.md#section)";
    assert_eq!(
        missing_anchors(&source_path, fixture),
        vec![(1, "#not-here".into()), (1, "anchors.md#absent".into())]
    );
    assert_eq!(missing_links(root, fixture), vec![(1, "absent.md#section".into())]);
    let code_and_setext = "[real](anchors.md#setext-heading) [fake heading](anchors.md#fake-heading) [fake id](anchors.md#fake-id) [second fake](anchors.md#another-fake-heading) [indented](anchors.md#indented-fake-id) [inline](anchors.md#inline-fake-id) [comment](anchors.md#comment-fake-id) [second comment](anchors.md#comment-fake-id-2) [comment heading](anchors.md#comment-fake-heading)";
    assert_eq!(
        missing_anchors(&source_path, code_and_setext),
        vec![
            (1, "anchors.md#fake-heading".into()),
            (1, "anchors.md#fake-id".into()),
            (1, "anchors.md#another-fake-heading".into()),
            (1, "anchors.md#indented-fake-id".into()),
            (1, "anchors.md#inline-fake-id".into()),
            (1, "anchors.md#comment-fake-id".into()),
            (1, "anchors.md#comment-fake-id-2".into()),
            (1, "anchors.md#comment-fake-heading".into()),
        ]
    );
}

#[test]
fn test_heading_slug_keeps_inline_code_text_without_registering_inline_html() {
    let directory = tempfile::tempdir().expect("create markdown fixture directory");
    let source_path = directory.path().join("anchors.md");
    std::fs::write(
        &source_path,
        "# `LocalFileSystem` guide\nInline `<a id=\"fake-code-anchor\">`\n",
    )
    .expect("write markdown fixture");

    assert!(
        missing_anchors(
            &source_path,
            "[correct](anchors.md#localfilesystem-guide) [wrong](anchors.md#-guide) [fake](anchors.md#fake-code-anchor)"
        )
        .contains(&(1, "anchors.md#-guide".into()))
    );
    assert_eq!(
        missing_anchors(
            &source_path,
            "[correct](anchors.md#localfilesystem-guide) [fake](anchors.md#fake-code-anchor)"
        ),
        vec![(1, "anchors.md#fake-code-anchor".into())]
    );
}
