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

use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::ffi::OsStr;
use std::fs;
use std::hint::black_box;
use std::io::IoSlice;
use std::io::IoSliceMut;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_io::Output;
use qubit_local_files::LocalFileReader;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalFileWriter;
use qubit_local_files::LocalTempDirectory;
use qubit_local_files::directory::LocalResolutionOrigin;
use qubit_local_files::directory::LocalResolutionPolicy;
use qubit_local_files::directory::LocalResolutionScope;
use qubit_local_files::directory::LocalTreeEntry;
use qubit_local_files::error::LocalFileError;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalDirectoryReopenPolicy;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalResolutionLimits;
use qubit_local_files::options::LocalRootOpenOptions;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::options::LocalTreeValidationOptions;
use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::outcome::LocalCopyResult;
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::outcome::LocalWriteOutcome;
use qubit_local_files::outcome::LocalWriterState;
use qubit_local_files::path::LocalPathCodec;
use qubit_local_files::policy::LocalDurabilityRequirement;
use tempfile::tempdir;

struct SamplingAllocator;

static SAMPLE_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static SAMPLED_ALLOCATION_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for SamplingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && SAMPLE_ALLOCATIONS.load(Ordering::Relaxed) {
            SAMPLED_ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() && SAMPLE_ALLOCATIONS.load(Ordering::Relaxed) {
            SAMPLED_ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        new_pointer
    }
}

#[global_allocator]
static BENCH_ALLOCATOR: SamplingAllocator = SamplingAllocator;

#[derive(Debug)]
struct NoopResolutionPolicy;

