# FreeBSD 兼容性状态

## 当前状态

`qubit-local-files` 暂时不支持 FreeBSD。当前 CI 证据来自 GitHub Actions 中的 FreeBSD 14.3，[run 41173303438](https://github.com/qubit-ltd/rs-local-files/actions/runs/41173303438)，revision `f72a5b26b0b598adc2d4f0d7edcbc0d019537892`（2026-10-31）。

“暂不支持”表示项目目前不承诺下列 API 在 FreeBSD 上可用；不表示所有 API 都已禁用，或每次调用都会返回 `Unsupported`。本次运行中，读取、部分 metadata 查询、清理以及若干特定功能测试成功过。单次调用成功或编译检查通过，都不能证明完整操作契约受到支持。

CI 没有记录 runner 后端文件系统类型或挂载参数。因此，`Operation not supported` 等错误只能归因于本次 runner 观察到的原生操作失败，不能进一步断言是某一种 FreeBSD 文件系统导致。

## 原生测试未通过的 API 范围

| API 范围 | 观察到的失败用例 | 本次运行显示的问题 |
| --- | --- | --- |
| `LocalFileSystem::copy` 和递归复制 | `test_host_facade_workflow_covers_copy_and_temporary_resource_paths`、`test_local_file_system_copy_unifies_file_and_directory_copy`、`test_directory_copy_statistics_match_publication_actions`、`test_scoped_copy_follows_in_scope_directory_link` | 暂存复制在安装目标时收到 `Unsupported`：FreeBSD 尚未实现 Host no-replace 移动路径。该组另有测试在应用目标 metadata 时失败。 |
| `LocalFileSystem::rename` 和 Rooted rename | `test_root_authority_manages_descendant_entries`、`test_rooted_local_file_system_rename_respects_overwrite`、`test_local_file_system_rename_respects_overwrite_policy` | 当前 FreeBSD 实现明确不支持 Rooted no-replace rename。Host no-replace 文件移动也进入 unsupported 分支。另有测试预期能力限制返回 `RequirementNotMet`，实际观察到 `AlreadyExists`；这一错误分类需单独复核。 |
| 原子 writer 提交和替换 | `test_atomic_commit_state_publishes_host_staging`、`test_atomic_commit_state_publishes_rooted_staging`、`test_following_write_modes_keep_existing_contract`、`test_rooted_local_file_system_writer_follows_final_symlink` | 提交到达 `ApplyDestinationMetadata` 阶段，原生文件系统返回 errno 45（`Operation not supported`），结果保持 `NotPublished`。runner 未记录挂载信息，无法确定具体失败的是哪种 metadata 操作或底层能力。 |
| 临时文件/目录 `persist` 与 `keep` | `test_local_temp_file_persist_reports_atomic_rename`、`test_rooted_temp_file_persist_publishes_absent_target`、`test_rooted_temp_directory_persist_supports_new_and_overwrite_targets`、`test_creation_parent_is_a_snapshot_after_close_and_keep` | no-replace 安装返回 `Unsupported`；这些测试无法证明 FreeBSD 上的发布与保留契约。 |
| 暂存目录 `publish_new` | `successful_publish_keeps_stage_contents_at_target`、`lf_review_tree_publish_depth_limit_retains_stage`、`test_published_outcome_transfers_secondary_cleanup_recovery_without_retracting_publication` | 发布依赖受支持的 no-replace 命名空间操作。当前 Rooted 目录 rename 路径会在发布前返回 `Unsupported` 或 `RequirementNotMet`。 |
| 复制/写入时保留 metadata | `test_successful_host_copy_reports_applied_permissions`、`test_metadata_policy_selects_permissions`、`rooted_copy_metadata_failure_matrix_preserves_sources_and_proven_destination_state` | 应用目标 metadata 时收到 errno 45（`Operation not supported`）。这是 runner 上的观测结果；具体文件系统及不支持的 metadata 原语没有记录。 |
| 当前工作目录已删除时的错误上下文 | `test_deleted_process_cwd_preserves_constructor_and_pair_operation_context` | 测试预期 `NotFound`，实际在解析已删除的进程工作目录时观察到 `Unsupported`。这是平台错误映射差异，不能推导为所有路径解析都不可用。 |
| 文件描述符耗尽后的临时资源清理 | `test_temp_creation_descriptor_budget_child` | 恢复清理返回 `DirectoryNotEmpty`；sandbox 中存在一个身份尚未得到确认的条目，安全清理逻辑因此没有删除它。当前证据还不能判断这是 FreeBSD 实现问题还是 runner 特定行为。 |

上表列出的是完整运行中有代表性的失败用例，不是所有失败测试的穷尽清单，也不是对 FreeBSD 行为的完整保证。CI 还报告了 workflow、复制、重命名、临时资源和树发布套件中的其他失败；它们会触发同一批尚未支持的底层操作。

## all-features 下失败的 8 个 doctest

FreeBSD all-features doctest 结果为 145 通过、8 失败、5 忽略。失败项如下：

1. `src/documentation_tests.rs` 第 936 行：英文用户指南中的暂存备份示例；Required durability 发布返回 `RequirementNotMet`。
2. `src/documentation_tests.rs` 第 2557 行：对应的中文用户指南示例；同样无法满足 rename 保证。
3. `LocalTreePublishOptions`：普通暂存目录发布不受支持。
4. `LocalCopyOutcome`：Host 复制无法执行原生 no-replace 目标安装。
5. `LocalCopyStats`：同上，Host 复制安装失败。
6. `LocalPersistCleanupState`：临时文件持久化无法执行 no-replace 安装。
7. `LocalRenameOutcome`：no-replace rename 不受支持。
8. `LocalTreePublishOutcome`：暂存目录发布无法满足请求的 rename 保证。

这些示例现已为 FreeBSD 明确处理对应的平台限制；受支持平台仍会执行成功路径。

## 已通过的操作

同一次 FreeBSD CI 运行成功执行了 FreeBSD xattr 短读/原子保留测试，以及针对当前不支持的 `Content` 条件写入协议的 `Unsupported` 合同测试。聚焦的 `test_attribute_read` 检查也通过。这些结果只覆盖列出的合同，不能说明上表中的失败操作族已受支持。

## 为什么不模拟 no-replace rename

当前 Rooted 实现没有 FreeBSD 原子、相对于已打开目录描述符执行的 no-replace rename 原语。先检查目标是否存在、再执行普通 rename 会产生竞态：检查之后，其他进程可能创建目标，而普通 rename 随后可能替换它。因此项目选择拒绝该操作，不削弱 no-replace 保证。FreeBSD 的 [`renameat(2)` 手册](https://man.freebsd.org/cgi/man.cgi?manpath=FreeBSD+14.1-RELEASE&query=renameat)说明了目标已存在时普通 rename 的行为。

部分文件安装和 xattr 辅助流程已有 FreeBSD 专用代码，但它们没有覆盖所有 Host 与 Rooted API 路径。必须按操作族及原生文件系统能力分别建立兼容性证据。

## 恢复支持的条件

重新评估 FreeBSD 支持前，需要验证受影响 Host 和 Rooted 操作的安全 no-replace 原语；记录 metadata 失败对应的 CI 文件系统及挂载细节；查明文件描述符耗尽后的清理问题；并在明确声明的 FreeBSD 文件系统上通过相关原生测试与 doctest。在此之前，FreeBSD 不属于受支持平台范围。
