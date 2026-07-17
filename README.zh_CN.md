# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

面向 Rust 的本地文件系统工具库。

## 概述

Qubit Local Files 承载从 `qubit-io` 拆出的本地文件系统工具。它专注于具体本地路径和本地文件系统条目：临时文件和目录、文件名 helper、递归目录操作，以及持久化同目录 atomic write。

适合使用本 crate 的场景包括：

- 需要 drop 时自动清理的 RAII 临时文件或临时目录；
- 打开或写入本地文件前需要自动创建父目录；
- 需要基于目录 descriptor 锚定、可抵御攻击者路径替换的相对文件 I/O；
- 需要递归清理目录、计算目录大小或复制目录树；
- 需要默认拒绝意外覆盖的保守复制和持久化行为；
- 需要随机、portable 或 lexical 文件名 helper；
- 需要持久化替换写入，使读取方只能观察到旧完整文件或新完整文件。

详细用法、示例和 API 选择建议请参见[中文用户手册](doc/user_guide.zh_CN.md)。API 参考文档可在 [docs.rs](https://docs.rs/qubit-local-files) 查看。

如果需要 stream 层 `std::io` trait、extension method、wrapper 和 codec，请参考
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 安装

```toml
[dependencies]
qubit-local-files = "0.5"
```

## 快速示例

```rust
use std::io::Write;

use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};

let work = LocalTempDir::with_prefix(Some("qubit-local-files-readme-"))?;
let src = work.path().join("src");
let dst = work.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("manifest.json"), br#"{"version":1}"#)?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;
assert_eq!(1, stats.files);

LocalFiles::atomic_write(dst.join("manifest.json"), br#"{"version":2}"#)?;

let final_path = work.path().join("result.txt");
std::fs::write(&final_path, "old payload")?;

let mut temp = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
temp.write_all(b"new payload\n")?;
temp.persist_with(&final_path, LocalPersistOptions::new().with_overwrite())?;

assert_eq!("new payload\n", std::fs::read_to_string(&final_path)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

## 主要能力

### LocalFiles 命名空间

`LocalFiles` 集中提供容易在业务代码中反复出现的小型本地文件系统操作：

| 方法 | 用途 |
| --- | --- |
| `exists` | 以 `std::io::Result<bool>` 检查路径是否存在，不把检查错误静默折叠成 `false`。 |
| `metadata` | 读取本地路径 metadata。 |
| `list` | 列出目录直接子项。 |
| `open_reader` | 使用 `FileReadOptions` 把普通文件打开为 `LocalFileReader`；拒绝目录和特殊资源。 |
| `open_writer` | 使用 `FileWriteOptions` 打开或创建普通文件 `LocalFileWriter`；拒绝目录和特殊资源。 |
| `ensure_dir` | 创建目录及缺失祖先目录。 |
| `ensure_parent` | 为文件路径创建缺失父目录。 |
| `dir_size` | 统计目录下普通文件的总字节数，不跟随 symbolic link。 |
| `clean_dir` | 删除目录中的所有子项，但保留目录本身。 |
| `remove_any` | 删除文件、目录树或 symbolic link。 |
| `copy_dir_all_with` | 使用显式选项递归复制本地目录树，并返回统计信息。 |
| `atomic_write` | 通过持久化同目录临时写入替换文件。 |
| `atomic_write_with` | 与 `atomic_write` 相同，但向调用方写入逻辑传入受保护的 `LocalAtomicWriter`。 |
| `begin_atomic_write` | 返回由调用方显式提交的 streaming `LocalAtomicWriter`。 |

### 临时文件和临时目录

`LocalTempFile` 和 `LocalTempDir` 创建真实的本地文件系统条目，并在 drop 时自动删除，除非通过 `keep` 或 `persist` 释放所有权。Drop 阶段的清理是 best-effort；失败会通过 `log` 门面以 `warn!` 记录告警，不会 panic。

`LocalTempFile` 持有最初创建的文件句柄，并实现 `Write` 和 `Seek`。可通过 `as_file` / `as_file_mut` 直接访问句柄，通过 `close` 丢弃无缓冲句柄后，再用其他 API 读取该路径。`close` 不调用 `sync_all`；需要持久化保证时，应先显式同步句柄。它有意不提供读取 helper；确实需要读取时，通过 `LocalFiles` 或 `std::fs` 操作它的路径。

`LocalTempDir::child_path` 只对非空相对路径执行 lexical 校验并完成拼接；它不检查已有 symbolic link，返回结果也不能证明文件系统 containment。`ensure_child_dir`、`open_child_reader` 和 `open_child_writer` 还会拒绝其文件系统检查期间观察到的 symbolic-link escape。`ensure_child_dir` 会像 `mkdir -p` 一样创建多层缺失父目录。

文件系统校验与后续操作并非原子过程。当不可信参与者能够并发修改目录树时，这些 helper 不能作为 sandbox 边界。

`LocalTempFile::persist` 默认在移动操作中拒绝已存在的目标。只有确实要替换已有目标时，才使用 `LocalTempFile::persist_with` 和 `LocalPersistOptions::new().with_overwrite()`。`LocalTempDir::persist` 同样拒绝已存在的目标，并且不提供 overwrite 选项。持久化失败会返回持有原临时 guard 的 `LocalPersistError`，调用方可以重试或检查资源。持久化只使用原生 move/rename，不会回退到 copy-and-delete，因此跨文件系统移动可能在 Unix 上返回 `EXDEV`，或返回其他平台的等价错误。覆盖文件时会保留临时文件的权限，而不会保留被替换目标的权限；如果需要替换内容并保留已有普通文件的权限，请使用 `LocalFiles::atomic_write`。临时资源的相对创建目录和持久化目标会在资源创建或操作开始时绑定到当时的进程工作目录；`path`、child path helper、`keep`、`persist` 和 `persist_with` 返回绝对路径，之后即使工作目录变化也可直接使用。相对 atomic-write 目标同样会在写入开始时绑定，因此后续工作目录变化不会重定向提交或清理。在 Windows 上，原生移动不会添加 verbatim-path prefix，因此路径长度和 verbatim path 语义仍遵循原生平台行为。在 Unix 上，临时文件以 `0600`、临时目录以 `0700` 创建，之后仍受进程 umask 约束。

### 读写选项

普通文件打开操作有意保持显式：

| 类型 | 用途 |
| --- | --- |
| `FileReadOptions` | 控制 reader 是否缓冲。 |
| `FileWriteOptions` | 控制是否创建父目录、写入模式和 writer 是否缓冲。 |
| `FileBuffering` | 选择无额外缓冲，或使用可选容量的缓冲 I/O。 |
| `FileWriteMode` | 选择 `OpenExistingAtStart`、`CreateNew`、`CreateOrTruncate`、`AppendExisting` 或 `AppendOrCreate`。 |

两个打开 helper 都只返回普通文件。目录、FIFO、socket 和其他特殊文件系统
资源会被拒绝；在 Unix 上，拒绝 FIFO 时不会等待另一端连接。

`LocalFileReader` 实现 `Read` 和 `Seek`。`LocalFileWriter` 实现 `Write` 和
`Seek`，并提供 `sync_all` / `sync_data` helper；这些 helper 会先 flush
缓冲内容，再同步底层文件。对 writer 执行 seek 不会关闭 append-mode 语义。

自定义容量构造器返回 `std::io::Result` 并拒绝零容量。内部容量使用
`NonZeroUsize` 表示，因此无效 buffering policy 无法传给文件打开方法。

`atomic_write` 仍然是独立 API，因为它执行的是完整替换协议，而不是普通写句柄打开。

### Rooted Capability

`LocalRoot` 把所有后代操作锚定到一个已打开的目录 capability。
后代名称必须先构造为 `LocalRelativePath`；它只接受由普通 component 组成的
非空相对路径。`open_reader`、`open_writer` 和 `begin_atomic_write` 都从已打开
的 root descriptor 开始遍历，并拒绝中间项和最终项上的 symbolic link。
即使诊断路径被重命名或替换，已打开的 capability 也不会被重定向。

当前安全 backend 使用 Unix descriptor-relative 操作。其他目标上的
`LocalRoot::open` 返回 `std::io::ErrorKind::Unsupported`，不会回退到
check-then-path。需要抵御攻击者并发替换文件系统名字时应使用 `LocalRoot`；
path-based `LocalFiles` 和临时资源 helper 仍面向可信本地应用路径。

### Atomic Write

`LocalFiles::atomic_write` 会在同一父目录下写入临时文件，flush 并 sync 这个临时文件，替换目标，并在支持的平台上从深到浅 sync 目标父目录以及本次新建目录项所在的各级父目录。它适合配置文件、cache manifest、checkpoint、生成索引等 whole-file replacement 场景。原子 writer 开始时会对已有普通文件的权限取快照，commit 时应用该快照而不会重新读取目标权限；如果并发创建目标或修改权限且这些变更必须保留，调用方需要在外部进行协调。symlink 目标不会从其 link target 继承权限。在 Unix 上，新目标使用 `0600`，之后仍受更严格的进程 umask 约束。

streaming 内容可以使用 `LocalAtomicWriter`：

```rust
use std::io::Write;
use qubit_local_files::LocalFiles;

let mut writer = LocalFiles::begin_atomic_write("state.bin")?;
writer.write_all(b"complete state")?;
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LocalAtomicWriter` 实现 `Write`，但首版不实现 `Seek`。只有 `commit`
成功后目标才会被替换；调用 `abort` 或直接 drop 会保留原目标并清理 staging
文件。自 `0.5.0` 起，配置类型的字段不再公开，调用方必须使用现有 getter、
constructor 和 builder。这个既有 writer 仍是 path-based；需要把替换限制在
锚定 root 下时使用 `LocalRootAtomicWriter`。
`atomic_write_with` 会把同一个受保护 writer 临时借给 callback。callback
可以写入 staging 内容，但不能 clone、保留、seek，也不能访问底层文件或 raw
handle，因此 callback 返回后无法继续修改已提交 inode。

失败时返回 `LocalAtomicWriteError`，其中包含失败阶段、临时路径、原始 I/O source error、替换是否已经提交，以及删除未提交 staging file 时产生的 secondary cleanup error。
如果 `atomic_write_with` callback panic，会先关闭并 best-effort 删除未提交临时文件，再继续传播 panic。清理失败不能替换原 panic，因此这种情况下 staging path 可能残留。

该操作不是多文件事务，也不协调并发写入。如果多个进程或线程可能同时替换同一路径，需要使用外部锁。

### 递归目录复制

`LocalFiles::copy_dir_all_with` 复制目录树并返回 `LocalCopyDirStats`：

| 字段 | 含义 |
| --- | --- |
| `files` | 已复制的普通文件数量。 |
| `directories` | 已创建的目标目录数量。 |
| `bytes` | 从普通文件复制的字节数。 |
| `skipped` | 因冲突策略而跳过的已有目标文件数量。 |

`LocalCopyDirOptions::default()` 是有意保守的默认值：`conflict` 和 `type_conflict` 均为 `Fail`，不跟随 symbolic link，也不保留源权限。通过 `LocalCopyConflictPolicy` 显式选择 `Overwrite` 或 `Skip`；文件/目录类型替换则必须单独设置 `LocalCopyTypeConflictPolicy::Replace`。复制失败返回 `LocalCopyDirError`，其中包含路径、失败阶段、部分统计、可选 staging path、可选次级 cleanup error 和原始 I/O source error。

默认不保留源权限。在 Unix 上，新建或替换的文件因此使用 `0600`，新建目录使用 `0700`，之后仍受更严格的进程 umask 约束。原始复制或提交错误保持为主 source error。

递归复制不是目录树级事务。失败前已经提交的条目会留在目标中，不会执行回滚；破坏性的类型冲突替换还可能先删除已有目标目录，随后才在后续操作中失败。

源路径检查、源文件打开、目标复查和破坏性替换是彼此分离的 path-based 操作。symlink 策略用于避免普通的意外穿越；当其他参与者能够并发修改任一目录树时，它不是可抵御攻击者的 sandbox 边界。

### 文件名 Helper

`LocalFilenames` 提供随机和 lexical 文件名工具：

| 方法组 | 用途 |
| --- | --- |
| `random`、`random_with` | 构造随机文件名 component，生成失败时 panic。 |
| `try_random`、`try_random_with` | 通过 `std::io::Result` 构造随机文件名 component。 |
| `validate_portable_file_name` | 校验保守 portable 的单 component 文件名。 |
| `file_name`、`file_stem`、`file_prefix` | 按 `Path` 语义提取 UTF-8 path component。 |
| `extension`、`dot_extension`、`has_extension` | 检查最终扩展名。 |
| `has_extension_ignore_ascii_case` | 使用 ASCII-only 大小写折叠检查最终扩展名。 |
| `file_name_from_path` | 从 path-like 字符串中提取最后一段。 |
| `file_name_from_url` | 提取 URL 最后一个 path segment，并解码安全的 percent-encoded UTF-8。 |

这些 lexical helper 不访问文件系统。返回文件名数据的公开方法返回 UTF-8 字符串，而不是 `OsStr`；无效 UTF-8 path component 返回 `None`。
portable 校验还会拒绝使用上标数字的 Windows device name，包括 `COM¹`、`COM²`、`COM³`、`LPT¹`、`LPT²` 和 `LPT³`；这一行为遵循 [Microsoft 文件命名规则](https://learn.microsoft.com/zh-cn/windows/win32/fileio/naming-a-file)。

## Crate 边界

`qubit-local-files` 有意只覆盖本地文件系统相关能力。它不提供：

- stream extension trait、binary codec 或 stream wrapper；
- 异步文件系统 API 或 runtime 集成；
- 远程文件系统、FTP、S3、对象存储或 VFS 抽象；
- file watching、globbing 或通用目录遍历框架；
- 锁或跨进程写入协调。

stream 和字节 I/O 相关能力请使用
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 运行时依赖

本 crate 运行时依赖 Rust 标准库、`getrandom`、`libc` 和 `log`。`getrandom`
用于生成随机临时名，`libc` 用于 Unix descriptor-relative rooted 操作和原生
rename 支持，`log` 用于 drop 阶段的清理失败告警。

## 测试

```bash
# 使用默认的空 feature 集测试核心 API
cargo test --no-default-features

# 测试核心 API 和正则校验
cargo test --all-features

# 运行项目 CI 检查
./ci-check.sh

# 检查代码覆盖率
./coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./align-ci.sh`格式化代码，运行`./ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
