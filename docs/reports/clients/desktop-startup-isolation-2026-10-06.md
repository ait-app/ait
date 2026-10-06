# 桌面启动连接与 Provider 发现隔离验证

- 日期：2026-10-06。
- 基线：PR [#186](https://github.com/ait-app/ait/pull/186) 的 `233c3112`，源码树与 `913f082c` 相同。
- 上游基线 [CI 37414697556](https://github.com/ait-app/ait/actions/runs/37414697556) 已成功；此前失败的超时根因仍未证明。
- 实现指纹、逐文件覆盖率与限制见[机器可读报告](desktop-startup-isolation-2026-10-06.json)。
- 边界决策见 [ADR-086](../../decisions/providers/adr-086-provider-catalog-startup-isolation.md)。

## 发现与修复

1. 保存的 managed endpoint 在新 daemon 就绪前被探测，主进程尚无可注入 token，触发 Bearer 长度错误。
   恢复时保留 host/cache，等待鉴权 readiness 后登记当前端点；端点变化立即启动新探测，旧结果失效。
   被一并失效的未完成 remote 探测也清除退避时间，避免额外等待。
2. 模型发现同时占用 Provider execution 队列和物理 WebSocket 请求循环。
   仅拆分后台 actor 不足以修复真实桌面：诊断包仍记录同连接历史请求等待 34.722 秒，
   与前序目录响应同时返回。最终实现同时拆分有界 catalog actor 和受跟踪的异步响应派发。
   普通连接与 single-connection relay 均由同 socket 回归覆盖，不增加客户端物理连接。
3. protocol 测试在单条断言内部动态加载约 4.5 MB 生成验证器，慢环境的 transform 超过 5 秒。
   改为模块顶层 import；保留全部断言、隔离边界与测试期限。该模块没有依赖测试级 mock 或可变初始化。

## 验证

### 确定性回归

- 离线 discovery gate 暂停 2 秒：原实现跨 socket 的 Agent/get、subscription、timeline 均阻塞约 2 秒；
  actor 修复后三者为 1.44–6.23 ms，三轮均通过。
- 最终同物理 socket 回归覆盖普通与 `connection.single.v1`：目录发现未完成时，ping、Agent、
  历史和订阅先返回；请求方断开后 refresh 完成，后续查询复用缓存。
- Provider 定向 42 项通过，含有界接纳、response permits、取消、cache、refresh、shutdown 与生命周期回归。
- Host runtime 72 项通过；新增旧端点、更换端点、remote host 与被失效 pending probe 回归。
  首批 4 项新回归在原基线上全部失败，在修复后通过。

### Linux Electron 实测

使用 Electron 44.2.0 / Ait 0.0.19、独立 HOME/XDG/profile、原生 OpenCode 2.0.20 和既有离线测试历史。
实际进程路径和 bundled daemon 均已核对；最终 daemon SHA-256：

`25200e0b5c62bb47b624fb1be799a4ac5966c6527c864db63728b4595ea88d80`

- 基线恢复约 20–40 秒；诊断包确认同连接目录请求造成约 35 秒队首等待。
- 最终冷进程启动、暖 renderer reload、再次完整 desktop+daemon 重启，在启动后约 8–10 秒的
  首次取样截图中均已完成历史同步。三次截图请求的相对时间为 8.6 / 8.3 / 7.7 秒；
  截图捕获耗时未单独测量，这些不是精确完成时间或统计基准。未清空 OS cache。
- 两次完整启动均自动恢复，没有靠手动刷新恢复；最终日志无 Bearer 长度错误。
- 历史顺序保持，输入框可编辑；测试草稿已清除，没有提交模型请求。
- 模型目录仍可在后台加载，其原生发现耗时不在本次优化承诺内。
- 旧实例及最终测试 app/managed daemon 均正常退出，未留下已核对的进程。

### 静态与前端检查

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -j2 -- -D warnings` 通过。
- Desktop 全套：404 通过、4 跳过。执行器的 NoNewPrivs 环境使一个既有 launcher fixture 初次失败；
  在正常 cloud desktop 环境中原断言及全套均通过，没有更改 sandbox 设置。
- Protocol 全套：764 通过；mobile CI 范围加相关 runtime/directory 回归：558 通过、1 跳过。
- Desktop/mobile typecheck、变更文件 oxfmt/oxlint、release/mobile-release 检查、版本一致性、
  local packages、桌面 main/renderer 构建和 packaged resources 检查通过。
- 依赖复用已安装且 manifest/lockfile 相同的测试树；自有 UI packages、renderer 和 Rust daemon 重建。
  本次未声称一次全新的 `npm ci` 成功。

## Test coverage

Linux x86_64 / Rust 1.98.1 / cargo-llvm-cov 0.8.7；workspace default features、默认文件过滤，
不额外排除文件，doctests 未插桩。为节省构建空间设置 `CARGO_INCREMENTAL=0`、
`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`。

命令：`cargo +1.98.1 llvm-cov --workspace --locked -j2 --html --no-clean --ignore-run-fail`。
报告合并同源码的整套尝试和 DSH fixture 的五次定向复跑；`--ignore-run-fail` 只允许生成报告，
不代表测试全部成功。

| 范围      | 已覆盖 / 总行数 | 行覆盖率 |
| --------- | --------------- | -------- |
| workspace | 50956 / 54012   | 94.3420% |
| provider  | 22969 / 24521   | 93.6707% |
| daemon    | 925 / 969       | 95.4592% |

完整本地 workspace 尝试为 1831 通过、2 失败、6 opt-in 忽略：

- `directory_links_are_listed_but_special_files_cannot_be_copied` 的临时 UnixListener 被执行环境以
  EPERM 拒绝，获准的执行权限重试仍相同。未改变 fixture 断言或绕过该限制。
- `denial_foreign_requests_and_context_updates_preserve_native_authority` 在 native fixture
  `create_session` 返回一次 `Unavailable`；随后隔离复跑 5/5 通过。原始 spawn errno 未证明。

变更 Rust 文件结果保存在同目录 JSON。全量逐文件 HTML 报告作为独立验证附件保留。没有可比的 Linux baseline，
不与此前 macOS arm64 百分比计算差值。前端行覆盖率未测量，测试数量与覆盖率分开记录。
未覆盖范围包括部分 dispatch 拒绝/响应失败路径和 OS 专属恢复分支；macOS、Windows、
原生移动端及外部付费模型未在本次实机执行。
