# Qubit Local Files 本地文件系统设计

> 状态：已批准的目标设计。本文定义 `qubit-local-files` 的公共边界与平台语义；
> 当前实现已完成统一入口、类型化 copy/rename failure、rooted temporary resource
> 与路径 codec。本文仍保留尚未实施的 persistence option 与扩展状态模型目标。

## 1. 定位

`qubit-local-files` 是不依赖 `qubit-fs` 的本地文件系统能力层。它直接面向 native
path 和操作系统文件 API，为以下使用者提供统一实现：

- 只需要本地文件系统、不需要 provider-neutral 抽象的应用；
- `qubit-fs-local` 等上层 adapter；
- 需要 root containment、临时资源、递归遍历、可靠 publication 或跨平台语义的库。

所有复杂平台逻辑必须在本 crate 内实现。上层 adapter 不复制 Unix、Windows 或其他
平台分支。

## 2. 目标与非目标

目标：

1. 提供 host-wide 与 rooted 两种本地 authority；
2. 对 copy、walk、temp、publication、durability 和 root 防逃逸提供可复用语义；
3. 在 Unix 与 Windows 上使用各自可靠的 descriptor/handle-relative 原语；
4. 所有操作返回结构化结果和错误，部分成功不能压缩成普通 I/O error；
5. 相对路径、symlink、hard link、overwrite 和部分成功有明确规则；
6. 公共工具由统一类型的关联方法或实例方法组织，不提供散落的 public free function；
7. 上层无需理解平台条件编译即可使用完整业务逻辑。

非目标：

- 定义 `qubit_fs::Path`、URI、provider capability 或 registry；
- 返回 `qubit_fs::FsError`；
- 把本地路径强制转换成 UTF-8 provider path；
- 为远程或对象存储提供实现；
- 通过 canonicalize 后的字符串比较代替 descriptor-relative 安全；
- 承诺绕过操作系统本身无法避免的所有 TOCTOU race。

## 3. 依赖边界

```text
只使用本地文件系统的应用
              │
              ▼
       qubit-local-files
              ▲
              │ native operation/result/error
       qubit-fs-local
              ▲
              │ FileSystemSpi
          qubit-fs
```

`qubit-local-files` 不依赖 `qubit-fs` 或 `qubit-fs-registry`。公共类型使用
`std::path::Path`、`PathBuf`、native metadata 和本 crate 自己的 option、outcome、
error。

## 4. 公共类型组织

### 4.1 Host filesystem

Host-wide 操作由零变体 enum 的关联方法组织：

```rust
pub enum LocalFileSystem {}

impl LocalFileSystem {
    pub fn metadata(path: &Path) -> LocalResult<LocalFileMetadata>;
    pub fn open_reader(
        path: &Path,
        options: &LocalReadOptions,
    ) -> LocalResult<LocalFileReader>;
    pub fn open_writer(
        path: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter>;
    pub fn copy(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
    ) -> LocalCopyResult;
    pub fn list(
        path: &Path,
        options: &LocalListOptions,
    ) -> LocalResult<LocalDirectoryWalker>;
    pub fn create_directory(
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome>;
    pub fn delete_file(
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome>;
    pub fn delete_directory(
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome>;
    pub fn rename(
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult;
    pub fn create_temp_file(
        options: &LocalTempFileOptions,
    ) -> LocalResult<LocalTempFile>;
    pub fn create_temp_directory(
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory>;
}
```

零变体 enum 表达“不可实例化的关联方法命名空间”，避免 free function 和无意义的
unit struct value。

### 4.2 Rooted filesystem

Rooted authority 是有状态类型：

```rust
pub struct RootedLocalFileSystem {
    // 已打开的 root descriptor/handle 与非权威诊断路径
}
```

它通过 `RootedLocalFileSystem::open` 建立 root，后续操作使用实例方法。安全性依赖已
打开 root descriptor/handle，不依赖 root path 字符串在之后仍指向同一目录。

### 4.3 路径与文件名工具

无状态工具按类型组织：

```rust
pub enum LocalFileNames {}
pub enum LocalPaths {}
pub enum LocalPathCodec {}
```

`LocalFileNames` 负责 native filename 校验、随机安全名称和保留名称规则；
`LocalPaths` 负责路径分类、安全 child/descendant 组合、相对路径绑定和平台 prefix
检查；`LocalPathCodec` 负责 native string 与可逆 canonical UTF-8 text 之间的
平台相关编解码。

