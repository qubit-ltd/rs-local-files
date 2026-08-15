# Qubit Local Files 本地文件系统设计

> 状态：已批准的目标设计。本文定义 `qubit-local-files` 的公共边界与平台语义；
> 当前实现已完成统一入口、类型化 copy/rename failure、rooted temporary resource、
> 路径 codec 与临时资源 persistence。本文描述当前公共边界与后续可选扩展。

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
6. 公共工具的主语义由统一类型的关联方法或实例方法组织；Host 命名空间通过
   `LocalFileSystem::host()` 提供与 rooted 对称的实例入口；
7. 上层无需理解平台条件编译即可使用完整业务逻辑。

非目标：

- 定义 `qubit_fs::Path`、URI、provider capability 或 registry；
- 返回 `qubit_fs::FsError`；
- 把本地路径强制转换成 UTF-8 provider path；
- 为远程或对象存储提供实现；
- 通过 canonicalize 后的字符串比较代替 descriptor-relative 安全；
- 承诺绕过操作系统本身无法避免的所有 TOCTOU race。

特别地，临时资源的 `cleanup`/`Drop` 只承诺在删除前检查创建时身份，并在检查到
普通替换时拒绝删除。身份检查与路径删除不是单个原子操作，inode/file ID 也可能被
文件系统复用。因此，若并发者能够在同一目录中删除临时条目并在同名路径反复安装
同类型条目，本 crate 不保证绝不会删除替换条目。需要该保证时，创建目录必须排除不受信任的
并发写入，或使用 `keep` 移交责任后由上层完成同步与删除。

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

### 4.1 统一 local filesystem service

`LocalFileSystem` 是可构造、可传递的具体服务，在创建时选择 host 或 rooted
namespace，之后通过同一套实例方法操作：

```rust
impl LocalFileSystem {
    pub const fn host() -> Self;
    pub fn rooted(root: &Path) -> LocalResult<Self>;
    pub fn scope(&self) -> LocalFileSystemScope;
    pub fn diagnostic_root(&self) -> Option<&Path>;
    pub const fn protocols(&self) -> LocalFileSystemProtocols;

    // metadata/open/list/copy/create/delete/rename/temp 操作均为 &self 方法
}
```

这个类型不复制 `qubit-fs` 的 provider-neutral 门面：它只处理 native `Path`、本地
option/result/error 和两种本地 namespace。Host 操作通过
`LocalFileSystem::host()` 提供，供普通程序直接调用。

### 4.2 Rooted authority

`LocalFileSystem::rooted` 建立 root，内部保存已打开的 descriptor/handle 与非权威
诊断路径。安全性依赖已打开 root，不依赖 root path 字符串在之后仍指向
同一目录。Rooted backend 不作为独立公开类型，避免 host/rooted API 演化分叉。

### 4.3 路径与文件名工具

无状态工具按类型组织：

```rust
pub struct LocalFileNames { _private: () }
pub struct LocalPaths { _private: () }
pub struct LocalPathCodec { _private: () }
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
    pub fn from_canonical_components<'a>(
        scope: LocalFileSystemScope,
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf>;

    pub fn to_canonical_components(
        scope: LocalFileSystemScope,
        path: &Path,
    ) -> LocalResult<Vec<String>>;
}
```

这些方法内部使用 `LocalPathCodec`，并负责 separator、root、prefix、drive 与 component
边界；上层不能逐 component `PathBuf::push` 后再复制一套 Windows/Unix 判断。
`Host` scope 的 Unix root 使用空序列表示 `/`，Windows 使用首个 drive component；
`Rooted` scope 的空序列表示已打开 authority root，其余组件始终是安全的 relative
descendant。UNC/remote authority 在没有独立、无歧义的 provider authority 映射前返回
unsupported。

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
    pub fn from_canonical_text<'a>(
        text: &'a str,
    ) -> Result<Cow<'a, OsStr>, LocalPathCodecError>;

    pub fn to_canonical_text<'a>(
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
- `from_canonical_text` 后必须重新 `to_canonical_text` 并与输入完全相等，因此 lowercase escape、不必要 escape
  和其他别名一律拒绝；
