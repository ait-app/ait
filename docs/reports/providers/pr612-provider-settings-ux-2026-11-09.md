# PR #612：角色表单、原生控制与 Tai 真实情景验收

功能源码为 `458e250e`，已合入 main `1e9c0886`（#630、#631）；本报告及截图不再改变功能代码。
Linux x64、Rust 1.98.1、用户安装的官方 OMP 18.17.0。PR 保持未合并，最终 head 和 CI 以 GitHub 为准。

## 真实任务及 API 边界

最终 PR 内嵌截图来自 Tai 真实 API 任务或该任务的配置页。使用用户已配置的
`https://api.inferknock.ai/v1`，模型 `tai/deepseek-v4.1-flash`、原生 `openai-responses` 通道。
凭据只在隔离目录使用，没有输出或提交。没有模拟响应、模型 peer、占位回答或私有 OMP。
早前本地 peer 的调试图片不作为最终 PR 验收图；历史报告保留各自执行范围。

任务是为三人团队制作离线中文任务看板：待办／进行中／已完成，支持添加任务、按钮移列、
负责人筛选、localStorage、键盘操作、手机布局、空态。director 最多创建两个 worker，
fast 写 DESIGN.md，good 写 index.html；通过 send/wait 核对并由已有 good worker 补写 VERIFY.md。
实际实现有数轮规格对齐，最终冻结源文件后验收，没有新增第三个 worker。
所有产物位于独立临时目录，没有修改本仓库的产品文件。

## 预设与角色表单

实际通过 GUI 创建 Tai-Board，主模型 High；SMOL Low、PLAN High、TASK High、ADVISOR High。
保存后重新打开回显正确，再通过设置页下拉应用到 director 会话，随后运行 `/vibe <任务>`。
角色按固定名称排列，“角色 → 模型 → 思考等级”一眼对应；添加时仅选择未列出的角色，
删除行移除覆盖。模型和思考均用下拉，无需 JSON、配置键或原生后缀。

原生继承由可选 metadata 返回原选择器和解析后的模型，别名有界解析，无法解析时不伪造模型。
OMP 保存会物化当前默认模型，因此重新打开按实际存储记录回填。
窄屏将思考等级放到模型下一行，保存按钮独立固定；390×844 浏览器检查模型完整可见、
保存按钮在视口内，并实际用键盘打开模型选择。不代表原生软键盘或完整无障碍验收。

- [角色模型与 thinking 回显](../../assets/providers/omp/tai-preset-editor.png)
- [默认继承指向](../../assets/providers/omp/tai-preset-role-defaults.png)
- [预设实际应用](../../assets/providers/omp/tai-preset-applied.png)
- [390px 编辑器](../../assets/providers/omp/tai-preset-editor-narrow.png)

## 实际 Vibe 调度证据

原生日志核对得到以下事实；没有将界面“启用”或一次握手当作调度成功。

| 会话              | 原生 ID                                | 原生模型                | thinking |
| ----------------- | -------------------------------------- | ----------------------- | -------- |
| director          | `01a1c05e-f758-728a-b057-1f476b5c2642` | tai/deepseek-v4.1-flash | high     |
| fast / design-md  | `01a1c061-4b86-753d-88ab-f712744511c4` | tai/deepseek-v4.1-flash | low      |
| good / index-html | `01a1c061-53c0-75d0-b729-117f2d73e28e` | tai/deepseek-v4.1-flash | high     |

两个 worker 的原生 session header 中 parentSession 都指向 director 的原生日志；
Ait 仅一个 actor/外部进程写入每份日志，只读观测不启动第二个 writer。
director 记录中实际执行 `vibe_spawn` 两次，以及 `vibe_send`、`vibe_wait`、`vibe_list`。
worker 实际读写文件并运行检查，最终均 completed，director 回到 idle。
原生记录包含非零输入／输出 token 与费用，不是模拟输出；director 最后汇总费用约 $0.0599，
这是该会话的记录，不能解释为包含所有 worker 和侧问答的总费用。

- [真实 Vibe worker 与父会话](../../assets/providers/omp/tai-vibe-workers.png)
- [从父会话打开只读 worker](../../assets/providers/omp/tai-worker-observed.png)
- [Tai 真实侧问答 Markdown](../../assets/providers/omp/tai-side-markdown.png)
- [原生统计摘要](../../assets/providers/omp/tai-native-statistics.png)

