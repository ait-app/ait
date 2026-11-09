# PR #612：原生操作完整路径与 Tai 真实任务复查

本轮复查从 `6731b30e` 开始，保留外部 OMP 边界。实现与截图跨 2026-11-09、2026-11-10；
最终源码和 CI 以 PR head 为准。没有合并 PR。

## 三项 P2 的处理

| 问题                           | 当前行为                                                                                                                                                    | 生产实现                                                                                              |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| 通用菜单另有预设写入路径       | GUI 隐藏 native preset save/delete；管理统一走齿轮中的同一编辑器和确认流程。原生接口保留。账号、运行配置也移至齿轮设置。                                    | `provider-native-action-model.ts`、`provider-native-actions.tsx`、`composer/agent-controls/index.tsx` |
| Vibe / Goal 切换动作发送旧参数 | 动作选择重建参数；只渲染、校验和提交当前动作的字段。任务、指导、目标为多行输入；提交按钮使用具体动作名。                                                    | `provider-native-action-model.ts`、`provider-native-field.tsx`                                        |
| GUI 要求手填对象 ID            | 历史按内容选择，分支只提供用户消息；账号按 Provider 和账号名称选择。worker 指导、停止放在共享列表条目，自动绑定父 writer 和目标；侧问答追问、取消留在话题。 | `provider-native-field.tsx`、`subagents/native-controls.tsx`、`provider-side-questions.tsx`           |

`…` 先按会话、历史、任务分组，再显示具体动作。历史变更与 Vibe 模式切换遵守主轮次和
模式状态的限制；Vibe 未协商就绪时不发送控制请求，关闭时先确认开启，开启后保留已写的任务草稿。
用户最终选择保持不变：没有活跃 worker 时，聊天输入区完全隐藏 Vibe 提示。

快捷历史入口采用对象选择器；没有为每个原生树节点新建一套消息气泡菜单。
原生读取回执和高级详情不等于完整的可视化历史树。

## 真实长会话暴露的额外缺陷

同一真实 Tai 会话的 `get_entries` 返回完整正文，原有显示层在 32,768 字符处截断 JSON，
导致新选择器无法读取。该故障通过真实 RPC 复现，不是模型模拟响应。

适配层现在只为该 GUI 回执生成有界条目摘要：保留 ID、角色、类型和可读标题，按编码字节预算
和最多 128 条取最近记录，并返回省略数量。完整消息仍在原生日志中，原生 RPC 和内部历史协调
读取不变。分支候选按官方“只能从用户消息分支”的规则过滤，界面不展示需要手填的 ID。
省略的旧条目会明确提示；该选择器不是无限历史浏览器。

新 daemon 读取同一份会话：4,292 字节、45 个条目、0 省略，JSON 完整可解析。
覆盖长消息、转义 ID、多字节标题及真正 Pending 回执出口的 4 个新探针随本轮代码提交。

## 真实任务与 API

使用用户配置的 Tai API `https://api.inferknock.ai/v1`，模型 `tai/deepseek-v4.1-flash`，
用户安装的官方 `/usr/bin/omp` 18.17.0。只有 OMP agent 目录和 daemon 数据目录被隔离；
没有安装、修改或打包私有 OMP/Bun。凭据没有输出、截图或提交。

场景是“三人值班交接卡”：离线中文单页，事项、负责人、待办/完成、添加、localStorage、
键盘和 390px 布局。通过实际 GUI 创建两个 worker：fast 写 REQUIREMENTS.md，good 写 index.html。
随后从共享条目指导原有 worker 对齐文档，没有创建第三个 worker。
Director 通过真实 `/vibe` 主回合调用 list/wait/read；侧问答另经 Tai API 返回 Markdown 建议。

| 会话                        | 原生 ID                                | 模型                    | thinking |
| --------------------------- | -------------------------------------- | ----------------------- | -------- |
| director                    | `01a1c0e0-e78f-77eb-b276-1583f4a2c4e3` | tai/deepseek-v4.1-flash | high     |
| fast / handoff-requirements | `01a1c0e4-77c0-734b-98e6-05b1e56bd076` | tai/deepseek-v4.1-flash | low      |
| good / handoff-page         | `01a1c0e5-d29e-7321-bd48-7c48e017b32a` | tai/deepseek-v4.1-flash | high     |

实际记录包含非零模型输入/输出与工具执行。worker header 的 parentSession 指向同一 director。
验收脚本曾在弹层状态变化后误用全局 Escape，取消了主轮次及 worker；改用明确控件后，
通过原有条目继续同一 worker，两者均完成。这个过程不作为无中断或零失败的证明。
随后新 daemon 的重启只用于核对历史摘要，模式按约定关闭。

