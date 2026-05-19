# Qubit Local FS

[![Rust CI](https://github.com/qubit-ltd/rs-local-fs/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-fs/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-fs/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-fs/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-fs.svg?color=blue)](https://crates.io/crates/qubit-local-fs)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

面向 Rust 的本地文件系统工具库。

## 概述

Qubit Local FS 承载从 `qubit-io` 拆出的本地文件系统工具：

- `Files`：父目录创建、buffered file helper、目录清理、目录大小、递归目录复制和
  持久化 atomic write；
- `Filenames`：随机文件名和 lexical 文件名操作；
- `TempFile` 和 `TempDir`：RAII 临时文件和临时目录；
- `CopyDirOptions` 和 `CopyDirStats`：显式递归复制行为和复制统计。

`qubit-io` 继续只关注 stream 层 `std::io` trait、extension method、wrapper 和 codec。

## 安装

```toml
[dependencies]
qubit-local-fs = "0.1"
```

## 临时文件和临时目录

`TempFile` 和 `TempDir` 会创建真实的临时文件系统条目，并在 drop 时自动删除，除非调用方
调用 `keep` 或 `persist`。Drop 阶段的清理是 best-effort；失败会通过 `log` 门面以
`warn!` 记录告警，不会 panic。

```rust
use std::io::Write;

use qubit_local_fs::{TempDir, TempFile};

let dir = TempDir::with_prefix(Some("qubit-local-fs-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

let mut file = TempFile::with_name(Some("qubit-local-fs-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

## Atomic Write

当文件不能被外部观察到“只写了一半”的状态时，使用 `Files::atomic_write`。它会使用同目录
临时文件写入，flush 并 sync 临时文件，替换目标文件，并在支持的平台上 sync 父目录。

```rust
use qubit_local_fs::{Files, TempDir};

let dir = TempDir::with_prefix(Some("qubit-local-fs-atomic-"))?;
let path = dir.path().join("state").join("manifest.json");

Files::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

## 主要 API

| API | 用途 |
| --- | --- |
| `Files::open_buffered_reader` | 以 `BufReader<File>` 形式打开文件。 |
| `Files::ensure_dir` | 创建目录及缺失祖先目录。 |
| `Files::ensure_parent` | 为文件路径创建缺失父目录。 |
| `Files::create_file_with_parent` | 创建缺失父目录后创建文件。 |
| `Files::create_buffered_writer_with_parent` | 创建缺失父目录后创建 `BufWriter<File>`。 |
| `Files::dir_size` | 统计目录下普通文件的总字节数，不跟随 symbolic link。 |
| `Files::clean_dir` | 删除目录中的所有子项，但保留目录本身。 |
| `Files::remove_any` | 删除文件、目录树或 symbolic link。 |
| `Files::copy_dir_all_with` | 使用显式复制选项递归复制本地目录树，并返回复制统计。 |
| `Files::atomic_write` | 执行持久化同目录 atomic file replacement。 |
| `Files::atomic_write_with` | 与 `atomic_write` 相同，但由调用方提供写入逻辑。 |
| `TempFile` | 临时文件 guard，drop 时删除文件，除非调用了 `keep` 或 `persist`。 |
| `TempDir` | 临时目录 guard，drop 时递归删除目录树，除非调用了 `keep` 或 `persist`。 |
| `Filenames` | 随机文件名和 lexical UTF-8 文件名 helper。 |
| `CopyDirOptions` | 控制递归目录复制行为的选项。 |
| `CopyDirStats` | 递归目录复制操作返回的统计信息。 |

## 运行时依赖

本 crate 运行时依赖 Rust 标准库、`getrandom` 和 `log`。`getrandom` 用于生成随机临时名，
`log` 用于 drop 阶段的清理失败告警。
