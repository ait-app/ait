# ADR-188：网页客户端以独立镜像部署在网站根路径

状态：已实现。日期：2026-10-24。修订 [ADR-132](adr-132-web-console-client.md)。

## 背景

ADR-132 的浏览器客户端由 ait-server 拉取本仓库、以 `AIT_WEB_BASE_PATH=/client` 导出并打进
网站 Nginx 镜像，设置侧栏另有仅托管浏览器可见的「管理」tab，用 iframe 内嵌同源 `/manage`。
客户端版本因此需在两个仓库间锁定，子路径构建还要改写 PWA manifest 与图标地址。
ait-server 已将管理控制台迁至 `/dash/*`（`/manage` 仅 301 到 `/dash/`），不再构建客户端，
并把自身不处理的路径全部转发给独立的客户端容器。

## 决策

- 客户端占用网站根路径 `/`。`apps/mobile/scripts/export-web-console.mjs`
  （`npm run build:web-console --workspace=@ait/mobile`）以 `AIT_WEB_PLATFORM=web`、
  `EXPO_PUBLIC_AIT_CONSOLE=1` 导出单页 Web（`web.output: "single"`），不设置
  `AIT_WEB_BASE_PATH`，输出到被忽略的 `apps/mobile/web-console/dist/`，
  并写入记录版本与提交的 `client-build.json`。
- `apps/mobile/web-console/` 提供基于 `nginx:1.28-alpine` 的静态镜像，只复制预先导出的文件，
  镜像内不运行 Node 构建。未命中文件的路径回退 `index.html` 以支持 `/settings`、`/h/...`、
  `/auth/callback` 等深链；`/_expo/static/` 与 `/assets/` 的带哈希文件长期缓存且缺失时返回 404，
  其他文件每次重新验证；提供 `/healthz`。镜像不代理 `/api`，由 ait-server 网关负责。
- 同源约定不变：网关保留 `/api/*`、`/dash/*` 及 `/login`、`/logout`、`/auth/device`、
  `/auth/client`、`/auth/complete`；客户端继续读取同源 `ait.access-token`，
  未登录跳转 `/login?client=1`，登录后整页回到 `/`。
- 删除设置侧栏的「管理」tab、对应 `management` 设置分区与仅供它使用的 `consoleOnly` 过滤；
  管理功能只在 ait-server 的 `/dash`，客户端不再内嵌。
- CI 在客户端相关路径变化时导出、构建并启动镜像，检查根路径、深链、静态资源与健康检查；
  发布标签将同一镜像推送到 `ghcr.io/ait-app/ait-web-client:<tag>` 与 `sha-<commit>`，
  附 OCI source、revision、version 标签。部署与网关路由仍归 ait-server。

ADR-132 中 `/client` 子路径构建、由 ait-server 构建与托管客户端、以及 `/manage` 内嵌管理页的决策
由本 ADR 取代；浏览器账号适配、中继和下载等其余决策保持不变。

## 后果

客户端随本仓库的发布标签产出可直接部署的镜像，不再需要在 ait-server 锁定客户端提交。
旧 `/client/*` 地址由网关跳转到对应根路径。浏览器中失去内嵌管理入口，需要时直接访问 `/dash`。
发布镜像独立于桌面安装包任务，同一标签重跑会覆盖镜像标签。
