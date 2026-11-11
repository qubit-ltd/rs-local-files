# rs-local-files 0.6.0 详细设计：异步本地操作、资源生命周期与交接

- 状态：待评审设计，尚未实现。
- 日期：2026-11-11。
- 当前版本：qubit-local-files 0.5.1；下一破坏性版本：0.6.0。
- 本文扩展现有 `local_file_system_design.zh_CN.md`，未被明确替代的 authority、路径、发布、持久性和恢复契约继续有效。

## 0. 实施前置条件：先实现 rs-operation

**必须先实现并验收 rs-operation 0.1.0 的基础协议，再开始本设计的库实现。** 只有设计文档或空 crate 不满足此前置条件；本轮仅修订设计，不修改实现或 Cargo 清单。

上游入口为 [rs-operation 设计](../../rs-operation/doc/design-0.1.0.zh_CN.md)。开始实施前，必须固定其已验收版本/不可变提交并验证：同步/异步观察同一操作、取消树与单调期限、子操作 reserve/attach/commit、ResourceUse、OwnedCleanup、恢复所有权、结果与释放分离、Drop 托管及 shutdown 超期继续观察。若上游缺少必需能力，应先在 rs-operation 补齐契约和测试，不在下游复制一套替代机制。

### 0.1 统一职责与接口规则

- 本库直接依赖 `qubit-operation = "^0.1"`；同步托管接口也使用它，不把通用协议仅放在 async feature 后。具体领域异步 API 仍可由本库 async feature 控制；导入 crate 或构造纯配置不启动服务线程。
- 应用创建并注入共享 `OperationSupervisor`；本库创建受管理子 scope，不因领域服务各异而再创建私有通用 supervisor。应用关闭共享服务，本库只关闭自己受理范围内的子操作；显式独占的便利构造必须说明其服务归属。
- 同步/异步共用 `OperationContext`、`OperationHandle<T,E>`、`OperationObserver`、`ResourceUse` 和 `SettlementReport`。领域包装仅增加强类型结果/领域方法，不创建另一套 token、deadline、detach、恢复注册表或调度循环。
- `result()/result_blocking()` 一次性取业务结果；`released()/released_blocking()` 观察稳定终结。等待 future Drop 不取消仍持有的 handle，handle Drop 请求停止，detach 返回公共 observer；Unconfirmed 只能观察或超期返回快照，不能视为 Released。
- 请求提交必须先预留公共登记槽和领域容量，attach 领域 control 后 commit 才能产生外部副作用。停止信号和启动竞争使用公共线性化协议，未启动受理失败归还 owned 输入。
- 领域 worker、进程 reaper、原生 Session 和领域字节预算仍由本库管理；通用 supervisor 不执行阻塞 I/O 或推理。子任务从父 scope 切分配额，不重复争抢 root 槽，也不将同一资源登记两次。
- `OperationContext` 只携带共有上下文；领域 options 保留权限、版本、原子性、输出或采样策略。总期限向下收紧，不重置；清理不复用已取消的业务 token。生命周期停止请求不撤销已发生副作用。
- 不增加进度接口、progress feature 或事件总线依赖。业务自行使用 rs-progress / rs-event-bus；本库状态与诊断仅用于正确终结，不自动上报业务阶段。

### 0.2 共用验收与发布门禁

先通过 rs-operation 核心测试，再用本库假执行器验证：取消先于 commit 不启动、业务结果早于资源释放、队列满仍能停止/清理、Unconfirmed 持有资源、恢复 owner 不丢、借用对象不逃逸、外部 driver 丢失不伪造释放、公共 handle 的同步/异步对等。以下领域专属验收在此基础上叠加，不能用领域测试替代上游前置验收。

## 1. 目标与边界

文件操作的核心是对特定对象进行读取或改变，并能说明改变是否已发生。同步或异步是调用方式，不能改变权限边界、原子性或错误信息。需要支持 CLI、服务请求、批处理、异步 provider 和外部程序的数据交接。

下一版提供可选的异步 facade、完整操作与有状态句柄、协作取消、受管理清理、有限执行容量和显式句柄租约。仍然只操作本地文件系统；不提供网络存储、业务事务、跨文件 ACID 或恶意外部进程的沙箱。

