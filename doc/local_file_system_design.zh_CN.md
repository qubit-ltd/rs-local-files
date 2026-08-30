# Qubit Local Files 本地文件系统完整设计

> 状态：规范性设计文档
>
> 最后更新：2026-08-30

本文定义 `qubit-local-files` 的完整、稳定设计。公共 API、平台实现、测试、README 和用户
指南都应与本文保持一致。本文描述库完成后的最终形态，不记录迁移历史，也不把临时实现
细节当成长期契约。

文中的“必须”“不得”“应当”用于表达规范性要求；代码片段用于说明 API 形态，具体
`const`、`inline` 等非语义修饰不属于设计契约。

## 0. 术语

| 术语 | 本文含义 |
| --- | --- |
| Host | 调用进程通过操作系统可见的本地 namespace；Unix 通常以 `/` 为根，Windows 可以包含多个 volume/prefix root |
| Rooted | 由一个已打开目录 descriptor/handle 锚定、对调用方呈现独立虚拟根 `/` 的本地 namespace |
| namespace path | 调用方传给某个 `LocalFileSystem`、或由它返回的路径；只在该对象的坐标系内解释 |
| Host diagnostic path | Rooted 对象对应的 Host 侧 best-effort 路径提示；只用于诊断，不授予访问能力 |
| PWD | `LocalFileSystem` 实例自己的当前工作目录，是规范化的 namespace-absolute 逻辑路径 |
| authority | 实际授予 namespace 访问能力的操作系统对象；Rooted 中是构造时打开的根 handle |
| native path/name | 使用 `Path`/`OsStr` 表达、保留平台原生 byte 或 code unit 的路径或名称 |
| canonical component | 为跨抽象层传输而定义、可逆编码成 UTF-8 的单个 native 名称组件 |
| publication | 新内容或新名称对该 filesystem namespace 中其他观察者变得可见的状态转换 |

除非特别写明 Host physical/diagnostic path，本文中的“绝对路径”“根”“路径输出”均指 owning
`LocalFileSystem` 的 namespace，而不是 Rooted 背后的 Host 路径。

## 1. 定位

`qubit-local-files` 是一个同步、原生、面向应用的本地文件系统能力层。它直接使用
`std::path::Path`、`PathBuf`、`OsStr` 和操作系统文件 API，向以下调用方提供一致的
本地文件系统模型：

- 直接构建本地应用的业务代码；
- `qubit-fs-local` 等 provider adapter；
- 需要 Rooted authority、可靠 publication、临时资源、递归遍历或结构化失败状态的库。

它不是 `std::fs` 的简单重命名，也不是 `qubit-fs` 的重复实现。它负责把跨平台本地
文件系统中容易出错的部分——authority、路径解析、symlink、原子发布、持久性、部分成功
和资源所有权——收敛成一个可预测的对象模型。

## 2. 设计原则

### 2.1 对象状态显式可见

`LocalFileSystem` 是有状态对象。每个实例持有自己的当前目录、默认操作 Options 和默认
symlink 策略。调用方可以在应用初始化阶段完成配置，之后通过普通实例方法执行操作，无需
反复传递相同参数。

对象状态必须可查询；库不得在调用方不可见的位置叠加另一套默认 Options 或资源限制。

### 2.2 调用方拥有策略决定权

资源安全策略属于调用方。深度、条目数、字节数、打开句柄数、deadline 和重试次数等预算
由调用方通过操作 Options 提供。库的初始资源预算为不限制，不以隐藏上限缩小调用方能力。

库仍负责：

- 验证调用方提供的 Options；
- 在达到显式预算时返回结构化错误；
- 防止内部逻辑循环；
- 正确映射操作系统自身的资源错误。

### 2.3 一个文件系统只有一个路径坐标系

每个 `LocalFileSystem` 都有自己的 namespace 坐标系和当前工作目录。Rooted 只有一个虚拟
根；Host 使用操作系统的 native root 集合。绝对路径从其对应 namespace root 开始，相对
路径从该对象的当前目录开始。Rooted 文件系统对调用方表现为独立的虚拟文件系统，而不是
要求调用方传入特殊的 root-relative 路径。

### 2.4 Authority 来自句柄，不来自字符串

Rooted 的权限边界由构造时打开的目录 descriptor/handle 决定。诊断用 Host 路径不是
authority。实现不得依靠 `canonicalize` 后的字符串前缀比较证明 containment。

### 2.5 词法语义由库定义，真实解析交给操作系统

库负责 PWD、绝对/相对路径、`.`、`..` 和虚拟根的词法规范化；实际目录访问、symlink、
reparse point、权限和文件身份尽量使用操作系统的 descriptor/handle-relative 原语。

### 2.6 部分成功必须结构化

copy、rename、writer commit 和 temp persist 可能在 namespace 已改变后失败。它们必须返回
带状态的专用 failure，不能压缩成 `Result<(), io::Error>`。递归 create/delete 的部分成功
至少必须通过稳定的 `PublicationIncomplete` kind 和失败路径表达，不得伪装成未发生修改。

### 2.7 原生路径必须无损

公共 API 使用 native `Path`/`OsStr`，保留 Unix 非 UTF-8 byte 和 Windows native code
unit。只有明确用于跨层传输的 codec 才转换成 canonical UTF-8 text，禁止 lossy 转换。

### 2.8 基础库只提供必要机制

本库不内置应用认证、用户授权、租户策略或全局线程同步。Local filesystem 的实际身份来自
进程和操作系统；更高层的认证授权由调用方或 provider 层完成。

## 3. 目标与非目标

### 3.1 目标

1. 使用同一个 `LocalFileSystem` 类型提供 Host 和 Rooted 文件系统。
2. 提供虚拟绝对路径、实例 PWD 和安全的相对路径语义。
3. 提供可配置、可观察的实例默认 Options，并允许单次完整替代。
4. 使用可靠的 Unix descriptor 和 Windows handle 原语保持 Rooted containment。
5. 为读取、写入、遍历、复制、重命名、临时资源和 capability 查询提供一致入口。
6. 在成功和失败结果中保留原子性、持久性、发布状态和部分统计。
7. 让 `qubit-fs-local` 只负责抽象层转换，不重复平台文件系统算法。

### 3.2 非目标

- 定义 URI、provider registry 或远程文件系统协议；
- 依赖或公开 `qubit_fs::Path`、`FsError`、`FileResource`；
- 绕过操作系统账户权限；
- 自动替调用方选择业务资源预算；
- 提供异步 I/O；异步调用方应在适当的 blocking 执行环境中使用本库；
- 保证底层文件系统或硬件已经真正持久化超过操作系统所能证明的范围；
- 在不受信任并发者可任意替换同名条目的目录中消除所有 TOCTOU 风险；
- 默认阻止 mount/device 边界；需要同设备隔离的调用方必须提供更高层策略。

## 4. 依赖边界

```text
直接使用本地文件系统的应用
              │
              ▼
       qubit-local-files
              ▲
              │ native Path / options / outcomes / errors
       qubit-fs-local
              ▲
              │ FileSystemSpi
          qubit-fs
```

`qubit-local-files` 不依赖 `qubit-fs` 或 registry。所有 Unix、Windows 和其他平台差异
都封装在本 crate 内；adapter 不复制 path codec、symlink traversal、publication 或
Rooted containment 实现。

## 5. 核心对象模型

### 5.1 LocalFileSystem

概念上的内部结构如下：

```rust
pub struct LocalFileSystem {
    // 构造后不可变，可由 clone 和已打开资源共享。
    authority: Arc<LocalAuthorityCore>,

    // 每个实例独立，clone 时形成快照。
    current_directory: PathBuf,
    symlink_policy: LocalSymlinkPolicy,
    default_read_options: LocalReadOptions,
    default_write_options: LocalWriteOptions,
    default_list_options: LocalListOptions,
    default_copy_options: LocalCopyOptions,
    default_create_directory_options: LocalCreateDirectoryOptions,
    default_delete_options: LocalDeleteOptions,
    default_rename_options: LocalRenameOptions,
    default_temp_file_options: LocalTempFileOptions,
    default_temp_directory_options: LocalTempDirectoryOptions,
}
```

这只是职责划分，不要求源码把所有字段写在一个文件中。关键不变量是：