`LocalPaths::bind_host_path` 绑定单个路径；
`LocalPaths::bind_host_paths([source, target])` 等一次性绑定同一操作的多个路径。
后者只读取一次 current working directory，避免 rename/copy 的 source 与 target
因并发修改进程工作目录而落入不同 namespace。

Adapter 需要的完整 native path 组合也由 `LocalPaths` 组织：

```rust
impl LocalPaths {
    pub fn from_canonical_absolute_components<'a>(
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf>;

    pub fn from_canonical_relative_components<'a>(
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf>;

    pub fn to_canonical_absolute_components(
        path: &Path,
    ) -> LocalResult<Vec<String>>;

    pub fn to_canonical_relative_components(
        path: &Path,
    ) -> LocalResult<Vec<String>>;
}
```

这些方法内部使用 `LocalPathCodec`，并负责 separator、root、prefix、drive 与 component
边界；上层不能逐 component `PathBuf::push` 后再复制一套 Windows/Unix 判断。Windows
absolute host path 第一版使用明确的 drive-absolute canonical form
`/<drive>:/...`；UNC/remote authority 在没有独立、无歧义的 provider authority
映射前返回 unsupported。Rooted conversion 始终使用 relative form，不接受 drive、
UNC 或其他 prefix。

`LocalPaths::is_lexically_within` 返回 `LocalResult<bool>`，拒绝含 `.`/`..` 或
absolute/relative form 不一致的输入。它只提供 early lexical classification；copy
遇到错误时跳过这项 optimization，而不是拒绝合法 native path。Copy 和 rooted
authority 仍必须通过 entry identity、ancestor handle 与 no-follow traversal 证明真实
containment。

读取 filename、stem、prefix、extension 时返回 `OsStr`/`OsString`，不得为了便利先转成
UTF-8 或 lossy string。只有名称本身定义为 portable text 的 API 才接受或返回 `str`。

`LocalPathCodec` 使用零变体 enum 的关联方法组织，不依赖 `qubit-fs`：

```rust
impl LocalPathCodec {
    pub fn encode<'a>(
        text: &'a str,
    ) -> Result<Cow<'a, OsStr>, LocalPathCodecError>;

    pub fn decode<'a>(
        native: &'a OsStr,
    ) -> Result<Cow<'a, str>, LocalPathCodecError>;
}
```

这里的 canonical text 是 native filename/path component 的可逆 UTF-8 表示，不是 URI
percent-decoding，也不解释 provider hierarchy：

- 初始实现把当前 `qubit-fs` 的 canonical native path text 算法原样下沉，不另造第二
  套编码；
- 合法、非 control 的 Unicode scalar 原样保留；
- `%`、control character 的 UTF-8 byte、无效 UTF-8 byte 及 Windows WTF-8 中表示
  非配对 surrogate 的 byte 使用 uppercase `%HH`；
- decode 后必须重新 encode 并与输入完全相等，因此 lowercase escape、不必要 escape
  和其他别名一律拒绝；
- Unix 必须无损往返任意非 NUL filename byte；
- Windows 必须无损往返 native UTF-16，包括非配对 surrogate；
- `%`、控制 byte 和 codec escape 必须只有一种 canonical 拼写，拒绝别名和畸形
  escape；
- encode 后产生 separator、root 或 prefix 的风险由调用方按“完整 path”还是“单一
  component”的上下文继续校验；
- 不支持稳定无损转换的平台必须返回明确的
  `LocalPathCodecError::UnsupportedNativeEncoding`，不能使用 lossy conversion。

`qubit-fs-local` 可以定义 adapter-local codec 类型，实现
`qubit_fs::NativePathCodec`，并把 `encode`/`decode` 委托给这里。平台字节、`OsStr`、
WTF-8 等算法不得保留在 `qubit-fs` 或 adapter 中。

不提供同功能的 free function 别名。

### 4.4 有状态资源

真正拥有资源或迭代状态的能力使用 struct：

- `LocalDirectoryWalker`：惰性递归遍历；
- `LocalFileReader`：拥有已打开文件的 `std::io::Read + std::io::Seek` 本地输入；
- `LocalFileWriter`：带 publication 生命周期的写入 session；
- `LocalTempFile`：拥有临时文件 cleanup responsibility；
- `LocalTempDirectory`：拥有临时目录 cleanup responsibility；
- 对应的 rooted 内部 session。

