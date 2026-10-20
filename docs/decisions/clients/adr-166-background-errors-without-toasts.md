# ADR-166：后台错误只记录、不弹错误提示

- 状态：经 [ADR-170](adr-170-revert-error-diagnostics-and-recovery.md) 撤回，后由 [ADR-176](adr-176-deferred-client-diagnostics.md) 提出扩展恢复方案（Draft）
- 日期：2026-10-20
- 修订：ADR-161（原规定 console 错误与 Query 失败都弹出错误 Toast）

## 背景

[ADR-161](adr-161-copyable-error-diagnostics.md) 让 `console.error`、Query/Mutation 失败、
未捕获异常和 Provider `turn_failed` 都进入同一个错误记录，并且每一条都弹出错误 Toast。
0.1.9 发布后，后台错误同样会弹出 Toast，而且会连续弹出很多个：

- SDK 对每个失败请求都写一条 `Daemon request failed`。主机断线重连时，每个未完成的请求
  各弹一个 Toast。客户端未处理回复的请求（例如 #391 修复的 `agent.attention.clear`）
  每 60 秒就弹一个超时 Toast。
- 鼠标悬停在回复中的文件链接上时会预查询路径；找不到文件也会以 Query 失败的形式弹出 Toast。
- 非 Git 目录查询 PR 状态返回 `NOT_GIT_REPO`，这本来是预期结果，也会弹出 Toast。

这些都不是用户发起的操作，也不需要用户处理。主机连接状态另有显示。
用户点击操作失败时，调用方本来就会自己提示。

## 决策

1. `console.error` / `console.warn` 采集到的记录，以及 Query 失败，只进入错误记录（`notify: false`），
   不弹出 Toast。错误记录、去重、脱敏和上限不变。
2. 以下情况继续弹出错误 Toast：未捕获异常、未处理的 Promise 拒绝、原生异常、
   Mutation 失败、Provider `turn_failed`，以及调用方主动报告的错误（错误对话框、`toast.error`）。
   同一异常先被 console 记录、后被主动报告时，仍然会弹一次 Toast。
3. 设置中的应用诊断报告继续包含全部近期错误记录，所以后台错误仍然可以复制和导出。

## 后果

用户不再因为断线重连、后台超时、悬停预查询或预期的业务错误收到连串 Toast。
后台错误需要从设置的诊断报告中查看。需要提示用户的 Query 失败，由调用方在
`fetchQuery` 被拒绝后自己提示（例如文件链接点击后的“找不到文件”）。

## 验证

- `install-error-handlers.test.ts`：console 错误进入记录但不弹 Toast；同一异常被主动报告后仍然弹 Toast。
- `query-client-errors.test.ts`：Query 失败保留 queryKey 和原因链，但不弹 Toast；Mutation 失败仍然弹 Toast。
