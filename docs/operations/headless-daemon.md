# Linux 无人值守 daemon

此功能需要配套 [ait-server PR #9](https://github.com/ait-app/ait-server/pull/9) 已部署，且中心开启
`DEVICE_AUTH_ENABLED` 并配置独立签名/回执密钥。生产中心固定为
`https://dash.ait-app.com:8443/api`，不能修改。安装与 systemd 管理由既有脚本负责。

## 登录与运行

以运行服务的相同账号和数据目录完成一次授权。默认数据目录沿用 `~/.ait-server`，可通过
`--data-dir` 或 `AIT_SERVER_DATA_DIR` 指定。login/logout 之前先用既有脚本停止同目录 daemon。

```sh
daemon login --name build-linux
```

终端显示固定网页地址和验证码。在任意设备的浏览器登录原有账号，输入验证码，核对机器并确认。
可以新建 Host，也可以显式选择已有未绑定 Host；不会按名字合并机器。

也可在网页 Host 详情签发一次性接入 JWT，通过 stdin 输入：

```sh
daemon login --token-stdin --name build-linux
```

stdin 读取到 EOF。脚本可以把已有秘密输入流连接到该命令。也支持 `--token '<JWT>'`，参数可能进入
shell 历史或进程列表，脚本优先使用 stdin。两种 token 参数互斥；错误 token 不触发网页登录。

服务仍需要本地 API 的 `AIT_SERVER_TOKEN`，由安装脚本的受保护环境配置提供，长度/字符规则与
[普通 daemon](daemon.md) 相同。这是本地连接凭据，不是网页接入 JWT 或机器 access JWT。
既有脚本的启动命令改为：

```sh
daemon run --headless
```

默认监听 `127.0.0.1:7316`，无需显示器、桌面应用或持续的 SSH 会话。没有 `service install` 子命令。
旧的直接 `daemon` 启动和 `daemon run` 保留客户端管理模式；headless 只在显式传入参数时开启。
现有 listen/config/log-level/web-origin 参数继续可用，不提供中心参数。

## 恢复、查看与退出

```sh
daemon status --json
```

status 不要求本地 API token，不创建文件，不争用运行中服务的凭据锁。输出稳定 ID、是否存在
凭据以及带时间戳的非秘密 runtime 缓存快照。缓存不是即时在线证明；鉴权后的本地
`GET /api/relay/control` 和网页 Host 在线状态可查询实际 Relay 状态。

授权成功后保存 `<data-dir>/device/.env`，权限 `0600`，目录 `0700`。文件是私有凭据 snapshot，
不要编辑、共享、复制到另一安装或提交到 Git。access JWT 只存内存，刷新 token 自动轮换。
Web/enrollment 的中断可重新执行不带 token 的 `daemon login` 恢复原请求；尚未完成的 refresh
由 `daemon run --headless` 恢复。保留 pending UUID，不能手动改 ID 后重试旧 refresh token。
中心恢复窗口为 10 分钟，初次领取也受原凭据期限约束；无法恢复时需要重新授权。

网络故障自动退避重连。JWT 或租约到期先关闭远程通道；运行冲突等待旧租约释放并每 30 秒重试；失效授权和持久化失败停止远程
管理，本地服务继续供诊断。排除冲突/磁盘错误后用既有脚本重启。重新授权需要先停止该服务。
停止服务保留 grant，释放本次节点会话；它不会撤销长期机器授权或删除本机任务数据。

```sh
# 先通过既有脚本停止 daemon，再执行：
daemon logout
```

logout 尝试中心撤销并清除本地凭据。中心不可达、或初次领取结果未知时，CLI 提示在网页 Host
详情补做撤销。网页撤销会关闭现有 Relay 控制和数据连接；普通网页登录退出不撤销机器授权。
桌面/手机连接到这种 Host 时只读取同步状态，不能用客户端的“停止同步”覆盖其授权。

协议、数据库差异与部署验收见 [设计与实施记录](../plans/linux-headless-device-auth.md)。
