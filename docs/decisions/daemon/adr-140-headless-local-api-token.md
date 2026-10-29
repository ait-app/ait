# ADR-140：Headless daemon 自动保存本地 API 凭据

> 启动与控制边界已由 [ADR-201](adr-201-daemon-owned-publication.md) 修订：所有 daemon 自主发布，外部可暂停 Hub。

状态：已接受

`daemon login` 保存中心机器授权；本地 API 的凭据与之独立。要求用户在登录后额外设置环境变量，导致 `run --headless` 无法直接启动。

Linux/macOS 的 `run --headless` 未提供 `AIT_SERVER_TOKEN` 时，在数据目录生成 `local-api.token`，随后启动复用。令牌含 244 位随机熵，文件权限 0600，使用同步后的临时文件以不覆盖方式原子发布；并发启动采用同一份已发布凭据。拒绝符号链接、非普通文件、过宽权限或损坏内容，不静默更换已有令牌。

显式 `AIT_SERVER_TOKEN` 优先，仅覆盖本次启动，不改写已保存的令牌。普通及桌面启动仍要求环境注入；不从 TOML 读取秘密。退出中心登录不删除本地 API 凭据。日志、status 和错误信息不输出令牌。
