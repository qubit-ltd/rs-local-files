# Qubit Local Files 用户手册

Qubit Local Files 是 Qubit Rust crate 家族中的本地文件系统 crate。它专注于具体本地路径、文件名、临时文件系统条目、递归目录操作，以及持久化同目录 atomic write。它有意不做 stream codec crate，也不做远程文件系统抽象。

如果需要 stream 层 `std::io` trait、extension method、wrapper 和 codec，请参考
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 何时使用本 crate

当代码处理的是本地文件系统路径，而不是 generic 字节流时，适合使用 `qubit-local-files`。典型场景包括 CLI 工具、代码生成器、cache writer、checkpoint 文件、本地导入导出任务、解包后的工作目录，以及需要临时本地文件的测试。

适合的场景：

- 创建通常应该自动清理的临时文件或临时目录。
- 成功完成工作后保留或持久化临时条目。
- 持久化临时文件时拒绝意外覆盖。
- 只有在显式指定 `LocalPersistOptions { overwrite: true }` 时才替换已有文件。
- 打开、写入或持久化文件前创建父目录。
- 通过同目录临时文件 atomic replacement 完整替换文件。
- 使用显式覆盖和 symlink 策略复制本地目录树。
- 保留目录自身，只清理目录内容。
- 在不跟随 symbolic link 的前提下计算本地目录大小。
- 生成随机文件名 component 或校验 portable 文件名。

不适合的场景：

- 读取、写入、比较、限制或编码任意字节流。
- 实现 binary、LEB128、ZigZag 或 length-prefixed string codec。
- 用一个 API 抽象本地、FTP、对象存储或远程文件系统。
- 监听文件变化。
- 用锁协调并发写入。
- 提供绑定某个 runtime 的异步文件系统 API。

这些 stream 和字节 I/O 能力请使用
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 安装

```toml
[dependencies]
qubit-local-files = "0.1"
```

## 导入方式

从 crate root 导入具体命名空间、guard 和 option struct：

```rust
use qubit_local_files::{
    FileBuffering,
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalCopyDirOptions,
    LocalFilenames,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};
```

本 crate 当前不暴露 prelude。显式导入可以让文件系统副作用和覆盖策略在调用点保持清晰。

## 读写选项

普通文件打开由显式 option struct 控制：

| 类型 | 字段 | 用途 |
| --- | --- | --- |
| `FileReadOptions` | `buffering` | 控制 `open_reader` 返回无额外缓冲 reader，还是 buffered reader。 |
| `FileWriteOptions` | `create_parent`、`mode`、`buffering` | 控制是否创建父目录、写入模式和 writer 是否缓冲。 |
| `FileBuffering` | `Unbuffered`、`Buffered { capacity }` | 选择原始文件 I/O，或带可选容量的 `BufReader` / `BufWriter`。 |
| `FileWriteMode` | enum variants | 选择目标文件的写入打开方式。 |

写入模式：

| 模式 | 行为 |
| --- | --- |
| `OpenExistingAtStart` | 打开已存在文件，从 offset zero 开始写入，不截断。 |
| `CreateNew` | 创建新文件，目标已存在时报错。 |
| `CreateOrTruncate` | 目标缺失时创建，目标已存在时截断。这是默认值。 |
| `AppendExisting` | 追加到已存在文件，目标缺失时报错。 |
| `AppendOrCreate` | 追加到已存在文件，目标缺失时创建。 |

`LocalFiles::atomic_write` 有意不并入 `FileWriteOptions`。它执行的是完整的持久化替换协议，而不是返回普通写句柄。

## 临时目录

当临时目录通常应该自动清理时，使用 `LocalTempDir`。目录会立即创建，并在 guard drop 时递归删除。

