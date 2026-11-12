---
id: ADR-223
status: accepted
date: 2026-11-08
amends:
  - workspace/adr-215-sandboxed-worktree-isolation.md
paths:
  - crates/provider
  - crates/provider-native
  - crates/provider-claude
  - crates/provider-pi
  - crates/provider-codex
---

# ADR-223：Linux 沙箱化 Worktree

- 关系：扩展 [ADR-215](adr-215-sandboxed-worktree-isolation.md)，复用 Workspace 记录、写入策略及 Provider 启动接口。
- OMP 修订：[ADR-248](../providers/adr-248-omp-paseo-native-rpc.md) 第 11 条把 `extensions`、`tools`、
  `natives`、`python-env`、`profiles` 加入初始化为空目录的条目，`.yml` 与其他 YAML 一样初始化为 `{}`，
  并先创建受保护条目缺失的父目录。

## 背景

macOS 使用 Seatbelt 限制 Agent 进程树的文件写入。Linux 需要对应实现，同时保留原生认证、会话历史、网络和 Git commit。

## 决策

Linux 的 `provider::local::sandbox::linux` 使用系统 `/usr/bin/bwrap`（bubblewrap），不增加 Rust 依赖或特权 daemon。要求 bubblewrap 0.8.0 或更新版本（支持 `--disable-userns`），且主机允许非特权 user namespace。deb、rpm 和 Arch 包将其声明为必装运行时依赖；AppImage、tar.gz 和独立 daemon 归档由用户自行安装。创建和 feature 协商支持 Linux；feature 表示已实现后端，不保证主机已安装或允许运行 bubblewrap。缺少程序、挂载准备失败或内核拒绝 namespace 时拒绝启动，不重试为普通进程。

命令建立独立 user、PID、IPC 和 mount namespace，移除 capabilities 并禁止子进程新建 user namespace。根文件系统递归只读挂载，再按父目录优先顺序绑定策略允许的目录，最后把受保护路径重新绑定为只读。受保护路径在可写树内的祖先目录也绑定为挂载点，阻止通过重命名祖先移走保护后重建同名配置。沙箱使用自己的 `/proc` 和 `/dev`，保留网络 namespace、环境变量、读取权限和路径布局；`--new-session` 脱离启动者的控制终端，阻止经 `/dev/tty` 注入输入；`--die-with-parent` 约束进程生命周期。

Pi 正常关闭时，通过 `/proc` 中监控进程的子进程关系和 `NSpid` 找到 namespace 内 PID 2 的原生进程，只向它发送 SIGTERM，保留 bubblewrap monitor 和 PID 1 reaper，等待原生清理。无法定位或超时后仍由既有强制终止路径回收整个沙箱；不会先杀 monitor 导致原生清理被 SIGKILL 中断。

可写范围复用 worktree、公共与私有 Git 目录、当前 Provider 状态目录和 `$TMPDIR`，加上 Linux 的 `/tmp` 和 `/var/tmp`。这两个临时目录仍与宿主共享。认证文件仍位于原生状态目录，可以读取和原子刷新；环境变量中的 key 保持原有传递方式。

### 尚不存在的路径

Bind mount 只能保护已经存在的对象。启动前创建缺失的 Provider 状态目录，并初始化缺失的受保护条目：`hooks`、`plugins`、`bin`、`shims` 为空目录，JSON/JSONC/YAML 配置为 `{}`，其余配置为空文件；新文件权限为 0600。已有内容不覆盖，受保护条目为符号链接（包括悬空链接）、特殊文件或具有多个硬链接的普通文件时拒绝启动，避免通过可写别名绕过配置保护。这些初始化会保留在宿主，包括原本不存在的 Git `config.worktree`。随后只读挂载这些条目，因此既禁止直接修改，也禁止删除、替换和创建同名配置。不会使用忽略缺失路径的 `--ro-bind-try`。

私有 Git 目录的 `commondir` 指针同样只读，防止通过改写它扩大后续会话的允许范围。若指针原本缺失，初始化为 `.\n`，保留“公共目录就是当前私有 Git 目录”的语义，再挂载只读；已有指针保持原内容。此规则也保护删除和重命名操作。

### Claude 的原子配置更新

Linux bind mount 无法表达 macOS 的 `~/.claude.json*` 可写前缀，单文件挂载也不允许原子替换。受限 Claude 会话显式使用 `CLAUDE_CONFIG_DIR`：用户已配置时保持原值，否则设为原生状态目录 `~/.claude`。首次启动时，如果 `~/.claude/.claude.json` 不存在，把已有 `~/.claude.json` 复制进去作为初始全局设置，之后不覆盖。认证文件、会话历史仍使用原来的 `~/.claude`，不复制认证目录。

初始化先写入同目录的私有临时文件并同步，再用 `persist_noclobber` 发布完整内容，避免并发首次启动读取半份设置。仅覆盖 HOME 时，推导出的目录仍执行这一步迁移；只有显式 `CLAUDE_CONFIG_DIR` 才跳过迁移。

受限会话的全局设置更新保存在 `~/.claude/.claude.json`，不自动同步回普通会话的 `~/.claude.json`。用户若希望两种会话共用该全局设置，可为两者显式配置同一个 `CLAUDE_CONFIG_DIR`。其他可写前缀如果不处于已允许的目录内，启动失败，不放宽整个父目录。

### 原生沙箱和边界

沿用外层隔离会话的 Provider 配置：Codex 使用 `danger-full-access`，Claude 关闭内置 Bash sandbox，避免嵌套 namespace 与禁用 user namespace 冲突。审批策略保持原状，写入边界由外层 bubblewrap 提供。

这仍是写入限制，不是秘密隔离或离线容器。可读取的凭据、环境变量和网络仍可被 Agent 子进程使用；共享 Git metadata、Provider 状态、临时目录及可代执行操作的宿主 Unix socket 服务沿用 ADR-215 的限制。Provider 辅助探测和普通 Workspace 不在此隔离范围内。`~/.cargo`、`~/.npm` 等未放行缓存仍只读。

只读挂载约束访问路径，不能隔离已共享的 inode。容器实测：启动前在可写 worktree 内建立外部普通文件的硬链接，写该别名仍会改变外部文件。当前只检查策略直接列出的受保护普通文件的链接数，不扫描全部可写树或受保护目录的后代；这些位置事先存在的跨边界硬链接仍属于未解决的限制。沙箱内新建跨挂载硬链接被拒绝。需要消除此限制时，应另行设计 inode 级写入约束或不共享 inode 的工作区，不能据此宣称任意文件布局下完整隔离。

## 验证

Linux 测试运行真实 bubblewrap 和 shell，覆盖 worktree 写入、linked worktree Git commit、凭据读取和原子刷新、环境变量继承、宿主 loopback 网络、临时文件、符号链接越界，以及 home、兄弟目录、Git 和 Provider 配置的写入拒绝。另测 Claude 全局设置原子替换、初始化不覆盖已有配置和悬空链接失败。运行环境必须安装 bubblewrap 并允许 namespace；环境不满足时测试失败，不把跳过当作通过。

[容器验证报告](../../archive/reports/providers/linux-sandbox-container-2026-11-08.md)记录 Ubuntu、Debian 的真实隔离探针、控制终端对照、Pi 关闭回归及上述硬链接限制。

## 参考

- [bubblewrap 官方选项文档](https://github.com/containers/bubblewrap/blob/main/bwrap.xml)：只读绑定、namespace 和进程生命周期选项。

- [bubblewrap 0.8.0 发布说明](https://github.com/containers/bubblewrap/releases/tag/v0.8.0)：引入 `--disable-userns`。
