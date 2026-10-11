# ADR-134：OpenCode 目录就绪与配置确认

- 状态：Accepted。
- 日期：2026-10-10。
- 修订：[ADR-115](adr-115-opencode-acp-provider.md) 的 2.x 最低版本与配置响应处理；Rust、官方 ACP、原生认证和历史所有权保持不变。

## 背景

上传诊断来自 Ait 0.0.26 / OpenCode 2.0.20。该版本的 ACP 在任意启用模型和 primary agent
出现后即缓存目录，未等待项目配置插件完成。隔离配置的真实 CLI 验证中，自定义模型缺失，
显式选择在创建会话阶段被拒绝，尚未发送 prompt。

上游 [2.0.20 实现](https://github.com/anomalyco/opencode/blob/v2.0.20/packages/cli/src/acp/service.ts)
缓存首次 `loadCatalog` 的结果。
[2.0.26 实现](https://github.com/anomalyco/opencode/blob/v2.0.26/packages/cli/src/acp/catalog.ts)
等待插件就绪，并按原生目录事件刷新。Ait 无法通过官方 ACP 刷新旧版本的内部缓存。

2.0.26 的 `session/set_config_option` 成功响应还可能暂时包含旧选项；随后通知才确认新值。
Ait 原先仅检查响应，将正常切换判为失败。目录查询也只遍历最初的模型列表，遗漏查询期间新增的模型。

进一步并发冷启动验证中，2.0.26 的多个 ACP standalone 进程同时初始化同一原生数据库，
可在握手前因 `SQLiteError: database is locked` 退出。四个隔离项目共享一个原生 home 的测试可复现。

## 决策

1. OpenCode 1.x 保留既有 ACP 和无 delete 时的只读 CLI 发现；2.x 要求 2.0.26 或更新版本。
   受影响的早期 2.x 在启动 ACP、创建会话或提交 prompt 前返回 unavailable，诊断说明安装版本与升级要求。
   Ait 不替换安装，不改写用户配置，不回退到私有 HTTP，不重发 prompt。
2. Provider unavailable 快照可复用适配器的安全 diagnostic，限制为非空、无 NUL、至多 4096 字节；
   缺失或无效诊断保留通用错误。目录失败和可用性检查异常继续按原有错误状态呈现。
3. 模型、effort 和 mode 的选择共用原生确认流程。返回配置已确认选值时立即继续；否则使用响应前
   的确认通知，或在同一控制期限内等待响应后的 `config_option_update`。
   只接受同一 session 的通知；确认缺失、协议错误或超时关闭连接，下一步不提交 prompt。
4. 目录查询遍历最新原生模型目录，保留模型专属 effort。发布最终目录中的模型、顺序、名称、描述和模式，
   纳入新增模型并移除已删除模型；累计检查至多 4096 个模型，查询临时会话仍由原生 delete 清理。

5. 同一个 2.x 适配器及其克隆在 ACP spawn 至 initialize 完成之间共用启动 gate，等待受既有控制期限约束。
   成功、失败及取消均释放 gate；活跃会话、模型发现和 prompt 不持有 gate，不串行化用户会话。
   这只协调 Ait 所有的子进程；不能保证与外部同时启动的 OpenCode 进程之间没有数据库竞争。

## 后果与验证

早期 2.x 不再显示成功握手后的残缺模型目录；用户需先升级原生 CLI，再刷新 Provider。
升级后的配置切换等待实际确认，不重复原生写操作。动态目录仍受既有发现期限和资源预算约束。

无网络 ACP 夹具覆盖新增/删除模型、升级诊断、确认通知的两种顺序、确认缺失和错误 session。
真实 2.0.26 使用隔离目录和 loopback 模型验证创建、多轮、发现、工具、审批、问答、历史恢复与辅助清理，
并覆盖四目录并发冷启动、64 个自定义模型、模型/effort/mode 切换、权限和 system 重连、图片与附件、取消及认证失败。
报告和测量范围见 [修复验证](../../reports/providers/opencode-catalog-readiness-2026-10-10.md)。