- authority 与能力快照不可变；
- PWD 和默认策略属于具体实例；
- 默认 Options 不放入共享 `Arc`；
- 对一个 clone 的配置修改不影响其他 clone；
- 不存在额外的实例级 walk/copy hard cap；
- 不存在第二套同时参与操作的 Rooted authority。

### 5.2 LocalAuthorityCore

`LocalAuthorityCore` 只承担资源和平台 authority 生命周期：

- namespace kind：Host 或 Rooted；
- Rooted 的唯一根 descriptor/handle；
- 非权威诊断 root；
- protocol capability snapshot；
- authority 级文件系统客观限制；
- 已打开资源需要共享的私有平台状态。

Options、PWD、调用方预算和业务认证不属于 authority core。

### 5.3 构造

```rust
impl LocalFileSystem {
    pub fn host() -> LocalResult<Self>;
    pub fn rooted(root: &Path) -> LocalResult<Self>;
}
```

`host()` 捕获并验证进程在构造时的当前目录，作为实例初始 PWD。读取当前目录可能失败，
因此构造返回 `LocalResult<Self>`，不得 panic 或伪造 fallback PWD。

`rooted(root)` 中的 `root` 是一次性的 Host native path，用来打开 Rooted authority；它
可以是 Host 绝对或相对路径。若为相对路径，构造时基于进程当前目录解析并打开。成功后：

- 虚拟根为 `/`；
- 初始 PWD 为 `/`；
- 后续操作路径全部属于虚拟 namespace；
- 构造参数的 Host 路径只保留为诊断信息。

只有在当前 target 上能够建立本文规定的根 authority 和 containment 语义时，`rooted()`
才成功；缺少这组基础原语时返回 `Unsupported`。原子 replace、durability 等可选能力不阻止
构造，而由 protocol snapshot、Options requirement 和具体 operation outcome 表达。

公共设计不使用独立 Builder。构造 authority 后，通过 `&mut self` setter 配置实例。

## 6. LocalFileSystem 公共 API

### 6.1 状态与能力

```rust
impl LocalFileSystem {
    pub fn scope(&self) -> LocalFileSystemScope;
    pub fn current_directory(&self) -> &Path;
    pub fn set_current_directory(&mut self, path: &Path) -> LocalResult<()>;

    pub fn symlink_policy(&self) -> LocalSymlinkPolicy;
    pub fn set_symlink_policy(
        &mut self,
        policy: LocalSymlinkPolicy,
    ) -> LocalResult<()>;

    pub fn diagnostic_root(&self) -> Option<&Path>;
    pub fn protocols(&self) -> LocalFileSystemProtocols;
    pub fn limits(&self) -> LocalFileSystemLimits;
    pub fn limits_at(&self, path: &Path) -> LocalResult<LocalFileSystemLimits>;
    pub fn space_at(&self, path: &Path) -> LocalResult<LocalFileSystemSpace>;
}
```

`set_current_directory()` 必须先规范化路径、在 authority 中解析、确认存在且为目录，全部
成功后才能替换 PWD。失败时对象状态不变。

`set_symlink_policy()` 必须在修改前验证 scope 支持。Rooted 不接受
`FollowAcrossScope`；失败时原策略不变。

### 6.2 默认 Options getter/setter

每种由 `LocalFileSystem` 直接执行的可配置操作都有 getter 和 setter：

```rust
impl LocalFileSystem {
    pub fn default_read_options(&self) -> &LocalReadOptions;
    pub fn set_default_read_options(
        &mut self,
        options: LocalReadOptions,
    ) -> LocalResult<()>;

    pub fn default_write_options(&self) -> &LocalWriteOptions;
    pub fn set_default_write_options(
        &mut self,
        options: LocalWriteOptions,
    ) -> LocalResult<()>;

    pub fn default_list_options(&self) -> &LocalListOptions;
    pub fn set_default_list_options(
        &mut self,
        options: LocalListOptions,
    ) -> LocalResult<()>;

    pub fn default_copy_options(&self) -> &LocalCopyOptions;
    pub fn set_default_copy_options(
        &mut self,
        options: LocalCopyOptions,
    ) -> LocalResult<()>;

    pub fn default_create_directory_options(
        &self,
    ) -> &LocalCreateDirectoryOptions;
    pub fn set_default_create_directory_options(
        &mut self,
        options: LocalCreateDirectoryOptions,
    ) -> LocalResult<()>;

    pub fn default_delete_options(&self) -> &LocalDeleteOptions;
    pub fn set_default_delete_options(
        &mut self,
        options: LocalDeleteOptions,
    ) -> LocalResult<()>;

    pub fn default_rename_options(&self) -> &LocalRenameOptions;
    pub fn set_default_rename_options(
        &mut self,
        options: LocalRenameOptions,
    ) -> LocalResult<()>;

    pub fn default_temp_file_options(&self) -> &LocalTempFileOptions;
    pub fn set_default_temp_file_options(
        &mut self,
        options: LocalTempFileOptions,
    ) -> LocalResult<()>;

    pub fn default_temp_directory_options(
        &self,
    ) -> &LocalTempDirectoryOptions;
    pub fn set_default_temp_directory_options(
        &mut self,
        options: LocalTempDirectoryOptions,
    ) -> LocalResult<()>;
}
```

setter 在应用初始化阶段尽早验证结构、scope 和已知 capability 约束。操作入口仍需验证，
因为显式 Options 可以绕过实例默认值，运行时 filesystem 能力也可能依目标路径变化。

### 6.3 操作双入口

每个有 Options 的 `LocalFileSystem` 操作提供两个入口：

```rust
impl LocalFileSystem {
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata>;

    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader>;
    pub fn open_reader_with_options(
        &self,
        path: &Path,
        options: &LocalReadOptions,
    ) -> LocalResult<LocalFileReader>;

    pub fn read_prefix(
        &self,
        path: &Path,
        max_bytes: usize,
    ) -> LocalResult<Vec<u8>>;
    pub fn read_prefix_with_options(
        &self,
        path: &Path,
        max_bytes: usize,
        options: &LocalReadOptions,
    ) -> LocalResult<Vec<u8>>;

    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter>;
    pub fn open_writer_with_options(
        &self,
        path: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter>;

    pub fn list(&self, path: &Path) -> LocalResult<LocalDirectoryWalker>;
    pub fn list_with_options(
        &self,
        path: &Path,
        options: &LocalListOptions,
    ) -> LocalResult<LocalDirectoryWalker>;

    pub fn copy(&self, source: &Path, destination: &Path) -> LocalCopyResult;
    pub fn copy_with_options(
        &self,
        source: &Path,
        destination: &Path,
        options: &LocalCopyOptions,
    ) -> LocalCopyResult;

    pub fn create_directory(
        &self,
        path: &Path,
    ) -> LocalResult<LocalCreateDirectoryOutcome>;
    pub fn create_directory_with_options(
        &self,
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome>;

    pub fn delete_file(&self, path: &Path) -> LocalResult<LocalDeleteOutcome>;
    pub fn delete_file_with_options(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome>;

    pub fn delete_directory(
        &self,
        path: &Path,
    ) -> LocalResult<LocalDeleteOutcome>;
    pub fn delete_directory_with_options(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome>;

    pub fn rename(&self, source: &Path, destination: &Path)
        -> LocalRenameResult;
    pub fn rename_with_options(
        &self,
        source: &Path,
        destination: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult;

    pub fn create_temp_file(&self) -> LocalResult<LocalTempFile>;
    pub fn create_temp_file_with_options(
        &self,
        options: &LocalTempFileOptions,
    ) -> LocalResult<LocalTempFile>;

    pub fn create_temp_directory(&self) -> LocalResult<LocalTempDirectory>;
    pub fn create_temp_directory_with_options(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory>;
}
```

Options 继续按引用传入，允许调用方复用。无 Options 的 metadata、capability 和 space 查询
不制造多余的 `*_with_options` 形式。

## 7. Options 与默认值

### 7.1 完整替代规则

普通方法只使用实例默认 Options；`*_with_options()` 只使用显式 Options：

```text
effective_options = explicit_options.unwrap_or(instance_default_options)
```

不得逐字段隐式合并，不得在显式 Options 外再套实例 hard cap，也不得偷偷选择两者中更小
的资源限制。

如果调用方需要“默认值基础上的单次修改”，必须显式复制：

