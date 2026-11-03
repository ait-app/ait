# Paseo 0.12.0 增量借鉴清单

研究基线记录：以下差距按研究时的固定提交描述。后续实施见 [0.12.0 移植与验证](paseo-0.12-port-2026-10-20.md)。

日期：2026-10-19。结论来自本地 Git 历史、提交 diff 和 Ait 当前实现；没有拉取远端，也没有修改业务代码。

## 对照范围

- Paseo：`/Users/necokeine/Documents/paseo`，版本 0.12.0，固定提交 [91154ba4e8ace3484595a05a4366279fb4818745](https://github.com/getpaseo/paseo/commit/91154ba4e8ace3484595a05a4366279fb4818745)，提交时间 2026-10-19 22:58:21 +08:00。包含 0.12.0 发布后的修复。
- 增量起点：上一批[选定改动移植报告](paseo-desktop-app-port-2026-10-10.md)采用的 Paseo 0.11.2 `a1da1b8f234afe7a75785022292cdf8a50ee11a5`。起点之后共 233 个提交，包含发布、测试和文档提交；不等于 233 项新功能。
- Ait：当前工作区固定提交 `808b9bcdc7267b68af6f844f9632de0fe4975d53`，2026-10-19 22:53:07 +08:00。研究开始时工作区还是 `799558f1`，随后外部更新到该提交；以下按更新后的实现核验。
- 上游来源均为本地 checkout 的源码。GitHub 链接用于定位同一提交；没有把 changelog 的性能数字作为 Ait 上的测量结果。

## 建议顺序

优先级表示建议实施顺序。小、中、大是相对改动范围，不是工时承诺；源码存在旧逻辑不等于每个实际会话都会触发问题。

| 顺序 | 借鉴项                                   | 对 Ait 的价值与已确认差距                                                                                | 范围                                   | 上游                                                                                                                                                                                                                                               |
| ---- | ---------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | 刷新时合并流式正文与缓存历史             | 防止同一回复出现旧副本、缺尾；Ait catch-up 命中 live head 后仍跳过 cached tail 检索                      | 小至中，客户端状态归并                 | [#7256](https://github.com/getpaseo/paseo/commit/90c6de058)                                                                                                                                                                                        |
| 2    | 聊天缓存按目标实体失效                   | 同主机另一个 Agent 流式输出时，打开已有聊天仍能使用缓存；Ait 仍以 host revision 重试读取                 | 中，缓存并发与生命周期                 | [#7074](https://github.com/getpaseo/paseo/commit/e0de0fdf6)                                                                                                                                                                                        |
| 3    | 宽字符终端恢复、复制与长按选择           | 中文、日文和韩文恢复后不插空格；复制字形的任一半得到完整字符；Ait 现有函数可直接复现问题                 | 小，协议辅助函数与原生选择             | [#6452](https://github.com/getpaseo/paseo/commit/e81d5d02b)、[#7254](https://github.com/getpaseo/paseo/commit/9bd58f66b)、[#7260](https://github.com/getpaseo/paseo/commit/e5d9307cf)、[#7353](https://github.com/getpaseo/paseo/commit/644d66ba4) |
| 4    | Git quoted path 与提交重命名 diff        | 特殊文件名可正确匹配；重命名保持前后文件关联，而不是整文件新增；Ait 两处仍有缺口                         | 中，Rust Git adapter 与回归用例        | [#6583](https://github.com/getpaseo/paseo/commit/938c4e263)、[#6895](https://github.com/getpaseo/paseo/commit/5c108f345)                                                                                                                           |
| 5    | 长聊天与侧栏减少重复计算                 | 复用未变历史行、工作区与子 Agent 投影，避免每个流式 tick 重算大列表；Ait 尚未覆盖这一批优化              | 中，可拆为独立小改                     | [#7052](https://github.com/getpaseo/paseo/commit/8d424e039)、[#7342](https://github.com/getpaseo/paseo/commit/559c0200e)、[#7157](https://github.com/getpaseo/paseo/commit/0d7a234e6)                                                              |
| 6    | 用量刷新失败保留上次成功值与稳定卡片顺序 | 网络或登录状态失败时仍能查阅旧额度和采样时间，保留错误提示；Ait 当前错误卡清空数值，完成顺序还会改变排列 | 排序小；快照需中等范围协议与 Rust 配合 | [#7246](https://github.com/getpaseo/paseo/commit/cf62b0091)、[#6947](https://github.com/getpaseo/paseo/commit/208e32303)                                                                                                                           |
| 7    | 文件链接与 Markdown 预览补齐             | 文件夹链接进入 Files、显式文件链接不受扩展名限制、相对图片以 Markdown 文件目录解析；应逐项迁入           | 中，与渲染引擎拆开                     | [#7205](https://github.com/getpaseo/paseo/commit/fa70b215a)、[#7229](https://github.com/getpaseo/paseo/commit/5384abdba)、[#7233](https://github.com/getpaseo/paseo/commit/a0aeca967)、[#6750](https://github.com/getpaseo/paseo/commit/9eaf7ad0c) |
| 8    | 手机长文编辑与 Diff 反馈                 | 全屏输入便于长提示词；在 Changes 直接发评审意见，减少手机上来回切聊天                                    | 中，按产品目标选择                     | [#6531](https://github.com/getpaseo/paseo/commit/750cb82fb)、[#6455](https://github.com/getpaseo/paseo/commit/b26b9234c)                                                                                                                           |

建议先完成 1–4，再推进 5–7。第 8 项属于产品体验增强。客户端逐项源码、移动端其他候选和迁移注意事项见[客户端详细研究](paseo-client-candidates-2026-10-19.md)。

## 已核验的终端差距

上游修复的核心是保留终端的单元格宽度语义：宽字符的第二格是 continuation，不能当作真实空格输出。上游 [snapshot 修复](https://github.com/getpaseo/paseo/commit/e81d5d02b11e489f7e1beb52df7e22bf4cc8e06e)与[原生复制修复](https://github.com/getpaseo/paseo/commit/9bd58f66b2dfc195898bf1178e84f02933c0d41e)是两条独立路径。

Ait 的 Rust [row_cells](../../../crates/terminal/src/screen.rs)已经把 continuation 编为 `char: ""`，无需重复迁移上游 Node 服务端的修改。但客户端仍有以下行为：

- [terminal-snapshot.ts](../../../packages/protocol/src/terminal-snapshot.ts)的 `renderTerminalRow` 输出 `cell.char || " "`，把 daemon 正确发来的空 continuation 重新变成空格。
- [terminal-selection.ts](../../../apps/mobile/src/terminal/native-renderer/terminal-selection.ts)的 `extractTerminalSelectedText` 同样用 `cell.char || " "`；从第二格开始选择时还不会回退到字形首格。
- 同文件 `resolveTerminalWordSelection` 把 continuation 视为词分隔符，长按宽字符词时会截断。

直接导入 Ait 当前 TypeScript 函数，以 `中`、continuation、`文`、continuation、`!` 五格输入进行最小验证，得到：

```text
应有正文：中文!
snapshotVisible：中 文 !
copied：中 文 !
长按第 0 格：只返回 col 0..0
```

建议把 snapshot、复制、选择和松手后保留词选区作为一组迁入；验证真实空格、软换行、从 continuation 开始选中，以及桌面和移动端恢复路径。上述结果来自纯函数调用，没有操作用户的运行中终端。

## 已核验的 Git 差距

Ait [checkout.rs](../../../crates/filesystem/src/git/local/checkout.rs)中：

1. `parse_diff_section` 直接取得 `+++`、`---` 和 `rename from/to` 后的字符串；`strip_diff_prefix` 只识别裸 `a/`、`b/`，没有解码 Git 的 C-style quoted path。即使 `core.quotepath=false`，引号、反斜线和控制字符仍会触发 Git 引用。
2. `commit_file_diff` 使用 `git show ... -- path`，没有先发现整个提交的 rename map，也没有把旧路径一并交给 Git。只给目标路径会失去重命名配对。
3. `require_git_directory` 检查 `--show-toplevel` 但返回传入 cwd，没有归一到 worktree root。子目录工作区应沿上游 [#6882](https://github.com/getpaseo/paseo/commit/8ff2424da)、[#6801](https://github.com/getpaseo/paseo/commit/5805b776c)、[#6777](https://github.com/getpaseo/paseo/commit/6d43a8c5b)的测试场景核验 diff、提交详情与 discard 的路径坐标；不能全局替换 cwd 后假设所有方法语义不变。

在 `/private/tmp` 创建并自动清理的独立 Git 仓库中验证了前两项：

```text
真实文件名：quote"name.txt
当前解析步骤的结果："b/quote\"name.txt"（仍含引用与转义）
git show ... -- new.txt：包含 new file mode
git show ... -M -- old.txt new.txt：保留 rename from old.txt
```

建议迁入路径解码、完整提交级 rename 检索、旧新路径同时选择和不可变 SHA 的有界共享缓存。上游 [#6895](https://github.com/getpaseo/paseo/commit/5c108f345c9ecad19a69873b6d10fb81d3022e1f)明确区分重命名与复制，不能简单加 `--follow`，否则普通复制可能被误作重命名。

已有防护需要保留：Ait Git runner 已强制 `color.ui=false`、`diff.noprefix=false`、`diff.mnemonicPrefix=false`，工作区 diff 已禁用 external diff；[literal pathspec 回归](../../../crates/filesystem/src/git/local/checkout/tests/file_diff.rs)也已存在。[#6536](https://github.com/getpaseo/paseo/commit/e61009d5f)与[#6562](https://github.com/getpaseo/paseo/commit/a5dba3c99)不能整组算作 Ait 缺失；但 `git show` 的读取参数仍应统一检查。Ait 的 hunk parser 在进入 hunk 后再识别 `+`/`-`，没有上游 [#6569](https://github.com/getpaseo/paseo/commit/a3f75aa9e)所修的那种全局跳过 `---` 行逻辑。

## 不宜直接搬运的后端修改

| 上游改动                                                                                                                                                                                                                                   | Ait 当前证据                                                                                                                                                                                                                                                          | 建议                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| 大目录并发搜索的惰性遍历 [#7265](https://github.com/getpaseo/paseo/commit/a7a21d91c)                                                                                                                                                       | [search.rs](../../../crates/filesystem/src/files/local/search.rs)逐个消费 `read_dir`，在构建候选路径前累计扫描量，并在 20,000 项后停止；没有同构的 eager `Promise.all` 子树展开                                                                                       | 借鉴宽目录、多查询并发的压力场景；未证明 Ait 有同一内存问题                                 |
| 高延迟双栈 relay [#7336](https://github.com/getpaseo/paseo/commit/aef7cc36a)                                                                                                                                                               | [transport.rs](../../../crates/relay/src/transport.rs)使用 Rust Tokio/Tungstenite，设置整体 5 秒连接预算；不是 Node 的 `autoSelectFamilyAttemptTimeout`                                                                                                               | 借鉴“慢 IPv4 + 不可达 IPv6”故障注入，再决定地址竞争策略；不能照抄 Node 参数或直接声称已解决 |
| Hub 静默断线重连 [#7315](https://github.com/getpaseo/paseo/commit/fcda2f1cf)                                                                                                                                                               | Ait 已移除 Paseo Hub；当前 [relay 控制连接](../../../crates/relay/src/lib.rs)有 65 秒读空闲退出，[当时的持久隧道](../../decisions/daemon/adr-155-persistent-relay-data-tunnel.md)有独立重连路径                                                                       | 复用故障场景，不恢复 Hub 模块；不据此认定整个网络黑洞场景已验收                             |
| 定时 slot 原子领取 [#6532](https://github.com/getpaseo/paseo/commit/ee0d901ef)                                                                                                                                                             | [service.rs](../../../crates/schedule/src/service.rs)单线程拥有 Engine，先持久 `begin` 再启动任务，结果写失败进入重试队列并暂停后续派发；[engine.rs](../../../crates/schedule/src/engine.rs)已有 running 排除和节拍推进                                               | 已有对应机制，借鉴末次运行、重启和落盘失败测试；不重复重写调度引擎                          |
| Claude 错误答案保持可重试 [#6936](https://github.com/getpaseo/paseo/commit/71f575e16)、问答失败恢复 [#7098](https://github.com/getpaseo/paseo/commit/d152f1529)                                                                            | [session.rs](../../../crates/provider/src/local/claude/session.rs)先 resolve、send，成功后才删除 pending；[permissions.rs](../../../crates/provider/src/local/claude/permissions.rs)已规范化答案；客户端已做[超时与失败恢复](question-response-timeout-2026-10-13.md) | 核心能力已覆盖，不列为新增缺口；上游按 request 显示内联错误仍可补                           |
| Node/OpenCode 私有 bridge 的内存、解析与 schema 修复 [#6262](https://github.com/getpaseo/paseo/commit/d172d060a)、[#5999](https://github.com/getpaseo/paseo/commit/335a11eb8)、[#7383](https://github.com/getpaseo/paseo/commit/91154ba4e) | Ait 的[当前架构](../../architecture/README.md)是 Rust daemon 与官方 ACP OpenCode adapter                                                                                                                                                                              | 研究协议行为与失败场景；不引入已替换的 TypeScript bridge                                    |
| 保存 symlink config.json [#6023](https://github.com/getpaseo/paseo/commit/bd9af256b)                                                                                                                                                       | Ait [daemon_config.rs](../../../crates/persistence/src/storage/daemon_config.rs)显式拒绝配置 symlink，而不是悄悄替换它                                                                                                                                                | 属于支持策略差异，有 dotfiles 需求再单独设计，不当成现有写坏链接的 bug                      |
| Plugin/Hub/OMP 专用功能                                                                                                                                                                                                                    | [当前架构](../../architecture/README.md)与[退役协议决策](../../decisions/clients/adr-137-remove-retired-feature-protocol.md)已采用不同边界                                                                                                                            | 本轮不恢复整套插件、Hub 或另一套 Agent runtime                                              |

整批合并上游风险较高：客户端已有 Ait 品牌、账号 relay、持久 timeline 和 Rust transport，后端更已换成 Rust。合适的单位是“一个行为和它的回归场景”，而不是按目录覆盖。

## Markdown 引擎与产品增强的取舍

Paseo [#6973](https://github.com/getpaseo/paseo/commit/eb1a8a2ec)把三端渲染统一到 `react-native-enriched-markdown`，带来跨段落原生选择、代码复制与表格横向滚动。这项值得专门验证，但不建议塞进第一批小修。

上游[渲染说明](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/docs/markdown-renderer.md)同时要求原生依赖重建、包补丁、流式 head/tail 拆分、Mermaid 例外、图片生命周期以及 Find/复制适配。Ait 已有自己的稳定 block 身份、图片处理和跨块复制；替换必须在手机真机验证后决定。文件链接和相对图片应先独立补齐。

下一档产品候选包括：手机文件/子 Agent 临时 Sheet、手机 Scripts 入口、Command Center 匹配度优先并以近期使用排序、子 Agent 展示模型和 effort。源码与成本见[客户端详细研究](paseo-client-candidates-2026-10-19.md)。这些不应抢在正文完整性和缓存隔离之前。

还有三项适合独立排期：

- Linux 桌面窗口显示兜底 [#6726](https://github.com/getpaseo/paseo/commit/9551b2cc11328170416b3d9753c7ec9a10ed35e7)：Ait [main.ts](../../../apps/desktop/src/main.ts)仍仅在 `ready-to-show` 时显示窗口，上游增加 Linux `did-finish-load` 路径，避免隐藏窗口没有渲染帧时永久不可见。改动小，迁移时保留 Ait 的窗口所有权和恢复逻辑；本次未在 Linux 复现。
- Launcher 标准输出关闭后保持桌面存活 [#6639](https://github.com/getpaseo/paseo/commit/14e638451652e443ac8b9f74c50c9bfd4b967cb6)：上游在主入口最早阶段保护 console streams 的 EPIPE。Ait SSH 传输已有局部管道保护，但主入口没有这一层；建议以关闭 launcher stdout/stderr 的子进程回归验证，再引入小补丁。
- Setup 日志只在可见面板订阅 [#7407](https://github.com/getpaseo/paseo/commit/1c8c68f93e12812210ff969faea0b7052d9a015e)：Ait [session-context.tsx](../../../apps/mobile/src/contexts/session-context.tsx)仍全局接收 setup progress。上游把状态摘要和完整日志观察分开，可减少后台 setup 对缓存、重连和消息处理的压力。它同时修改 SDK、能力协商和服务端订阅，属于中等范围协议工作；不能只删客户端 listener。

## 本次验证与限制

- 阅读全部 233 项增量提交列表，对推荐项和排除项进一步阅读 diff、源码及现有测试；不是逐个提交的完整兼容性审计。
- 终端使用当前 Ait 导出的纯函数进行最小复现；Git 使用独立临时仓库与当前解析规则核验。没有启动真实 Provider、连接远程主机或测试移动端安装包。
- 性能建议来自算法与状态依赖对照；没有测量 Ait 的帧率、内存峰值或启动耗时，不套用上游的加速百分比。
- 仅新增研究文档并更新分类索引；按仓库指导跳过 Rust 测试。文档格式、链接与空白检查在交付前执行。