这些类型不被无状态 namespace enum 取代。

`LocalFileMetadata` 通过 `LocalFileKind`、长度和可选 timestamp getter 提供 normalized
native metadata；`qubit-fs-local` 不应读取裸 `std::fs::Metadata` 后再实现一套平台
分支。

`LocalTempFile` / `LocalTempDirectory` 使用统一公开类型和私有 authority backend：

- host backend 保存已绑定的 host path 与 cleanup responsibility；
- rooted backend 保存打开的 root authority、root-relative descendant 与 cleanup
  responsibility；
- `path()` 返回 authority-local native path：host 为已绑定 host path，rooted 为
  root-relative descendant；
- rooted persist、cleanup、child 和 descendant 操作始终使用保存的 root
  descriptor/handle，不把诊断路径重新解析为授权依据。

### 4.5 命名

不增加 `Native` 前缀：

```text
qubit_local_files::LocalFileSystem
qubit_local_files::RootedLocalFileSystem
```

所属 crate 已经明确表达 native 层。需要同时导入 `qubit-fs` 时，上层 adapter 使用：

```rust
use qubit_local_files as native_files;
```

## 5. Host path 语义

### 5.1 相对路径

Host API 可以接受绝对或相对 native path。任何会跨越多个系统调用的操作必须在开始时
把相对路径绑定到当时的 current working directory：

- copy 与 recursive walk；
- temp resource；
- staged writer 与 persist；
- 递归 create/delete；
- 返回给调用者并在之后继续使用的资源路径。

绑定完成后，即使进程 working directory 改变，操作也不能被重定向。

### 5.2 Final symlink

`LocalFileSystem::metadata` 和 `RootedLocalFileSystem::metadata` 观察 final directory
entry 本身，不跟随 final symlink。未来若增加 follow 行为，必须使用新的明确 option
或方法，不能改变现有入口的语义，也不能因某个平台 API 默认行为不同而改变。

Overwrite 默认替换 target entry，不跟随 target symlink 写入其指向对象。

### 5.3 Native path

本 crate 原样接受 `Path` / `OsStr`，保留 Unix 非 UTF-8 byte 和 Windows native code
unit。它不承担 URI percent decode 或 `qubit_fs::Path` hierarchy/component 转换；
只通过 `LocalPathCodec` 提供与 native string 可逆的 canonical text 编解码原语。

内部拒绝 native NUL、无效 root/prefix 组合以及会把 child 解释成 absolute/prefixed
path 的输入。

## 6. Rooted authority

### 6.1 基本不变量

所有 rooted 操作必须：

1. 从已打开 root descriptor/handle 出发；
2. 逐 component 解析 descendant；
3. 拒绝 absolute path、platform prefix、`.` 和逃逸用 `..`；
4. 不通过中间 symlink 离开 root；
5. 不因诊断路径被 rename 或替换而改变 authority；
6. 在返回 native path 仅供诊断时明确其不参与授权判断。

### 6.2 平台实现

Unix 优先使用 directory-fd-relative 操作和 no-follow flag。Windows 使用已打开目录
handle、reparse-point-aware traversal 和 handle-relative 能力。

平台缺少可靠原语时：

- 不静默退化为字符串 canonicalization；
- 返回明确的 unsupported 或 requirement-not-met 错误；
- capability 查询准确说明当前构建和运行平台能保证的行为。

公开快照使用 `LocalFileSystemCapabilities`。单个 native path limit 使用
`LocalPathLimit { value, unit }`，其中
`LocalPathLengthUnit::{Bytes, Utf16CodeUnits}` 明确计量单位。Unix 通常是 bytes，
Windows 是 UTF-16 code units；不能把 Windows 长度直接标成 byte limit，运行环境
无法证明稳定上界时保持 unknown。`LocalFileSystem::capabilities()` 返回 host
snapshot；`RootedLocalFileSystem` 在 `open` 时缓存与已打开 authority 对应的 snapshot。

### 6.3 Symlink、junction 与 mount

Rooted recursive operation 默认不跟随 symlink、junction 或其他 reparse point。
显式 follow 模式只有在能够持续证明 containment 时才可启用。

跨 mount/device 是否允许由 option 控制，并在 outcome 中报告实际边界行为。

## 7. Copy

### 7.1 统一入口

