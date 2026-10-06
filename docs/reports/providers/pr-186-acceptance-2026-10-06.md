# PR #186 验收反馈修复

日期：2026-10-06。基线 `951e9cec`，上游已包含 `5df233ac`；验证源码以随附制品指纹标识。

## 修复与证据

- **OpenCode 2 拒绝工具后不结束**：真实 Homebrew 2.0.20 + 隔离 XDG + 本地确定性模型复现。
  拒绝 shell 权限后原生 active 列表为空，但会话无 outcome、历史无 idle 行，公开 log 仅有 `log.synced`。
  最后助手记录为 `finish=error`、`error.type=aborted` 且有完成时间。
  现在仅在该记录属于最近输入之后、创建/完成时间有序、历史读取前后都不活跃时结算 interrupted。
  不发送额外 interrupt，不重放提示词。真实 CLI 回归证明拒绝后自动结束，下一轮成功。
  离线回归覆盖旧消息、缺失/逆序时间、不同错误类型、新输入，以及无 idle 行的结束/恢复。
- **OpenCode 2 冷启动漏模型**：原实现仅等待空列表变非空，可能返回插件激活中途的部分目录。
  官方 v2.0.20 的 [Plugin.activate](https://github.com/anomalyco/opencode/blob/v2.0.20/packages/core/src/plugin.ts)
  在初始插件批次应用后发布 inventory；现在先有界等待同 location 的 `/api/plugin` inventory，再读取模型。
  HTTP 失败/畸形 inventory 仍报错，未就绪不会返回部分列表；不创建探测会话、不重载配置。
  初始批次包括本地已可用插件和配置；后续远程插件安装、运行中配置更新仍需重新发现。
  2.0.10 同样具有该只读端点。离线回归提供提前可见的部分非空模型目录，并验证其不会被返回；
  inventory 永久未就绪与空模型目录都在五秒预算内失败。
  真实 2.0.20 首次冷发现同时包含两个配置 provider 的模型，随后文本多轮与恢复通过。
- **DSH 多选选项/自定义答案同文提交失败**：数组表单原来发送重复字符串，原生 adapter 拒绝重复值。
  前端构造数组答案时按完整字符串去重，保留首次顺序和不同文本，不按逗号拆分。
  测试覆盖 `Alpha, beta` 与带首尾空白的同文输入，以及大小写不同的自定义文本。
  后端继续拒绝格式不合法的直接请求。

## 验证

- `nix develop --command cargo test -p provider local::opencode --lib`：55 passed、2 ignored。
- `nix develop --command cargo test -p provider v2_cold_model_catalog_retries_are_bounded --lib`：1 passed。
- `nix develop --command env AIT_TEST_OPENCODE_BIN=/private/tmp/ait-provider-compat/opencode/2.0.20/bin/opencode cargo test -p provider installed_opencode --lib -- --ignored`：2 passed。
  该二进制版本为 2.0.20；只调用本地确定性模型，不使用用户模型账户。
- `npm test --workspace=@ait/mobile -- src/components/question-form-card-core.test.ts`：6 passed。
- `nix develop --command cargo test -p daemon --test process unix::opencode`：2 passed；Python HTTP 夹具已补齐 `/api/plugin`。
- Mobile typecheck、修改 TS 的 oxfmt/oxlint、Rust fmt、workspace all-targets Clippy、workspace build、文档链接和 diff 检查通过。
- `nix develop --command cargo test --workspace`：1826 passed、0 failed、6 ignored。

## Test coverage

`nix develop --command cargo llvm-cov --workspace --html`，macOS arm64 / Rust 1.98.1，
默认 features、默认文件过滤，无额外排除；doctest 未插桩。
覆盖率测试：1826 passed、0 failed、6 ignored。

| 范围 | 覆盖行 / 总行 | 行覆盖率 | 相较 951e9cec（百分点） |
| --- | ---: | ---: | ---: |
| workspace | 50861 / 53909 | 94.3460% | -0.0038 |
| provider | 22877 / 24418 | 93.6891% | -0.0114 |
| opencode | 2756 / 3154 | 87.3811% | +0.0134 |

基线同平台同范围，基线以单线程执行、当前以默认并发执行，系统故障/调度分支存在运行差异。
六个忽略项为 opt-in CLI/在线模型测试，其中两项真实 OpenCode 测试已单独运行，不计入覆盖率。
前端行覆盖率 **not measured**：本轮运行定向 Vitest，未启用覆盖率收集；测试数量不是覆盖率。
[可审阅覆盖率数据与源码/日志指纹](pr-186-acceptance-2026-10-06.json)，
完整 HTML 在 `target/llvm-cov/html/index.html`。

本轮未重新执行 Electron/Windows/原生移动端端到端验收或真实 DSH 模型问答；
DSH 改动由表单单元回归验证，完整桌面验收仍应使用最终提交。
未覆盖所有 OpenCode OS 启动、网络断线和资源上限分支；晚到的远程插件模型仍需重新发现。
用户先前报告的桌面启动延迟本轮没有独立复现或宣称修复。
推送后的远端 CI 结果以对应提交检查为准。