```rust
use qubit_local_files::LocalTempDir;

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

创建方法：

| 方法 | 用途 |
| --- | --- |
| `LocalTempDir::new` | 使用默认前缀在 `std::env::temp_dir()` 中创建临时目录。 |
| `LocalTempDir::with_prefix` | 使用自定义前缀在 `std::env::temp_dir()` 中创建临时目录。 |
| `LocalTempDir::in_dir` | 在调用方指定的父目录和重试次数下创建临时目录。 |

所有权方法：

| 方法 | 行为 |
| --- | --- |
| `path` | 借用生成的目录路径。 |
| `exists` | 检查目录路径是否存在，返回 `std::io::Result<bool>`。 |
| `metadata` | 读取目录 metadata。 |
| `list` | 列出直接子项。 |
| `child_path` | 解析安全相对 child 路径，但不创建它。 |
| `ensure_child_dir` | 创建 child 目录和缺失父目录，语义类似 `mkdir -p`。 |
| `open_child_reader` | 使用 `FileReadOptions` 打开 child 文件 reader。 |
| `open_child_writer` | 使用 `FileWriteOptions` 打开 child 文件 writer。 |
| `cleanup` | 立即删除目录，并关闭后续 drop 清理。 |
| `keep` | 消费 guard，并把目录留在生成路径。 |
| `persist` | 把目录移动到最终路径，并关闭自动清理。 |

`LocalTempDir::persist` 会为目标创建缺失父目录，并拒绝已存在目标。它不提供 overwrite 选项。如果移动失败，guard 仍然拥有该临时目录，并会在 drop 时清理。

child 路径必须是非空相对路径，并且只能由 normal path component 组成。绝对路径、root 或 prefix component、`.` 和 `..` 都会被拒绝。`open_child_reader` 要求 child 是文件；目录或其他非文件条目会返回 `ErrorKind::InvalidInput`。`open_child_writer` 会校验已存在目标必须是文件，并确保 child 写入留在临时目录内。`ensure_child_dir` 会创建缺失的多层父目录，但在创建目录时会拒绝 symbolic link component，避免通过 child 路径离开临时目录。

`Drop` 中的清理是 best-effort。如果删除失败，`LocalTempDir` 会通过 `log` 门面记录 warning，不会 panic。

## 临时文件

当你需要一个唯一临时文件路径和一个由 guard 持有的 writer 时，使用 `LocalTempFile`。除非调用 `keep` 或 `persist`，否则文件会在 drop 时删除。

```rust
use std::io::Write;

use qubit_local_files::{
    FileWriteMode,
    FileWriteOptions,
    LocalTempFile,
};

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
file.writer(FileWriteOptions::new(FileWriteMode::CreateOrTruncate).buffered())?
    .write_all(b"temporary payload\n")?;
file.close()?;

# Ok::<(), std::io::Error>(())
```

创建方法：

| 方法 | 用途 |
| --- | --- |
| `LocalTempFile::new` | 使用默认前缀在 `std::env::temp_dir()` 中创建临时文件。 |
| `LocalTempFile::with_name` | 使用自定义前缀和后缀在 `std::env::temp_dir()` 中创建临时文件。 |
| `LocalTempFile::in_dir` | 在调用方指定的父目录和重试次数下创建临时文件。 |

writer 和所有权方法：

| 方法 | 行为 |
| --- | --- |
| `path` | 借用生成的文件路径。 |
| `exists` | 检查文件路径是否存在，返回 `std::io::Result<bool>`。 |
| `metadata` | 读取文件 metadata。 |
| `writer` | 使用 `FileWriteOptions` 配置并返回内部 `LocalFileWriter`。 |
| `close` | flush 并关闭 writer，但保留路径清理。 |
| `cleanup` | 立即删除文件，并关闭后续 drop 清理。 |
| `keep` | flush、关闭、消费 guard，并把文件留在生成路径。 |
| `persist` | 不覆盖地把文件移动到最终路径。 |
| `persist_with` | 使用 `LocalPersistOptions` 把文件移动到最终路径。 |

第一次 `writer(options)` 调用会配置内部 writer。后续调用必须传入相同 options，并返回同一个 writer；不同 options 会以 `ErrorKind::InvalidInput` 拒绝。因为 `LocalTempFile` 会先创建文件再配置 writer，所以 `FileWriteMode::CreateNew` 会返回 `ErrorKind::AlreadyExists`。

`LocalTempFile` 有意不提供读取 helper。临时文件的常见用法是写入、关闭，然后持久化。如果确实需要检查内容，先调用 `close`，再通过 `LocalFiles::open_reader` 或 `std::fs` 读取 `path()`。

`LocalTempFile::persist` 会 flush 并关闭 writer，为目标创建缺失父目录，并通过 no-clobber move 操作拒绝已存在目标。它有意不依赖单独的 metadata precheck。这可以在支持的平台上避免 time-of-check/time-of-use 覆盖竞态。

只有覆盖策略确实不同的时候才使用 `persist_with`：

```rust
use std::io::Write;