实现采用现有同步核心加有界 worker 服务，不宣称普通文件操作已成为内核原生异步。无需让每个 getter、路径工具或纯内存配置方法拥有 async 副本。

## 2. 当前实现依据与必须保留的能力

- `LocalFileSystem::host/rooted` 区分宿主命名空间和保留目录 authority；显示路径不是 authority。
- `LocalFileReader`、`LocalFileWriter` 已具有自己的流契约；使用 qubit-io，不假定实现 std::io::Read/Write。
- `LocalTempFile` 和 `LocalTempDirectory` 有显式 cleanup、keep 和 persist；Drop 为尽力清理。
- `LocalSealedTempFile` 保留只读句柄，Host 才能暴露 native_path；seal 不承诺文件对外部别名不可变。
- copy/delete 已有部分预算和期限，writer/copy/rename 有结构化部分结果；不得为了统一错误而丢失这些内容。
- 同步实例有可变默认选项和目录语义。异步排队不能让未来的配置修改改变已提交操作。
- rs-fs 已有异步 facade，rs-fs-local 目前为同步适配器。此设计需要为后续异步适配提供足够机制。

## 3. 模块、feature 与依赖

新增 async feature，默认关闭，控制异步 facade/流资源；qubit-operation 为同步/异步托管接口的共同依赖。现有 digest/test-support feature 保持，标准 Future 与 qubit-io 异步流不绑定特定应用运行时。

| 类型 | 职责 |
| --- | --- |
| `LocalIoExecutor` | 有界文件 I/O worker、领域字节/句柄容量、清理执行通道 |
| `AsyncLocalFileSystem` | authority/config 快照、本地策略与注入 scope 的 facade |
| `LocalOperation<T,E>` | 公共 OperationHandle<T,E> 的领域别名，E 默认 LocalFileError |
| `LocalIoControl` | ManagedOperationControl 适配；确认实际 I/O 和原生句柄不再使用资源 |
| `LocalOperationOptions` | 领域预算收紧；共有取消/Deadline 通过 OperationContext 传递 |
| `AsyncLocalFileReader/Writer` | 由公共资源登记支持的独占异步流 handle |
| `AsyncLocalTempFile/Directory` | 已登记 owner 的访问 handle，清理由 OwnedCleanup 管理 |
| `LocalReadLease` | 本地只读导出能力及公共 ResourceUse 的领域包装 |

LocalIoExecutor 不建立私有通用 supervisor/恢复注册表。领域 owner 实现 OwnedCleanup；恢复 owner 存入同一个公共 scope，类型化恢复访问继续由本库包装。rs-fs 不能成为本库的依赖，两者通用语义通过文档和 rs-fs-local 适配对齐。

## 4. API 结构与覆盖面

以下为目标签名摘要，省略 getter 和各专用错误，不是当前 API 示例。

```rust,ignore
type LocalOperation<T, E = LocalFileError> = OperationHandle<T, E>;
impl LocalIoExecutor {
    fn new(supervisor: OperationSupervisor, config: LocalExecutionConfig) -> Self;
}
impl LocalFileSystem {
    fn into_async(self, executor: LocalIoExecutor) -> LocalResult<AsyncLocalFileSystem>;
    fn submit_read_all_in(&self, scope: &OperationScope, executor: &LocalIoExecutor,
        path: PathBuf, read: LocalReadAllOptions, budget: LocalOperationOptions)
        -> Result<LocalOperation<Vec<u8>>, LocalReadSubmitFailure>;
}
impl AsyncLocalFileSystem {
    async fn read_all_in(&self, scope: &OperationScope, path: &Path,
        read: &LocalReadAllOptions, budget: LocalOperationOptions) -> LocalResult<Vec<u8>>;
    async fn open_reader_in(&self, scope: &OperationScope, path: &Path)
        -> LocalResult<AsyncLocalFileReader>;
    async fn create_temp_file_in(&self, scope: &OperationScope, options: &LocalTempFileOptions)
        -> LocalResult<AsyncLocalTempFile>;
}
```