impl LocalResolutionPolicy for NoopResolutionPolicy {
    fn validate_component(
        &self,
        _: &OsStr,
        _: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> qubit_local_files::LocalResult<()> {
        Ok(())
    }

    fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> qubit_local_files::LocalResult<()> {
        Ok(())
    }
}

fn bench_path_codec(c: &mut Criterion) {
    let native = OsStr::new("manifest%2Fready");
    c.bench_function("path_codec", |b| {
        b.iter(|| {
            let canonical =
                LocalPathCodec::encode_component(black_box(native)).expect("benchmark component should encode");
            let restored = LocalPathCodec::decode_component(&canonical).expect("benchmark component should decode");
            black_box(restored);
        });
    });
    let plain = OsStr::new("ordinary-unicode-文档");
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

const VECTORED_BUFFER_COUNTS: [usize; 6] = [0, 1, 2, 8, 16, 64];
const VECTORED_SEGMENT_SIZES: [usize; 2] = [64, 4096];

fn vectored_payload(count: usize, segment_size: usize) -> [Vec<u8>; 64] {
    std::array::from_fn(|index| {
        if index < count {
            vec![(index % 251) as u8; segment_size]
        } else {
            Vec::new()
        }
    })
}

fn vectored_reader_fixture(payload: &[Vec<u8>; 64], count: usize) -> (LocalFileReader, tempfile::TempDir) {
    let fixture = tempdir().expect("vectored reader fixture should exist");
    let contents = payload[..count].iter().flatten().copied().collect::<Vec<_>>();
    let path = fixture.path().join("payload");
    fs::write(&path, contents).expect("vectored reader payload should be written");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let reader = filesystem.open_reader(&path).expect("vectored reader should open");
    (reader, fixture)
}

fn vectored_writer_fixture() -> (LocalFileWriter, tempfile::TempDir) {
    let fixture = tempdir().expect("vectored writer fixture should exist");
    let path = fixture.path().join("payload");
    fs::write(&path, []).expect("vectored writer target should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let writer = filesystem
        .open_writer_with_options(&path, &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect("vectored append writer should open");
    (writer, fixture)
}

fn vectored_read_once(
    reader: &mut LocalFileReader,
    payload: &[Vec<u8>; 64],
    count: usize,
    size: usize,
    direct: bool,
) -> usize {
    let mut output = std::array::from_fn::<_, 64, _>(|index| if index < count { vec![0; size] } else { Vec::new() });
    let read = if direct {
        let mut slices = output.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>();
        qubit_io::Input::read_vectored(reader, &mut slices[..count])
    } else {
        let mut buffers = output
            .iter_mut()
            .map(|buffer| IoSliceMut::new(buffer.as_mut_slice()))
            .collect::<Vec<_>>();
        reader.read_vectored(&mut buffers[..count])
    }
    .expect("vectored reader call should succeed");
    let actual = output.iter().flatten().copied().take(read).collect::<Vec<_>>();
    let expected = payload
        .iter()
        .take(count)
        .flatten()
        .copied()
        .take(read)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "vectored read should preserve the ordered prefix");
    read
}

fn sampled_allocations(call: impl FnOnce()) -> usize {
    SAMPLED_ALLOCATION_COUNT.store(0, Ordering::Relaxed);
    SAMPLE_ALLOCATIONS.store(true, Ordering::SeqCst);
    call();
    SAMPLE_ALLOCATIONS.store(false, Ordering::SeqCst);
    SAMPLED_ALLOCATION_COUNT.load(Ordering::Relaxed)
}

fn sample_temp_file_vectored_allocations() {
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let mut inherent_file = filesystem.create_temp_file().expect("temporary file should be created");
    let mut trait_file = filesystem.create_temp_file().expect("temporary file should be created");
    let inherent_path = inherent_file.path().to_path_buf();
    let trait_path = trait_file.path().to_path_buf();
    let buffers = [IoSlice::new(b"payload")];
    let slices: [&[u8]; 1] = [b"payload"];

    let inherent_allocations = sampled_allocations(|| {
        let result = inherent_file.write_vectored(&buffers);
        black_box(result).expect("single-buffer temp write succeeds");
    });
    let trait_allocations = sampled_allocations(|| {
        let result = Output::write_vectored(&mut trait_file, &slices);
        black_box(result).expect("single-buffer trait write succeeds");
    });

    inherent_file.close();
    trait_file.close();
    assert_eq!(
        fs::read(&inherent_path).expect("inherent temp output should be readable"),
        b"payload"
    );
    assert_eq!(
        fs::read(&trait_path).expect("trait temp output should be readable"),
        b"payload"
    );
    drop(inherent_file);
    drop(trait_file);
    assert!(!inherent_path.exists(), "inherent temp file should be cleaned up");
    assert!(!trait_path.exists(), "trait temp file should be cleaned up");
    eprintln!(
        "ALLOC_SAMPLE vectored_io backend=LocalTempFile buffers=1 segment=7 method=inherent_write_vectored write_alloc_calls={inherent_allocations} scope=single_adapter_call"
    );
    eprintln!(
        "ALLOC_SAMPLE vectored_io backend=LocalTempFile buffers=1 segment=7 method=Output_trait write_alloc_calls={trait_allocations} scope=single_adapter_call"
    );
}

fn sample_vectored_allocations(payload: &[Vec<u8>; 64], count: usize, size: usize) {
    for direct in [false, true] {
        let method = if direct { "trait_direct" } else { "local_adapter" };
        let (mut reader, _fixture) = vectored_reader_fixture(payload, count);
        let mut output =
            std::array::from_fn::<_, 64, _>(|index| if index < count { vec![0; size] } else { Vec::new() });
        let read_allocations = if direct {
            let mut slices = output.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>();
            sampled_allocations(|| {
                let read = qubit_io::Input::read_vectored(&mut reader, &mut slices[..count])
                    .expect("sampled vectored reader call should succeed");
                black_box(read);
            })
        } else {
            let mut buffers = output
                .iter_mut()
                .map(|buffer| IoSliceMut::new(buffer.as_mut_slice()))
                .collect::<Vec<_>>();
            sampled_allocations(|| {
                let read = reader
                    .read_vectored(&mut buffers[..count])
                    .expect("sampled vectored reader call should succeed");
                black_box(read);
            })
        };

        let (mut writer, _fixture) = vectored_writer_fixture();
        let write_slices: [IoSlice<'_>; 64] = std::array::from_fn(|index| IoSlice::new(&payload[index]));
        let direct_slices = payload.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let write_allocations = if direct {
            sampled_allocations(|| {
                let written = qubit_io::Output::write_vectored(&mut writer, &direct_slices[..count])
                    .expect("sampled vectored writer call should succeed");
                black_box(written);
            })
        } else {
            sampled_allocations(|| {
                let written = writer
                    .write_vectored(&write_slices[..count])
                    .expect("sampled vectored writer call should succeed");
                black_box(written);
            })
        };
        eprintln!(
            "ALLOC_SAMPLE vectored_io buffers={count} segment={size} method={method} read_alloc_calls={read_allocations} write_alloc_calls={write_allocations} scope=single_adapter_call"
        );
    }
}

fn bench_vectored_io(c: &mut Criterion) {
    let mut group = c.benchmark_group("vectored_io");
    sample_temp_file_vectored_allocations();
    for count in VECTORED_BUFFER_COUNTS {
        for size in VECTORED_SEGMENT_SIZES {
            let payload = vectored_payload(count, size);
            let byte_count = (count * size) as u64;
            group.throughput(criterion::Throughput::Bytes(byte_count));
            sample_vectored_allocations(&payload, count, size);

            // Verify both reader interfaces and their exact ordered prefix before timing.
            let (mut inherent_reader, _reader_fixture) = vectored_reader_fixture(&payload, count);
            let (mut direct_reader, _direct_fixture) = vectored_reader_fixture(&payload, count);
            let inherent_read = vectored_read_once(&mut inherent_reader, &payload, count, size, false);
            let direct_read = vectored_read_once(&mut direct_reader, &payload, count, size, true);
            assert_eq!(inherent_read, direct_read, "reader paths should report equal progress");

            for direct in [false, true] {
                let method = if direct { "trait_direct" } else { "local_adapter" };
                let id = format!("reader/{method}/buffers_{count}/segment_{size}");
                let (mut reader, _fixture) = vectored_reader_fixture(&payload, count);
                let mut output =
                    std::array::from_fn::<_, 64, _>(|index| if index < count { vec![0; size] } else { Vec::new() });
                group.bench_function(id, |bench| {
                    bench.iter_custom(|iterations| {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iterations {
                            reader.seek(SeekFrom::Start(0)).expect("vectored reader should rewind");
                            if direct {
                                let mut slices = output.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>();
                                let started = Instant::now();
                                let amount = qubit_io::Input::read_vectored(&mut reader, &mut slices[..count])
                                    .expect("vectored reader call should succeed");
                                elapsed += started.elapsed();
                                black_box(amount);
                            } else {
                                let mut buffers = output
                                    .iter_mut()
                                    .map(|buffer| IoSliceMut::new(buffer.as_mut_slice()))
                                    .collect::<Vec<_>>();
                                let started = Instant::now();
                                let amount = reader
                                    .read_vectored(&mut buffers[..count])
                                    .expect("vectored reader call should succeed");
                                elapsed += started.elapsed();
                                black_box(amount);
                            }
                        }
                        elapsed
                    });
                });
            }

            let write_slices: [IoSlice<'_>; 64] = std::array::from_fn(|index| IoSlice::new(&payload[index]));
            let direct_slices = payload.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let expected_output = payload[..count].iter().flatten().copied().collect::<Vec<_>>();
            for direct in [false, true] {
                let method = if direct { "trait_direct" } else { "local_adapter" };
                let id = format!("writer/{method}/buffers_{count}/segment_{size}");

                let (mut writer, fixture) = vectored_writer_fixture();
                let written = if direct {
                    qubit_io::Output::write_vectored(&mut writer, &direct_slices[..count])
                } else {
                    writer.write_vectored(&write_slices[..count])
                }
                .expect("vectored writer call should succeed");
                drop(writer);
                assert_eq!(written, byte_count as usize, "writer should write the complete fixture");
                assert_eq!(
                    fs::read(fixture.path().join("payload")).expect("vectored output should be readable"),
                    expected_output,
                    "writer paths should produce identical bytes"
                );

                let (mut writer, fixture) = vectored_writer_fixture();
                let truncate = fs::OpenOptions::new()
                    .write(true)
                    .open(fixture.path().join("payload"))
                    .expect("vectored writer target should remain openable");
                group.bench_function(id, |bench| {
                    bench.iter_custom(|iterations| {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iterations {
                            truncate.set_len(0).expect("vectored append fixture should reset");
                            let started = Instant::now();
                            if direct {
                                let amount = qubit_io::Output::write_vectored(&mut writer, &direct_slices[..count])
                                    .expect("vectored writer call should succeed");
                                black_box(amount);
                            } else {
                                let amount = writer
                                    .write_vectored(&write_slices[..count])
                                    .expect("vectored writer call should succeed");
                                black_box(amount);
                            }
                            elapsed += started.elapsed();
                        }
                        elapsed
                    });
                });
            }
        }
    }
    group.finish();
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

/// Measures wide recursive traversals with and without directory reopening.
fn bench_walk_name_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("walk_name_cache");
    for rooted in [false, true] {
        let scope = if rooted { "rooted" } else { "host" };
        for width in [100usize, 10_000] {
            let fixture = tempdir().expect("wide walker fixture should exist");
            for index in 0..width {
                let name = format!("entry-{index:05}-fixed-name");
                fs::write(fixture.path().join(name), b"x").expect("wide walker fixture entry should be written");
            }
            let child = fixture.path().join("z-child");
            fs::create_dir(&child).expect("reopen fixture child should be created");
            fs::write(child.join("payload"), b"x").expect("reopen fixture payload should be written");
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem should open")
            } else {
                LocalFileSystem::host().expect("Host filesystem should open")
            };
            let path = if rooted { Path::new("/") } else { fixture.path() };
            for (mode, options) in [
                ("retained_handles", LocalListOptions::new().with_recursive()),
                (
                    "reopen_cache",
                    LocalListOptions::new()
                        .with_recursive()
                        .with_max_open_directories(1)
                        .with_reopen_policy(LocalDirectoryReopenPolicy::Reopen),
                ),
            ] {
                let expected = width + 2;
                assert_eq!(count_entries(&filesystem, path, &options), expected);
                group.bench_function(format!("{scope}/{mode}/entries_{width}"), |bench| {
                    bench.iter(|| {
                        let count = count_entries(&filesystem, black_box(path), &options);
                        black_box(count);
                    });
                });
            }
        }
    }
    group.finish();
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

/// Measures staged-tree validation across wide and deep fixtures.
/// The verifier is crate-private, so the benchmark calls publication to reach
/// it. Setup and fixture cleanup are outside the measured interval; the timing
/// includes the final no-replace rename in addition to validation.
fn bench_tree_validation_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_validation_index");
    for width in [100usize, 10_000] {
        for mode in ["exact", "no_exact", "hardlink"] {
            #[cfg(not(unix))]
            if mode == "hardlink" {
                continue;
            }
            group.bench_function(format!("wide/{mode}/{width}"), |bench| {
                bench.iter_custom(|iterations| {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iterations {
                        let fixture = tempdir().expect("tree validation fixture should exist");
                        let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
                        let parent = filesystem
                            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
                            .expect("benchmark parent should open");
                        let stage = parent
                            .create_staged_directory(&LocalTempDirectoryOptions::new())
                            .expect("benchmark stage should be created");
                        let root = fixture.path().join(stage.relative_name());
                        let mut expected = Vec::new();
                        if mode == "hardlink" {
                            fs::write(root.join("entry-00000"), b"x").expect("hardlink source should be written");
                            for index in 1..width {
                                fs::hard_link(root.join("entry-00000"), root.join(format!("entry-{index:05}")))
                                    .expect("hardlink entry should be created");
                            }
                        } else {
                            for index in 0..width {
                                let name = format!("entry-{index:05}");
                                fs::write(root.join(&name), b"x").expect("wide entry should be written");
                                expected.push(
                                    LocalTreeEntry::new(PathBuf::from(name), LocalFileKind::File)
                                        .expect("expected entry should be valid"),
                                );
                            }
                        }
                        let validation = match mode {
                            "exact" => LocalTreeValidationOptions::new().with_exact_entries(expected),
                            "hardlink" => LocalTreeValidationOptions::new()
                                .with_reject_external_hardlinks()
                                .with_limits(LocalListOptions::new().with_max_entries(width)),
                            _ => LocalTreeValidationOptions::new()
                                .with_limits(LocalListOptions::new().with_max_entries(width)),
                        };
                        let options = LocalTreePublishOptions::new().with_validation(validation);
                        let started = std::time::Instant::now();
                        let result = stage.publish_new(&parent, OsStr::new("published"), &options);
                        elapsed += started.elapsed();
                        let outcome = result.expect("benchmark validation and publication should succeed");
                        let _ = black_box(outcome);
                        drop(parent);
                        drop(fixture);
                    }
                    elapsed
                });
            });
        }
    }
    for depth in [16usize, 64] {
        for mode in ["exact", "no_exact", "hardlink"] {
            #[cfg(not(unix))]
            if mode == "hardlink" {
                continue;
            }
            group.bench_function(format!("deep/{mode}/{depth}"), |bench| {
                bench.iter_custom(|iterations| {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iterations {
                        let fixture = tempdir().expect("deep validation fixture should exist");
                        let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
                        let parent = filesystem
                            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
                            .expect("benchmark parent should open");
                        let stage = parent
                            .create_staged_directory(&LocalTempDirectoryOptions::new())
                            .expect("benchmark stage should be created");
                        let mut path = fixture.path().join(stage.relative_name());
                        let mut expected = Vec::new();
                        for _ in 0..depth {
                            path.push("level");
                            fs::create_dir(&path).expect("deep directory should be created");
                            expected.push(
                                LocalTreeEntry::new(
                                    path.strip_prefix(fixture.path())
                                        .expect("deep path is beneath fixture")
                                        .strip_prefix(stage.relative_name())
                                        .expect("deep path is beneath stage")
                                        .to_path_buf(),
                                    LocalFileKind::Directory,
                                )
                                .expect("expected directory should be valid"),
                            );
                        }
                        let leaf = path.join(if mode == "hardlink" { "leaf-0" } else { "leaf" });
                        fs::write(&leaf, b"x").expect("deep leaf should be written");
                        if mode == "hardlink" {
                            fs::hard_link(&leaf, path.join("leaf-1")).expect("deep hardlink should be created");
                        } else {
                            expected.push(
                                LocalTreeEntry::new(
                                    leaf.strip_prefix(fixture.path())
                                        .expect("leaf path is beneath fixture")
                                        .strip_prefix(stage.relative_name())
                                        .expect("leaf path is beneath stage")
                                        .to_path_buf(),
                                    LocalFileKind::File,
                                )
                                .expect("expected file should be valid"),
                            );
                        }
                        let validation = match mode {
                            "exact" => LocalTreeValidationOptions::new()
                                .with_exact_entries(expected)
                                .with_limits(LocalListOptions::new().with_max_depth(depth + 1)),
                            "hardlink" => LocalTreeValidationOptions::new()
                                .with_reject_external_hardlinks()
                                .with_limits(
                                    LocalListOptions::new()
                                        .with_max_depth(depth + 1)
                                        .with_max_entries(depth + 2),
                                ),
                            _ => LocalTreeValidationOptions::new()
                                .with_limits(LocalListOptions::new().with_max_depth(depth + 1)),
                        };
                        let options = LocalTreePublishOptions::new().with_validation(validation);
                        let started = std::time::Instant::now();
                        let result = stage.publish_new(&parent, OsStr::new("published"), &options);
                        elapsed += started.elapsed();
                        let outcome = result.expect("deep validation and publication should succeed");
                        let _ = black_box(outcome);
                        drop(parent);
                        drop(fixture);
                    }
                    elapsed
                });
            });
        }
    }
    group.finish();
}

/// Counts a complete traversal and fails the benchmark on any entry error.
fn count_entries(filesystem: &LocalFileSystem, path: &Path, options: &LocalListOptions) -> usize {
    let walker = filesystem
        .list_with_options(path, options)
        .expect("benchmark traversal should complete without errors");
    let mut count = 0_usize;
    for entry in walker {
        let _ = entry.expect("benchmark traversal entry should be readable");
        count = count.saturating_add(1);
    }
    count
}

/// Measures copying one immutable tree into a fresh destination per iteration.
fn bench_copy(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("benchmark source should be created");
    fs::write(source.join("payload"), b"payload").expect("benchmark source file should be written");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let options = LocalCopyOptions::default();
    let (fixture, target) = fresh_copy_target();
    let outcome = filesystem
        .copy_with_options(&source, &target, &options)
        .expect("copy fixture should succeed");
    assert_eq!(outcome.stats().files(), 1);
    assert_eq!(
        fs::read(target.join("payload")).expect("copied fixture should be readable"),
        b"payload"
    );
    drop(fixture);
    c.bench_function("copy", |b| {
        b.iter_batched_ref(
            fresh_copy_target,
            |(_, target)| {
                let outcome = filesystem
                    .copy_with_options(black_box(&source), black_box(target), &options)
                    .expect("benchmark copy should succeed");
                let _ = black_box(outcome);
            },
            criterion::BatchSize::PerIteration,
        );
    });
}

/// Owns an absent copy target whose setup and destruction are outside timing.
fn fresh_copy_target() -> (tempfile::TempDir, PathBuf) {
    let directory = tempdir().expect("copy target parent should be created");
    let target = directory.path().join("target");
    assert!(!target.exists(), "copy iteration must start with an absent target");
    (directory, target)
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
                        let failure = filesystem
                            .copy_with_options(black_box(source), black_box(target), &options)
                            .expect_err("limited benchmark copy should fail");
                        let _ = black_box(failure);
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
                            let result = filesystem.copy_with_options(black_box(source), black_box(target), &options);
                            if limited {
                                let failure = result.expect_err("limited benchmark copy should fail");
                                let _ = black_box(failure);
                            } else {
                                let outcome = result.expect("benchmark tree copy should succeed");
                                let _ = black_box(outcome);
                            }
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

/// Measures the first installation performed by a Host CreateOrReplace writer.
fn bench_writer(c: &mut Criterion) {
    bench_fresh_writer(c, "writer", false);
}

/// Measures the first installation performed by a Rooted CreateOrReplace
/// writer.
fn bench_rooted_writer(c: &mut Criterion) {
    bench_fresh_writer(c, "rooted_writer", true);
}

/// Builds a scope and absent target, keeping authority construction outside
/// timing.
fn fresh_writer_target(rooted: bool) -> (LocalFileSystem, PathBuf, tempfile::TempDir) {
    let directory = tempdir().expect("writer target parent should be created");
    let (filesystem, target) = if rooted {
        (
            LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open"),
            PathBuf::from("target"),
        )
    } else {
        (
            LocalFileSystem::host().expect("Host filesystem should open"),
            directory.path().join("target"),
        )
    };
    assert!(
        !directory.path().join("target").exists(),
        "writer iteration must start with an absent target"
    );
    (filesystem, target, directory)
}

/// Writes and commits the fixed benchmark payload through the supplied scope.
fn write_benchmark_payload(filesystem: &LocalFileSystem, target: &Path) {
    let mut writer = filesystem
        .open_writer_with_options(
            black_box(target),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("benchmark writer should open");
    Output::write_fully(&mut writer, b"payload").expect("benchmark write should succeed");
    let outcome = writer.commit().expect("benchmark commit should succeed");
    let _ = black_box(outcome);
}

/// Checks the fixture once, then measures I/O while borrowing per-iteration
/// resources.
fn bench_fresh_writer(c: &mut Criterion, name: &str, rooted: bool) {
    {
        let (filesystem, target, directory) = fresh_writer_target(rooted);
        write_benchmark_payload(&filesystem, &target);
        assert_eq!(
            fs::read(directory.path().join("target")).expect("writer fixture should be readable"),
            b"payload"
        );
        assert_eq!(
            filesystem
                .metadata(&target)
                .expect("writer metadata should be readable")
                .len(),
            7
        );
    }
    c.bench_function(name, |b| {
        b.iter_batched_ref(
            || fresh_writer_target(rooted),
            |(filesystem, target, _)| write_benchmark_payload(filesystem, target),
            criterion::BatchSize::PerIteration,
        );
    });
}

/// Measures independent new/replacement writes for both authorities and
/// policies. Setup creates the old target when requested; payload allocation,
/// authority construction, and scratch-directory destruction stay outside
/// timing.
fn bench_writer_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("writer_scenarios");
    for rooted in [false, true] {
        let scope = if rooted { "rooted" } else { "host" };
        for existing in [false, true] {
            let target_mode = if existing { "replace" } else { "new" };
            for (metadata_name, metadata) in [
                ("preserve_existing", LocalWriteMetadataPolicy::PreserveExisting),
                ("use_staging", LocalWriteMetadataPolicy::UseStaging),
            ] {
                for (durability_name, durability) in [
                    ("not_required", LocalDurabilityRequirement::NotRequired),
                    ("required", LocalDurabilityRequirement::Required),
                ] {
                    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                        .with_metadata_policy(metadata)
                        .with_durability(durability);
                    for (size_name, size) in [("4KiB", 4 * 1024), ("1MiB", 1 << 20), ("16MiB", 16 << 20)] {
                        let id = format!("{scope}/{target_mode}/{metadata_name}/{durability_name}/{size_name}");
                        let payload = vec![0x5a; size];
                        let (filesystem, target, directory) = writer_scenario_target(rooted, existing);
                        match write_scenario_payload(&filesystem, &target, &options, &payload) {
                            Ok(outcome) => {
                                assert_writer_outcome(outcome, size as u64, durability);
                                assert_eq!(
                                    fs::read(directory.path().join("target"))
                                        .expect("fixture target should be readable"),
                                    payload
                                );
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    LocalFileErrorKind::Unsupported | LocalFileErrorKind::RequirementNotMet
                                ) =>
                            {
                                eprintln!("UNSUPPORTED writer_scenarios/{id}: {error}");
                                continue;
                            }
                            Err(error) => panic!("writer_scenarios/{id} preflight failed: {error}"),
                        }
                        drop((filesystem, directory));
                        group.throughput(criterion::Throughput::Bytes(size as u64));
                        group.bench_function(id, |bench| {
                            bench.iter_batched_ref(
                                || writer_scenario_target(rooted, existing),
                                |(filesystem, target, _)| {
                                    let outcome = write_scenario_payload(filesystem, target, &options, &payload)
                                        .expect("registered writer scenario should succeed");
                                    let _ = black_box(outcome);
                                },
                                criterion::BatchSize::PerIteration,
                            );
                        });
                    }
                }
            }
        }
    }
    group.finish();
}

/// Creates a fresh scope and target; replacement setup writes old bytes before
/// timing and panics if the independent fixture cannot be prepared.
fn writer_scenario_target(rooted: bool, existing: bool) -> (LocalFileSystem, PathBuf, tempfile::TempDir) {
    let (filesystem, target, directory) = fresh_writer_target(rooted);
    if existing {
        fs::write(directory.path().join("target"), b"old content").expect("replacement target should be created");
    }
    (filesystem, target, directory)
}

/// Opens, writes, and commits one payload, returning open/commit failures for
/// untimed capability probing. A stream write failure always fails the run.
fn write_scenario_payload(
    filesystem: &LocalFileSystem,
    target: &Path,
    options: &LocalWriteOptions,
    payload: &[u8],
) -> Result<LocalWriteOutcome, Box<LocalFileError>> {
    let mut writer = filesystem.open_writer_with_options(black_box(target), options)?;
    Output::write_fully(&mut writer, black_box(payload)).expect("scenario payload should be written");
    writer.commit().map_err(|failure| {
        let (error, _state, _writer) = failure.into_parts();
        Box::new(error)
    })
}

/// Fails a sample if commit did not establish the requested successful result.
fn assert_writer_outcome(outcome: LocalWriteOutcome, size: u64, durability: LocalDurabilityRequirement) {
    assert_eq!(outcome.state(), LocalWriterState::Committed);
    assert_eq!(outcome.bytes_written(), size);
    assert!(outcome.failure_state().is_none());
    if durability == LocalDurabilityRequirement::Required {
        assert!(outcome.durable(), "required durability must not silently downgrade");
    }
}

/// Registers successful cleanup of wide and deep temporary trees. Each call
/// receives a fresh tree; allocation and final fixture destruction are untimed.
fn bench_temp_directory_cleanup(c: &mut Criterion) {
    let mut group = c.benchmark_group("temp_directory_cleanup");
    for rooted in [false, true] {
        let scope = if rooted { "rooted" } else { "host" };
        for wide in [true, false] {
            let shape = if wide { "wide_10000" } else { "deep_64x4" };
            for (limit_name, options) in cleanup_scenarios() {
                let id = format!("{scope}/{shape}/{limit_name}");
                let (mut temporary, physical, _directory) = cleanup_fixture(rooted, wide, &options);
                temporary
                    .cleanup()
                    .expect("cleanup fixture should succeed within its limits");
                assert!(!physical.exists(), "cleanup must remove the fixture tree");
                group.throughput(criterion::Throughput::Elements(if wide { 10_001 } else { 321 }));
                group.bench_function(id, |bench| {
                    bench.iter_batched_ref(
                        || cleanup_fixture(rooted, wide, &options),
                        |(temporary, _, _)| {
                            temporary
                                .cleanup()
                                .expect("benchmark cleanup should succeed within its limits");
                        },
                        criterion::BatchSize::PerIteration,
                    );
                });
            }
        }
    }
    group.finish();
}

/// Returns unlimited and generously bounded successful-cleanup configurations.
/// All four limits accommodate both fixtures; the deadline is per cleanup call.
fn cleanup_scenarios() -> Vec<(&'static str, LocalTempDirectoryOptions)> {
    vec![
        ("unlimited", LocalTempDirectoryOptions::new()),
        (
            "bounded_sufficient",
            LocalTempDirectoryOptions::new().with_cleanup_limits(
                LocalTempCleanupLimits::new()
                    .with_max_depth(65)
                    .with_max_entries(10_001)
                    .with_max_pending_path_bytes(64 * 1024 * 1024)
                    .with_deadline(Duration::from_secs(60)),
            ),
        ),
    ]
}

/// Builds 10,000 root files or 64 nested levels with four files at each level.
/// Returns the owned resource, physical diagnostic path, and scratch parent.
/// Native setup I/O is untimed and panics on failure; Rooted cleanup itself
/// continues to use its retained authority rather than this physical path.
fn cleanup_fixture(
    rooted: bool,
    wide: bool,
    options: &LocalTempDirectoryOptions,
) -> (LocalTempDirectory, PathBuf, tempfile::TempDir) {
    let directory = tempdir().expect("cleanup scratch parent should exist");
    let (filesystem, options) = if rooted {
        (
            LocalFileSystem::rooted(directory.path()).expect("cleanup Rooted filesystem should open"),
            options.clone(),
        )
    } else {
        (
            LocalFileSystem::host().expect("cleanup Host filesystem should open"),
            options.clone().with_parent(directory.path()),
        )
    };
    let temporary = filesystem
        .create_temp_directory_with_options(&options)
        .expect("temporary tree should be created");
    let physical = if rooted {
        directory.path().join(
            temporary
                .path()
                .strip_prefix(Path::new("/"))
                .expect("Rooted path should be namespace absolute"),
        )
    } else {
        temporary.path().to_path_buf()
    };
    if wide {
        for index in 0..10_000 {
            fs::write(physical.join(format!("entry-{index}")), b"x").expect("wide cleanup entry should exist");
        }
    } else {
        let mut current = physical.clone();
        for _ in 0..64 {
            current.push("d");
            fs::create_dir(&current).expect("deep cleanup directory should exist");
            for index in 0..4 {
                fs::write(current.join(format!("entry-{index}")), b"x").expect("deep cleanup entry should exist");
            }
        }
    }
    (temporary, physical, directory)
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

fn bench_read_all(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let path = directory.path().join("read-all-payload");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let mut group = c.benchmark_group("read_all");
    for (name, size) in [("empty", 0), ("8192", 8192), ("8193", 8193), ("1MiB", 1 << 20)] {
        let payload = vec![0x5a_u8; size];
        fs::write(&path, &payload).expect("benchmark read-all payload should be written");
        let allocations = sampled_allocations(|| {
            black_box(
                filesystem
                    .read_all(black_box(&path))
                    .expect("benchmark read-all should succeed"),
            );
        });
        eprintln!("ALLOC_SAMPLE read_all case={name} bytes={size} alloc_calls={allocations} scope=single_read_all");
        if size > 0 {
            group.throughput(criterion::Throughput::Bytes(size as u64));
        }
        group.bench_function(name, |bench| {
            bench.iter(|| {
                let bytes = filesystem
                    .read_all(black_box(&path))
                    .expect("benchmark read-all should succeed");
                black_box(bytes.len());
            });
        });
    }
    group.finish();
}

/// Compares std, Host, and Rooted metadata across repeatable path depths.
fn bench_deep_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("deep_metadata");
    for depth in [1usize, 8, 32, 64, 128] {
        let directory = tempdir().expect("deep metadata benchmark directory should exist");
        let mut relative = PathBuf::new();
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
        let finite = LocalFileSystem::rooted_with_options(
            directory.path(),
            &LocalRootOpenOptions::new().with_resolution_limits(
                LocalResolutionLimits::new().with_max_actual_components(
                    directory
                        .path()
                        .components()
                        .filter(|component| matches!(component, std::path::Component::Normal(_)))
                        .count()
                        .saturating_add(depth + 1),
                ),
            ),
        )
        .expect("finite-resolution Rooted filesystem should open");
        let policy = LocalFileSystem::rooted_with_options(
            directory.path(),
            &LocalRootOpenOptions::new().with_resolution_policy(Arc::new(NoopResolutionPolicy)),
        )
        .expect("policy Rooted filesystem should open");
        group.bench_function(format!("std/depth_{depth}"), |bench| {
            bench.iter(|| {
                let metadata =
                    fs::symlink_metadata(black_box(&physical)).expect("std deep metadata lookup should succeed");
                black_box(metadata.len());
            });
        });
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
        group.bench_function(format!("rooted_finite/depth_{depth}"), |bench| {
            bench.iter(|| {
                let metadata = finite
                    .metadata(black_box(&relative))
                    .expect("finite Rooted deep metadata lookup should succeed");
                black_box(metadata.len());
            });
        });
        group.bench_function(format!("rooted_policy/depth_{depth}"), |bench| {
            bench.iter(|| {
                let metadata = policy
                    .metadata(black_box(&relative))
                    .expect("policy Rooted deep metadata lookup should succeed");
                black_box(metadata.len());
            });
        });
    }
    group.finish();
}

criterion_group!(
    local_files,
    bench_path_codec,
    bench_vectored_io,
    bench_walk,
    bench_walk_name_cache,
    bench_walk_handle_budget,
    bench_tree_validation_index,
    bench_copy,
    bench_copy_budget_width,
    bench_copy_tree_depth,
    bench_writer,
    bench_rooted_writer,
    bench_writer_scenarios,
    bench_temp_directory_cleanup,
    bench_read_prefix,
    bench_read_all,
    bench_deep_metadata
);
criterion_main!(local_files);