```rust
let options = filesystem
    .default_copy_options()
    .clone()
    .with_conflict(LocalCopyConflictPolicy::Overwrite);

filesystem.copy_with_options(source, target, &options)?;
```

这种组合在调用点可见、可审查、可测试。

`symlink_policy_override: None` 表示继承 filesystem 的 symlink policy，是该字段公开定义
的取值语义，不是两个 Options 对象的合并。

### 7.2 初始语义默认值

构造后的实例可以直接使用，初始默认值以避免意外破坏为原则：

| Options | 初始语义 |
| --- | --- |
| `LocalReadOptions` | 不重试打开 |
| `LocalWriteOptions` | `CreateNew`、不创建父目录、原子性 `Preferred`、持久性 `NotRequired` |
| `LocalListOptions` | 非递归、继承 symlink policy、`FailFast` |
| `LocalCopyOptions` | 冲突失败、类型冲突失败、不保留 metadata、`Auto` source、不创建父目录、原子性 `Preferred`、持久性 `NotRequired` |
| `LocalCreateDirectoryOptions` | 非递归、已存在时报错 |
| `LocalDeleteOptions` | 非递归、不存在时报错 |
| `LocalRenameOptions` | 不覆盖、持久性 `NotRequired` |
| `LocalTempFileOptions` | parent 为 PWD、默认名称格式、不创建父目录 |
| `LocalTempDirectoryOptions` | parent 为 PWD、默认名称格式、不创建父目录 |

### 7.3 资源预算默认值

下列预算统一使用 `Option` 表达，初始值为 `None`：

- walk/copy 最大深度；
- 最大条目数；
- 最大累计名称字节数；
- copy 最大字节数；
- 最大同时打开目录数；
- 操作 deadline；
- temp 名称最大尝试次数。

`None` 的含义固定为“调用方未设置该预算”，不得在不同入口中解释为“继承另一个隐藏
值”。若操作系统自身返回句柄、路径或空间上限错误，仍映射成相应结构化错误。

`LocalDirectoryReopenPolicy` 只有在调用方设置 `max_open_directories` 时才决定达到预算后的
行为；没有显式预算时，库不使用固定的 64-handle 阈值改变遍历策略。

`open_retry_timeout` 是是否启用库级重试的显式授权，不是操作总预算：`None` 表示只执行
初始 open，`Some(Duration::ZERO)` 也只允许初始尝试，正值允许在该 monotonic 时间窗内重试。
它不改变操作系统单次 open 自身的 blocking 与错误语义。

### 7.4 Options 的职责

- `LocalReadOptions`：打开重试窗口；
- `LocalWriteOptions`：写入模式、父目录创建、原子性、持久性、打开重试；
- `LocalListOptions`：递归、symlink override、深度、条目、名称内存、deadline、打开目录、
  reopen 和错误策略；
- `LocalCopyOptions`：冲突、类型冲突、metadata、symlink override、source mode、父目录、
  原子性、持久性、深度、条目、字节、打开目录和 deadline；
- `LocalCreateDirectoryOptions`：递归与 exists-ok；
- `LocalDeleteOptions`：递归与 missing-ok；
- `LocalRenameOptions`：overwrite 与 durability；
- `LocalTempFileOptions` / `LocalTempDirectoryOptions`：parent、prefix、suffix、名称尝试预算和
  父目录创建；
- `LocalPersistOptions`：overwrite 与父目录创建。

`LocalPersistOptions` 属于已经创建的临时资源。临时资源通常只 persist 一次，因此它不进入
`LocalFileSystem` 的实例默认 Options；`persist(target)` 使用安全初始值，
`persist_with(target, options)` 使用显式完整值。

### 7.5 Options 值语义与时间语义

Options 是拥有数据的普通值类型：字段私有、提供只读 getter 和消费 `self` 的 `with_*`
组合方法，并至少实现 `Clone`、`Debug` 和 `Default`。存在无参数 `new()` 时，它必须与
`Default::default()` 产生同一组初始语义；要求必要语义参数的构造函数可以保留参数。
Options 不保存 filesystem 引用，也不在后台读取全局配置。

名称为 `deadline` 的字段在公共类型中使用 `Duration`，含义是从 operation 入口被调用时
开始计算的最长经过时间；实现把它转换为 monotonic clock deadline，不得使用可回拨的
wall clock。Walker 的计时从 `list()` 创建 walker 时开始，而不是第一次调用 `next()` 时
开始。Open retry timeout 从第一次 open 尝试开始计算。

所有 `Option` 预算都必须同时提供清除方法，例如 `without_max_entries()`；调用方因此可以从
实例默认 Options clone 后，显式恢复为无限制。`with_*` 不执行 I/O；依赖 scope、目标 mount
或多个字段组合的校验由 filesystem setter 或 operation 入口完成。

## 8. Namespace、虚拟根与 PWD

### 8.1 统一调用模型

Host 和 Rooted 使用相同的路径规则：

- namespace-absolute path 从该 filesystem 的根开始；
- relative path 从该实例的 PWD 开始；
- `.` 表示 PWD；
- 空路径 `""` 也表示 PWD；
- `..` 返回一层，但不得越过 namespace 根。

PWD 始终保存为规范化的 namespace-absolute `PathBuf`。它不是进程全局工作目录，也不随
进程后续调用 `set_current_dir()` 自动变化。

### 8.2 Rooted 映射

```rust
let filesystem = LocalFileSystem::rooted(Path::new("/srv/app"))?;
```

对该对象：

| 虚拟路径 | Host 侧概念位置 |
| --- | --- |
| `/` | `/srv/app` |
| `/etc/hosts` | `/srv/app/etc/hosts` |
| `/var/data/a.txt` | `/srv/app/var/data/a.txt` |

“概念位置”只帮助理解；实际授权和访问从打开的 root handle 执行，不通过
`diagnostic_root.join(virtual_relative_path)` 完成。

Rooted 操作入口不存在“Host absolute path”分支。即使输入文本看起来等于或位于
`diagnostic_root()` 下，它仍按虚拟 absolute path 解释；例如输入 `/srv/app/log` 表示虚拟
`/srv/app/log`，概念位置是 Host `/srv/app/srv/app/log`。Windows drive、UNC 和 device
prefix 不属于 Rooted 虚拟路径语法，必须返回 `InvalidPath`，不能借此选择另一个 authority。

### 8.3 路径规范化

每次操作在任何 namespace I/O 前执行以下步骤：

1. 捕获该实例当前 PWD；
2. absolute input 以 namespace 根为初始 component stack；
3. relative input 以 PWD components 为初始 stack；
4. normal component 入栈；
5. `.` 和空 component 不改变 stack；
6. `..` 弹出一层；若 stack 已在根则返回 `InvalidPath`；
7. 产生规范化的 namespace-absolute path；
8. Rooted 再把虚拟 root 后的 components 交给 root handle-relative 平台层。

解析必须使用 `std::path::Component` 和原生 `OsStr`，不能先把路径转成 UTF-8 字符串再按
`/` 分割。

示例，PWD 为 `/`：

```text
""                  -> "/"
"."                 -> "/"
"a/./b"             -> "/a/b"
"a/../b"            -> "/b"
".."                -> InvalidPath
"a/./.././../b"     -> InvalidPath
```

PWD 为 `/work/project`：

```text
"data.db"           -> "/work/project/data.db"
".."                -> "/work"
"../../tmp"         -> "/tmp"
"../../../tmp"      -> InvalidPath
"/tmp"              -> "/tmp"
```

重复分隔符和末尾分隔符按平台 native `Path` 语义规范化。NUL、无效 prefix 组合以及不能
无损表示的 native value 必须拒绝。

规范化器不能因为输出 `PathBuf` 相同就丢掉输入的目录意图。平台把末尾 separator、末尾
`.` 或其他形式解释为“最终对象必须是目录”时，resolver 必须在内部结果中保留
`directory_required`，直到具体 operation 完成类型检查。例如 file writer 不得把
`"missing/"` 规范化成 `"missing"` 后创建普通文件。重复 separator 只有在 native 语义确实
等价时才能折叠；Windows prefix/UNC separator 规则必须先由平台 component parser 识别。

### 8.4 双路径操作

