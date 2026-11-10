# 任务总览

从侧栏「任务总览」或命令中心打开 `/tasks`。页面使用当前已连接主机的 Agent 目录，支持按主机、项目、标题、模型搜索，以及最近／最早活跃排序。

| 列     | 归类条件                                  |
| ------ | ----------------------------------------- |
| 待处理 | 等待权限／输入，或执行失败；等待输入优先  |
| 运行中 | Agent 正在初始化、执行，或仍有开放的 turn |
| 待查看 | 已完成并带有未读提醒                      |
| 已完成 | 无执行、权限或未读提醒                    |

同一主机上的托管子任务和已登记的只读原生子任务归入父任务，父任务显示整个任务树最需要关注的状态；归档任务不再显示。找不到父任务的活跃子任务独立展示。不同主机的相同 Agent ID 不会合并。尚未确认状态的原生子任务进入「待查看」，不会计为已完成。

点击卡片可查看最近回复、权限请求和子任务。宽屏使用右侧详情栏，窄屏使用详情弹层；手机端通过顶部标签切换状态列。

- 权限按钮保留 Provider 提供的选项 ID；计划确认和问答请求复用会话中的组件。
- 运行中的任务可以停止；待查看任务可以标记已查看。
- 没有运行或待输入子任务的任务可以归档；归档失败恢复原任务并提示错误。
- 「打开会话」进入该 Agent 所属 Workspace。
- Provider 原生子代理在选中父任务后按需读取。支持统一子代理的主机把它们放入同一任务树，可点击查看只读详情；旧主机在详情内显示兼容状态列表。审批、停止和归档遵守会话能力，只读子代理不能审批或停止，只有确认终态且主机允许时才能归档。
- 主机离线或目录读取失败时显示提示和最近状态。离线期间禁止审批、停止和归档。

看板只订阅共享目录；最近回复仅为选中的任务读取最多 40 条尾部记录，运行期间每 15 秒刷新，不会为所有任务建立历史订阅。预览不会清除未读状态。

实现位于共享 React Native 客户端，没有新增服务端 API、数据库、运行调度器或独立任务状态。Rust daemon 保持原有职责。

设计参考：[Better Paseo](https://paseo.sh/plugins/roundreddot/better-paseo) 的状态看板和 [Mission Control 展示](https://x.com/flohoeller/status/2120072353459716239) 的父子任务关系。[设计图](../assets/task-board/design.png)中的任务文案是示例，运行页面显示真实主机数据。

验证命令：

```sh
npm run typecheck --workspace=@ait/mobile
npm run test --workspace=@ait/mobile -- --project unit src/task-board/model.test.ts src/components/permission-request-card.test.tsx src/sidebar-nav/model.test.ts src/i18n/resources.test.ts src/agent-stream/question-response.test.ts
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/task-board.spec.ts
```

浏览器测试使用独立临时主机和仓库内的离线 Codex fixture，不调用已登录的真实模型 CLI。可通过 `E2E_AIT_SERVER_BIN` 指定现有 daemon 构建。
