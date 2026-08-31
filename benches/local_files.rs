// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::path::Path;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDirectoryReopenPolicy;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
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
            let walker = LocalFileSystem::host()
                .expect("Host filesystem should open")
                .list_with_options(black_box(directory.path()), &Default::default())
                .expect("benchmark walker should open");
            black_box(walker.count());
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

criterion_group!(
    local_files,
    bench_path_codec,
    bench_walk,
    bench_walk_handle_budget,
    bench_copy,
    bench_writer,
    bench_rooted_writer,
    bench_read_prefix
);
criterion_main!(local_files);