Tai-Shift 预设通过 GUI 新建、保存、重新打开，并从当前会话设置中应用；主模型 High，
SMOL Low、PLAN/TASK/ADVISOR High。默认继承、原生存储后的回填、固定角色名称和用途均可见。
应用后另发送真实 Tai 确认请求并收到回答；没有用手写 JSON 完成 GUI 配置。
随后从条件表单选择同一 fast worker，发送两处文档文字修正指导，完成后由 director 读回确认。
显式开启 Vibe 保留了已填草稿；切换只读查询成功且未创建新 worker，结束后从表单关闭模式。

最终产物（验收结束后计算；产物保留在私有隔离目录，不加入生产代码）：

| 文件            |   字节 | SHA-256                                                            |
| --------------- | -----: | ------------------------------------------------------------------ |
| index.html      | 11,378 | `488f85a04ae4135fb514a86d3bd97b7b2adf6b9e96738522e763ce081d3505b0` |
| REQUIREMENTS.md |  8,433 | `b6a4f0143c206a149fd33d0ab56e5419f00bafad139dab4570e3b6cf7b1a7b24` |

## 外部浏览器检查与截图

作者独立打开生成的 index.html，检查空态、三位负责人各添加事项、完成切换、刷新持久化、
Enter 添加、删除、Tab 到负责人、390px 无横向溢出。均按实际控件操作，未把 worker 自述
的 Playwright/对比度结论计作自己的验证。产物不是本 PR 的产品代码。

原生 Vibe 和 Goal 表单均实际执行“先填创建参数，再切换为只读查询”：请求成功，旧参数未提交；
空白 worker / Goal 的提交被前端阻止。历史选择在新 daemon 的同一会话中显示真实内容。
账号切换的迟到响应与失败路径由组件测试覆盖，没有对用户真实账号执行登录或退出。

- [按任务分组的入口](../../assets/providers/omp/tai-native-actions-groups.png)
- [任务菜单](../../assets/providers/omp/tai-native-actions-tasks.png)
- [条件表单中的真实 worker 指导](../../assets/providers/omp/tai-native-vibe-guidance-form.png)
- [历史对象选择](../../assets/providers/omp/tai-native-history-target.png)
- [共享 worker 条目的指导](../../assets/providers/omp/tai-native-worker-guidance.png)
- [角色用途与不可改名说明](../../assets/providers/omp/tai-native-roles-help.png)
- [应用实际创建的预设](../../assets/providers/omp/tai-native-preset-applied.png)
- [真实侧问答的紧凑展示](../../assets/providers/omp/tai-native-side-compact.png)
- [真实生成的交接卡](../../assets/providers/omp/tai-native-handoff-result.png)
- [交接卡窄屏](../../assets/providers/omp/tai-native-handoff-result-phone.png)

侧问答同时只展示所选话题；取消/追问位于正文滚动区外，标题有展开标识。回答最多 144px，
只有明确选择新问题或追问才打开第二个编辑器。实测桌面面板约 273px，默认只保留主输入框。
没有声称已经完成桌面独立侧栏、原生软键盘、完整键盘导航或无障碍审计。

## 检查范围

- 前端：本轮定向套件覆盖动作字段/参数、对象列表迟到与失败、账号切换、共享 worker 操作、
  预设刷新/权限/编辑、角色说明、侧问答、Vibe、composer 与九语种。13 个套件、165 项通过；同一用例重复运行不累计。
- Rust：`cargo test --locked -p provider local::omp::actions`，10 项通过。
- Rust 1.98.1：`cargo build --locked --workspace`、
  `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。
- 前端 typecheck、oxlint、oxfmt、`check:ui` 和文档校验；最终状态见 PR。
- 未在本地运行完整 Rust workspace 测试。没有重新进行签名安装包、macOS/Windows 或版本矩阵验收。

## 保留的 P3

worker 异步回执仍可能以转义 JSON 出现在主时间线。实际生产者是公开扩展的
`ait-vibe-result` 自定义消息，当前通用 custom 消息投影按助手文本展示。共享列表、对象指导
和停止入口已经复用，但回执本身尚未改成“任务/状态/摘要 + 折叠原文”的结构化投影。
本轮截图中的这类内容不被当成已修复。侧问答独立侧栏/窄屏独立面板仍可进一步改进。

## Test coverage

**未测量**：本轮未请求覆盖率。通过数量与实际 API 请求不代表行覆盖率。
若后续请求，对受影响的前端套件及 Provider actions 模块定向测量；Rust 使用 `cargo llvm-cov`，
不扩大为完整 workspace 覆盖率。