copy 和 rename 在操作开始时只捕获一次 PWD，source 与 destination 使用同一个快照解析。
任何一个路径解析失败时，不得进入底层 namespace 修改；专用 failure state 必须为
`Unchanged`。

### 8.5 set_current_directory

`set_current_directory(path)` 使用与普通操作完全相同的 absolute/relative 和 root escape
语义。它在旧 PWD 下解析输入，按 filesystem symlink policy 解析最终目标，确认目标存在且为
目录，最后一次性替换 PWD；`Reject` 策略下，最终目录是链接时失败。

PWD 是逻辑 namespace path，而不是额外的授权根。Rooted 的 authority 根始终不变。PWD
指向的条目若之后被重命名或删除，后续相对操作会按保存的逻辑路径重新解析，并可能返回
`NotFound`；需要稳定目录身份的调用方应使用已经打开的资源句柄。

### 8.6 Rooted 根操作

虚拟 `/` 是正常可寻址目录，但不得被移除或替换：

- metadata、list、limits、space 和 temp parent 可以使用 `/`；
- create-directory 对 `/` 按“目录已存在”处理；
- writer target、copy source/destination、delete、rename operand 和 persist target 不得是 `/`；
- 违反根保护规则返回 `InvalidPath`，不能依赖平台偶然报错。

### 8.7 Host 路径

Host authority 表示进程可见的本地 namespace。Unix absolute path 从 `/` 开始；relative
path 从实例 PWD 开始。

Windows Host 保留 native drive/prefix 语义：

- fully-qualified drive path 可作为 absolute path；
- ordinary relative path 从实例 PWD 开始；
- root-relative path 使用实例 PWD 所在 volume；
- 易受进程隐式 per-drive current-directory 影响的 drive-relative 形式（如 `C:foo`）应
  拒绝，除非平台层能够以实例状态无歧义解析；
- UNC/device namespace 只有在平台层定义并实现完整 authority 语义时才报告支持。

### 8.8 非 UTF-8 与名称边界

路径规范化只解释结构组件，不修改 normal component 的字节/code unit。Unix 非 UTF-8
名称和 Windows native 字符保持原值。`.`、`..`、root 和 prefix 是路径结构，不作为普通
文件名传递。

## 9. Symlink 与 reparse point

### 9.1 策略

```rust
pub enum LocalSymlinkPolicy {
    Reject,
    FollowWithinScope,
    FollowAcrossScope,
}
```

- `Reject`：拒绝任何操作必须穿过的 symlink/reparse point；查看、删除或重命名链接 entry
  本身不属于“穿过”。
- `FollowWithinScope`：允许跟随，但解析结果必须留在当前 filesystem namespace 内。
- `FollowAcrossScope`：只适用于 Host；Rooted 配置该值返回 `InvalidOptions`。

Rooted 初始值为 `FollowWithinScope`；Host 初始值为 `FollowAcrossScope`。

### 9.2 链接目标路径

链接目标进入与调用方路径相同的 component 规范化逻辑：

- relative target 相对于链接所在目录；
- `.` 保持当前链接解析目录；
- `..` 返回一层，越过虚拟根返回 `InvalidPath`；
- Rooted 中的 absolute target 从虚拟 `/` 开始，而不是从 Host `/` 开始。

例如 Rooted authority 的 Host 锚点为 `/srv/app`，其中：

```text
/srv/app/link -> /etc
```

虚拟 `/link` 应解析到虚拟 `/etc`，也就是同一 authority 内的概念位置
`/srv/app/etc`，不能访问 Host `/etc`。

这项语义与 chroot namespace 一致。Linux 应优先使用能提供 `RESOLVE_IN_ROOT` 等保证的
原语；其他平台的 fallback 必须实现相同公开契约。

### 9.3 循环

symlink/reparse point 循环必须终止并返回结构化路径错误。实现优先使用操作系统循环错误或
底层对象 identity 检测。固定 expansion 数量不得成为调用方不可见的业务资源上限；平台
为单次路径解析施加的固有限制应作为平台 I/O/unsupported 事实报告。

### 9.4 最终组件

最终 symlink 按操作性质处理，跨平台公开语义必须一致：

| 操作 | 最终 symlink/reparse point |
| --- | --- |
| `metadata` | 查看链接 entry 本身，不跟随 |
| `list` 返回 entry | 返回链接 entry metadata；递归进入由策略决定 |
| `open_reader` | 按策略跟随内容目标；`Reject` 时失败 |
| writer `CreateNew` | 把链接视为已存在 entry，不跟随创建 |
| writer `Append` | 按策略跟随目标后追加 |
| writer `CreateOrReplace` | 按策略跟随目标并替换目标内容，保留链接 entry |
| `delete_file` | 删除链接 entry，不跟随最终目标，即使目标是目录 |
| `delete_directory` | 不跟随；最终 entry 是链接时返回类型错误 |
| `rename` | 移动或替换链接 entry |
| copy source | 复制链接 entry 本身 |
| copy destination | 替换 destination entry 本身 |
| temp persist target | 通过 rename 安装并替换 target entry 本身 |

某个平台无法可靠实现相同语义时必须返回 `Unsupported` 或 `RequirementNotMet`，不能静默
采用不同结果。

## 10. Authority 与平台安全模型

### 10.1 Rooted 单一 authority

一个 Rooted `LocalFileSystem` 只打开一个根 authority。所有操作、capability/limit 探测、
walker、writer 和临时资源都从该 authority 或它的安全 duplicate 派生。

实现不得同时维护两个分别打开的 root 对象并让不同操作走不同对象。这样可以避免构造期间
root path 被替换时形成两个实际目录，也避免 operation contract 因内部 authority 状态而分叉。

### 10.2 诊断路径不是 authority

`diagnostic_root()` 返回构造时捕获的 Host absolute path snapshot：

```rust
let filesystem = LocalFileSystem::rooted(Path::new("/srv/app"))?;
assert_eq!(filesystem.diagnostic_root(), Some(Path::new("/srv/app")));
```

Host filesystem 的 `diagnostic_root()` 返回 `None`，因为它没有单一 Host 根。相对 root
构造参数先按构造时进程 current directory 形成 Host absolute lexical snapshot；该值不承担
canonical containment 证明。

它只用于日志和调试。若该 Host path 后来被重命名、删除或替换，已打开 Rooted authority
仍指向原目录；实现不得重新打开 `diagnostic_root()` 继续操作。

资源的 `diagnostic_path()` 同样只是 Host 侧 best-effort hint，可能因 rename 过时。它不得
作为访问接口或权限凭据。需要把路径交给不理解虚拟 namespace 的外部程序时，应使用 Host
filesystem 创建资源，或者显式设计可信的 handle 传递；不得把 Rooted diagnostic path 当成
稳定授权路径。

Host 资源在能够形成 fully-qualified native path 时返回该路径。Rooted 资源可以基于
diagnostic root 与虚拟 components 形成 lexical hint；无法无损或无歧义映射时返回 `None`，
不得为了始终返回路径而使用 lossy conversion。

### 10.3 平台原语

Unix 实现优先使用：

- directory-fd-relative `openat`/`renameat`/`unlinkat` 等操作；
- `O_NOFOLLOW`、`AT_SYMLINK_NOFOLLOW` 等最终组件控制；
- Linux `openat2` 的 `RESOLVE_IN_ROOT`、`RESOLVE_BENEATH` 或等价组合；
- descriptor metadata 与 identity，而不是重新查找诊断路径。

Windows 实现使用：

- 已打开目录 handle；
- root-handle-relative name resolution；
- reparse-point-aware 打开和 metadata；
- file ID/volume identity；
- 能表达 no-replace、replace 和 durability 的 native primitive。

平台缺少可靠原语时不得退化成 `canonicalize` 后比较字符串前缀。无法满足完整 contract 的
操作返回 `Unsupported` 或 `RequirementNotMet`，并由 capability snapshot 准确反映。

### 10.4 词法层与操作系统层分工

库内手动逻辑只处理：

- 虚拟 root；
- absolute/relative input；
- PWD；
- `.`、`..` 和 root escape；
- native component shape；
- Options 和 operation preflight。

操作系统层处理：

- 目录项实际查找；
- symlink/reparse point 内容；
- case sensitivity 与 Unicode normalization；
- mount、hard link 和文件 identity；
- 权限、锁、共享模式和 native I/O；
- 原子 rename/replace 和 sync。

