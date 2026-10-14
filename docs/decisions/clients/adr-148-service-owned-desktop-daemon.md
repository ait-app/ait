# ADR-148：桌面连接系统服务与后台进程所有权分离

- 状态：已实现；实机验收结果单独记录。
- 日期：2026-10-14。
- 关系：扩展 [ADR-128](adr-128-desktop-daemon-keep-running.md) 的桌面子进程生命周期，沿用 [ADR-091](../providers/adr-091-independent-session-execution.md) 的后台 Catalog；扩展桌面本机启动契约，不改变远程连接鉴权与功能 crate 的归属。

## 背景

Linux 用户可通过启用 linger 的 systemd 用户服务在登录桌面前启动 daemon。桌面需要连接已有服务；连接超时并不说明原进程已经退出，也不能授权桌面启动第二个进程。

当前桌面沿用 ADR-128 的留存后台接管：读取并删除 `desktop-daemon.json`，验证超时后会尝试 spawn；该接管又将 daemon 标记为桌面所有。systemd 管理的进程需要独立的连接契约，不能通过伪造桌面留存记录进入这条路径。`daemon.get_status.request` 还会等待 Provider 模型探测，不能作为基础 readiness。

## 决策

### 服务连接与所有权

- daemon 的 `--service` 在持有数据目录实例锁、完成服务组装之后，原子发布 `service-connection.json`。它包含 schema version、external 管理模式、稳定 server ID、当前 instance ID、监听地址、PID、启动时间及本机连接 token。
- Unix 临时文件从创建时即使用 0600；写入并同步后 rename。桌面主进程拒绝非普通文件、符号链接、过大的文件、其他用户所有或向其他用户开放的记录。token 不进入 renderer 的状态、URL 或日志。
- 连接记录留在服务的数据目录，服务停止后保留。陈旧记录只表示连接需要重试，永远不表示桌面可以取得进程所有权；客户端不删除记录。旧 `desktop-daemon.json` 不作为新契约输入。
- 桌面支持 `auto`、`desktop`、`external` 模式，通过 `settings.daemon.mode` 或 `AIT_DESKTOP_DAEMON_MODE` 选择；环境变量优先。默认 auto 在发现服务记录时连接服务，否则沿用自建子进程。明确 external 模式即使记录缺失也只重试连接。已选择 external 的 manager 在本次进程生命周期内不会因记录丢失回退到 spawn。
- 明确 desktop 模式与同目录服务记录冲突时拒绝启动。切换回桌面子进程应由维护者先停用服务并处理其记录，不能在连接失败时自动切换。
- 认证握手必须匹配记录中的 server ID 和 instance ID；PID 只作展示，不能用于接管或发信号。外部服务始终 `ownedByDesktop=false`、`desktopManaged=false`，额外的 `managementMode=external` 表达连接类型。

### 重启、退出与界面

- 桌面自建模式保留 terminate/spawn；外部服务模式调用已存在的认证 `server.restart.request`，等待记录和认证握手报告同一 server ID 的新 instance ID。该 RPC 在原 PID 中重建服务，systemd 继续是进程管理者；这不是替换 daemon 可执行文件的进程重启。
- 外部服务连接失败、重启失败或等待超时均不使用 spawn、SIGTERM 或 SIGKILL 作为恢复手段。普通停止 IPC 在该模式下只断开桌面连接。
- 桌面明确显示“已连接到系统服务”和“断开连接”；服务模式不显示用于桌面 spawn 的监听地址编辑器。打开、关闭、强杀和再次打开桌面不转移服务所有权。
- 外部服务仍通过本机 transport bridge 使用 token；重新启动桌面及服务重启时更新本机连接，即使同一主机已有其他连接也不能跳过更新。

### Readiness 与 Provider

认证 hello / server_info 继续作为连接 readiness，不调用模型目录检查。API 的 daemon status 和 diagnostics 改为读取非阻塞的 `provider.snapshot.get.request` 当前投影，复用 Provider 已有的后台 discovery、去重、超时和推送机制。

现有 `available: bool` 字段保持兼容：冷加载时 `available=false`，并返回安全的“Provider discovery in progress”说明；完成后返回实际可用性。显式模型/availability 查询仍可等待对应探测。daemon 可连接不代表模型、PTY 或模型对话已经验证成功。

## 后果与范围

服务配置使用明确的 PATH 或已有 `AIT_SERVER_*_BIN` 程序路径覆盖，不依赖 KDE 登录 shell。提供 [systemd unit](../../../assets/systemd/ait-daemon.service)，不包含伪造记录或延长模型探测等待的脚本。

独立中心授权与 HostLink 仍由现有 `daemon run --headless` 实现。本改动不改变它的授权/租约归属。SSH/TCP 服务可在无人登录时工作；云端 Relay 需要另外完成机器授权与 headless 配置。认证连接、模型目录、真实 PTY、真实 Agent 和两端界面分别验收。

运维步骤见 [daemon 文档](../../operations/daemon.md)。
