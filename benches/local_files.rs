// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Windows `LocalFileError` remains public and large by contract; benchmark
// closures propagate it without changing the observed workload.
#![cfg_attr(windows, allow(clippy::result_large_err))]

use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDirectoryReopenPolicy;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::outcome::LocalCopyResult;
use qubit_local_files::path::LocalPathCodec;
use tempfile::tempdir;

fn bench_path_codec(c: &mut Criterion) {
    let native = std::ffi::OsStr::new("manifest%2Fready");
    c.bench_function("path_codec", |b| {
        b.iter(|| {
            let canonical =
                LocalPathCodec::encode_component(black_box(native)).expect("benchmark component should encode");
            let restored = LocalPathCodec::decode_component(&canonical).expect("benchmark component should decode");
            black_box(restored);
        });
    });
    let plain = std::ffi::OsStr::new("ordinary-unicode-文档");
    c.bench_function("path_codec_plain", |b| {
        b.iter(|| {
            let canonical =
                LocalPathCodec::encode_component(black_box(plain)).expect("plain benchmark component should encode");
            let restored =
                LocalPathCodec::decode_component(&canonical).expect("plain benchmark component should decode");
            black_box(restored);
        });
    });
}

fn bench_walk(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    for index in 0..32 {
        fs::write(directory.path().join(format!("entry-{index}")), b"payload")
            .expect("benchmark entry should be written");
    }
    c.bench_function("walk", |b| {
        b.iter(|| {
            let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
            black_box(count_entries(
                &filesystem,
                black_box(directory.path()),
                &LocalListOptions::new(),
            ));
        });
    });
}

fn bench_walk_handle_budget(c: &mut Criterion) {
    let directory = tempdir().expect("budget benchmark directory should exist");
    let tree = directory.path().join("tree");
    fs::create_dir(&tree).expect("budget benchmark tree should be created");
    let mut current = tree.clone();
    for depth in 0..32 {
        for index in 0..4 {
            fs::write(current.join(format!("entry-{depth}-{index}")), b"payload")
                .expect("budget benchmark entry should be written");
        }
        current.push(format!("level-{depth}"));
        fs::create_dir(&current).expect("budget benchmark level should be created");
    }
    fs::write(current.join("payload"), b"payload").expect("budget benchmark leaf should be written");

    let host = LocalFileSystem::host().expect("Host filesystem should open");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("budget rooted benchmark filesystem should open");
    let mut group = c.benchmark_group("walk_handle_budget");
    for max_open_directories in [1, 4, 64] {
        let options = LocalListOptions::new()
            .with_recursive()
            .with_max_open_directories(max_open_directories)
            .with_reopen_policy(LocalDirectoryReopenPolicy::Reopen);
        let host_count = host
            .list_with_options(&tree, &options)
            .expect("host budget benchmark should open")
            .collect::<Result<Vec<_>, _>>()
            .expect("host budget benchmark fixture should be valid")
            .len();
        let rooted_count = rooted
            .list_with_options(Path::new("tree"), &options)
            .expect("rooted budget benchmark should open")
            .collect::<Result<Vec<_>, _>>()
            .expect("rooted budget benchmark fixture should be valid")
            .len();

        group.bench_function(format!("host_reopen_{max_open_directories}"), |bench| {
            bench.iter(|| {
                let count = count_entries(&host, black_box(&tree), &options);
                black_box(count);
            });
        });
        group.bench_function(format!("rooted_reopen_{max_open_directories}"), |bench| {
            bench.iter(|| {
                let count = count_entries(&rooted, black_box(Path::new("tree")), &options);
                black_box(count);
            });
        });
        black_box((host_count, rooted_count));
    }
    group.finish();
}

/// Counts a complete traversal and fails the benchmark on any entry error.
fn count_entries(filesystem: &LocalFileSystem, path: &Path, options: &LocalListOptions) -> usize {
    filesystem
        .list_with_options(path, options)
        .and_then(|mut walker| walker.try_fold(0_usize, |count, entry| entry.map(|_| count.saturating_add(1))))
        .expect("benchmark traversal should complete without errors")
}