- Unix 必须无损往返任意非 NUL filename byte；
- Windows 必须无损往返 native UTF-16，包括非配对 surrogate；
- `%`、控制 byte 和 codec escape 必须只有一种 canonical 拼写，拒绝别名和畸形
  escape；
- `from_canonical_text` 后产生 separator、root 或 prefix 的风险由调用方按“完整 path”还是“单一
  component”的上下文继续校验；
- 不支持稳定无损转换的平台必须返回明确的
  `LocalPathCodecError::UnsupportedNativeEncoding`，不能使用 lossy conversion。

`LocalPathCodec` 和 `LocalPaths` 是本 crate 的平台边界；`qubit-fs-local` 通过
`LocalPaths` 使用它们完成逻辑路径与 native 路径转换。平台字节、`OsStr`、WTF-8
等算法不得复制到 `qubit-fs` 或 adapter 中。

Host 与 rooted 操作统一使用 `LocalFileSystem` 实例方法；需要访问进程可见命名空间时，
先构造 `LocalFileSystem::host()`，再通过该实例调用具体操作。

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
- rooted persist 和 cleanup 始终使用保存的 root descriptor/handle，不把诊断路径
  重新解析为授权依据；`child` 与 `descendant` 只返回经过 lexical validation 的诊断路径。

### 4.5 命名

不增加 `Native` 前缀：