### 10.5 Mount 与 hard link

Rooted 保证 namespace path 不越过打开的根，不默认保证所有后代位于同一个 device。根下的
mount point 仍是根 namespace 的后代。需要禁止跨 device 的调用方应在更高层依据 metadata
或未来明确的 operation option 实现。

Hard link 可以让根内 entry 与根外 entry 指向同一 inode/file ID，但操作仍从根内 name
执行。Copy 和 overwrite 必须检测 source/target hard-link alias，避免把 source 作为 target
覆盖。

## 11. 通用操作数据流

每个 `LocalFileSystem` 操作遵循相同阶段：

1. 捕获实例 PWD、symlink policy 和所选 Options；
2. 选择实例默认 Options 或显式完整 Options；
3. 验证 Options 与已知 capability；
4. 把所有输入路径规范化为 namespace-absolute path；
5. 在任何破坏性 I/O 前完成可证明的 preflight；
6. 通过 Host 或 Rooted authority 执行 native operation；
7. 把结果、资源路径和错误上下文转换成虚拟 namespace path；
8. 返回 actual atomicity、durability、publication state 和 partial statistics。

两路径操作在步骤 1 只捕获一次状态。已经打开的 Reader、Writer、Walker 和 temp resource
持有自己的路径/authority snapshot，后续不读取原 `LocalFileSystem` 的可变配置。

## 12. 路径输出契约

### 12.1 主路径都是可复用的虚拟绝对路径

任何名为 `path()`、`root()`、`source_path()`、`target_path()` 或 `staging_path()` 的公开
资源身份路径都使用 owning filesystem 的 namespace-absolute 坐标：

- Rooted：以虚拟 `/` 开始；
- Unix Host：以 Host `/` 开始；
- Windows Host：使用稳定 fully-qualified native absolute form。

这些路径可以直接传回同一个 filesystem，不依赖当前 PWD。

### 12.2 Directory entry

`LocalDirectoryEntry` 同时提供：

```rust
impl LocalDirectoryEntry {
    pub fn path(&self) -> &Path;
    pub fn relative_path(&self) -> &Path;
    pub fn diagnostic_path(&self) -> Option<&Path>;
    pub fn metadata(&self) -> &LocalFileMetadata;
}
```

例如 `list("/assets")` 产生 `/assets/icons/add.png`：

```text
path()            = "/assets/icons/add.png"
relative_path()   = "icons/add.png"
diagnostic_path() = Host 侧 best-effort hint（若可得）
```

`path()` 用于再次调用 filesystem；`relative_path()` 用于相对 walker root 的过滤、显示或
层级计算。

### 12.3 Temp 与 publication path

`LocalTempFile::path()`、`LocalTempDirectory::path()`、persist outcome target 和 copy failure
staging path 都返回虚拟绝对路径。内部 authority-relative representation 不作为公共
Path 泄漏。

## 13. Metadata、能力与读取

### 13.1 Metadata

`metadata(path)` 返回最终 entry 自身的 normalized metadata，不跟随最终 symlink。返回的
`LocalFileMetadata` 至少表达：

- `LocalFileKind`；
- 文件长度；
- `LocalFilePermissions`；
- 平台可可靠获取的时间信息；
- 操作需要的 identity/device 信息通过私有或明确扩展字段保留。

不存在以 `Option` 表示“没找到”的隐式查询；NotFound 使用结构化错误。应用需要 exists 时
可以依据 `LocalFileErrorKind::NotFound` 明确处理。

### 13.2 Reader

`LocalFileReader` 拥有已打开 native file，实现 `Read + Seek`，并允许读取与同一 handle
对应的 metadata。打开成功后路径 rename、PWD 变化或 diagnostic root 变化不重定向 reader。

`read_prefix(path, max_bytes)` 最多读取 `max_bytes`，不会把整个文件隐式载入内存。`max_bytes`
是该方法的业务参数，不是隐藏资源限制。

打开重试只在 `LocalReadOptions` 明确提供窗口时发生。重试不得吞掉不可重试错误，也不得在
deadline 后继续。

### 13.3 Filesystem limits 与 space

`LocalFileSystemLimits` 描述底层 filesystem 的客观 path/name 限制，不是应用预算：

```rust
pub enum SizeLimit {
    Maximum(usize),
    VariesByPath,
    Unknown,
}
```

Rooted 可以从已打开 authority 缓存稳定 snapshot；Host 的 `limits()` 在限制依路径变化时
返回 `VariesByPath`，`limits_at(path)` 针对规范化目标探测。

`LocalFileSystemSpace` 返回 capacity、free 和 calling identity available bytes；未知值为
`None`。Space 是动态观察，不缓存为长期保证。

## 14. Writer publication

### 14.1 模式

```rust
pub enum LocalWriteMode {
    CreateNew,
    CreateOrReplace,
    Append,
}
```

- `CreateNew`：目标任何 entry 已存在都失败；
- `CreateOrReplace`：完整写入后替换目标内容；
- `Append`：直接修改已有 regular file。

`CreateNew` 是实例初始默认值，因为它不会意外覆盖已有数据。

### 14.2 生命周期与发布状态

`LocalFileWriter` 同时是 `Write` byte stream 和 publication session：

```rust
pub enum LocalWriterState {
    Open,
    Committed,
    Aborted,
}

pub enum LocalWriteFailureState {
    NotPublished,
    Published,
    Indeterminate,
}
```

生命周期与 namespace 事实是两个维度。只有 `Open` 可以继续 write/flush。普通 byte I/O
error 不能自动证明没有副作用；无法证明时 failure state 为 `Indeterminate`。

### 14.3 Staged publication

`CreateNew` 和 `CreateOrReplace` 优先在 destination 同目录创建 private staging entry：

1. 写入 bytes；
2. flush 用户空间 buffer；
3. 按 durability requirement 同步 file data/metadata；
4. 通过 native rename/install 发布；
5. 按 requirement 同步 parent directory；
6. 返回实际 publication method、atomicity、durability 和 bytes written。

Required requirement 在无法满足时必须尽量于 namespace 修改前失败。若 publish 已完成但
parent sync 失败，commit failure state 为 `Published`，不能报告 `NotPublished`。

### 14.4 Append

Append 直接修改目标，不能满足 `LocalAtomicityRequirement::Required`。`Preferred` 可以降级
为 direct append，但 outcome 必须报告 `atomic = false`。写入发生后 abort 不承诺回滚已经
追加的 bytes。

### 14.5 Commit error

`LocalFileCommitError` 保留：

- 主 `LocalFileError`；
- `LocalWriteFailureState`；
- 在可安全继续处理时返回 writer 所有权。

调用方可以基于 state 决定重试、清理、保留或人工检查，不能根据 error message 猜测。

## 15. Lazy directory walk

### 15.1 Walker

`LocalDirectoryWalker` 是惰性迭代器：

```rust
Iterator<Item = LocalResult<LocalDirectoryEntry>>
```

它按需读取 directory entry，不预先把整棵树或单个大目录完整收集到 `Vec`。Walker 创建时
固定规范化 root、Options、symlink policy、PWD snapshot 和 authority。

### 15.2 预算

调用方可显式设置：

- `max_depth`；
- `max_entries`；
- `max_seen_name_bytes`；
- `max_open_directories`；
- `deadline`。

未设置时库不添加默认业务上限。设置 `max_open_directories` 后，
`LocalDirectoryReopenPolicy` 决定达到预算时重新打开 frame 还是返回 `ResourceLimit`。

### 15.3 错误策略

`LocalWalkErrorPolicy::FailFast` 在第一个错误终止；continue 模式逐项返回错误并继续可安全的
其他分支。任何模式都不能静默跳过未报告错误。

递归跟随目录链接时通过 directory identity 检测循环。输出 `path()` 保留调用方看到的逻辑
link 路径，而不是改写成物理 target path；深度按逻辑输出层级计算。

Walker drop 只释放 handle 和内存，不执行 namespace 修改。

## 16. Copy

### 16.1 统一入口与结果

file 和 directory tree 使用同一个 copy 操作。实现根据 source metadata 和
`LocalCopySourceMode` 选择 file、tree 或返回类型错误。

```rust
pub type LocalCopyResult =
    Result<LocalCopyOutcome, LocalCopyFailure>;

pub enum LocalCopyFailureState {
    Unchanged,
    PartiallyPublished,
    Published,
    Indeterminate,
}
```

