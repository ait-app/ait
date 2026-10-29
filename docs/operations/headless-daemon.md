# Linux/macOS 无人值守 daemon

此功能需要配套 [ait-server PR #9](https://github.com/ait-app/ait-server/pull/9) 已部署，且中心开启
`DEVICE_AUTH_ENABLED` 并配置独立签名/回执密钥。生产中心固定为
`https://aitapp.cn/api`，不能修改。Linux 的 systemd、macOS 的 launchd 安装与管理由外部脚本负责。

## 获取 daemon

从 0.1.8 起，每个正式版和 beta 的 GitHub Release 附带独立归档
`Ait-VERSION-daemon-linux-x64.tar.gz` 与 `Ait-VERSION-daemon-macos-arm64.tar.gz`，正式版同步到
国内 COS 镜像；内含可执行文件 `ait-daemon`。下载、校验、安装与 systemd/launchd 示例见官网
[独立 daemon](https://ait-app.com/zh/docs/daemon/) 文档，发布方式见 [发布说明](releasing.md)。
[nightly 预发布](https://github.com/ait-app/ait/releases/tag/nightly) 也附带同名格式的
`Ait-<构建标识>-daemon-<平台>.tar.gz`，对应 main 最新一次成功构建。macOS nightly 归档未公证，
用浏览器下载后需在解压目录执行 `xattr -d com.apple.quarantine ait-daemon` 才能运行。

下文命令中的 `daemon` 对应归档中的 `ait-daemon` 或源码构建的 `target/release/daemon`。

## 登录与运行

以运行服务的相同账号和数据目录完成一次授权。默认数据目录沿用 `~/.ait-server`，可通过
`--data-dir` 或 `AIT_SERVER_DATA_DIR` 指定。login/logout 之前先用既有脚本停止同目录 daemon。

```sh
daemon login --name build-linux
```

`daemon login` 优先读取本机保存的机器凭据并向中心续期，成功时无需再次打开浏览器。
只有没有凭据、凭据已过期或中心明确拒绝授权时，才发起新的网页授权。网络或服务故障
保留凭据及恢复请求，可稍后重试；不会因为暂时断网而清除登录。

需要主动重新授权时执行 `daemon login --force`；配合 `--token-stdin` 可替换旧的未完成授权。
该选项放弃旧请求的恢复并开始新授权，不删除机器身份或工作区数据。

需要网页授权时，终端显示固定网页地址和验证码。在任意设备的浏览器登录原有账号，输入验证码，核对机器并确认。
可以新建 Host，也可以显式选择已有未绑定 Host；不会按名字合并机器。

也可在网页 Host 详情签发一次性接入 JWT，通过 stdin 输入：

```sh
daemon login --token-stdin --name build-linux
```

stdin 读取到 EOF。脚本可以把已有秘密输入流连接到该命令。也支持 `--token '<JWT>'`，参数可能进入
shell 历史或进程列表，脚本优先使用 stdin。两种 token 参数互斥；错误 token 不触发网页登录。

首次 `run` 在未设置 `AIT_SERVER_TOKEN` 时自动生成 `<data-dir>/local-api.token`，
权限为 `0600`，后续启动复用。显式环境变量优先，但不会覆盖文件；长度/字符规则与
[普通 daemon](daemon.md) 相同。这是本地连接凭据，不是网页接入 JWT 或机器 access JWT。
既有脚本的启动命令改为：

```sh
daemon run
```

未显式断开时，再次启动 `daemon run` 会读取已保存的 refresh token，恢复未完成的刷新并自动连接，
不需要再次登录。若授权确实失效，状态变为 `reauthorization_required`；可以通过下文 API 写入新授权，
或先停止服务再执行 `daemon login`。后台运行不会自动打开交互登录。

默认监听 `127.0.0.1:7316`，无需显示器、桌面应用或持续的 SSH 会话。没有 `service install` 子命令。
直接 `daemon`、`daemon run` 与 Desktop 启动使用同一流程。`--headless` 已删除，既有脚本需移除该参数。
未登录时本地 API 正常运行并报告 `unauthorized`，授权写入后自动发布。
现有 listen/config/log-level/web-origin 参数继续可用，不提供中心参数。

## 运行中的授权与统一断开

以下 HTTP 接口都要求 `Authorization: Bearer <本地 API token>`，沿用 daemon 的 Host/Origin 校验：

| 接口                               | 行为                                                    |
| ---------------------------------- | ------------------------------------------------------- |
| `GET /api/device/authorization`    | 身份、管理阶段/绑定、Hub 在线状态、Relay 状态；不含秘密 |
| `PUT /api/device/authorization`    | 持久化授权交接，202 表示已接受，随后异步领取凭据并发布  |
| `DELETE /api/device/authorization` | 断开 Hub/Relay、停止续租并清除机器凭据，204             |

授权请求有三种形态，`request_id` 必须为每次新交接生成唯一 UUID，重试使用原 ID：

```json
{ "kind": "enrollment", "token": "<一次性接入 JWT>", "request_id": "<UUID>" }
```

已在中心批准的 Web/device flow 可交接
`{kind:"device_code", device_code, user_code, request_id, expires_at, interval}`。
`expires_at` 是原授权截止时间（RFC 3339），不能延长；`interval` 为中心返回的轮询秒数。
客户端只发起 `/v1/auth/device/authorize` 并经用户账号批准 `/v1/auth/device/approve`，不领取机器 refresh token；
daemon 保存 pending 后领取，网络中断可使用原 ID 恢复。

已有机器凭据可交接
`{kind:"credential", request_id, refresh_token, refresh_expires_at, binding}`，
其中 `binding` 包含 `node_id/host_id/server_id/grant_id`，`server_id` 必须与当前 daemon 匹配。
refresh 凭据交接后只能由该 daemon 轮换。HTTP/API 返回值及状态查询均不回传这些秘密。

WebSocket/Hub 客户端协商 `publication.status.request`、`publication.authorize.request`、
`publication.stop.request` 后可执行同样的操作；authorize 参数是
`{authorization: <上述请求>, mode?: "explicit" | "initial"}`。默认显式登录；`initial` 只允许自动初始化从未授权、
从未断开的 daemon，由 daemon 原子检查条件，避免自动授权覆盖并发断开。成功响应是脱敏的 publication 状态。
授权写入后自动连接，不需要 start/resume。运行中的 daemon 拒绝直接 `PUT /api/hub/control` 或 Relay 票据。

`DELETE /api/hub/control`、`DELETE /api/relay/control` 和兼容的 `relay.stop.request` 也执行相同的完整断开。
登录产生一份机器授权与节点会话，由 Hub/Relay 共同使用；两条链路的协议票据分别领取，不可互换。
断开会取消取票、注册、续租、刷新，关闭两条链路及数据通道，清除已保存和内存中的机器凭据、
enrollment/device code 与待恢复请求。仅保留机器身份和不含秘密的 `disconnected` 标记；重启和
Desktop 自动同步不会重新授权。重新连接需要显式授权，或停止 daemon 后执行 `daemon login`。
本地 API token、任务和工作区数据保留。Desktop 退出/账号退出本身不触发这个断开操作。

断开时尝试关闭中心会话及撤销机器授权，每项最多等 5 秒；中心不可达不阻止本地凭据清除。
若中心撤销未完成，旧会话按租约失效，必要时在 Host 页面撤销；尚未领取的授权不会再继续领取。
持久化失败返回错误并在当前进程内保持断开，修复磁盘后需重试断开，不能将失败视为已永久清除。

## 恢复、查看与退出

```sh
daemon status --json
```

status 不要求本地 API token，不创建文件，不争用运行中服务的凭据锁。输出稳定 ID、是否存在
凭据以及带时间戳的非秘密 runtime 缓存快照。缓存不是即时在线证明；鉴权后的本地
`GET /api/device/authorization` 查看实时 Hub、Relay 及管理状态，`GET /api/hub/control` 查看 Hub。

授权成功后保存 `<data-dir>/device/.env`，权限 `0600`，目录 `0700`。文件是私有凭据 snapshot，
不要编辑、共享、复制到另一安装或提交到 Git。access JWT 只存内存，刷新 token 自动轮换。
Web/enrollment 的中断可重新执行不带 token 的 `daemon login` 恢复原请求；尚未完成的 refresh
由 `daemon run` 恢复。保留 pending UUID，不能手动改 ID 后重试旧 refresh token。
中心回执恢复窗口为 10 分钟；已消费的 device code 即使超过批准期限，仍使用原请求 ID 向中心尝试恢复。
初次领取受原凭据期限约束，无法恢复时需要重新授权。

网络故障自动指数退避重连：注册/续租/刷新与 Relay 的退避等待上限为 30 秒，Hub 重新取票和授权领取为 60 秒。
Web/device code 轮询还会遵守中心指定的最小间隔。
连接任务启动不清零退避，握手在线才恢复初始间隔；Hub 物理续连另有 250 毫秒到 8 秒的退避。
短期 access JWT 被拒绝时关闭远程通道并刷新一次，新 JWT 或 refresh 仍被拒绝才要求重新登录。
JWT 或租约到期先关闭远程通道；运行冲突等待旧租约释放并每 30 秒重试；失效授权和持久化失败停止远程
管理，本地服务继续供诊断。排除冲突/磁盘错误后用既有脚本重启。运行中的服务也可通过授权 API 接收新凭据。
停止服务保留 grant，释放本次节点会话；它不会撤销长期机器授权或删除本机任务数据。

```sh
# 先通过既有脚本停止 daemon，再执行：
daemon logout
```

logout 尝试中心撤销并清除本地凭据。中心不可达、或初次领取结果未知时，CLI 提示在网页 Host
详情补做撤销。网页撤销会关闭现有 Relay 控制和数据连接；普通网页登录退出不撤销机器授权。
桌面/手机可以完整断开 Host，也可以显式重新授权；节点租约始终由 daemon 自行维护。

协议、数据库差异与部署验收见 [设计与实施记录](../plans/linux-headless-device-auth.md)。

## 机器名称与平台

首次登录默认使用系统 hostname，读取失败回退为 `ait-daemon`。后续登录（包括 `--force`）
保留本地名称，只有显式 `--name` 才修改授权请求的名称。恢复未完成请求时保持原请求内容。
网页和在线发现统一使用中心 Host 名称；已有 Host 的名称由网页修改，不会被重启或本地
`--name` 覆盖。系统 hostname 单独记录为主机元数据，稳定身份仍是 server_id。
Linux 上报 `linux`，macOS 上报 `darwin`，与桌面端一致。

此版本需要先部署支持注册字段 `hostname` 和 `ait-daemon` client_id 的中心版本；中心
只接受 `ait-daemon`，旧版 daemon 需要升级。
