# ADR-128：桌面内置 daemon 可在退出后继续运行

- 状态：已实现
- 日期：2026-10-11
- 关系：修订 [ADR-048](adr-048-paseo-desktop-rust-launcher.md) 中「服务随桌面正常退出而停止，移除旧的后台驻留开关」与「不实现孤儿进程接管」的部分；沿用其只管理自己创建的服务、令牌不进入 renderer 的边界。

## 背景

`keepRunningAfterQuit` 设置一直随桌面设置保存，但界面开关已移除，退出流程也从不读取它。
用户希望退出 Ait 后 Agent 和终端继续运行，下次启动时直接回到这些会话。

## 决策

- Host 设置的内置 daemon 卡片恢复「退出后保持守护进程运行」开关，默认关闭；暂停内置 daemon 管理时开关不可用。
- 内置 daemon 以独立进程组启动，stderr 直接写入 `daemon.log`，不再经 Electron 管道转发；就绪仍以日志中的 `daemon ready` 加带认证的握手确认，握手同时读取 `daemon.get_status` 返回的进程 PID。
- 退出时若开关开启且 daemon 正常运行，主进程把 PID、Bearer token、实际监听地址、配置的监听地址和启动时间写入数据目录的 `desktop-daemon.json`（权限 0600，原子替换），释放子进程句柄后退出；交接失败时照常停止 daemon，避免留下无法连接的服务。开关关闭时行为不变。
- 下次启动先读取该记录并立即删除：仅当 PID 存活、使用保存的 token 握手成功且返回的 PID 一致时才重新接管，沿用原 token 与地址；任何一项不符都不向该 PID 发送信号，并按原流程启动新 daemon。退出期间监听设置变更时，经认证的关闭 RPC 停止旧 daemon 后按新地址启动。
- 停止或重启接管的 daemon 时先发送 `server.shutdown.request`，超时后再次认证确认 PID 未变才 SIGKILL。
- token 仅以文件权限保护：同一系统用户的其他进程可以读取。该文件位于已为 0700 的 daemon 数据目录内，不进入 renderer、日志或主机配置。

## 后果

开启开关后，退出 Ait 不再中断运行中的 Agent 与终端；下次启动连接到同一 daemon，`server_id`、PID 与端口保持不变。
Electron 崩溃或被强制结束时仍不会写入交接记录；此时 detached daemon 继续运行但不会被自动接管，数据目录锁会使下次启动报告目录被占用，需要手动结束该进程。
`daemon.log` 由 daemon 直接追加，桌面仅在启动新 daemon 前按大小轮转。
验证见 `apps/desktop/src/daemon/rust-daemon.test.ts` 的真实进程用例与 `npm run test:e2e:keep-running --workspace=@ait/desktop`。
