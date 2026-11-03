# ADR-203：MCP 的可选 Unix socket 监听

- 状态：接受
- 日期：2026-10-30

本机客户端需要通过 socket 文件访问 Ait MCP。daemon 在原 TCP 监听之外提供可选 Unix domain
socket，在其上复用 ADR-148 的无状态 HTTP MCP，而不新增 JSON-RPC 分帧或业务分发实现。
这保留现有工具、鉴权、请求预算和停止语义，但客户端必须支持 HTTP over Unix socket，
仅接受普通 HTTP URL 的客户端需要桥接。

- 启动配置为 `--mcp-socket`、`AIT_SERVER_MCP_SOCKET` 或 TOML `mcp_socket`，依次优先。
  默认关闭；路径必须是绝对文件路径，父目录须存在，非 Unix 平台拒绝启用。
- socket 仅提供 `/mcp/agents`，HTTP authority 使用 `localhost`。复用 Bearer、Origin、
  查询参数禁用和 `mcp.enabled` 规则；原 daemon 凭证与 MCP 专用凭证均可用。
  不把 HTTP 主机名校验绑定到 TCP 的地址或端口，也不在 socket 上公开其他 daemon 路由。
- socket 文件为 `0600`。绑定前只回收已确认无监听者且文件身份未变的 socket；其他类型和
  活跃监听者拒绝覆盖。停止时只删除本次绑定的 device/inode，保护已被替换的路径。
- 监听由 daemon 宿主拥有，工具处理归 `api`，两条入口共享同一能力实例、预算和 draining 状态。
  任一监听退出都会进入统一停止流程，在同一关闭期限内等待两条入口和已接纳任务。
- Provider 的自动注入继续使用实际 TCP HTTP 地址和会话私有 MCP 凭证；新增 socket 不改变
  原生客户端的连接配置，也不将 socket 文件路径伪装成标准 HTTP URL。

用法见 [MCP 手册](../../operations/mcp.md#unix-socket-监听)。
