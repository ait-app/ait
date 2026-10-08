# Ait Desktop

Electron desktop app for Ait. Versions follow the root workspace release.

Run `npm run verify:release` at the repository root to check that desktop, mobile,
Web, local package manifests and lockfiles match the Rust workspace release.
The display name is Ait. Release builds use Ait's `dev.ait.desktop` application ID.
Ait uses its own profile directories and the `ait://` link protocol.

Official releases build this app for Linux x64 and macOS arm64. The only bundled
daemon resource is `resources/bin/daemon`; no legacy daemon, worker or CLI shim
is shipped. See [release operations](../../docs/operations/releasing.md).

从仓库根目录运行 `npm run build:dmg` 生成包含 Rust daemon 的本地 macOS 安装包。
正式签名、公证及产物路径见 [Apple 构建说明](../../docs/operations/apple-builds.md)。

### Settings regression against Rust

With the desktop development server running, run:

```sh
EXPO_DEV_URL=http://localhost:8082 npm run test:e2e:settings-rust --workspace=@ait/desktop
```

The test launches a separate Electron profile and Rust data directory, visits all 20 settings pages,
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
