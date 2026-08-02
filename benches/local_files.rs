// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{fs, hint::black_box, io::Write};

use criterion::{Criterion, criterion_group, criterion_main};
use qubit_local_files::{
    LocalCopyOptions, LocalFileSystem, LocalPathCodec, LocalWriteMode, LocalWriteOptions,
};
use tempfile::tempdir;

fn bench_path_codec(c: &mut Criterion) {
    let native = std::ffi::OsStr::new("manifest%2Fready");
    c.bench_function("path_codec", |b| {
        b.iter(|| {
            let canonical = LocalPathCodec::to_canonical_text(black_box(native))
                .expect("benchmark component should encode");
            let restored = LocalPathCodec::from_canonical_text(&canonical)
                .expect("benchmark component should decode");
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
            let walker = LocalFileSystem::list(black_box(directory.path()), &Default::default())
                .expect("benchmark walker should open");
            black_box(walker.count());
        });
    });
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
                let outcome = LocalFileSystem::copy(
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
                let mut writer = LocalFileSystem::open_writer(
                    black_box(&target),
                    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                )
                .expect("benchmark writer should open");
                writer
                    .write_all(b"payload")
                    .expect("benchmark write should succeed");
                let _ = black_box(writer.commit().expect("benchmark commit should succeed"));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    local_files,
    bench_path_codec,
    bench_walk,
    bench_copy,
    bench_writer
);
criterion_main!(local_files);