状态只陈述 destination 的已知事实：

- `Unchanged`：本次操作未改变 destination；
- `PartiallyPublished`：destination 已产生修改，但请求内容未完整发布；
- `Published`：完整内容已发布，随后 metadata/durability/cleanup 步骤失败；
- `Indeterminate`：native failure 无法证明最终状态。

### 16.2 Failure 内容

`LocalCopyFailure` 保留：

- 主 `LocalFileError`；
- request source/target；
- 实际失败 source/target；
- strongest known state；
- partial `LocalCopyStats`；
- 只有清理失败时才存在的 staging path 和 cleanup error。

所有路径遵循虚拟绝对路径契约。

### 16.3 Preflight

在 destructive target I/O 前检查：

- source 与 destination 是同一路径；
- hard-link alias；
- directory destination 位于 source subtree；
- source/destination kind conflict；
- overwrite 与 type-conflict policy；
- required atomicity/durability 是否已知可满足；
- 所有显式 traversal budget 是否有效。

Preflight 只能提前拒绝可证明的不合法情况，不能用不可靠字符串判断替代 authority 操作。

### 16.4 File publication

需要原子 publication 时，staging entry 位于 destination 同目录。Outcome 报告：

- `LocalCopyMethod`；
- `LocalCopyStats`；
- actual atomicity；
- actual durability；
- metadata preservation 结果。

Required semantics 不能通过 outcome 静默降级。Publish 成功而 parent sync 失败时返回
`Published`。

### 16.5 Tree copy

Tree copy 按 Options 控制递归、symlink、metadata 和预算。特殊文件默认拒绝，除非公开策略
明确允许。发生错误时保留已经创建的 entry 统计并报告 `PartiallyPublished`；本库不假装
能够普遍事务化整棵目录树。

Copy 永远不得修改 source。

## 17. Directory create、delete 与 rename

### 17.1 Create directory

`LocalCreateDirectoryOptions` 控制 recursive 与 exists-ok：

- 非递归模式要求 parent 已存在；
- 递归模式只创建缺失目录，不替换非目录 entry；
- exists-ok 只把“目标已经是目录”视为成功；同名其他类型仍返回 type conflict；
- outcome 明确报告本次是否创建了目标目录。

Rooted `/` 已存在；exists-ok 为 true 时返回 `created = false`，否则返回 AlreadyExists。

Recursive create 不是事务。若已经创建至少一个父目录后失败，返回
`LocalFileErrorKind::PublicationIncomplete`，`path` 指向第一个未能完成的虚拟绝对路径；
已经创建的目录不回滚。尚未创建任何 entry 时保留原始错误 kind。

### 17.2 Delete

file 和 directory delete 分开，避免调用方意外删除错误类型：

- `delete_file` 删除 regular/symlink/非目录 entry，不跟随最终链接；
- `delete_directory` 只删除目录；recursive 必须显式设置；
- missing-ok 只改变 NotFound 结果，不吞掉 permission、type 或 I/O 错误；
- Rooted `/` 永远不能删除。

Recursive delete 不是事务。若至少一个 entry 已删除后失败，返回
`LocalFileErrorKind::PublicationIncomplete`，`path` 指向失败的虚拟绝对 entry；已经完成的
删除不回滚。尚未删除任何 entry 时保留原始错误 kind。`LocalDeleteOutcome` 只表示完整成功，
调用方处理 `PublicationIncomplete` 时必须重新检查剩余树，不能根据 message 猜测状态。

### 17.3 Rename

```rust
pub type LocalRenameResult =
    Result<LocalRenameOutcome, LocalRenameFailure>;

pub enum LocalRenameFailureState {
    Unchanged,
    Renamed,
    Indeterminate,
}
```

Rename 必须使用同一 authority 内的 native rename primitive，永远不在本库内伪装成
copy+delete。跨 device 或无法满足 no-replace/replace guarantee 时返回明确错误。

`LocalRenameOptions` 表达 overwrite 和 durability。Rename 的 namespace transition 本身必须
原子；它不是一个可以通过 Options 静默降级的偏好。

Native rename 成功但 parent durability 失败时，failure state 为 `Renamed`。只有能证明
source/target 未改变时才返回 `Unchanged`；不确定的 native error 返回 `Indeterminate`。

source 或 destination 为 Rooted `/` 时返回 `InvalidPath`。

## 18. Temporary resource

### 18.1 所有权模型

`LocalTempFile` 和 `LocalTempDirectory` 是拥有 cleanup responsibility 的 RAII 对象：

```text
Owned
 ├─ persist success ─────────────► Persisted
 ├─ keep ────────────────────────► Kept
 ├─ cleanup success ─────────────► Cleaned
 ├─ published / cleanup pending ─► CleanupRequired
 └─ namespace unknown ───────────► Indeterminate
```

Drop 只对仍可证明 owned 的资源执行 best-effort cleanup；`Indeterminate` 状态不得自动删除。
需要观察清理错误的调用方必须显式调用 `cleanup()`。

### 18.2 创建位置与路径

`LocalTempFileOptions::parent` 和 `LocalTempDirectoryOptions::parent` 接受 absolute 或 relative
虚拟路径。`None` 表示创建时的 filesystem PWD。创建成功后：

- `path()` 返回虚拟绝对路径；
- 资源保存 authority；
- 资源保存创建时 PWD snapshot；
- 后续 filesystem PWD 变化不改变资源身份。

随机 prefix、suffix 和最终 component 在创建 entry 前完成 separator、NUL 和平台保留名称
校验。调用方未提供 `max_attempts` 时，本库不施加业务重试上限；遇到非 collision 的 native
错误立即返回。

### 18.3 私有 sandbox

每个临时资源在选定 parent 下先创建 private sandbox，再在 sandbox 中创建实际 entry。
成功 cleanup 或 persist 后移除空 sandbox；`keep()` 把 sandbox 与资源路径一起移交调用方。

这样可以缩小共享 parent 中同名替换的攻击面，但不把 sandbox 宣称为跨不受信任并发写入者
的绝对同步边界。

### 18.4 Close、keep 与 cleanup

`LocalTempFile::close(&mut self)` 只关闭内容 I/O handle，资源仍然 owned；path、persist、keep
和 cleanup 继续可用。这支持把已关闭文件交给要求 path 的外部进程。

`keep(self)` 消耗资源并移交 cleanup responsibility，返回虚拟绝对路径。`cleanup(&mut self)`
成功后进入 Cleaned；重复调用遵循明确的 idempotent/invalid-state contract，不能误删新 entry。

### 18.5 Persist

```rust
temp.persist(target)
temp.persist_with(target, options)
```

相对 target 基于 temp 创建时捕获的 PWD，而不是某个后来变化或已经销毁的
`LocalFileSystem`。Absolute target 从同一 namespace root 开始。Rooted `/` 不能作为 target。

Persist 在同一 authority 内使用 native rename/install。Outcome 报告：

- 最终虚拟 target path；
- `LocalPersistMethod`；
- actual atomicity；
- actual durability；
- sandbox cleanup state 和可选 cleanup error。

Failure 保留 `LocalPersistStage` 和 `LocalPersistFailureState`：

- `NotPublished`：target 未发布，temp 仍可安全处理；
- `Indeterminate`：native install 可能改变 namespace，不得自动清理。

### 18.6 Temp directory child

`child(component)` 只接受一个合法 normal component；`descendant(path)` 接受相对于 temp
directory 的路径，允许 `.`、`..` 规范化但不得越过 temp directory 自身。两者返回虚拟绝对
路径，不隐式打开或创建 entry。

### 18.7 身份检查与限制

临时资源记录创建时 native entry identity。Persist、cleanup 和 Drop 在删除或发布前核对
当前 name 仍指向该 identity；发现普通替换时拒绝操作并转入不可安全清理的状态。

Identity check 与删除通常不是一个跨平台原子操作，inode/file ID 也可能复用。因此，如果
不受信任并发者能在同一目录反复替换同名同类型 entry，本库不保证绝不会删除替换项。需要
更强保证时，调用方必须排除并发写入，或 `keep()` 后自行同步。

## 19. 结构化错误

### 19.1 基础错误域

