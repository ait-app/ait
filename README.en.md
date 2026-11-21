# Ait

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo-dark.svg" />
  <img src="assets/brand/logo.svg" alt="Ait logo" width="96" height="96" />
</picture>

**Manage multiple coding agents from one interface.**

[简体中文](README.md) · English

Ait supports coding agents such as Codex, Claude Code, OpenCode, DeepSeek Harness, Antigravity, Pi, Oh My Pi, and MiniMax Code.
You can run several agents side by side on your computer, review files and Git changes, and use terminals. You can also connect from a phone or another computer to check progress, send messages, and handle approvals.
Ait provides a central relay service: when your devices are on different networks, sign in to the same account to reach your online work computer, with no public IP or router port forwarding needed on the work computer.

[Download](https://github.com/ait-app/ait/releases) · [Changelog](CHANGELOG.md)

## What you can do

- **Agent chat**: Send tasks, follow replies, tool calls, and run status, and handle permission approvals.
- **Projects and files**: Add projects, create Git worktrees, browse files, review code changes, and use terminals.
- **Central relay and remote access**: Through the central relay, reach the computer running your agents from another computer or an Android or iOS device across networks, to continue conversations, view files, and use terminals. Browsers can also connect to the daemon directly.
- **Agent settings**: Choose the models and permission options an agent offers, and resume existing sessions. Available options depend on which agents are installed and their versions.

Agents run on the computer you connect to. Using local agents does not require an Ait account;
models and accounts come from each agent's own configuration.

## Getting started

### On your computer

1. Download the desktop installer for your system from [GitHub Releases](https://github.com/ait-app/ait/releases): macOS Apple Silicon, Linux x86_64, or Windows x64.
2. On the macOS/Linux work computer, install the coding agents you need and complete their authentication so that each runs on its own.
3. Launch Ait, choose the local or a remote host, add a project, open a workspace, and pick an available agent to start a task.

The macOS and Linux desktop installers bundle the daemon, which the app starts automatically. The Windows desktop app is a client that connects to remote hosts and does not run agents locally.
The Linux deb, rpm, and Arch packages depend on `bubblewrap >= 0.8.0` automatically; AppImage and tar.gz users must install it themselves and allow unprivileged user namespaces before using sandboxed worktrees.

Arch Linux users can also install from the AUR: [`ait-bin`](https://aur.archlinux.org/packages/ait-bin) is the prebuilt release package, and [`ait-git`](https://aur.archlinux.org/packages/ait-git) builds from `main` locally. Pick one (for example `paru -S ait-bin`).

### Connecting from other devices

1. In the desktop app on the work computer, open **Online Service** and sign in through the browser (email, Google, or WeChat). The entry is on the welcome page, under **Settings → App → Online Service**, or under **Add Host → Online Service**.
2. Sign in to the same account on another computer or in the Android or iOS app, choose the online host, and connect through the central relay.

The app uses `https://aitapp.cn/api` as the online service address by default.
Both sides only need to reach the online service; they do not have to share a LAN, and the work computer needs no open inbound ports. The relay forwards the connection, while agents and project files stay on the work computer.
After sign-in, the macOS/Linux desktop app authorizes the bundled daemon by default, and the daemon keeps itself online and renews its lease. Signing out of the client does not revoke the machine's authorization.
If you previously stopped sync explicitly, reauthorize it in the host connection settings. For remote access, the work computer and its daemon must keep running.

Android and iOS connect to computers as clients and do not run a daemon on the phone.
You can also sign in to the same account in the [web client](https://aitapp.cn/) and connect to online hosts through the central relay.
For sign-in, sign-out, and credential recovery, see [Unified sign-in](docs/operations/authing-client-login.md). Online features use the Hub; file downloads use a separate relay channel.

### Running the daemon standalone on Linux/macOS

Servers without a desktop environment can use the standalone daemon archive from Releases (executable `aitd`), or build from source:

```sh
cargo build --locked --release -p daemon --bin daemon
./target/release/daemon login
./target/release/daemon run
```

Install and authenticate the agents you need on the host first. On first sign-in, follow the terminal prompts to complete web authorization; after that the daemon renews its lease and reconnects on its own.
The default data directory is `~/.ait` (if it does not exist, an existing `~/.ait-server-desktop` or `~/.ait-server` is used, in that order), and the daemon listens on `127.0.0.1:7316` by default; a local API token is generated and saved automatically.
For long-running use, hand it to systemd or launchd. For downloads, credential recovery, and authorization while running, see the [headless daemon guide](docs/operations/headless-daemon.md).

## Local development

Install Git, Node.js 26, npm, CMake, a system C/C++ toolchain, and the Rust toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml).
The repository provides an optional Nix development environment: run `nix develop` to enter a shell with Rust, Node.js, and the build tools.

From the repository root, run:

```bash
npm ci
npm run dev:desktop
```

The dev entry point builds the shared packages, the Rust daemon, and the Electron main process, then starts the UI and the desktop app.

To run the daemon on its own:

```bash
export AIT_SERVER_TOKEN="$(openssl rand -hex 32)"
cargo run -p daemon --bin daemon -- --listen 127.0.0.1:7316
```

When connecting a client, enter the host address, port `7316`, and the value of `AIT_SERVER_TOKEN` above.
`127.0.0.1` accepts local connections only.

For mobile and web development, see the [client development guide](apps/mobile/README.md);
for configuration, architecture, and release material, see the [documentation index](docs/README.md).
Contributors should follow the [repository guidelines](AGENTS.md) and the [Rust style guide](docs/policy/rust.md); for the list of CI checks, see the [CI configuration](.github/workflows/ci.yml).

Most project documentation is written in Simplified Chinese.

## License and acknowledgements

Ait is licensed under the [Apache License 2.0](LICENSE). The client was originally based on [Paseo](paseo/README.md) and has been developed further since;
for its origin, the scope of changes, and copyright notices, see the [Paseo origin notes](paseo/README.md) and [NOTICE](third-party/paseo/NOTICE).
Third-party code keeps its own licenses and copyright notices.
