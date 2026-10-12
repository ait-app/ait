# Workspace 定时任务列表与统一创建流程：验证

本次改动基于 `main` 的 `4ab7e7be`，是已合并 Workspace 定时任务功能的界面修订。
列表与次数的首轮验证对应 `989bde4a`，单一执行 Agent 选择器对应 `34370061`；
全局 Host → Project → Workspace 创建流程及最后一轮验证对应本报告所在提交。
没有修改 Rust、调度协议或持久化数据结构。
设计见 [ADR-138](../../decisions/workspace/adr-138-workspace-owned-schedules.md)，使用方式见
[定时任务](../../operations/schedules.md)。

## 行为

- Workspace 新建菜单中的定时任务入口默认打开列表，按服务器返回的下一次触发时间升序排列，
  分为今天、明天与具体日期；已到期但尚未执行的任务仍显示在待触发分组。
- 点击任务行直接进入编辑；新建、编辑保存或取消后返回列表。运行历史入口保留手动运行、
  暂停、恢复、删除和 Agent 跳转。暂停与已完成任务可独立筛选，筛选及编辑视图随布局恢复。
- 表单只提供固定间隔和 Cron 周期。新任务的运行次数预填 `1`，更大的值表示重复执行，
  清空表示不限次数；已有不限次数任务不会因为打开编辑而变成一次。
- 单个执行 Agent 选择器同时提供每次新建入口和当前 Workspace 的未归档 Agent，支持搜索。
  选择现存 Agent 直接绑定并收起创建配置，切回新建会保留之前的模型选择及提示词。
- 全局 Schedules 创建按 Host、Project、Workspace 逐级选择，再使用相同任务表单。
  更换上级清空下级及任务草稿；提交时禁用位置选择。筛选依据实际主机、项目身份，
  创建配置使用选中 Workspace 的真实目录，保留 Worktree 归属。
- 旧 `Once` 数据仍可显示、运行与编辑；仅修改名称或提示词时保留原触发规则。
  修改时间、周期或次数时转为周期任务，未修改的原始触发时间保持精度。
- 列表只展示当前 Workspace 的任务，使用服务器的真实下一次触发时间，未在客户端推演
  Cron 或补造后续事件。

## 定向验证

验证平台为本机 macOS，浏览器为 Playwright Chromium。

| 检查                                                                            | 结果                                |
| ------------------------------------------------------------------------------- | ----------------------------------- |
| 列表与次数首轮：模型、表单、列表、布局恢复与标签页身份的 Vitest 测试            | 12 个文件、229 项通过（`989bde4a`） |
| 选择器与全局创建后续：表单模型、模型 Hook、创建编辑表单与任务详情的 Vitest 测试 | 4 个文件、33 项通过                 |
| 定时任务创建、编辑与模型恢复的浏览器 E2E                                        | 8 项通过                            |
| 移动端 TypeScript 检查                                                          | 通过                                |
| 修改文件的 ESLint、UI lint、格式与文档链接检查                                  | 通过                                |

首轮测试命令：

```sh
npm exec --workspace=@ait/mobile -- vitest run src/schedules src/stores/workspace-layout-store.test.ts src/workspace-tabs/identity.test.ts
```

选择器与全局创建修改后的定向验证命令：

```sh
npm exec --workspace=@ait/mobile -- vitest run src/schedules/schedule-form-model.test.ts src/schedules/use-schedule-form-model.test.tsx src/schedules/workspace-schedule-form.test.tsx src/schedules/workspace-schedule-tab.test.tsx
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/schedules-project-target.spec.ts e2e/browser/schedules-edit-model-hydration.spec.ts
npm run typecheck --workspace=@ait/mobile
npm run check:ui
npm run check:docs
```

浏览器用例通过真实本地 daemon、SDK 与持久化路径验证当前 Workspace 的已有 Agent
及新建 Agent 目标，确认默认一次、保存后的列表导航、时间顺序、直接编辑与刷新恢复。
后续用例通过单一选择器搜索和选择已有 Agent，再切回新建并检查原模型与提示词保留，
编辑时检查已绑定的 Agent 正确回填；跨 Workspace、已归档及其他主机候选的排除另由定向表单测试检查。
桌面下拉菜单和已有 Agent 表单截图已检查，执行方式和具体 Agent 共用一个控件。
全局创建用例检查窄屏逐级选择与重新打开时的初始状态；另以两个真实隔离主机检查
Project、Workspace 候选隔离，以及切换上级后清空草稿。最终创建任务的 Host、Workspace、
新 Agent 模型和 Worktree 目录均由实际持久化查询核对；原主机没有出现该任务。
Provider 使用仓库的离线 Codex 原生协议夹具，没有调用真实模型服务。
桌面与 390px 窄屏列表截图已人工检查，无横向溢出，时间、任务名及新建入口可见。
本次未验证原生 iOS/Android 设备或新的安装包。

## Test coverage

未测量代码覆盖率；测试通过数量不代表行或分支覆盖率。
用户未请求覆盖率测量，本次只运行改动代码及直接相关行为的定向测试。
没有修改 `bins/` 或 `crates/` 中的 Rust 代码，按仓库要求跳过 Rust 测试。
浏览器夹具使用本地 daemon 构建产物并启动隔离实例，属于端到端运行验证，不属于 Rust 单元测试或全工作区测试。