```rust
pub type LocalResult<T> = Result<T, LocalFileError>;

pub struct LocalFileError {
    kind: LocalFileErrorKind,
    operation: LocalFileOperation,
    path: Option<PathBuf>,
    target: Option<PathBuf>,
    current_directory: Option<PathBuf>,
    reason: Option<&'static str>,
    source: Option<LocalFileErrorSource>,
    cleanup_error: Option<Box<LocalFileError>>,
}
```

字段是概念模型；实现可以用等价的紧凑表示。必须可稳定查询 kind、operation、主路径、目标
路径、PWD context、typed source 和 cleanup error。

### 19.2 Path context

路径规范化失败时：

- `path`/`target` 保留调用方输入；
- `current_directory` 保留解析 snapshot；
- kind 为 `InvalidPath`；
- reason 明确指出 root escape、prefix、NUL 或其他路径契约。

规范化成功后的 I/O/operation error 使用虚拟绝对 path/target。Host physical diagnostic path
不覆盖主路径字段，可作为受控诊断附加信息。

例如：

```text
kind: InvalidPath
operation: OpenReader
path: "../../../../etc/passwd"
current_directory: "/work/project"
reason: "path escapes the virtual root"
```

### 19.3 Error kind

`LocalFileErrorKind` 至少区分：

- `InvalidPath`；
- `InvalidOptions`；
- `InvalidState`；
- `NotFound` / `AlreadyExists`；
- `NotDirectory` / `IsDirectory` / `TypeConflict`；
- `PermissionDenied`；
- `Unsupported`；
- `RequirementNotMet`；
- `ResourceLimit`；
- `DataCorruption`；
- `PublicationIncomplete`；
- `Indeterminate`；
- ordinary `Io`。

`ResourceLimit` 只能来自调用方显式预算或操作系统实际限制，不能来自隐藏实例 cap。

### 19.4 Operation

`LocalFileOperation` 表达失败发生的公共语义，包括 configure、set-current-directory、path
compose、metadata、open/read/write、list、copy、create/delete、rename、temp、persist、
commit、abort 和 cleanup。外层操作错误保持外层 operation；底层 stage 通过 typed source 或
专用 failure 表达，避免调用方看到无关内部函数名。

### 19.5 Typed source

`LocalFileErrorSource` 至少保留：

- `std::io::Error`；
- `LocalPathCodecError`；
- 结构化 resource-limit detail。

转换成 `io::Error` 是显式的 lossy adapter 操作；默认错误链不得丢失 publication state、
path codec kind 或 cleanup error。

### 19.6 专用 failure

以下结果继续使用专用类型：

- `LocalCopyFailure`；
- `LocalRenameFailure`；
- `LocalFileCommitError`；
- `LocalPersistError`。

专用 state 是恢复逻辑的一部分。Display message 只用于人类诊断，不能成为下游分支依据。

## 20. Protocols、requirements 与运行时事实

### 20.1 Protocol snapshot

`LocalFileSystemProtocols` 报告当前 build/target 是否实现完整协议：

- rooted operations；
- atomic rename；
- atomic replace；
- atomic no-replace temp persist；
- durable rename；
- durable file copy。

这些 flag 描述库实现能力，不代表任意 runtime mount、network filesystem 或硬件一定支持。

### 20.2 Requirement

```rust
pub enum LocalAtomicityRequirement {
    Required,
    Preferred,
    NotRequired,
}

pub enum LocalDurabilityRequirement {
    Required,
    Preferred,
    NotRequired,
}
```

- `Required`：不能满足时返回 `RequirementNotMet`，并尽可能保证 namespace 未改变；
- `Preferred`：允许安全降级，但 outcome 必须报告实际结果；
- `NotRequired`：调用方不要求该保证，仍可使用平台自然提供的能力。

Capability preflight 和 operation outcome 都不可省略：前者帮助规划，后者陈述实际执行结果。

### 20.3 Runtime probing

`limits_at()` 和 `space_at()` 从规范化路径的 nearest existing authority handle 探测，避免为
不存在 target 错误地要求最终 entry 已存在。探测失败返回结构化 error，不伪造值。

## 21. Clone、并发与线程边界

### 21.1 Clone

`LocalFileSystem::clone()`：

- 共享 immutable authority core；
- 复制 PWD；
- 复制 symlink policy；
- 复制全部默认 Options。

Clone 后修改一个实例的 PWD 或默认 Options 不影响另一个实例。Rooted clones 仍指向同一个
打开的根 authority。

### 21.2 配置与操作

- 配置 setter 使用 `&mut self`；
- filesystem 操作使用 `&self`；
- 不使用 `Mutex`、`RwLock` 或对配置的内部可变性；
- 不提供多个线程并发修改同一实例的内部同步。

调用方通常为每个线程持有自己的 clone。必须共享可变实例时，由调用方使用
`Arc<Mutex<LocalFileSystem>>` 或其他同步边界。

库不通过人为 marker 强制取消 `Send`/`Sync`；auto trait 由实际字段决定，但“配置可并发
修改”不属于 contract。

### 21.3 已打开资源

Reader、Writer、Walker 和 temp resource 不借用可变 filesystem 状态。它们拥有或共享所需
handle、规范化路径、Options 和 PWD snapshot，因此可以在 filesystem 修改配置或销毁后继续
遵循创建时契约。

## 22. Path 与 filename 工具

### 22.1 LocalPaths

`LocalPaths` 负责 scope-aware native path 与 canonical components 转换。它不执行业务 I/O，
但必须使用与 `LocalFileSystem` 相同的虚拟根规则。

```rust
impl LocalPaths {
    pub const fn host() -> Self;
    pub const fn rooted() -> Self;

    pub fn from_canonical_components<'a>(
        &self,
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf>;

    pub fn to_canonical_components(
        &self,
        path: &Path,
    ) -> LocalResult<Vec<String>>;
}
```

Rooted 的空 canonical component sequence 表示虚拟 `/`，非空 sequence 产生虚拟绝对路径
`/a/b`，不产生对外 root-relative path。Host conversion 保留平台 root/drive authority。

上述 canonical conversion API 只接受和产生 namespace-absolute path；传入 relative path
返回 `InvalidPath`。相对路径与 PWD 的绑定由有状态 `LocalFileSystem` 负责，`LocalPaths`
不得拥有 PWD，也不得隐式读取进程 current directory。

### 22.2 LocalFileNames

`LocalFileNames` 负责：

- native single-component 校验；
- portable name policy；
- 调用方显式选择的最大 component byte/code-unit 限制；
- 安全随机名称；
- prefix/suffix 与平台保留名称检查。

Filename API 返回 `OsStr`/`OsString`，只有明确的 portable text API 使用 `str`。
最大 component 大小使用 `Option<usize>`；初始值为 `None`，表示不施加库级长度预算，交由
实际 filesystem 返回其客观限制。`with_max_component_bytes(n)` 设置显式限制，
`without_max_component_bytes()` 清除限制。`portable()` 选择字符、separator 和保留名称规则，
不以常见的 `255` 假装所有目标 filesystem 都具有同一上限。

### 22.3 LocalPathCodec

`LocalPathCodec` 提供 native component 与 canonical UTF-8 text 的可逆转换，不解释 URI 或
provider hierarchy：

- 合法、非 control Unicode scalar 原样保留；
- `%`、control byte、无效 UTF-8 byte 和 Windows WTF-8 非配对 surrogate 使用 uppercase
  `%HH`；
- decode 后重新 encode 必须与输入完全相等；
- lowercase escape、不必要 escape、alias 和 malformed escape 一律拒绝；
- Unix 任意非 NUL filename byte 必须无损 round-trip；
- Windows native UTF-16 必须无损 round-trip；
- 不支持稳定无损转换的平台返回 `UnsupportedNativeEncoding`，不得 lossy fallback。

Codec 只处理单 component text。Decode 后是否包含 separator、root、prefix、`.` 或 `..` 由
调用该 codec 的 filename/path context 继续验证。

## 23. 与 qubit-fs-local 的契约

`qubit-fs-local` 只承担抽象层转换：

```text
qubit_fs::spi::Request
  → canonical logical path / abstract options
  → LocalFileSystem virtual path / native options
  → local outcome / failure / resource
  → qubit_fs outcome / error / session
```

具体要求：

