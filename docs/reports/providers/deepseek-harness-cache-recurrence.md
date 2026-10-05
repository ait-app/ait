# DSH 旧回复重复累积：缓存重复加载修复

交付日期：2026-10-05；本地验证执行于 2026-10-04。基于 PR #169 的 `4a706e5f`。
本次只修改共享客户端缓存恢复、缓存版本和 UI CI，不修改 Rust 或 provider 执行协议。
[验证附件](deepseek-harness-cache-recurrence-evidence.json)记录精确源码与日志 SHA-256。

## 外部复测与根因

外部测试者在固定 `4a706e5f` 上确认内部上下文过滤通过，真人消息保留；
空参数/非法 JSON、问答、审批、取消和重启续聊通过。但重复回复仍会复发：
升级首次清理成功，随后刷新与完整重启累积到至少八份尾回复，daemon 对应记录只有一条，
没有模型请求。因此前一轮修复没有完成重复问题的桌面验收。

本次用生产 replica、session store 和 SQLite 缓存复现了第二条路径：

1. 页面路由和可见聊天可以先后调用同一个 replica 的 `prepare`。
2. 第二次加载将已经显示的缓存历史全部放进 `previousHead`，当作实时输出。
3. 当较早回答与最后回答内容相同，连续性合并会把较早的回答再放到尾部。
4. 服务器没有新增消息时，空 `after` 响应保留这些显示数据并标记同步完成。
   错误副本被保存回客户端缓存，下一次完整重开继续累积。

旧 loader 的独立诊断从两条身份不同、文本相同的真实回答开始，四次 SQLite 关闭重开后
回答总数依次为 **3、5、9、17**。第三次是较早的一条回答加八份尾部显示，
与外部截图的增长现象一致。诊断暂时以计数替换断言，仅用于观察增长，不计为通过的验证；
最终提交的回归在每次刷新后断言完整记录与原始历史完全一致。

## 修复

`prepareCachedTimeline` 按 epoch/seq 区分已有缓存历史与新到达的内容：
同 epoch、位于缓存结束游标以内的已显示记录作为 `previousTail`，不再当成实时回答合并。
读取过程中到达的实际 live head，以及缓存游标之后的已显示内容继续保留。
不按文本去重，两条内容相同但身份不同的真实回答仍然是两条。

展示缓存投影版本从 1 升至 2，让 `4a706e5f` 已保存的错误副本重新从 daemon 读取。
只失效可重建的 timeline 展示缓存，不修改目录缓存、daemon 数据库、DSH 原生历史或模型调用。
升级后需要连接 Host 重新取得历史。

UI CI 增加 `src/timeline/replica.test.ts` 和 `src/runtime/replica-cache`，让这条恢复路径受持续检查。

## 验证

- 新回归在 `4a706e5f` 的原始 loader 上失败：两条合法回答被显示为三条。
- 修复后 replica/cache **66 passed**。新回归使用磁盘 SQLite，四轮关闭重开，每轮两次
  顺序准备、两次空历史追赶，验证同步完成后的 UI 与持久化缓存都和原历史一致。
- 原有缓存读取竞争、真实 live head、晚到历史、无游标缓存等回归继续通过。
  缓存版本回归覆盖无版本和版本 1 的失效，并确认目录缓存保留。
- 扩展 timeline/stream/cache：**305 passed、3 failed**。三项仍是已在旧基线复现的
  plugin identity 测试，名称与证据见[前轮报告](deepseek-harness-display-recovery.md)。
  本轮没有修改插件身份实现，也未删掉或跳过这些失败测试。
- 更新后的 UI CI 本地命令：**186 passed、1 skipped**。跳过项为要求
  `AIT_TEST_RUST_SERVER` 的既有 daemon 集成测试，非本次新增测试。
- 移动/Web 全量类型检查、变更文件 oxfmt/oxlint、文档链接与 release 版本检查通过。

```sh
npm run test --workspace=@ait/mobile -- src/timeline/replica.test.ts src/runtime/replica-cache --project unit
npm run test --workspace=@ait/mobile -- src/timeline src/types/stream.test.ts src/runtime/replica-cache --project unit
npm run test --workspace=@ait/mobile -- src/i18n/resources.test.ts src/runtime/rust-daemon maestro/support native-release-version.test.ts src/timeline/replica.test.ts src/runtime/replica-cache
npm run typecheck --workspace=@ait/mobile
```

## Test coverage

前端行覆盖率 **not measured**：本次使用既有 Vitest 执行配置，没有启用前端覆盖率收集器；
上述数字是测试执行数量，不能视为行覆盖率。后续如需百分比，须单独配置并测量前端覆盖率。
Rust 覆盖率 **not applicable — no Rust behavior changed**；按 AGENTS.md 不重跑 Rust 测试或覆盖率。
此前 `4a706e5f` 的 Rust 94.37% 是历史测量，不能作为本轮前端覆盖率。

[可审阅验证附件](deepseek-harness-cache-recurrence-evidence.json)包含源码指纹、失败重现、
旧代码增长计数和测试摘要。平台为 macOS arm64。本次未执行修复版真实 Electron/模型、
Debian amd64、Windows 或原生移动端验收；Node/SQLite 集成不等于完整桌面测试。

## 桌面复测交接

使用本次固定提交同时构建桌面与 daemon，记录两者 SHA 和 DSH 版本。

1. 保留 `4a706e5f` 的坏展示缓存升级，确认第一次连接清除旧副本，用户和工具历史完整。
2. 使用原 q169 会话：它包含较早和末尾两条文本相同的回答。不要发送新 prompt，
   连续刷新、断线重连、切换工作区，并完整重启至少四次。
3. 每次等待连接和加载结束再核对尾回复数量、消息顺序和原生模型请求计数；不能仅验升级首屏。
4. 再造两条同文但不同身份的回答，确认没有被文本去重；在缓存恢复期间继续流式输出，确认不丢失。
5. 保留已通过的上下文过滤、异常参数、问答、审批、取消与重启续聊回归；抽测其他 provider 的缓存恢复。

修复版真实桌面复测仍待完成，PR 暂不合并。