同步托管入口和异步入口使用相同 scope；普通同步入口保持直接执行，不强制创建服务。owned submit 请求先 reserve_child 和领域 worker 容量，attach LocalIoControl 后 commit；创建持久资源还需先预留 register_resource 槽位。执行线程不借用请求栈；需要 owned copy 时先计量预算。返回流/temp 时将资源 owner 留在 scope 注册表，T 中返回访问 handle，避免根操作结果被丢弃时直接析构原生 owner。

完整操作在一个领域 worker 作业内完成，不对每次系统调用重新创建公共 root。0.6 必须覆盖 metadata、read_prefix、有限 read_all/read_text、一致性读取、copy/rename/create/delete、reader/writer、批次遍历、writer commit/abort、temp cleanup/persist/keep/seal，以及 staged-tree 创建/验证/发布/恢复；digest 启用时包括条件写入。缺失异步能力必须 Unsupported，不内联阻塞。

原子性、持久性、权限、条件写入和恢复仍是本地领域类型；LocalCopyFailure/commit/persist 专用错误作为公共 handle 的 E 保留，不统一压成字符串。一般便利方法只在必需收尾确认后映射旧 Result；高级 handle 可分别获取 result 和 released，提前取得访问 handle 不代表其资源 owner 已释放。

临时资源访问 handle Drop 关闭自己的使用租约；scope 仍持有 owner，只有不再使用时才调度清理。跨操作保留必须显式 transfer/promote，不能依赖隐式泄漏 root scope。

## 5. 配置、路径与 authority

`into_async` 消费一个同步配置快照；Rooted 共享不可变的目录 authority，其他默认选项复制。异步 facade 无改变进程 cwd 的 setter，提供显式配置 builder 派生新实例，不改变已提交任务。

Host 相对路径基准在构造 facade 时固定为捕获的绝对目录；Rooted 保留实例虚拟目录。所有多路径操作使用同一配置快照。Host 绝对化只是词法解释，不提前承诺路径所指对象已绑定；真正打开仍在 worker 完成，错误需区分绑定前后。构造本身可能访问文件系统，提供异步 `host/rooted` 构造入口；不能在异步请求路径先同步打开 Rooted 再声称构造无阻塞。

禁止在 worker 中依据排队后变化的进程 cwd 解释同一个请求。不同于 0.5 Host 需要时读取 cwd，这是本版明确的异步接口语义；同步接口维持原行为。Rooted 不暴露 native_path、不转换为 Host 重开；权限不足或平台能力不支持时返回结构化 Unsupported/权限错误。

## 6. 操作状态与取消

```text
排队 → 绑定/打开 → 读取或修改 → 发布决策 → 回收 → 完成
```

公共 `OperationHandle::result(&mut self)` 是可重新等待的观察，丢弃其 future 不取消作业；operation Drop 请求取消并由公共 supervisor 接管。便利 async 方法 future Drop 请求取消。取消不会强制杀线程或在进行中的 OS 写入中回滚字节。

| 时点 | 取消后的保证 |
| --- | --- |
| 排队尚未执行 | 不打开文件、不创建目标、不发布 |
| 读取中 | 停止后续块读取，丢弃或归还有限缓冲；源无库主动修改 |
| staged 写入但尚未发布 | 停止后续写入，abort/清理；失败保留 recovery |
| append/in-place 写入 | 已修改内容不回滚，返回修改范围/计数与状态 |
| rename/原子替换正在提交 | 等待该原语返回，按实际发布结果报告 |
| 已发布后收到取消 | 返回实际已发布结果，取消请求作为伴随事实 |
| 删除中 | 返回已删除数量及未完成状态，不能报告整体无副作用 |

writer commit 的“最后可取消点”在发布原语前；越过后必须取得结果。若发布成功但目录持久化失败，仍报告已发布且持久性未达标，不改写为未发布。不能使用 `Cancelled` 擦除已发布状态。

取消完成报告包括阶段、取消是否请求、修改/发布状态、计数、清理状态及恢复 owner。未知事实显式 Unknown；保留现有 typed error 和 source。未观察结果中的 recovery 由 rs-operation 的 scope 恢复注册表接管，诊断流只保存摘要，不能因错误被丢弃而悄悄删除需要保留的恢复文件。

## 7. 有状态异步流与取消安全

同一 reader/writer 使用独占可变访问，后台串行拥有原生句柄；不为每个读块创建线程。流采用 qubit-io 的 AsyncInput/AsyncOutput、异步 seek 契约，复用现有适配器；不重新发明 Tokio 专用 trait。