文件与目录复制使用同一个 `LocalFileSystem::copy` 或
`RootedLocalFileSystem::copy`。实现根据 source metadata 选择 file、directory 或拒绝
特殊文件，不公开两套行为逐渐分叉的复制算法。

两个入口都返回类型化结果：

```rust
pub type LocalCopyResult =
    Result<LocalCopyOutcome, LocalCopyFailure>;

pub enum LocalCopyFailureState {
    Unchanged,
    PartiallyPublished,
    Published,
    Indeterminate,
}

pub struct LocalCopyFailure {
    error: LocalFileError,
    state: LocalCopyFailureState,
    partial_stats: LocalCopyStats,
    staging_path: Option<PathBuf>,
    cleanup_error: Option<LocalFileError>,
}
```

`LocalCopyFailureState` 只陈述 target 的已知事实：

- `Unchanged`：本次 copy 未改变 target；
- `PartiallyPublished`：target 已创建或修改，但请求内容尚未完整发布；
- `Published`：目标内容已经完整发布，后续 metadata 或 durability 步骤失败；
- `Indeterminate`：无法确认 target 最终状态。

Copy 不能修改 source。目录复制既有的执行阶段、partial stats、staging path 和 cleanup
failure 必须归一到 `LocalCopyFailure`，不能在统一入口映射成只剩
`LocalFileError` 的结果。

`staging_path` 使用 authority-local 语义：host 为已绑定 host path，rooted 为
root-relative descendant。只有 staging cleanup 确认失败时才保留它及
`cleanup_error`；没有遗留 staging 时两者都为 `None`。

`LocalCopyFailure` 提供 `error()`、`state()`、`partial_stats()`、
`staging_path()`、`cleanup_error()` 和 consuming decomposition；字段保持私有，
adapter 不通过 message 猜测状态。

`LocalCopyOptions` 至少明确：

- target conflict policy；
- file/directory type conflict policy；
- metadata preserve policy；
- symlink policy；
- cross-device policy；
- atomicity requirement；
- durability requirement；
- recursive traversal policy。

### 7.2 安全检查

在打开 destructive overwrite target 前检查：

- source 与 target 文本或 identity self-copy；
- hard-link alias；
- directory target 是否位于 source subtree；
- source/target 类型冲突；
- target symlink replacement 语义；
- required atomicity 是否可满足。

目录复制默认不遍历 symlink。启用 follow 时必须检测循环，并持续执行 containment
检查。

### 7.3 Publication

需要 staged publication 时，临时条目尽量在 target 同目录创建，以提高 rename
原子性。Outcome 明确报告：

- 实际 copy method；
- actual atomicity；
- source/target bytes 与 entry 统计；
- metadata preserve 结果；
- durability 结果；
- 是否跨 device 或使用 fallback。

Required semantics 不能通过 outcome 静默降级。

如果 publication 已完成而 parent directory sync 失败，返回
`LocalCopyFailureState::Published`；不能把它报告成 `Unchanged`。如果 recursive copy
已经产生部分 target entry 后失败，返回 `PartiallyPublished` 并保留已知统计。

## 8. Rename

Host 与 rooted rename 都使用专用结果：

```rust
pub type LocalRenameResult =
    Result<LocalRenameOutcome, LocalRenameFailure>;

pub enum LocalRenameFailureState {
    Unchanged,
    Renamed,
    Indeterminate,
}

pub struct LocalRenameFailure {
    error: LocalFileError,
    state: LocalRenameFailureState,
}
```

`LocalRenameOptions` 明确 target conflict、replace、atomicity 和 durability
requirement。Rename 只调用 native namespace primitive；跨 device、无法满足
no-replace/atomic replace 或其他 requirement 时必须在可证明无副作用的阶段失败。

Native rename 成功、随后 parent durability 失败时必须返回 `Renamed`。只有 native
原语能够证明 source/target 未改变时才能返回 `Unchanged`；普通未知 I/O error 映射为
`Indeterminate`。Rename 永远不在本 crate 内伪装成 copy+delete。

`LocalRenameFailure` 提供 `error()`、`state()` 和 consuming decomposition，供直接
调用者与 adapter 无损处理。

## 9. Lazy walk

`LocalDirectoryWalker` 是惰性迭代器，不预先把整棵目录树读入内存：

- 按需打开目录并产生 entry；
- directory handle 的并发上限明确；
- 遍历顺序由 option 定义或明确为 unspecified；
- 遇到错误时返回带 offending path 的结构化错误；
- fail-fast 与 collect-errors 模式不能混为隐式行为；
- symlink、mount/device 和最大深度策略在创建 walker 时固定；
- rooted walker 始终从 root authority 派生 child handle。