```text
qubit_local_files::LocalFileSystem
qubit_local_files::LocalFileSystem::rooted(...)
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

`LocalFileSystem` 实例保存符号链接解析策略。Rooted 默认
`FollowWithinScope`，Host 默认 `FollowAcrossScope`；显式策略还可以是 `Reject`。
策略统一作用于中间路径组件。Rooted 仅支持 `Reject` 和 `FollowWithinScope`；选择
`FollowAcrossScope` 会返回 `InvalidOptions`，因为 Rooted authority 不能越出已打开的 root。

最终组件必须按操作系统操作语义处理，而不是使用一个全局的 follow/no-follow 布尔值：

- `metadata` 查看链接 entry 本身；
- Unix reader、append writer 和 `CreateOrReplace` writer 跟随链接目标；Windows
  reader 拒绝最终 name-surrogate reparse point，append 与 replace 仍按 native 语义处理；
- `CreateNew` 将最终链接视为已存在；
- delete、rename、copy target 和 temp persist 操作链接 entry 本身；
- copy source 复制链接 entry 本身。

Host 可以支持 `/etc` 链接到 Git checkout 后的透明修改；Rooted 则保持
`delete(link)` 与 `rename(link, ...)` 不会误删或改写链接目标。

### 5.3 Native path

本 crate 原样接受 `Path` / `OsStr`，保留 Unix 非 UTF-8 byte 和 Windows native code
unit。它不承担 URI percent decode 或 `qubit_fs::Path` hierarchy/component 转换；URI
解码属于 `rs-fs-local` provider 边界。
只通过 `LocalPathCodec` 提供与 native string 可逆的 canonical text 编解码原语。

内部拒绝 native NUL、无效 root/prefix 组合以及会把 child 解释成 absolute/prefixed
path 的输入。

## 6. Rooted authority

### 6.1 基本不变量

所有 rooted 操作必须：

1. 从已打开 root descriptor/handle 出发；
2. 逐 component 解析 descendant；
3. 拒绝 absolute path、platform prefix、`.` 和逃逸用 `..`；
4. 按实例策略处理中间 symlink；Rooted 的 `FollowWithinScope` 证明 containment，
   Rooted 不支持 `FollowAcrossScope`；
5. 不因诊断路径被 rename 或替换而改变仍采用 descriptor authority 的操作；
6. 在返回 native path 仅供诊断时明确其不参与授权判断。

### 6.2 平台实现

Unix 优先使用 directory-fd-relative 操作和 no-follow flag。Windows 使用已打开目录
handle、reparse-point-aware traversal 和 handle-relative 能力。

平台缺少可靠原语时：

- 不静默退化为字符串 canonicalization；
- 返回明确的 unsupported 或 requirement-not-met 错误；
- capability 查询准确说明当前构建和运行平台能保证的行为。

公开快照使用 `LocalFileSystemProtocols`。路径和文件名限制统一由
`SizeLimit` 表示：`Maximum(value)` 是已验证的有限上限，`VariesByPath` 表示上限
随目标 filesystem 而变化，`Unknown` 表示无法可靠探测。Host 实例的 `limits()`
返回 `VariesByPath`，调用 `limits_at(path)` 才会针对目标路径探测；rooted 实例在
打开 authority 时缓存对应 snapshot。

### 6.3 Symlink、junction 与 mount

Rooted recursive operation 默认继承 filesystem 的符号链接策略。跟随模式按底层目录对象
身份检测循环，返回路径保留逻辑 link 组件；Rooted 的 `FollowWithinScope` 持续证明 containment，
而 `FollowAcrossScope` 仅适用于 Host。Host 的 `FollowWithinScope` 与
`FollowAcrossScope` 实际访问范围相同。

当前不提供 mount/device 边界 option，也不承诺在递归操作中检测或报告该边界；调用方若有
该隔离要求，必须在 crate 外建立专门策略。

## 7. Copy

### 7.1 统一入口

文件与目录复制使用同一个 `LocalFileSystem::copy`；Host 操作通过
`LocalFileSystem::host().copy` 调用。
实现根据 source metadata 选择 file、directory 或拒绝
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
`staging_path()` 和 `cleanup_error()`；字段保持私有，
adapter 不通过 message 猜测状态。

`LocalCopyOptions` 至少明确：

- target conflict policy；
- file/directory type conflict policy；
- metadata preserve policy；
- symlink policy；
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

目录复制继承 filesystem 的 symlink policy；copy source 的最终 symlink 复制 entry 本身，
递归目录中的 link 目录在允许跟随时才进入。跟随时必须检测循环；Rooted
`FollowWithinScope` 持续执行 containment，`FollowAcrossScope` 仅在 Host 中可用。

### 7.3 Publication

需要 staged publication 时，临时条目尽量在 target 同目录创建，以提高 rename
原子性。Outcome 明确报告：

- 实际 copy method；
- actual atomicity；
- source/target bytes 与 entry 统计；
- metadata preserve 结果；
- durability 结果；
- 实际使用的复制方法与是否满足请求的原子性、持久性保证。

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

`LocalRenameOptions` 只表达 overwrite 和 durability requirement。Rename 的原子性
来自所选 native namespace primitive，而不是一个可独立降级的选项；跨 device、
无法满足 no-replace/atomic replace 或其他 requirement 时必须在可证明无副作用的
阶段失败。

Native rename 成功、随后 parent durability 失败时必须返回 `Renamed`。只有 native
原语能够证明 source/target 未改变时才能返回 `Unchanged`；普通未知 I/O error 映射为
`Indeterminate`。Rename 永远不在本 crate 内伪装成 copy+delete。

`LocalRenameFailure` 提供 `error()`、`state()` 和 consuming decomposition，供直接
调用者与 adapter 无损处理。

## 9. Lazy walk

`LocalDirectoryWalker` 是惰性迭代器，不预先把整棵目录树读入内存：

- 按需打开目录并产生 entry；
- directory handle 的并发上限由 `LocalListOptions::max_open_directories`
  明确控制，默认值为 64；默认 `Reopen` 策略达到上限时会关闭并重新打开活动
  frame，显式 `Fail` 策略才返回 `ResourceLimit`；
- 遍历顺序由 option 定义或明确为 unspecified；
- 遇到错误时返回带 offending path 的结构化错误；
- fail-fast 与 collect-errors 模式不能混为隐式行为；
- symlink、最大深度和句柄预算策略在创建 walker 时固定；当前契约不包含
  mount/device 边界检测；
- rooted walker 始终从 root authority 派生 child handle，并按 entry 流式读取，不为单个
  目录预先收集完整 `Vec`；

capability 将原子 rename、原子 replace、临时资源无替换持久化、durable rename 和
durable file copy 分别建模；adapter 不得用单一 no-replace 标志推断其他保证。
能力快照只报告当前 target 是否实现了对应的完整操作协议，不探测挂载点，也不声称证明
物理介质已经落盘。durable 能力分别通过 `supports_durable_rename()` 和
`supports_durable_file_copy()` 查询，adapter 必须分别映射这两项能力。

Walker drop 只释放本地 handle，不执行 namespace 修改。

## 10. Writer publication

`LocalFileWriter` 同时是 byte output 和 publication session。生命周期状态包括：

- `Open`；
- `Committed`；
- `Aborted`。

发布结论另用 `LocalWriteFailureState` 表示：

- `NotPublished`：目标尚未被本次操作改变；
- `Published`：目标已被改变或完整发布；
- `Indeterminate`：无法确认目标最终状态。

`LocalWriteOptions` 明确区分 `CreateNew`、`CreateOrReplace` 和 `Append`。
前两者使用同目录 staging 与 publication；append 直接修改已存在 entry，因此拒绝
`LocalAtomicityRequirement::Required`。`Preferred` 可以降级为 direct append，但
commit 必须报告 `atomic = false`；写入 bytes 后 abort 也不能声称已经回滚。

Commit failure 使用 `LocalWriteFailureState`，并在
`LocalWriteOutcome::failure_state()` 中保留已知的发布结论；生命周期则通过
`LocalWriterState` 独立报告。

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
- `Indeterminate`。

`LocalPersistOptions` 对 file 与 directory 提供同一套 overwrite 语义；
`persist_with` 返回实际的 target、publication method、atomicity、durability 与
sandbox cleanup state。
当前 native 实现以同一 authority 内的 rename 发布，报告 `AtomicRename`、
`atomic = true` 与 `durable = false`。`LocalPersistFailureState` 在已知未发布的错误上
报告 `NotPublished`，无法证明结果的 native install error 报告 `Indeterminate`；

后续扩展可为 `LocalPersistOptions` 增加 atomicity、durability 和 metadata-preservation
requirement；所有 `Required` requirement 必须在 namespace 修改前验证。

Drop 只在 `Owned` 或 `CleanupRequired` 执行 best-effort cleanup；`Indeterminate` 不
自动操作。需要确认 cleanup 的调用者必须显式调用 `cleanup`。

`LocalTempFileOptions` 与 `LocalTempDirectoryOptions` 统一承载 parent directory、
prefix 和 suffix。所有 affix 与最终随机 component 必须在创建 entry 前完成 native
separator、NUL 和平台保留名称校验；失败不能留下临时条目。
每个临时资源还会先创建一个私有 sandbox，再在其中创建实际 entry。cleanup 或成功
persist 后会移除空 sandbox；`keep` 会把 sandbox 与资源一起交给调用方，避免共享
parent 下的 cleanup 路径被并发替换。

Host 与 rooted authority 通过同一类型提供对称入口：

```rust
impl LocalFileSystem {
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

也就是说，rooted 实例明确提供 `create_temp_file` 与
`create_temp_directory`，而不是让 adapter 回退到 host temp。

Rooted options 中的 parent 必须是经验证、不能逃逸 root 的 relative descendant。
创建、persist 与 cleanup 都从保存的 root authority 执行。Root 的诊断路径在资源
生命周期内被 rename 或替换，不得改变 cleanup target，也不能导致回退到 host-path 删除。

`LocalTempFile::close(&mut self)` 只关闭内容 I/O handle，状态仍为 `Owned`，path、
persist、keep 和 cleanup responsibility 都继续保留。这使需要先关闭文件再交给外部
进程的调用者不必把资源降级成裸路径。

临时目录的 child API 只接受已经验证的单 component 或不能逃逸的 relative descendant，
并返回对应的 lexical join；它不打开或验证子项，因此不提供 no-follow containment。

临时资源在创建时记录 native entry identity。persist、cleanup 和 Drop 在操作前核对
当前路径仍指向该 identity；若同一路径被外部同类型 entry 替换，资源转为
indeterminate 并拒绝删除或发布该替换项。

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
├── local/internal/
├── rooted/
└── temp/
```

要求：

- `cfg` 分支与相应的 local/rooted internal implementation 放在一起；
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
- writer overwrite 跟随 final symlink 修改目标并保留链接；delete、rename、copy target
  与 temp persist 替换 final symlink entry；
- recursive copy/walk 的 symlink、cycle 和 depth；mount/device 边界不属于当前契约；
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
