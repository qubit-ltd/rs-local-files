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

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalPathCodec;
use qubit_local_files::LocalReadOptions;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
use tempfile::tempdir;

fn bench_path_codec(c: &mut Criterion) {
    let native = std::ffi::OsStr::new("manifest%2Fready");
    c.bench_function("path_codec", |b| {
        b.iter(|| {
            let canonical =
                LocalPathCodec::to_canonical_text(black_box(native))
                    .expect("benchmark component should encode");
            let restored = LocalPathCodec::from_canonical_text(&canonical)
                .expect("benchmark component should decode");
            black_box(restored);
        });
    });
    let plain = std::ffi::OsStr::new("ordinary-unicode-文档");
    c.bench_function("path_codec_plain", |b| {
        b.iter(|| {
            let canonical = LocalPathCodec::to_canonical_text(black_box(plain))
                .expect("plain benchmark component should encode");
            let restored = LocalPathCodec::from_canonical_text(&canonical)
                .expect("plain benchmark component should decode");
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
                .list(black_box(directory.path()), &Default::default())
                .expect("benchmark walker should open");
            black_box(walker.count());
        });
    });
}

fn bench_copy(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("benchmark source should be created");
    fs::write(source.join("payload"), b"payload")
        .expect("benchmark source file should be written");
    let target = directory.path().join("target");
    c.bench_function("copy", |b| {
        b.iter_batched(
            || {
                let _ = fs::remove_dir_all(&target);
            },
            |_| {
                let outcome = LocalFileSystem::host()
                    .copy(
                        black_box(&source),
                        black_box(&target),
                        &LocalCopyOptions::default(),
                    )
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
                    .open_writer(
                        black_box(&target),
                        &LocalWriteOptions::new(
                            LocalWriteMode::CreateOrReplace,
                        ),
                    )
                    .expect("benchmark writer should open");
                writer
                    .write_all(b"payload")
                    .expect("benchmark write should succeed");
                let _ = black_box(
                    writer.commit().expect("benchmark commit should succeed"),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_rooted_writer(c: &mut Criterion) {
    let directory = tempdir().expect("rooted benchmark directory should exist");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("rooted benchmark filesystem should open");
    let target = std::path::Path::new("target");
    c.bench_function("rooted_writer", |b| {
        b.iter_batched(
            || {
                let _ = fs::remove_file(directory.path().join(target));
            },
            |_| {
                let mut writer = filesystem
                    .open_writer(
                        target,
                        &LocalWriteOptions::new(
                            LocalWriteMode::CreateOrReplace,
                        ),
                    )
                    .expect("rooted benchmark writer should open");
                writer
                    .write_all(b"payload")
                    .expect("rooted benchmark write should succeed");
                let outcome = writer
                    .commit()
                    .expect("rooted benchmark commit should succeed");
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
    fs::write(&path, vec![0x5a_u8; 1 << 20])
        .expect("benchmark prefix payload should be written");
    let filesystem = LocalFileSystem::host();
    let mut group = c.benchmark_group("read_prefix");
    for max_bytes in [4 * 1024, 64 * 1024, 1 << 20] {
        group.throughput(criterion::Throughput::Bytes(max_bytes as u64));
        group.bench_function(format!("max_{max_bytes}"), |bench| {
            bench.iter(|| {
                let bytes = filesystem
                    .read_prefix(
                        black_box(&path),
                        &LocalReadOptions::new(),
                        max_bytes,
                    )
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
    bench_copy,
    bench_writer,
    bench_rooted_writer,
    bench_read_prefix
);
criterion_main!(local_files);