Walker drop 只释放本地 handle，不执行 namespace 修改。

## 10. Writer publication

`LocalFileWriter` 同时是 byte output 和 publication session。状态包括：

- `Open`；
- `Committed`；
- `Aborted`；
- `NotPublished`；
- `Published`；
- `Indeterminate`。

`LocalWriteOptions` 明确区分 `CreateNew`、`CreateOrReplace` 和 `Append`。
前两者使用同目录 staging 与 publication；append 直接修改已存在 entry，因此拒绝
`LocalAtomicityRequirement::Required`。`Preferred` 可以降级为 direct append，但
commit 必须报告 `atomic = false`；写入 bytes 后 abort 也不能声称已经回滚。

Commit failure 使用：

- `RetryableNotPublished`；
- `NotPublished`；
- `Published`；
- `Indeterminate`。

只有 `Open` 状态允许继续 `Write`/`flush`。任一 byte-stream write/flush error 都把
writer 置为 `Indeterminate`，因为普通 I/O error 不能证明 direct append 或 staging
写入没有产生副作用。

典型流程：

1. 在安全 staging path 写入；
2. flush 用户空间 buffer；
3. 按 requirement 同步 file data/metadata；
4. publish 到 target；
5. 按 requirement 同步 parent directory；
6. 返回 actual publication method、atomicity 与 durability。

如果 publish 已完成但 parent sync 失败，必须报告 `Published`，不能报告
`NotPublished`。

Abort 在 target 已发布时只能清理 staging，不能回滚 target。

## 11. Temporary resource

`LocalTempFile` 和 `LocalTempDirectory` 使用 RAII 管理 cleanup responsibility：

```text
Owned
 ├─ persist complete ─────────────► Persisted
 ├─ keep ─────────────────────────► Kept
 ├─ cleanup ──────────────────────► Cleaned
 ├─ published/source retained ────► CleanupRequired
 └─ unknown ──────────────────────► Indeterminate
```

Persist failure state 包括：

- `NotPublished`；
- `PublishedSourceRetained`；
- `Indeterminate`。

当前 `LocalPersistOptions` 对 file 与 directory 提供同一套 overwrite 语义；
`persist_with_outcome` 返回实际的 target、publication method、atomicity 与 durability。
当前 native 实现以同一 authority 内的 rename 发布，报告 `AtomicRename`、
`atomic = true` 与 `durable = false`。`LocalPersistFailureState` 在已知未发布的错误上
报告 `NotPublished`，无法证明结果的 native install error 报告 `Indeterminate`；
`PublishedSourceRetained` 为未来分阶段 publish 实现保留。

后续扩展可为 `LocalPersistOptions` 增加 atomicity、durability 和 metadata-preservation
requirement；所有 `Required` requirement 必须在 namespace 修改前验证。

Drop 只在 `Owned` 或 `CleanupRequired` 执行 best-effort cleanup；`Indeterminate` 不
自动操作。需要确认 cleanup 的调用者必须显式调用 `cleanup`。

`LocalTempFileOptions` 与 `LocalTempDirectoryOptions` 统一承载 parent directory、
prefix 和 suffix。所有 affix 与最终随机 component 必须在创建 entry 前完成 native
separator、NUL 和平台保留名称校验；失败不能留下临时条目。

Host 与 rooted authority 提供对称入口：

```rust
impl RootedLocalFileSystem {
    pub fn create_temp_file(
        &self,
        options: &LocalTempFileOptions,
    ) -> LocalResult<LocalTempFile>;

    pub fn create_temp_directory(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory>;
}
```

也就是说，rooted public API 明确提供
`RootedLocalFileSystem::create_temp_file` 与
`RootedLocalFileSystem::create_temp_directory`，而不是让 adapter 回退到 host temp。

Rooted options 中的 parent 必须是经验证、不能逃逸 root 的 relative descendant。
创建、persist、cleanup 及 temporary directory child 操作都从保存的 root authority
执行。Root 的诊断路径在资源生命周期内被 rename 或替换，不得改变 cleanup target，
也不能导致回退到 host-path 删除。

`LocalTempFile::close(&mut self)` 只关闭内容 I/O handle，状态仍为 `Owned`，path、
persist、keep 和 cleanup responsibility 都继续保留。这使需要先关闭文件再交给外部
进程的调用者不必把资源降级成裸路径。