侧问答在主任务运行时通过 Tai API 返回键盘流程、空态和代码建议，未修改任务文件；首问
不在同一 topic 的正文重复，回答使用共享 Markdown，输入提示区分新问题和追问。
统计默认显示 token、费用和上下文摘要，原始 JSON／完整路径在详情中；导出显示文件名。
Vibe 当前开关一直可见，包括零 worker 空态和退出入口；旧 daemon 缺字段时仍隐藏。
另用一个 Low 档位 Tai 会话实际回答简短验收问题，再发送裸 `/vibe`，确认零 worker 状态可见；
实际点击退出后显示 off。两张图来自真实 API 回答后的会话：
[启用空态](../../assets/providers/omp/tai-vibe-empty.png)、[退出后](../../assets/providers/omp/tai-vibe-off.png)。

## 最终产物和外部浏览器检查

最终交付文件哈希（SHA-256）：

| 文件       |  字节 | SHA-256                                                            |
| ---------- | ----: | ------------------------------------------------------------------ |
| TASK.md    |   432 | `705ca427ccde2b955599b5c6f80c0db9402ac83f43cd88b666cafb15c4cd0f14` |
| DESIGN.md  | 12462 | `204a5bce721ad6979029593c8a8fb0afc3ab779dab0e2f5fc23556ba485f244f` |
| index.html | 24565 | `9179f2d141532058676ecff75bd9ad6116c89cbfd604f3a274c9755253dc314e` |
| VERIFY.md  | 12096 | `fb9616ec614894da64f8fb8b69b0720f6470c48f0ae8ba867814a7b1efdfb28d` |

直接在浏览器加载生成的真实 index.html，通过界面验证：空态、添加三位负责人的任务、
按钮移到三列、负责人筛选、刷新后状态保留、Enter 添加与移列、390px 无横向溢出，且无页面异常。
这些是外部验收，不冒称由 director 自行完成。产物截图展示实际创建与移动后的数据。

- [Tai 生成的实际看板](../../assets/providers/omp/tai-board-result.png)
- [实际产物窄屏](../../assets/providers/omp/tai-board-result-narrow.png)

## 刷新、权限和删除

设置页刷新同一主机、Provider、目录下的模型、独立预设缓存和相关角色候选；角色读取等待
模型刷新确认，分别报告结果。五项回归覆盖失败恢复、外部变化、pending、候选读取顺序和
模型确认后仍报告错误。临时移除预设刷新调用后，失败恢复探针失败；还原后通过。

删除先确认名称、Provider、全局作用域和不可撤销提示，再复查来源并写入；pending 禁用重复操作，
成功反馈名称，错误保留在确认页。未知、失败、刷新中和项目覆盖始终禁止修改；后端写前校验保留。
项目只读、来源未知和读取失败使用不同提示；长路径、完整标识和规则放到详情，项目项允许查看。
删除截图流程取消确认，没有删除正在使用的 Tai-Board。真实写入、等待和失败路径由组件测试验证。

- [当前项目真实刷新反馈](../../assets/providers/omp/tai-provider-refresh.png)
- [全局预设删除确认](../../assets/providers/omp/tai-preset-delete-confirmation.png)

## 最新主线与定向检查

保留 #630 Photon 三个 RPC 与本 PR 原生／预设能力。初次合并使客户端一组 hello 达到 65 项；
Photon 请求现映射到 daemon 已有的平台配对通道，四组恢复为 64／62／47／62。
客户端 228 个方法，Rust 组件方法 239、发布能力 240、业务目录 220。

- 前端 11 suites、143 项通过；窄屏角色布局调整后，编辑器和转换规则 27 项重新通过。
- 客户端 connection/messages 100 项通过；协议 Bot 7 项、预设回执 6 项通过。
- Provider model_roles 11 项通过、2 项忽略，包含继承别名 metadata。
- API 能力 10、连接校验 3、single lane 2、placeholder 过滤 5、HTTP readiness 1 项通过；
  placeholder 过滤含一项与 validation 重叠。daemon 组件安装与真实进程目录各 1 项通过。
- SDK 构建、移动端 typecheck、Clippy、rustfmt、oxlint、oxfmt、UI 和文档检查通过。
  客户端方法声明与 239 个 Rust 组件方法核对通过。

没有本地完整 Rust workspace 测试或覆盖率运行。正式签名包、macOS/Windows、原生软键盘
及完整 OMP 版本矩阵未验收。Claude advisor 与 DSH/OpenCode 的专属路径未在本轮 Tai 场景中
执行真实推理，既有历史报告不能替代本次验证。

## Test coverage

**未测量**：用户没有请求覆盖率。通过数量和截图均不是覆盖率。