- `qubit-fs` 的绝对逻辑 `/a/b` 映射为 local filesystem 虚拟绝对 `/a/b`；
- Rooted adapter 不剥离开头 `/`，不维护第二套 root-relative 坐标；
- `qubit-fs` 当前只接收 absolute logical path，因此 adapter 通常保持 local filesystem PWD
  为 `/`；
- Local provider defaults 直接配置到其 `LocalFileSystem` 实例，不另存
  `list_defaults`/`copy_defaults`；
- 单次 abstract request 转换成完整 native Options，并调用 `*_with_options()`；
- `LocalDirectoryEntry::path()` 已是完整虚拟绝对路径，adapter 不重新拼接 listing root；
- copy/rename/writer/temp 的 typed failure state 必须无损映射；
- codec 和 Windows/Unix native component 逻辑只存在于 `qubit-local-files`。

Provider identity、registry、URI、user metadata 和远程 capability 仍属于 `qubit-fs` 层。

## 24. 直接应用使用方式

### 24.1 配置一个 Rooted 应用文件系统

```rust
use std::path::Path;
use qubit_local_files::{
    LocalCopyOptions,
    LocalFileSystem,
    LocalListOptions,
    LocalSymlinkPolicy,
};

let mut filesystem = LocalFileSystem::rooted(Path::new("/srv/app"))?;
filesystem.set_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)?;
filesystem.set_default_list_options(
    LocalListOptions::new()
        .with_recursive()
        .with_max_entries(100_000),
)?;
filesystem.set_default_copy_options(
    LocalCopyOptions::new().with_max_bytes(1 << 30),
)?;
filesystem.set_current_directory(Path::new("/workspace"))?;

let walker = filesystem.list(Path::new("assets"))?;
for entry in walker {
    let entry = entry?;
    // entry.path() 是可直接回传的虚拟绝对路径。
    let metadata = filesystem.metadata(entry.path())?;
    assert_eq!(metadata.kind(), entry.metadata().kind());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### 24.2 单次完整覆盖

```rust
let options = filesystem
    .default_copy_options()
    .clone()
    .with_max_bytes(16 * 1024 * 1024);

filesystem.copy_with_options(
    Path::new("input.bin"),
    Path::new("/archive/input.bin"),
    &options,
)?;
# Ok::<(), qubit_local_files::LocalCopyFailure>(())
```

显式 Options 是本次完整配置；实例的其他 copy defaults 不会隐式合并回来。

## 25. 内部组件边界

建议的职责边界如下；具体文件名可以演化，但不得重新混合职责：

```text
LocalFileSystem facade
├── instance configuration
│   ├── PWD
│   ├── symlink policy
│   └── operation defaults
├── virtual path resolver
│   ├── absolute/relative
│   ├── dot/parent normalization
│   ├── root escape
│   └── native component validation
├── authority core
│   ├── Host authority
│   └── Rooted single-handle authority
├── operations
│   ├── metadata/read
│   ├── writer publication
│   ├── lazy walk
│   ├── copy
│   ├── create/delete/rename
│   └── temp/persist
├── structured outcome/error
└── platform
    ├── Unix descriptor-relative primitives
    └── Windows handle-relative primitives
```

公共 facade 不包含平台 `cfg` 状态机。Unix/Windows module 只实现 primitive，不复制 copy、
writer、persist 等业务状态机。

Fault injection 只在明确 test-support feature 下可用，不属于默认应用 API。测试 hook 不能改变
production 对象的正常 state model。

## 26. 验证策略

### 26.1 API contract tests

- Host/Rooted 使用相同方法集合；
- getter 返回实际实例默认 Options；
- setter 成功后普通方法使用新默认值；
- `*_with_options()` 完整替代而非合并；
- 构造和 setter 失败保持对象原状态；
- compile examples 覆盖所有主入口。

### 26.2 Path property tests

至少覆盖：

- `/`、`.`、`""` 与不同 PWD；
- absolute/relative 等价关系；
- `a/./b`、`a/../b`、重复和末尾 separator；
- 每一种越过虚拟根的 parent 序列；
- 两路径操作使用同一 PWD snapshot；
- Unix 非 UTF-8；
- Windows drive、root-relative、prefix、UNC/device policy；
- canonical component round-trip；
- PWD 修改失败不改变状态。

规范化器适合使用 table-driven 和 property-based 测试：任何成功结果必须是 namespace-absolute、
不含 `.`/`..`，再次规范化保持不变。

### 26.3 Rooted authority tests

- absolute virtual path 映射到 root handle 后代；
- diagnostic root rename/replacement 不重定向操作；
- 构造期间只形成一个 authority；
- lexical parent escape 返回 `InvalidPath`；
- absolute symlink target 从虚拟 `/` 解析；
- relative symlink target 的 `.`、`..`；
- symlink/reparse escape 与循环；
- platform primitive unavailable 时明确 Unsupported；
- 不存在 canonicalize-prefix authorization fallback。

### 26.4 Clone 与生命周期 tests

- clone 共享 Rooted authority；
- clone 的 PWD、symlink policy 和默认 Options 独立；
- 进程 current directory 改变不影响已有 Host instance；
- Reader、Writer、Walker 不受后续 PWD 修改影响；
- temp persist relative target 使用创建时 PWD；
- filesystem drop 后已打开资源仍遵循创建时 authority。

### 26.5 Operation state tests

- writer 每个 commit/abort/failure state；
- publish 成功但 durability 失败报告 Published；
- copy 四种 failure state、partial stats、staging cleanup failure；
- hard-link alias、self-copy 和 destination-inside-source；
- rename 成功后 durability 失败报告 Renamed；
- walk 显式 depth/entries/name-bytes/open-directories/deadline；
- 无显式预算时不存在固定库内 cap；
- temp 每个 persist/cleanup/keep/identity-replacement state；
- Rooted `/` 的允许与禁止操作矩阵。

### 26.6 跨平台和下游

- Linux、Windows、macOS runtime tests；
- Android、FreeBSD 等只有 compile-check 时文档不得宣称 runtime guarantee；
- `cargo test --all-features`；
- `cargo clippy --all-targets --all-features -- -D warnings`；
- `qubit-fs-local` 全特性与 provider contract tests；
- `qubit-mime` 临时文件集成测试；
- README 和用户指南示例作为编译测试。

测试必须验证可观察行为和真实操作结果，不能只验证字段 getter 已保存某个值。

## 27. 安全保证与明确限制

### 27.1 保证

在受支持平台和有效 Options 下，本库保证：

- Rooted operation 从唯一已打开 root authority 执行；
- 输入路径不能通过 lexical `..` 越过虚拟根；
- Rooted symlink absolute target 被重新锚定到虚拟根；
- `FollowWithinScope` 不访问 root namespace 外的 name；
- 诊断路径不作为 authority；
- 显式 Options 不受隐藏实例 hard cap；
- Required guarantee 不静默降级；
- 已知部分成功通过 typed state 报告；
- native filename 不经 lossy UTF-8 转换。

### 27.2 不保证

本库不保证：

- 绕过调用进程的 OS 权限；
- 阻止 Rooted 后代中的 mount point；
- 不受信任并发写入者下所有跨平台 TOCTOU 都可消除；
- diagnostic Host path 在 rename 后仍有效；
- `Preferred` requirement 一定实现；
- 操作系统报告 success 后物理介质绝不丢失数据；
- 不设置资源预算的调用一定只消耗有限应用资源；
- 同一个可变 `LocalFileSystem` 可以无外部同步地由多个线程配置。

## 28. 设计完成条件

实现被视为符合本设计，必须同时满足：

1. 公共 API 与第 6 节一致；
2. Options 选择满足完整替代规则；
3. Host/Rooted 都具有实例 PWD；
4. Rooted absolute path、`.`、`..`、absolute symlink target 和 root escape 满足本文；
5. Rooted 只有一个 authority；
6. 所有公开资源身份路径使用虚拟绝对坐标；
7. copy、rename、writer、temp 的结构化状态无损；
8. 无调用方未设置的业务资源上限；
9. `qubit-fs-local` 不重复 path/authority/default-option 逻辑；
10. 第 26 节验证矩阵在声明支持的平台通过。

本文是 `qubit-local-files` 的长期设计基线。新增 API 或改变行为时，应先判断是否仍满足上述
原则和不变量；若需要改变本设计，应同步修改本文、公开文档和 contract tests。