- worker 不能持有调用方借用的 `&mut [u8]`。读入内部有限 owned buffer，完成后由 poll 拷入调用方；写入在受理时复制到有限内部缓冲，按已接受字节返回。
- 一次 poll 返回 Pending 后，丢弃读 future 不能丢失已经从文件读取的字节；未交付数据留在句柄缓冲。新读先交付这些数据。
- 写入成功返回表示字节已被句柄接受，不代表已持久化。flush/commit 等待必要写入完成。不能在 pending 后偷偷重复提交同一缓冲。
- 丢弃 seek future 后句柄仍记录进行中的 seek；后续操作等待结果，不与它竞争。提供 `settle` 显式等待并检查状态；错误后按状态允许恢复或关闭。
- 优先支持 `read_at` 这类不改变共享偏移的访问；平台不具备相应原语时由独占 worker 串行实现，不声称并发随机访问原子性。
- walker 每次获取有限批次，默认 128 条且附加 256 KiB 名称/记录字节预算；单条超出预算返回资源限制错误而非无限扩容。Drop 停止后续遍历，后台关闭目录句柄。

默认每个流内部读/写缓冲上限各 64 KiB；调用方大请求分块。大文件不默认 read_all；完整读取必须应用 LocalReadAllOptions 的有限字节预算。

## 8. 容量、期限、清理和关闭

领域默认 4 个 I/O worker、64 个排队作业、64 个受理等待者、16 MiB owned payload；与公共 scope 登记配额分别检查，取更严格上限。普通 reader/writer 内部读写延续同一资源子 scope，不逐块占 root。字节、原生句柄和磁盘预留由本库计量，公共框架只负责租约生存期。

每个 public scope 的资源登记采用 rs-operation 默认 128 项；本地创建前 reserve 资源槽，创建后把 owner 原子登记，失败返回 partial owner/recovery。worker 清理通道使用预登记许可，不与普通任务争夺队列空位；scope 的清理并发也受公共限制。公共取消信号即使业务队列满仍能送达。

期限从首次提交尝试开始，已有 copy/delete 时限与 context deadline 取更早者，内部重试不重置。不可中断 OS 操作返回前保持 ResourceUse 和领域许可。嵌套同步本地核心直接在当前领域 worker 内执行，不再等待自己的 worker 队列；通用 supervisor 不执行这些阻塞代码。

显式 cleanup 交由 OwnedCleanup，在独立清理 context 下执行，不能因为业务取消立即跳过。Async temp/reader/writer Drop 只归还访问租约/请求关闭，由公共服务接管 owner，不在异步调用线程触发同步递归析构。失败携带 typed 恢复 owner 转入同一个公共恢复注册表，不能再维护 LocalIoExecutor 私有无限 recovery 队列。

关闭先停止本地服务受理，关闭其资源新使用，按公共 scope 协议 Drain/Cancel 并等待所有 LocalIoControl 和 cleanup；然后排空本地 worker。注入的 supervisor 由应用最后关闭。超期只返回 observer/Unconfirmed，不释放仍在使用的目录 authority；恢复注册项占配额直到 retry/keep/take_recovery 明确处理。

## 9. 数据暂存与跨库交接

提供完整 `stage_bytes` / `stage_stream` 操作：配置私密性、最大内容字节数、临时空间预留、写入完成后关闭写端或 seal，返回 cleanup-owned 临时资源。seal 的 fsync 要求沿用现有选项；短期只读交接可显式选择不要求同步，不默认把“暂存完成”升级成持久性承诺。

`LocalReadLease` 持有只读文件和公共 ResourceUse；导出为标准文件句柄交给 rs-command 时，调用方必须同时保留 lease 至命令进程与全部输入 helpers 真正释放。导出返回 `(std::fs::File, LocalReadLease)`，租约绑定所导出句柄的使用范围；它是显式协议而非类型系统对外部工具的强制保证，文档禁止只留下句柄而提前丢弃 lease。rs-mime 的 scope 必须将两者一起登记并保留至命令输入 helpers 的 ResourceReleased。禁止提供隐式原生句柄逃生口或声称能够追踪调用方任意 clone 的 File。底层文件权限、偏移和是否可 seek 清楚可查。

