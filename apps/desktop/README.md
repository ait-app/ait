# Ait Desktop

Electron desktop app for Ait. Release versions follow the root workspace release.
Nightly builds display `YYYY-MM-DD-<8-character hash>` using the UTC build date.
Git installs display `git-YYYY-MM-DD-<8-character hash>` using the source commit date,
including when building a checkout at a release tag. Desktop and daemon share this identity.
Installer and updater metadata use a date-based SemVer (`2026.10.25-gabcdef01` for nightly,
`2026.10.25-git.gabcdef01` for Git), without the workspace release number.
`AIT_BUILD_VERSION` supplies an explicit build identity; official releases set `RELEASE_TAG`.
Builds without Git metadata must supply one of these variables.

Run `npm run verify:release` at the repository root to check that desktop, mobile,
Web, local package manifests and lockfiles match the Rust workspace release.
The display name is Ait. Release builds use Ait's `dev.ait.desktop` application ID.
Ait uses its own profile directories and the `ait://` link protocol.

Official releases build this app for Linux x64, macOS arm64 and Windows x64.
Linux and macOS bundle only `resources/bin/daemon`; Windows ships without a daemon.
No legacy daemon, worker or CLI shim is shipped. See [release operations](../../docs/operations/releasing.md).

macOS checks for updates through AIT Server at `https://aitapp.cn/api/v1/updates/macos/`.
The server routes versioned downloads to COS or GitHub by region. Stable and beta retain
their existing channels; Linux and Windows use the GitHub provider. See
[the update source decision](../../docs/decisions/clients/adr-165-macos-ait-server-updates.md).

从仓库根目录运行 `npm run build:dmg` 生成包含 Rust daemon 的本地 macOS 安装包。
正式签名、公证及产物路径见 [Apple 构建说明](../../docs/operations/apple-builds.md)。

从仓库根目录运行 `npm run build:windows` 构建 Windows x64 版本，输出到
`apps/desktop/release/Ait-<version>-windows-x64-setup.exe` 和
`apps/desktop/release/Ait-<version>-windows-x64.zip`。ZIP 解压后运行 `Ait.exe`。
在 macOS/Linux 上交叉构建时仅输出未签名 ZIP，不依赖 Wine 或 Intel NSIS 工具；
EXE 的 Ait 图标与版本资源由 JavaScript 写入。安装包和代码签名请在 Windows 上构建。
Windows 包不包含 Rust daemon，不构建 Rust，不自动创建或启动默认本地主机，也不展示
内置 daemon 管理入口；通过账号、TCP 或 SSH 连接远端主机。账号与桌面功能继续使用
Electron 主进程。该命令不发布产物；未配置 Windows 代码签名证书时生成未签名构建。
参见 [Windows 客户端边界](../../docs/decisions/clients/adr-184-windows-desktop-without-daemon.md)。
Release CI 和相关 PR 共用 [Windows 工作流](../../.github/workflows/windows-desktop.yml)，
在 Windows runner 上构建并运行 `node apps/desktop/e2e/windows-startup.e2e.mjs`，
验证打包后的客户端可在没有 daemon 的情况下进入欢迎页。PR 仅上传 CI 产物。

### Settings regression against Rust

With the desktop development server running, run:

```sh
EXPO_DEV_URL=http://localhost:8082 npm run test:e2e:settings-rust --workspace=@ait/desktop
```

The test launches a separate Electron profile and Rust data directory, visits all 19 settings pages,
checks project details, Skills and Agent profile editors, saves host and terminal configuration,
and opens the native Provider settings. It removes its temporary data when finished.
The daemon binary defaults to `target/debug/daemon`; set `AIT_SERVER_BIN` to use another build.

### Server listen settings

Settings → Host → Overview → Daemon edits the built-in daemon's IP and port.
The fields save on blur or Enter; the current listener appears in the existing Status row.
The desktop saves `settings.daemon.listen` in Electron's `userData/desktop-settings.json`
and reads it when starting the daemon. Saving does not interrupt the current daemon.
The hint below Listen address directs users to Restart daemon below. For the local built-in
daemon, that action relaunches the owned process with the saved settings and updates its connection
to the new listener; remote hosts retain their service restart RPC.
The default is `127.0.0.1:0`; port `0` selects an available port. IPv4, IPv6, wildcard
and LAN addresses are supported. `AIT_SERVER_LISTEN` overrides the saved setting.

With the development server running, `npm run test:e2e:server-listen --workspace=@ait/desktop`
checks the form, persisted configuration and connection recovery across three desktop launches.
See [the implementation report](../../docs/reports/clients/desktop-server-listen.md) for validation.

### Remote Workspace browser

The built-in browser connects directly for the local daemon's Workspaces. For all other
Workspaces, every destination goes through the corresponding daemon, including public,
LAN, link-local and loopback addresses. DNS resolution also happens on that daemon.
Online Hub/relay, direct TCP and SSH connections use authenticated daemon transports;
remote profiles never fall back to client networking. Update both Electron and the remote
daemon. Browser profiles are isolated by Host and connection target.

After building `target/debug/daemon` and desktop main, run the focused network test:

```sh
env -u ELECTRON_RUN_AS_NODE npm exec --workspace=@ait/desktop -- electron e2e/browser-relay-network.e2e.mjs
```

It uses temporary profiles, a real daemon and a local relay fixture to verify Chromium
proxy selection, authenticated relay and TCP forwarding, a 200 KB response and reconnection.
See [ADR-186](../../docs/decisions/clients/adr-186-workspace-browser-egress.md).

### Coexistence with Paseo

Ait uses its own Electron profile and `ait://` links. Desktop overrides now use `AIT_*`
(for example `AIT_ELECTRON_USER_DATA_DIR`, `AIT_ELECTRON_FLAGS`, `AIT_TEST_APP_NAME`,
`AIT_DISABLE_SINGLE_INSTANCE_LOCK`, `AIT_DESKTOP_WINDOW_CONTROLS`, `AIT_DEBUG` and
`AIT_SHELL_ENV_TIMEOUT_MS`); the old Paseo variables are ignored. The developer launcher
uses `.tmp/ait/` and the web build uses `AIT_WEB_PLATFORM=electron`.

Remote SSH connects to the remote Ait Rust daemon at loopback port 7316 by default.
Use `ssh://user@host:22?daemonPort=7316` to specify ports, and enter the daemon Bearer
token in the separate token field. Existing SSH connections without a token must be added
again. The desktop's embedded local daemon can still use an automatically allocated port.

Skills install into `ait-<name>` directories and only Ait-marked directories are managed.
Paseo skills and `paseo-auto-stash:` entries are retained. See
[ADR-056](../../docs/decisions/clients/adr-056-paseo-coexistence.md) for upgrade behavior.