use qubit_local_files::{
    FileWriteMode,
    FileWriteOptions,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-persist-"))?;
let target = dir.path().join("result.txt");
std::fs::write(&target, "old")?;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
file.writer(FileWriteOptions::new(FileWriteMode::CreateOrTruncate))?
    .write_all(b"new\n")?;

file.persist_with(&target, LocalPersistOptions { overwrite: true })?;

assert_eq!("new\n", std::fs::read_to_string(&target)?);

# Ok::<(), std::io::Error>(())
```

如果目标文件不能被外部观察到“只写了一半”，最终文件替换优先使用 `LocalFiles::atomic_write`。

## Atomic Write

`LocalFiles::atomic_write` 会在同一父目录下写入临时文件，flush 并 sync 这个临时文件，替换目标，并在支持的平台上 sync 父目录。

```rust
use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-guide-"))?;
let path = dir.path().join("state").join("manifest.json");

LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

当内容生成逻辑需要直接使用临时文件句柄时，使用 `LocalFiles::atomic_write_with`：

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-json-"))?;
let path = dir.path().join("state.json");

LocalFiles::atomic_write_with(&path, |file| {
    writeln!(file, "{{\"complete\":true}}")
})?;

assert_eq!("{\"complete\":true}\n", std::fs::read_to_string(&path)?);

# Ok::<(), std::io::Error>(())
```

重要语义：

- 写入前会创建父目录。
- 临时文件创建在目标目录下，因此在常见本地文件系统上可以 atomic replacement。
- 如果目标已有普通文件，会在替换前把已有权限复制到临时文件。
- 如果写入、flush 或 sync 临时文件失败，目标保持不变。
- 如果替换已经成功，但 sync 父目录失败，方法可能在目标已经包含新内容后返回错误。
- 如果目标路径是 symbolic link，并且平台 rename-over-symlink 语义是替换 link 本身，则该 link 会被新普通文件替换，原 link target 保持不变。
- 该操作不是多文件事务，也不协调并发写入。

## 文件和目录 Helper

`LocalFiles` 提供小型本地文件系统 helper：

| 方法 | 行为 |
| --- | --- |
| `exists` | 检查路径是否存在，并且不吞掉检查错误。 |
| `metadata` | 通过 `std::fs::metadata` 读取路径 metadata。 |
| `list` | 列出目录直接子项。 |
| `open_reader` | 使用 `FileReadOptions` 打开 `LocalFileReader`。 |
| `open_writer` | 使用 `FileWriteOptions` 打开 `LocalFileWriter`。 |
| `ensure_dir` | 创建目录及缺失祖先目录。 |
| `ensure_parent` | 为文件路径创建缺失父目录。没有父目录的路径会被接受。 |
| `dir_size` | 统计目录下普通文件总字节数，不跟随 symbolic link。 |
| `clean_dir` | 删除所有子项但保留目录自身。 |
| `remove_any` | 删除文件、目录树或 symbolic link。 |

示例：

```rust
use std::io::Write;

use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-helpers-"))?;
let path = dir.path().join("nested").join("data.txt");

let mut writer = LocalFiles::open_writer(
    &path,
    FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
        .with_parent()
        .buffered(),
)?;
writer.write_all(b"payload")?;
writer.close()?;

let mut reader = LocalFiles::open_reader(&path, FileReadOptions::buffered())?;
let mut payload = String::new();
std::io::Read::read_to_string(&mut reader, &mut payload)?;
assert_eq!("payload", payload);

assert_eq!(7, LocalFiles::dir_size(dir.path())?);
LocalFiles::clean_dir(dir.path())?;
assert_eq!(0, LocalFiles::dir_size(dir.path())?);

# Ok::<(), std::io::Error>(())
```

`dir_size` 和 `clean_dir` 要求根路径是目录。symbolic link 不会被跟随。`remove_any` 会删除 link 本身，包括指向目录的 link。

## 递归目录复制

当目录树复制需要显式覆盖策略和 symlink 策略时，使用 `LocalFiles::copy_dir_all_with`。

```rust
use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-copy-"))?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;

assert_eq!(1, stats.files);
assert_eq!(1, stats.directories);
assert_eq!(4, stats.bytes);

# Ok::<(), std::io::Error>(())
```

选项：

| 选项 | 默认值 | 行为 |
| --- | --- | --- |
| `overwrite` | `false` | 已存在的目标文件或非目录条目会被拒绝。 |
| `follow_symlinks` | `false` | 源目录树中的 symbolic link 会被拒绝。 |
| `preserve_permissions` | `false` | 不把源权限复制到目标条目。 |

统计信息：

| 字段 | 含义 |
| --- | --- |
| `files` | 已复制的普通文件数量。 |
| `directories` | 已创建的目标目录数量。 |
| `bytes` | 从普通文件复制的字节数。 |

复制操作会拒绝位于源目录内部的目标，因为把目录复制进自身可能导致无限递归。当启用 symlink following 时，由跟随 symbolic link 引入的目录环也会被拒绝。不支持的源条目会返回 `std::io::ErrorKind::Unsupported`。

## 文件名 Helper

`LocalFilenames` 提供不访问文件系统的 lexical helper。返回文件名数据的方法返回 UTF-8 字符串（`&str` 或 `String`），而不是 `OsStr`；无效 UTF-8 path component 返回 `None`。

```rust
use std::path::Path;

use qubit_local_files::LocalFilenames;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), LocalFilenames::file_stem(path));
assert_eq!(Some("archive"), LocalFilenames::file_prefix(path));
assert_eq!(Some("gz"), LocalFilenames::extension(path));
assert_eq!(Some(".gz".to_owned()), LocalFilenames::dot_extension(path));
assert!(LocalFilenames::has_extension(path, ".gz"));
assert!(LocalFilenames::has_extension_ignore_ascii_case(path, "GZ"));

