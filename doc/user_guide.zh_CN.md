# Qubit Local Files 用户手册

Qubit Local Files 提供从 `qubit-io` 拆出的本地文件系统工具。它覆盖本地路径、文件名、
临时文件系统条目、递归目录操作和持久化 atomic write。

## 导入方式

```rust
use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFilenames,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};
```

## 临时目录

当临时目录通常应该自动清理时，使用 `LocalTempDir`。目录会立即创建，并在 guard drop 时递归
删除。调用 `keep` 可以保留生成位置；调用 `persist` 可以移动到最终路径。

```rust
use qubit_local_files::LocalTempDir;

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

`Drop` 中的清理是 best-effort。如果删除失败，`LocalTempDir` 会通过 `log` 门面记录 warning，
不会 panic。

## 临时文件

当你既需要唯一路径，又需要一个已经打开的文件句柄时，使用 `LocalTempFile`。除非调用 `keep`
或 `persist`，否则文件会在 drop 时删除。

```rust
use std::io::Write;

use qubit_local_files::LocalTempFile;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

`LocalTempFile::persist` 会先关闭文件句柄，再把临时文件移动到最终路径，并默认在移动操作中拒绝
已存在的目标。确实要替换已有目标时，使用 `LocalTempFile::persist_with` 和
`LocalPersistOptions { overwrite: true }`。如果目标文件不能被外部观察到“只写了一半”，优先使用
`LocalFiles::atomic_write`。

## Atomic Write

`LocalFiles::atomic_write` 会在同一父目录下写入临时文件，flush 并 sync 这个临时文件，替换目标，
并在支持的平台上 sync 父目录。

```rust
use qubit_local_files::{LocalFiles, LocalTempDir};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-guide-"))?;
let path = dir.path().join("state").join("manifest.json");

LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

当内容生成逻辑需要直接使用临时文件句柄时，使用 `LocalFiles::atomic_write_with`。

## 目录 Helper

`LocalFiles` 提供本地目录 helper：

- `ensure_dir` 创建目录及缺失祖先目录；
- `ensure_parent` 为文件路径创建缺失父目录；
- `dir_size` 统计普通文件总字节数，不跟随 symbolic link；
- `clean_dir` 删除所有子项但保留目录自身；
- `remove_any` 删除文件、目录树或 symbolic link；
- `copy_dir_all_with` 使用显式选项递归复制目录树。

```rust
use qubit_local_files::{LocalCopyDirOptions, LocalFiles, LocalTempDir};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-copy-"))?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;
assert_eq!(1, stats.files);

# Ok::<(), std::io::Error>(())
```

## 文件名 Helper

`LocalFilenames` 提供不访问文件系统的 lexical helper。返回文件名数据的公开方法返回 UTF-8
字符串（`&str` 或 `String`），而不是 `OsStr`；无效 UTF-8 path component 返回 `None`。

```rust
use std::path::Path;

use qubit_local_files::LocalFilenames;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), LocalFilenames::file_stem(path));
assert_eq!(Some("gz"), LocalFilenames::extension(path));
assert!(LocalFilenames::has_extension(path, ".gz"));

let name = LocalFilenames::try_random_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));

# Ok::<(), std::io::Error>(())
```

## 路径长度和平台限制

`LocalTempFile` 和 `LocalTempDir` 创建的是本地文件系统条目；如果创建失败，会返回操作系统错误。
它们不承诺生成的路径适用于所有平台 API。某些 API，例如 Unix domain socket，有比普通文件
短得多的路径限制。遇到这类场景，应在较短的父目录下创建临时条目，例如 `/tmp`。