临时目录的 child API 只接受已经验证的单 component 或不能逃逸的 relative descendant。
Child operation 仍执行 no-follow containment，不只做 lexical join。

## 12. 结构化错误

本 crate 定义独立错误域：

```rust
pub type LocalResult<T> = Result<T, LocalFileError>;

pub struct LocalFileError {
    kind: LocalFileErrorKind,
    operation: LocalFileOperation,
    path: Option<PathBuf>,
    target: Option<PathBuf>,
    source: Option<LocalFileErrorSource>,
}

#[non_exhaustive]
pub enum LocalFileErrorSource {
    Io(std::io::Error),
    PathCodec(LocalPathCodecError),
}
```

错误必须区分：

- invalid path/options；
- not found/already exists/type conflict；
- permission；
- unsupported platform capability；
- requirement not met；
- resource limit；
- publication not complete；
- indeterminate；
- ordinary I/O。

Copy、rename、writer 与 temp 的部分成功使用专用 failure 类型携带 state，不能只依赖
message。`LocalCopyFailure` 还必须携带 partial stats；存在 staging cleanup failure
时保留其 native path 和 typed source，但普通格式化不应丢失主失败。

Name/path 工具错误分别使用 `ValidateName`、`BindPath`、`ComposePath` 和
`GenerateName` operation；不能借用 `Metadata` 或 `CreateTempFile` 冒充工具操作。

Codec 使用独立的 `LocalPathCodecError`，因为它不执行文件 I/O，也没有
`LocalFileOperation`。它必须区分 non-canonical text、invalid escape、native NUL、
unsupported native encoding 和无法无损表示的 native value。`LocalPaths` 把 codec
错误包装进具体的 `LocalFileErrorSource::PathCodec`，不能只复制 message。

错误保留 native path 供本地调用者诊断；是否能跨安全边界显示这些路径由上层 adapter
决定。

## 13. 平台代码组织

公共模块只表达语义，平台细节放入私有平台模块：

```text
src/
├── local_file_system.rs
├── rooted_local_file_system.rs
├── local_file_names.rs
├── local_paths.rs
├── local_path_codec.rs
├── copy/
├── walk/
├── writer/
├── temp/
├── error/
└── platform/
    ├── unix/
    ├── windows/
    └── portable/
```

要求：

- `cfg` 分支尽量停留在 `platform/`；
- 公共层不复制同一业务状态机的 Unix/Windows 版本；
- portable fallback 只有在满足同一公开契约时才启用；
- platform capability 从真实实现派生，不靠调用者猜测。

## 14. 与 `qubit-fs-local` 的接口

`qubit-fs-local` 只做以下转换：

```text
qubit_fs::spi::Request
  → native Path/options
  → qubit-local-files operation
  → local outcome/error/session
  → qubit_fs outcome/error/session SPI
```

`qubit-local-files` 不接受 `qubit_fs::Path`、不构造 `FileResource`、不读取 registry
config，也不为 `qubit-fs` 重复维护 capability 校验。它只提供
`LocalPathCodec`，让 adapter 无损转换 canonical logical component text 与 native
string。

## 15. 验证策略

测试至少覆盖：

- 相对路径在操作开始时绑定；
- source/target self-copy 与 hard-link alias；
- overwrite 不跟随 target symlink；
- recursive copy/walk 的 symlink、cycle、depth 和 device 边界；
- rooted path lexical escape 与 symlink/reparse escape；
- root 诊断路径被 rename 后 authority 仍稳定；
- writer 每个 commit/abort failure state；
- copy 每个 failure state、partial stats 和 staging cleanup failure；
- rename 成功后 durability 失败报告 `Renamed`；
- temp 每个 persist/cleanup/keep state；
- rooted temp 在 root 诊断路径 rename/replacement 后仍使用原 authority；
- publish 成功但 durability 失败；
- `LocalPathCodec` 的 canonical escape、Unix 非 UTF-8 byte 与 Windows 非配对
  surrogate 往返；
- Unix、Windows 和 portable capability 差异；
- Drop 不 panic，且不处理 `Indeterminate`；
- Unicode、非 UTF-8 Unix name、Windows prefix/reserved name 等平台输入。

Linux、Windows 和 macOS 执行 runtime tests；仅 compile-check 的平台不能被文档描述为
runtime 保证。