let name = LocalFilenames::try_random_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));
assert!(name.ends_with(".tmp"));

# Ok::<(), std::io::Error>(())
```

当调用方提供的名称应该是跨常见平台保守安全的单 path component 时，使用 `validate_portable_file_name`：

```rust
use std::io::ErrorKind;

use qubit_local_files::LocalFilenames;

LocalFilenames::validate_portable_file_name("report.csv")?;

let error = LocalFilenames::validate_portable_file_name("CON.txt")
    .expect_err("Windows reserved names are rejected");
assert_eq!(ErrorKind::InvalidInput, error.kind());

# Ok::<(), std::io::Error>(())
```

portable 校验是 lexical 的。它不检查当前文件系统权限、mount option、Unicode normalization，或每个文件系统独有的限制。

对于还不是 `Path` 的字符串，可以使用字符串 helper：

```rust
use qubit_local_files::LocalFilenames;

assert_eq!("file.txt", LocalFilenames::file_name_from_path(r"C:\tmp\file.txt"));
assert_eq!(
    "report 2026.csv",
    LocalFilenames::file_name_from_url("https://example.test/files/report%202026.csv?download=1"),
);
```

`file_name_from_url` 会先去掉 query 和 fragment，再选择最后一个 slash-delimited segment。只有当 percent-encoded UTF-8 解码后仍然是安全的单文件名 fragment 时，它才返回解码结果。

## 错误和清理模型

大多数 API 返回 `std::io::Result`，并尽量保留 `std::io::ErrorKind`。

重要错误行为：

- 临时文件持久化目标已存在时会被拒绝，除非显式设置 `LocalPersistOptions { overwrite: true }`。
- 临时目录持久化目标已存在时会被拒绝。
- 递归复制遇到已存在目标条目时会被拒绝，除非显式设置 `LocalCopyDirOptions { overwrite: true, .. }`。
- 递归复制遇到 symbolic link 时会被拒绝，除非显式设置 `LocalCopyDirOptions { follow_symlinks: true, .. }`。
- Drop 阶段清理失败会通过 `log::warn!` 记录，不会 panic。
- `LocalTempFile::writer` 在 `close` 之后返回 `ErrorKind::NotFound`。
- `LocalTempFile::writer` 如果已经用不同 options 配置过，会返回 `ErrorKind::InvalidInput`。
- `LocalTempDir` child API 会在不安全 child 路径、child reader 目标不是文件、以及通过 symbolic link 离开临时目录时返回 `ErrorKind::InvalidInput`。

## 路径长度和平台限制

`LocalTempFile` 和 `LocalTempDir` 创建的是本地文件系统条目；如果创建失败，会返回操作系统错误。它们不承诺生成的路径适用于所有平台 API。某些 API，例如 Unix domain socket，有比普通文件短得多的路径限制。遇到这类场景，应在较短的父目录下创建临时条目，例如 `/tmp`。

## Crate 边界

`qubit-local-files` 有意把本地文件系统工具从 `qubit-io` 中分离出来。需要本地路径、临时文件和目录、递归目录操作、目录清理、文件名 helper 和 atomic file write 时，使用本 crate。

需要 stream trait、extension method、stream wrapper、内容比较、有界读取或 binary codec 时，使用
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 测试和 CI

本项目包含公开 helper、临时条目、覆盖语义、递归复制行为、文件名校验、atomic write 和平台相关边界情况的测试。

常用命令：

```bash
cargo test
./coverage.sh
./coverage.sh text
./ci-check.sh
```