fn bench_copy(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("benchmark source should be created");
    fs::write(source.join("payload"), b"payload").expect("benchmark source file should be written");
    let target = directory.path().join("target");
    c.bench_function("copy", |b| {
        b.iter_batched(
            || {
                let _ = fs::remove_dir_all(&target);
            },
            |_| {
                let outcome = LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .copy_with_options(black_box(&source), black_box(&target), &LocalCopyOptions::default())
                    .expect("benchmark copy should succeed");
                black_box(outcome.stats().files());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Measures early entry-budget rejection without timing fixture cleanup.
fn bench_copy_budget_width(c: &mut Criterion) {
    let mut group = c.benchmark_group("copy_budget_width");
    group.sample_size(10);
    for rooted in [false, true] {
        let scope = if rooted { "rooted" } else { "host" };
        for width in [100usize, 10_000] {
            let options = LocalCopyOptions::new().with_tree_source().with_max_entries(1);
            let fixture = tempdir().expect("width fixture should exist");
            let source = create_wide_source(fixture.path(), width);
            let (filesystem, source, target) = copy_coordinates(fixture.path(), &source, rooted);
            assert_copy_limit(
                filesystem.copy_with_options(&source, &target, &options),
                LocalResourceKind::Entry,
            );
            group.bench_function(format!("{scope}/entries_{width}"), |bench| {
                bench.iter_batched_ref(
                    || {
                        let directory = tempdir().expect("width benchmark directory should exist");
                        let source = create_wide_source(directory.path(), width);
                        let coordinates = copy_coordinates(directory.path(), &source, rooted);
                        (directory, coordinates)
                    },
                    |(_, (filesystem, source, target))| {
                        let _ = black_box(filesystem.copy_with_options(black_box(source), black_box(target), &options));
                    },
                    criterion::BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

/// Measures complete copies and explicit depth rejection for both authorities.
fn bench_copy_tree_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("copy_tree_depth");
    group.sample_size(10);
    for rooted in [false, true] {
        let scope = if rooted { "rooted" } else { "host" };
        for depth in [1usize, 8, 32, 64] {
            for limited in [false, true] {
                let label = if limited { "limit" } else { "success" };
                let options =
                    LocalCopyOptions::new()
                        .with_tree_source()
                        .with_max_depth(if limited { depth } else { depth + 1 });
                let fixture = tempdir().expect("depth fixture should exist");
                let source = create_deep_source(fixture.path(), depth);
                let (filesystem, source, target) = copy_coordinates(fixture.path(), &source, rooted);
                let result = filesystem.copy_with_options(&source, &target, &options);
                if limited {
                    assert_copy_limit(result, LocalResourceKind::Depth);
                } else {
                    let outcome = result.expect("success fixture must copy the complete tree");
                    assert_eq!(1, outcome.stats().files());
                    let mut leaf = fixture.path().join("target");
                    for _ in 0..depth {
                        leaf.push("child");
                    }
                    assert_eq!(
                        b"payload",
                        fs::read(leaf.join("payload"))
                            .expect("copied leaf should exist")
                            .as_slice()
                    );
                }
                group.bench_function(format!("{scope}/{label}_depth_{depth}"), |bench| {
                    bench.iter_batched_ref(
                        || {
                            let directory = tempdir().expect("depth benchmark directory should exist");
                            let source = create_deep_source(directory.path(), depth);
                            let coordinates = copy_coordinates(directory.path(), &source, rooted);
                            (directory, coordinates)
                        },
                        |(_, (filesystem, source, target))| {
                            let _ =
                                black_box(filesystem.copy_with_options(black_box(source), black_box(target), &options));
                        },
                        criterion::BatchSize::PerIteration,
                    );
                });
            }
        }
    }
    group.finish();
}

/// Creates a wide source entirely outside the measured interval.
fn create_wide_source(parent: &Path, width: usize) -> PathBuf {
    let source = parent.join("source");
    fs::create_dir(&source).expect("wide source should exist");
    for index in 0..width {
        fs::write(source.join(format!("entry-{index}")), b"x").expect("wide entry should exist");
    }
    source
}

/// Creates a tree whose final payload has descendant depth `depth + 1`.
fn create_deep_source(parent: &Path, depth: usize) -> PathBuf {
    let source = parent.join("source");
    let mut current = source.clone();
    fs::create_dir(&current).expect("deep source should exist");
    for _ in 0..depth {
        current.push("child");
        fs::create_dir(&current).expect("deep directory should exist");
    }
    fs::write(current.join("payload"), b"payload").expect("deep leaf should exist");
    source
}

/// Binds one fixture to equivalent Host or Rooted operation coordinates.
fn copy_coordinates(parent: &Path, source: &Path, rooted: bool) -> (LocalFileSystem, PathBuf, PathBuf) {
    if rooted {
        (
            LocalFileSystem::rooted(parent).expect("root should open"),
            PathBuf::from("source"),
            PathBuf::from("target"),
        )
    } else {
        (
            LocalFileSystem::host().expect("Host should open"),
            source.to_path_buf(),
            parent.join("target"),
        )
    }
}

/// Verifies the intended budget failure before registering a timed workload.
fn assert_copy_limit(result: LocalCopyResult, resource: LocalResourceKind) {
    let failure = result.expect_err("limited fixture must exhaust its selected budget");
    assert_eq!(LocalFileErrorKind::ResourceLimit, failure.error().kind());
    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert_eq!(0, failure.partial_stats().files());
    assert!(failure.partial_stats().directories() > 0);
    assert!(failure.staging_path().is_none());
    assert!(failure.cleanup_error().is_none());
    assert_eq!(
        Some(resource),
        failure.error().resource_limit_error().map(|error| error.resource())
    );
}

fn bench_writer(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let target = directory.path().join("target");
    c.bench_function("writer", |b| {
        b.iter_batched(
            || {
                let _ = fs::remove_file(&target);
            },
            |_| {
                let mut writer = LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .open_writer_with_options(
                        black_box(&target),
                        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    )
                    .expect("benchmark writer should open");
                writer.write_all(b"payload").expect("benchmark write should succeed");
                let _ = black_box(writer.commit().expect("benchmark commit should succeed"));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_rooted_writer(c: &mut Criterion) {
    let directory = tempdir().expect("rooted benchmark directory should exist");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted benchmark filesystem should open");
    let target = std::path::Path::new("target");
    c.bench_function("rooted_writer", |b| {
        b.iter_batched(
            || {
                let _ = fs::remove_file(directory.path().join(target));
            },
            |_| {
                let mut writer = filesystem
                    .open_writer_with_options(target, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
                    .expect("rooted benchmark writer should open");
                writer
                    .write_all(b"payload")
                    .expect("rooted benchmark write should succeed");
                let outcome = writer.commit().expect("rooted benchmark commit should succeed");
                let _ = black_box(outcome.state());
                black_box(outcome.bytes_written());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_read_prefix(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let path = directory.path().join("prefix-payload");
    fs::write(&path, vec![0x5a_u8; 1 << 20]).expect("benchmark prefix payload should be written");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let mut group = c.benchmark_group("read_prefix");
    for max_bytes in [4 * 1024, 64 * 1024, 1 << 20] {
        group.throughput(criterion::Throughput::Bytes(max_bytes as u64));
        group.bench_function(format!("max_{max_bytes}"), |bench| {
            bench.iter(|| {
                let bytes = filesystem
                    .read_prefix_with_options(black_box(&path), max_bytes, &LocalReadOptions::new())
                    .expect("benchmark prefix read should succeed");
                black_box(bytes.len());
            });
        });
    }
    group.finish();
}

/// Compares Host and Rooted metadata lookup across repeatable path depths.
fn bench_deep_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("deep_metadata");
    for depth in [1usize, 8, 32, 64, 128] {
        let directory = tempdir().expect("deep metadata benchmark directory should exist");
        let mut relative = std::path::PathBuf::new();
        for _ in 0..depth {
            relative.push("d");
        }
        let physical_dir = directory.path().join(&relative);
        fs::create_dir_all(&physical_dir).expect("deep metadata benchmark directories should be created");
        relative.push("payload");
        let physical = directory.path().join(&relative);
        fs::write(&physical, b"payload").expect("deep metadata benchmark payload should be written");
        let host = LocalFileSystem::host().expect("Host filesystem should open");
        let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
        group.bench_function(format!("host/depth_{depth}"), |bench| {
            bench.iter(|| {
                let metadata = host
                    .metadata(black_box(&physical))
                    .expect("Host deep metadata lookup should succeed");
                black_box(metadata.len());
            });
        });
        group.bench_function(format!("rooted/depth_{depth}"), |bench| {
            bench.iter(|| {
                let metadata = rooted
                    .metadata(black_box(&relative))
                    .expect("Rooted deep metadata lookup should succeed");
                black_box(metadata.len());
            });
        });
    }
    group.finish();
}

criterion_group!(
    local_files,
    bench_path_codec,
    bench_walk,
    bench_walk_handle_budget,
    bench_copy,
    bench_copy_budget_width,
    bench_copy_tree_depth,
    bench_writer,
    bench_rooted_writer,
    bench_read_prefix,
    bench_deep_metadata
);
criterion_main!(local_files);