清理 owner 请求 cleanup 后，必须等活动 lease 归还才删除资源；该等待是异步的，调用方不能持有自己的 lease 再等待 cleanup。提供 Busy 查询和示例帮助诊断。Rooted 默认拒绝导出原生句柄，除非应用构造时显式开启 ExportReadHandle 能力；即使允许，也不导出宿主路径。这是权限能力的显式转交，不是 rooted 沙箱保证。

按路径工作的外部工具必须使用明确的 Host 临时区或应用授权路径。标准输入能够满足的场景优先使用句柄；需要随机访问/路径的工具仍走暂存文件，不能用一个 stdin 优化替代所有路径语义。

## 10. 与 rs-fs 0.4 和下游的对齐

rs-fs 0.4 的通用语义应尽可能覆盖本库：有限完整读取/前缀/随机访问、一致性事实、条件写入、writer 提交与放弃、copy/rename 部分结果、有限遍历、temp cleanup/persist/seal 和目录树暂存发布。两库使用相同公共 scope/control/recovery 协议，但不共享本地原生类型。详见 [rs-fs 0.4 设计](../../rs-fs/doc/design-0.4.0.zh_CN.md) 的能力矩阵。

本地 authority、非 UTF-8 原生路径、ACL/Unix mode、安全观测、原生句柄导出仍是本库职责；rs-fs 用标准化能力/requirement 和类型化本地扩展，不把 POSIX 或 Host 路径强塞给对象存储。不具备原子性/持久性/条件写入的平台必须明确 Unsupported 或 Conditional，不能映射为 Guaranteed。

rs-fs-local 需在后续适配中把 FsOperationContext 与 LocalIoControl 绑定到同一公共子操作，嵌套 adapter 只透传，不双重登记或重复计量；fs 逻辑资源 wrapper 可以引用同一 ResourceId。其独立版本和发布不在本轮擅自确定，未完成适配前不可宣称本地异步 provider 可用。

rs-command 0.12 接收标准文件与 ResourceUse，不直接依赖本库。rs-mime 0.22 通过公共 scope 组合暂存/命令/清理，不另建 MIME reaper。rs-magika 0.19 的 provider 输入通过 rs-fs 0.4 的同对象随机访问会话，直接本地路径异步打开也由本库执行器负责。

关联文档：[rs-command](../../rs-command/doc/design-0.12.0.zh_CN.md)、[rs-mime](../../rs-mime/doc/design-0.22.0.zh_CN.md)、[rs-magika](../../rs-magika/doc/design-0.19.0.zh_CN.md)。

## 11. 迁移与实施顺序

先实现并验收 rs-operation，再保持同步对象和语义，通过 feature 引入异步领域服务。优先落实操作状态、容量/关闭和完整读写，再实现流/临时对象、租约和高级发布。不能先发布只有 async 方法而没有取消与恢复契约的中间稳定 API。

调用方迁移：显式构造共享 executor；从配置快照创建 async facade；将外部 blocking 包装替换为本库操作；所有临时资源显式 cleanup；应用退出显式 shutdown。0.5 错误匹配和资源恢复代码应逐项对照新增取消/容量状态；不得把 Unsupported 当作可忽略成功。

## 12. 验收标准

必须验证 sync-only 导入无隐式服务线程且不依赖特定异步运行时、不同运行时可 poll、相同同步异步结果和 authority、配置/cwd 排队竞争、非 UTF-8 路径、Rooted 逃逸与符号链接替换、有限大读取、队列/字节容量、池内嵌套无死锁、读写 future 丢弃后无字节重复或丢失、seek 取消后顺序、发布前后取消、fsync 失败仍保留发布事实、copy/delete 部分结果、临时创建失败 recovery、cleanup 队列满、lease 与 cleanup 竞争、清理失败未观察结果、关闭超期和恢复取回。

跨平台沿用现有 Linux/Windows/FreeBSD 等支持矩阵，Unsupported 行为也要测试，不将同步已有平台限制隐瞒为异步成功。测试用闸门、故障注入和可控时钟；报告线程峰值、内存峰值、排队时间和批量操作吞吐，与同步基线比较。文档评审不等于实现验收。
