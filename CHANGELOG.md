# Ait changelog

## 0.2.10 - 2026-11-12

- Test feature: Workspace notifications wake the Workspace Bot. When an Agent it started finishes, fails or waits for permission, or one of its schedule runs settles, the Bot answers in the conversation that asked for the work, such as the Telegram chat. Updates for one conversation are grouped, limited to 12 wake turns per hour, and the Bot may stay silent. Bot settings add a Notifications card to choose between the Bot's own work (default) and every Workspace notification, with a recent list showing why any were skipped. Paused Bots and Workspaces without a Bot make no model calls.
- Keep Claude Code background tasks marked as background across restarts and task updates, and stop their output from opening phantom new turns. Cancelling a Claude session still ends its background tasks.
- Show a quick merge button beside the pull request or merge request action whenever a direct merge is allowed, for both GitHub and GitLab; squash, merge and rebase stay in the menu. A pull request already in GitHub's merge queue shows its queue status.
- Task overview polish: long permission summaries wrap inside the card and stop after three lines, the details panel matches the cards' styling and no longer repeats a project name that equals the workspace name, and the details header lines up with the toolbar.
- The sidebar no longer shows a workspace as still being created after the Host has reported it finished.
- Web and desktop tooltips are removed as soon as they close instead of waiting for an exit animation, which could leave small grey shapes behind. Tooltips on the web no longer fade.
- Test feature: Telegram Bot replies include the quoted passage or the replied-to message, or its media caption, so the agent sees what a reply refers to.
- Stable releases attach the Google-signed Android APK: the release check now reads every certificate `apksigner` reports instead of matching its signer labels.
- Native apps display host CPU and memory statistics correctly with React Native’s cancellation signals.

- Native apps keep the current page and task details when folding or unfolding a device, keep conversations readable beside side panels, prevent messages from showing through the chat header, and keep dropdown options readable and clear of the keyboard.

- Build: the enabled React Compiler now skips `packages/protocol` sources (schemas and generated validators contain no React components), so desktop startup and protocol hot reload no longer pay for compiling their large control-flow graphs. Windows and POSIX paths are both normalized; application components are still compiled.

- Add `npm run package:daemon -- linux|mac VERSION BINARY DEST [--dev LABEL]` as an alias for the existing `node scripts/release-assets.mjs daemon` standalone-daemon archive command; no new packaging path.

- Test feature: a Workspace Bot now reports back on its own. When an agent it started finishes, fails or waits for permission, or a schedule it created runs, the Bot wakes and replies in the conversation that started the work, so a task handed over in Telegram reports back in Telegram. Updates arriving close together are combined into one turn, a conversation wakes at most 12 times an hour, the Bot is not woken again for an agent it already checked in that conversation, and it stays quiet when there is nothing worth saying. A wake-up cut short by a daemon restart runs again after the restart. In the Bot tab these turns show what happened, with links to the agent or schedule, instead of a user message. Settings can widen the wake-ups to every agent, schedule and terminal event in the workspace, which uses more model calls, and list recent notifications, including ones that were not delivered. The Host daemon must be upgraded. Once a workspace has had any activity on the new daemon, older daemons generally cannot load its Bot data, so downgrading means removing the Bot data.

- OMP sessions can now assign its native model roles (director, worker and the rest of the installed profile) from a catalog read out of OMP's own metadata instead of a fixed table. Assignments ride a private `--config` overlay, an assignment change relaunches the process, and inheriting a role writes no file. A new assignment the live catalog cannot resolve is rejected rather than stored.

- Desktop passes provider launch overrides (`AIT_SERVER_CODEX_BIN`, `AIT_SERVER_CLAUDE_BIN`, `AIT_SERVER_ANTIGRAVITY_BIN`, `AIT_SERVER_OPENCODE_BIN`, `AIT_SERVER_DEEPSEEK_HARNESS_BIN`, `AIT_SERVER_PI_BIN`, `AIT_SERVER_OMP_BIN`, `AIT_SERVER_MINIMAX_CODE_BIN`, plus `AIT_SERVER_DEEPSEEK_HARNESS_PROFILE` and `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT`) through to its built-in daemon instead of stripping them. All other `AIT_SERVER_*` settings stay isolated to the desktop runtime.
- Bold text in Chinese, Japanese and Korean now renders as bold when the `**` sits next to punctuation, such as `**…。**后文` or `种**“…”**只`, instead of showing the asterisks.
- A command approval in a conversation shows the command and its working directory instead of the raw request, and the primary choice is highlighted. Requests that carry arguments Ait does not recognize still show the full input.
- Error text is readable in light themes; it used a pale red meant for dark surfaces. History rows keep their title in a narrow pane, such as a foldable's inner screen or a small window, and the History empty state and the turn duration label follow the app language.

## 0.2.9 - 2026-11-12

- Rewrite the Oh My Pi (OMP) Provider to run one `omp --mode rpc-ui` process per session and use OMP's native RPC directly, matching real OMP 18.18.8. Tool approvals offer OMP's scope options, questions use OMP's form, and `/compact`, `/autocompact`, `/handoff`, `/steer` and `/follow-up` are built in. History, session listing, background subagent transcripts and usage all come from OMP. Resuming a session whose saved model is no longer available now fails instead of switching models.
- Remove the `aitd omp-acp` subcommand along with the OMP ACP bridge. External ACP clients that started OMP through it need another entry point.
- Sandboxed workspaces can no longer modify OMP's config, models, MCP and `.env` files, its extensions, hooks, tools and plugins, or its native addon.
- Stable releases attach the Google-signed Android APK again: the release check now accepts the key-rotation signatures Google Play uses for this app.

## 0.2.8 - 2026-11-11

- Show a Host resource monitor at the bottom of the sidebar: a CPU and memory summary that opens to Host status, recent history, disk capacity and upload and download speeds. It follows the current workspace's Host or can be pinned to one, keeps the last sample time when a Host is offline, and stops refreshing while hidden. The Host daemon must be upgraded; workspace and project share guests see a permission notice instead.
- Act on Task overview cards directly: approve, stop, mark as reviewed, archive or open the conversation without opening the details, and answer questions through the existing form. The toolbar fits on one row, desktop columns can collapse, phones use status tabs with counts, and failed tasks move to To review.
- Settle observed OpenCode subagents from OpenCode's saved final state, so finished or failed children are no longer shown as canceled when the parent closes. While such a child is running, the subagent list refreshes every 15 seconds to catch missed completion events.
- Test feature: Telegram Bot channels react with 👀 when a message is accepted, edit a single reply as the answer grows instead of sending separate messages, and render Markdown through Telegram's HTML mode. A warning appears when group access is on but Telegram Privacy Mode keeps ordinary mentions from reaching the bot.
- Show pull requests for workspace branches that were pushed under a different remote name, such as `git push -u origin HEAD:refs/heads/fix/...`, in the sidebar and PR panel, including merge actions. Branches pushed without an upstream are still not recognized.
- Test feature: QQ Bot channels with group chats enabled now also receive ordinary group messages when the group grants full message access, not only messages that mention the bot. They pass through the same sender allowlist and pairing.
- Stable releases now attach a Google-signed Android APK, `Ait-VERSION-android.apk`, to the GitHub Release. It shares the Google Play signing key, so it installs over the Play version and vice versa; 0.1.x GitHub APKs used a different key and must be uninstalled first.
- Add a per-Host usage ledger. Each daemon records its provider calls and, where the provider reports them, token usage per model request in `agents/usage.sqlite3`, and the Usage dialog has a new History tab (24 hours, 7 days or 30 days) that adds all connected Hosts together and shows the totals by provider and model or by day; unreachable Hosts are listed separately, and a Host's settings page shows its 30-day totals. Providers that do not report usage (OpenCode, ACP, DeepSeek, and summary calls) show call counts only, and figures that may be incomplete are marked "≥". Call details are kept for 90 days and daily totals indefinitely; prompts, outputs, file paths and credentials are never recorded and the data stays on the Host. The Host daemon must be upgraded for its history to appear.

## 0.2.7 - 2026-11-11

- Add a Task overview, opened from the sidebar or the command center, that sorts agents from every connected Host into Needs attention, Running, To review and Done. Filter by Host or project, search titles, projects and models, and from a task's details answer permission requests, stop it, mark it as reviewed, archive it or open its conversation. Subtasks are grouped under their parent, and actions are unavailable while the Host is offline.
- New schedules start from a template gallery: test run and repair, PR risk review, topic briefing, weekly workspace recap and documentation check. A template fills in the name, prompt and timing, which stay editable, and the form is split into "Content & timing" and "Execution & limits". Templates are written in English, Chinese, Japanese and Korean; other interface languages show the English text.
- Test feature: add a Webhook channel to a Workspace Bot. Ait Hub issues an HTTPS address and a secret that is shown only once; other systems post text to the address, then fetch the reply (optionally waiting up to 30 seconds) or receive it at a callback URL. The Host must be connected to an Ait Hub that supports webhooks, otherwise creation shows the reason it failed. Only the final text reply is returned, without progress updates or approvals in the conversation.
- After a Webhook channel has been created, older daemons cannot load the Bot catalog, even if the channel was deleted, and clients that have not been upgraded cannot read a channel list containing it.

## 0.2.6 - 2026-11-10

- Keep a record of each workspace script run, with bounded output, starting commit and dirty state, duration, exit code and result. Records are held in memory and are lost when the daemon restarts or the workspace is archived. New MCP tools `get_workspace_script_run` and `wait_for_workspace_script` read them.
- Scripts that declare `"acceptsArgs": true` in `ait.json` can take arguments, passed as positional parameters. Archiving a workspace that removes an Ait-owned worktree now runs `teardown` first; a failing teardown logs a warning and the worktree is still removed.
- OpenCode, Pi and DSH native agents can use Ait orchestration tools and accept user-configured MCP servers (HTTP and absolute-path stdio). An `ait` entry in a user's own Pi `mcp.json` takes precedence over the injected one, including a Bot's scoped credentials.
- Remove the legacy `paseo` tool name aliases. Commands still receive both `AIT_*` and `PASEO_*` environment variables.
- Test feature: Bots keep every user-visible assistant message from a turn, while provider-internal records stay filtered out.
- Fix chat timeline gaps and duplicates after switching tabs, sessions freezing after an event from another epoch, and thinking indicators that stayed on after a turn ended.
- Fix duplicated text and attachments after a failed send, attachments lost when editing a queued message, and uploaded files dropped when a multi-file upload failed partway. Slash commands that run immediately now clear the input only after they succeed.
- Pages in the workspace embedded browser can no longer use the camera or microphone or read the clipboard. Writing to the clipboard and fullscreen still work, and dictation in the main window is unaffected.
- Desktop: a temporarily unavailable keychain no longer signs you out permanently, a damaged config file no longer breaks reads, and an adopted daemon restarts after it exits.
- Shortcut recording now follows the typed key, so shortcuts work on non-QWERTY layouts. Shortcuts already recorded on those layouts need to be recorded again.
- Opening Settings no longer checks for updates every time: a channel checked within the last 30 minutes is skipped, and manual checks are unaffected.
- Show a loading indicator during manual sidebar refresh, add pull-to-refresh to the native sidebar list, and fully translate schedule text in Russian, Arabic, Spanish, French, Brazilian Portuguese, Japanese and Korean.

## 0.2.5 - 2026-11-09

- Open a workspace in its own window from the workspace menus, with independent navigation, tabs and layout. If two windows edit the same draft at once, the last save wins.
- Pin sidebar project groups to the top and keep pinned sessions at the front of their group.
- Add sandboxed worktrees on Linux through bubblewrap 0.8.0 or later, restricting writes while reads and network access stay open. The deb, rpm and Arch packages declare the dependency; AppImage, tar.gz and standalone daemon installs need bubblewrap installed separately, and the host must allow unprivileged user namespaces. Sandboxed Claude sessions keep their global settings in `~/.claude/.claude.json`, copied from `~/.claude.json` on first use and not synced back.
- Test feature: connect Workspace Bots from the Bot tab with Feishu or WeChat QR scan, QQ QR scan or manual entry, guided Telegram pairing, and iMessage through Photon Beta. Secrets entered here are stored in plain text in the daemon data directory with owner-only file permissions, and iMessage content passes through Photon's cloud service.
- After a WeChat, QQ or iMessage channel has been created, older daemons cannot load the Bot catalog and may fail to start, even if the channel was deleted. Roll back only by restoring a `<data>/workspace-bots` backup taken before the channel was created, or by giving up that Bot data. Clients that have not been upgraded cannot read a channel list containing these types.

## 0.2.4 - 2026-11-09

- Keep chat history and subscriptions loading while a slow provider usage lookup is still running. The host daemon must be upgraded for connected clients to benefit.
- Allow one Cron tab per workspace. Launching Cron again reveals the open tab and keeps its current view, and saved layouts with several Cron tabs keep only the first. Bot tabs follow the same one-per-workspace rule.
- Simplify the Cron list: the header is gone, a single New button sits at the end of the list, and the list shows at most 20 crons per page with Previous and Next.

## 0.2.3 - 2026-11-08

- Bot chat now matches the session page, with separate prompt and reply messages and Stop and New conversation in the composer. The Bot's internal session opens read-only from a "Bot session" subagent pill.
- Simplify Bot permissions to one mode per provider plus "Not allowed". A saved profile with several modes for a provider keeps only the first, and a Bot-created schedule stored with another mode is refused at run time until it is changed. Bots can no longer create workspaces or worktrees through Ait MCP; worktrees they already recorded stay in scope.
- Share a project from a chosen Host when it exists on several Hosts.
- Show the five most recently active workspaces per sidebar project, with "Show more" revealing up to ten more at a time.
- Share Host usage data across the daemon with a global cache and background refresh, and add a "Force refresh" button to AIT usage.
- Restore correct colors in retained UI, including chat message content, after the system theme changes.
- Keep Pi sessions and native history consistent after a prompt admission failure, without automatically resending input.

## 0.2.2 - 2026-11-08

- Fix tiny terminal glyphs under Electron 45.2.3: the WebGL renderer now draws regular, bold and italic text with the configured font size and monospace family.
- Show "renewal pending" for Claude usage when the stored access token has expired but a valid refresh token remains, instead of asking to run `claude /login`; the CLI renews the token on its next run.

## 0.2.1 - 2026-11-08

- Add Pi slash commands and extension notifications through its native provider integration.
- Consolidate online sessions with Hub leases, credit-based flow control and on-demand bulk download connections. New clients require a center that declares the complete Hub feature set; older centers now produce an explicit compatibility error. Connections to older Hosts remain supported through their advertised capabilities.
- Speed up desktop restarts by avoiding unchanged observed-Agent registry writes, resolving login-shell environment asynchronously and prestarting the built-in daemon while the interface loads.
- Prefer the macOS Keychain for Claude usage credentials in the default configuration directory, with the credentials file as fallback; custom CLAUDE_CONFIG_DIR behavior is unchanged.
- Sort sidebar projects by their most recent session activity and redirect signed-out browser clients to account login.
- Upgrade Electron to 45.2.3 and make completed Git commands tolerate bounded trailing stderr diagnostics.

## 0.2.0 - 2026-11-07

- Align daemon command help, errors, desktop version checks and service examples with the packaged `aitd` executable name; standalone archive filenames stay unchanged.
- Use `~/.ait` for new installations. Without an explicit data directory, reuse the first existing directory in this order: `~/.ait`, `~/.ait-server-desktop`, `~/.ait-server`; existing directories are not moved or renamed. If both legacy directories exist, pin `--data-dir` to keep using a specific one.
- Separate manual development builds from CI checks. PR and main CI run checks; development installers use the manual Dev Build workflow, while version tags continue to produce official Release Drafts.

## 0.1.31 - 2026-11-06

- Let a Workspace Bot manage its Workspace within the permissions you grant: it can start and prompt Agents, manage schedules, and optionally run scripts, terminals and worktrees through scoped Ait MCP credentials, with a per-provider mode cap. The setup wizard and settings add permission groups and mode caps.
- Existing Bots upgrade with the default groups but an empty mode cap, so configure modes in Bot settings before the Bot can start Agents.
- Bot models from providers other than Codex and Claude degrade by capability (instructions prepended to each input, default native mode, no preapproved tools) and settings describe the difference. Agents that can run commands can still read local daemon credentials, as the settings warning explains.
- Add opt-in sandboxed worktrees on macOS through MCP, restricting supported Provider processes to writes in the worktree and required runtime paths while leaving reads and network access unchanged.
- Fail closed for unsupported platforms and Providers; document the write-confinement limitations and the loss of enforcement when downgrading to older daemons.

## 0.1.30 - 2026-11-05

- Rebuilt on 2026-11-06 with the matching Workspace Bot v2 client and live GitHub clone progress.
- Add the Workspace Bot tab for `workspace-bots-v2` hosts: one Bot tab per Workspace with a setup wizard, chat, and settings for model, pause and resume, memory, Telegram and Feishu channels, and removal. It remains behind the experimental features setting, and guests get read-only access.
- Migrate existing Bot tabs automatically: saved per-Bot tabs collapse into a single Bot tab per pane.
- Breaking: Workspace Bots are now one manager Bot per Workspace, addressed by `workspaceId` through the `workspace-bots-v2` interface (replacing `workspace-bots-v1`; `bot.create`, `bot.list`, `bot.archive` and `bot.permission.resolve` are removed). Existing Bots, Channels, and history from earlier versions are discarded at startup.
- Store Bot memory under the daemon data directory instead of the workspace; archiving a workspace deletes it, removing a Bot keeps it, and a missing workspace directory pauses the Bot until resumed.
- Rename the packaged daemon executable to `aitd`, both in desktop resources and standalone archives. Update existing service commands and scripts when upgrading from `ait-daemon`; archive filenames stay the same.
- Hide cached workspaces for unavailable online hosts in the sidebar while preserving pinned host filters.
- Consolidate CI jobs and keep nightly publication gated on all configured checks.

## 0.1.29 - 2026-11-05

- Improve connection recovery and release desktop transports when windows close or reload.
- Preserve drafts, queued messages, workspace creation intent, and host-specific session state across retries.
- Harden guest access, share revocation, workspace persistence, Git operations, and resource cleanup.
- Fix scheduling edge cases, bot queue recovery, native provider cancellation, and resumed session history.
- Bound large provider payloads and permission previews while preserving complete native approval input.
- Align Agent tool names with the ait namespace while retaining compatibility with older transcripts.
- Fix terminal keyboard handling, editor saves, downloads, and browser lifecycle issues.
- Restore iOS notification tap callbacks and keep host synchronization buttons stable during background refresh.

## 0.1.28 - 2026-11-04

- Apply Pi reasoning preferences before prompts and keep provider controls consistent with native sessions.
- Keep shared-resource sessions consistent across browser and desktop clients.
- Improve daemon recovery, host publication, guest access scopes, and shared-resource cleanup.
- Fix schedule editing, background-task access, sidebar press feedback, and transient notices.
- Synchronize iOS native versions and the local CocoaPods lockfile for subsequent Xcode Cloud builds.

## 0.1.25 - 2026-11-01

- Keep the existing daemon running when a readiness probe fails, and reconnect to the same process after recovery.
- Simplify schedules and shared-resource lists, improve agent history fallback, and clarify loading states.
- Keep sidebar rows stable when shortcut badges appear on hover.
- Use a gentler loading indicator while reconnecting to the online service.
- Clarify Chinese wording for host sharing, local connections, and session states.

## 0.1.24 - 2026-11-01

- Share workspaces, projects, or entire hosts with group members through Hub guest sessions, with scoped access, revocation, and recovery.
- Fix shared project/workspace directories remaining in a loading state, and organize sharing, host navigation, and account login on the home page.
- Restore native and nested subagent lists; improve OpenCode subagent state, history recovery, and live updates.
- Handle Codex child-agent completion notifications without interrupting the parent session.
- Correct provider execution ownership and public API contracts, improve Pi availability/version diagnostics, and mark Claude sessions launched by Ait.
- Fix macOS sidebar and home-page buttons being blocked by window drag regions.

## 0.1.23 - 2026-10-30

- Use Hub for online business sessions across clients and hosts, while keeping dedicated file downloads on Relay. Hub support is now required for online sessions.
- Automatically connect the daemon's orchestration MCP to supported agent sessions, and add Unix socket listeners for MCP.
- Unify workspace ordering in the sidebar and remove drag sorting; improve internal transport URL compatibility in browsers.
- Hide Claude's injected subagent hand-back records and display returned reports as messages; discover DSH through npm configuration.
- Connect Vim write commands to file saving and filter silent PCM frames in realtime voice input and output.
- Simplify Rust asynchronous interfaces with async-trait.

## 0.1.22 - 2026-10-29

- Let the daemon own machine authorization, node registration and independent Hub/Relay connections, so publishing continues when the desktop account client exits. Direct daemon startup now uses the same lifecycle; remove the former `--headless` flag from startup scripts.
- Reuse the daemon's private local API token across restarts, while honoring an explicit environment override, and simplify connection status and disconnect controls.
- Automatically distribute bundled orchestration skills to supported coding agents.
- Fix OMP interaction blocking and multiline input, and preserve MiniMax model selection when creating sessions.
- Add browser confirmation to account deletion and synchronize authorization regression tests with the daemon-owned lifecycle.

## 0.1.21 - 2026-10-28

- Add MiniMax Code through its native ACP interface.
- Add account deletion and a unified account center entry to online-service settings.
- Allow Codex child agents to send messages back to their parent session.

## 0.1.20 - 2026-10-27

- Preserve Codex native protocol behavior across session configuration, approvals, commands, background events, subagents, terminal state and rewind.
- Fix OpenCode cancellation blocking and reconcile stale OpenCode/OMP running states; normalize OMP output and group thinking by turn.
- Improve Antigravity native-session resume and history projection, including tool, subagent and interrupted-turn handling.
- Restore provider model-discovery snapshots and clear stale workspace-sidebar highlights.
- Keep workspace.list metadata within the relay record limit and retain useful DeepSeek Host transport-failure evidence.
- Fix iOS background notification registration and notification-center delivery in source; mobile binaries are not included in this desktop release.

## 0.1.19 - 2026-10-26

- Project large Codex native histories incrementally page by page, preserving load errors and preventing failed or cancelled history loads from admitting new messages.
- Record realtime voice close sources and media-exit reasons to make provider shutdowns and transport failures easier to diagnose.
- Extract embedded Android R8 mappings directly to disk and validate them as a stream, avoiding the 64 MiB output limit in Play release collection.

## 0.1.18 - 2026-10-25

- Relay Codex native realtime voice through the daemon and keep voice transcripts, turn membership and restored session history consistent.
- Start voice sessions from new-workspace drafts and handle audio interruption, playback and capability selection across clients.
- Bridge the official Oh My Pi ACP interface and generic plugins, including session configuration, permissions, history and subagent events.
- Unify the online-service settings layout and improve translated speed, Host and usage controls.
- Give nightly and local/AUR source builds date-and-commit identities while keeping stable release versions unchanged.

## 0.1.17 - 2026-10-24

- Merge consecutive Thinking blocks into a single presentation group.
- Scope workspace creation keys to the submitted intent, keeping retries stable without conflating different requests.
- Isolate model-selector provider loading state from unrelated provider queries.
- Discover Oh My Pi model capabilities and improve agent control icons, labels and localization.
- Extend Codex provider requests to a 20-second timeout.
- Remove Pi's ineffective synthetic permission mode while preserving compatibility with saved defaults.
- Package the hosted browser client as a standalone image served at the site root, and remove its management tab.

## 0.1.16 - 2026-10-24

- Add Windows x64 installers and ZIP packages for connecting to remote hosts, without a bundled daemon.
- Route workspace browser traffic through the selected workspace daemon and improve remote browser profile handling.
- Show the account login name and expiry date, and keep saved hosts scoped to the signed-in account.
- Handle missing upstream branches when creating workspaces and preserve actionable creation errors.
- Preserve native provider failure details and guard Antigravity background calls with cached authentication.
- Render Claude background task notifications as tool rows, improve Chinese translations, and adapt branding to light and dark themes.

## 0.1.15 - 2026-10-22

- Add the Oh My Pi native RPC provider, including session history, approvals, streaming and subagent events.
- Add resumable account Hub connections, Host event publication and desktop account access, with legacy relay fallback when the service lacks Hub support.
- Keep uploaded filenames in Chinese and other Unicode scripts intact.
- Follow OpenCode sessions after session_move and support Claude native history files up to 128 MiB.
- Size the desktop model picker to its provider rows, fix web tooltip trigger measurement and keep the first message below the floating header.

## 0.1.14 - 2026-10-21

- Restore interrupted Codex turns after out-of-memory restarts and preserve concrete upstream failure details in diagnostics.
- Reduce OpenCode model-catalog cold starts, probe readiness before starting turns, and restore stalled sessions without duplicating work.
- Preserve Pi project identity and user-image history when discovering, importing and resuming native sessions.
- Accept void DeepSeek Harness event acknowledgements and keep completion delivery consistent.
- Add daemon-backed native push delivery and local notification settings; iOS and Android store builds remain separate from this desktop release.
- Move provider behavior scenarios into their owning crate and keep daemon tests focused on host integration.

## 0.1.13 - 2026-10-20

- Keep dialogs, popovers and menus outside desktop window drag regions so their controls remain clickable.
- Queue relay event bursts behind renderer acknowledgements to preserve ordering and avoid flooding the desktop renderer.
- Import Pi native sessions and preserve compatible session history when resuming agent conversations.
- Align workspace splits with supported platforms, share native terminal input and keep bottom sheet actions visible.
- Correct native diff status labels and iOS build configuration; mobile store releases remain separate.
- Require an explicit workflow option to include Android APK builds in a desktop release.

## 0.1.12 - 2026-10-20

- Prevent native startup crashes by guarding xterm browser detection and installing navigator, crypto and screen orientation polyfills before other application modules load.
- Adopt the iOS Scene lifecycle for Xcode 27 builds, preserving app links, foreground recovery and paste input initialization.
- Remove the standalone GitHub Actions APK workflow; Google Play AAB and iOS TestFlight releases remain separate from desktop publishing.

## 0.1.11 - 2026-10-20

- Roll back the diagnostics and restart-recovery changes from PR #381 after an iOS startup regression. Restore the previous client error handling, SDK diagnostic logging, Codex failure classification and daemon session startup behavior.
- Remove global startup error collection, unified diagnostic copying/export, native Codex error details and automatic interrupted-turn recovery. Preserve later independent Markdown, panel editing, usage and setup observation improvements.
- This release distributes desktop clients and standalone daemons; the iOS startup fix requires a separately requested mobile build.

## 0.1.10 - 2026-10-20

- Unify Markdown rendering across clients, improve image and file link handling, and preserve chat search, code highlighting, copying and Mermaid diagrams.
- Improve long prompt editing, draft retention, review comments, mobile panels, workspace Scripts and Command Center ranking.
- Preserve complete chat history during catch-up, avoid unrelated cache invalidation, and improve wide-character terminal selection and scroll behavior.
- Keep account-scoped last successful usage values when refresh fails, and subscribe to detailed setup logs only while their workspace panel is visible.
- Fix Git diffs and discard for quoted paths and renamed files; explain schedule edits blocked by running tasks.
- Record background errors without disruptive toasts, open the Ait website by default in the browser pane, and add Linux window and closed-console fallbacks.
- Publish ait-bin automatically after stable desktop releases and allow longer Google Play submission queues.

## 0.1.9 - 2026-10-19

- Recover interrupted agent sessions, preserve OpenCode history and compaction across ACP versions, and provide copyable error diagnostics with original Codex failure details.
- Improve mobile keyboard, sidebar drag, terminal and split touch handling; add camera image attachments and reconnect known hosts earlier during startup.
- Preserve host and offline records across account channel failures, fix local daemon startup races, and keep long model names and toolbar modes readable.
- Remove legacy relay and workspace service proxy URLs, align file tab path actions, fully clean untracked files on workspace reset, and keep budgeted filesystem replies below transport limits.
- Add Linux deb and rpm installers, repair native package dependencies and removal hooks, and include standalone daemon archives in nightly builds.
- Route macOS automatic updates through aitapp.cn with single-range COS downloads; existing GitHub-fed clients migrate after installing this release.

## 0.1.8 - 2026-10-18

- Add Pi 1.2+ through its native RPC interface, including model discovery, native session resume, attachments, streamed responses and tool progress.
- Keep each attached image whole, enforce exact image budgets, and restore Codex image previews across devices and session restarts.
- Resume Claude sessions using their launch directory even after a shell command changes directory; identify the Codex app-server client as Ait.
- Keep the centered composer usable with a tall keyboard and limit desktop drag exclusions to window chrome and overlays.
- Publish standalone Linux x86_64 and macOS Apple Silicon daemon archives; sign and notarize the macOS binary and build Linux against glibc 2.28.

## 0.1.7 - 2026-10-18

- Keep routable account Hosts up to date through a dedicated status channel, preserve unavailable Host records, and show only online Hosts when choosing a connection.
- Reuse a negotiated persistent Relay data tunnel for Host visits when the server supports it, with independent streams and compatibility with older servers.
- Recover stalled Relay connections when the app returns to the foreground, and avoid unnecessary fallback probes when a direct connection is available.
- Pause or resume scheduled tasks directly from the global task list, including tasks attached to existing Agents, with duplicate-action protection while requests are pending.

## 0.1.6 - 2026-10-17

- Show startup connection failures and retry immediately while preserving background recovery and shared reconnect attempts.
- Toggle Codex between Normal and Fast directly on desktop and mobile, with one Fast option and compatibility for saved priority selections and older native catalogs.
- Show bounded OpenCode ACP error explanations with sensitive-field redaction, and recover turns when a model no longer supports the saved thinking level.
- Transfer large forked chat histories in bounded chunks and compact them on the first send with the selected model, keeping recent original messages and marking omitted context when compaction is unavailable.

## 0.1.5 - 2026-10-16

- Forward loopback browser requests through the authenticated relay to the remote Workspace Host, preserving URLs, HTTP(S)/WebSocket traffic, and reconnect behavior.
- Allow OpenCode ACP cold starts and initial catalog discovery to finish without the previous 30-second cutoff, while preserving normal control-request timeouts.
- Keep the composer usable during conversation rewind by closing the menu immediately while the request completes.
- Preserve per-file paths and diffs in Codex multi-file edit cards, and hide invalid open-file targets on older cached cards.
- Share one menu highlight between pointer and keyboard navigation, respecting pointer capabilities and disabled items.
- Close open Android panels before system back navigation and route completed system edge gestures through panel navigation.
- Preserve exact project configuration revision timestamps across JSON requests, avoiding false save conflicts when the file has not changed.

## 0.1.4 - 2026-10-15

- Show native context-compaction progress, completion, and failure across Codex, Claude, OpenCode, DeepSeek Harness, and Antigravity, keeping live updates and restored history consistent.
- Refresh the selected usage Host when connections change instead of retaining the connection state from the initial render.
- Match native mobile status-bar icons to the app theme.
- Use the default HTTPS port for official services and migrate saved legacy official endpoints while preserving custom service addresses.

## 0.1.3 - 2026-10-14

- Automatically approve Codex computer-use access requests in Auto-review and Full Access while preserving explicit tool overrides and Bot permission rules.
- Limit concurrent startup Timeline reads and wait for a real session Timeline before querying the chat outline, avoiding busy queues and missing-agent errors for draft tabs.
- Keep Codex model-catalog refresh timeouts separate from turn execution failures, and validate headless identity before starting background services so failed startup releases the instance lock.
- Add a daemon MCP endpoint for external clients to manage Agents, Workspaces, terminals, scripts, schedules, heartbeats, and permissions through the existing authenticated API.
- Discover DeepSeek Harness scenario modes from native plugin presets and keep permission presets independent from the selected scenario.
- Restore Antigravity account quota windows and DeepSeek Harness balances, including official DeepSeek API-key balances with their native currency and availability.
- Preserve the initial branch in cached Workspaces so reset remains available after reopening, and remove Edit Profiles from the new-tab menu.
- Hide expansion controls for empty tool details while keeping real tool output expandable.
- Update fork prompts to use Ait branding and remove obsolete help links.

## 0.1.2 - 2026-10-13

- Accept valid large image prompts within the shared input budget; preserve original image bytes unless the request exceeds its size limits, then compress or split readable images without replacing the saved originals.
- Support OpenCode 2.0.25 and preserve long shell and large file-edit permission requests through ACP.
- Show safe Provider error explanations and persist failure notices in conversation history.
- Restore question actions after five-second response timeouts so users can retry without losing their answers.
- Deny permission requests during automated Bot turns and report the denial instead of leaving the Bot waiting for interactive approval.
- Reuse Relay TLS sessions, reduce reconnect delay, and retain native mobile account connections for two background minutes before closing them.
- Answer terminal device-attribute queries before a client subscribes, avoiding delayed fish startup.
- Detach stale pull-request associations when resetting a Workspace, clarify scheduled-task system labels, and remove the Worktree setup reminder.
- Move blocking filesystem work off the async reactor and remove a forced panic during Workspace recovery.

## 0.1.1 - 2026-10-12

- Add Workspace-owned scheduled tasks, ordered task timelines, and a unified Host → Project → Workspace creation flow with a default of one run.
- Add experimental Workspace Bots behind a default-off setting, with persistent conversations, configuration and shared memory; support Telegram and Feishu channels in the backend.
- Support unattended Linux/macOS Hosts with persistent machine authorization, credential recovery and a generated local API token.
- Keep oversized Codex MCP results from closing sessions by displaying bounded previews while preserving full values in native transcripts.
- Keep GitHub clones connected during foreground verification and prevent duplicate output in batched Read cards.
- Add correlated Relay disconnect diagnostics, remove retired protocol methods, and simplify internal module organization.

## 0.1.0 - 2026-10-11

- Publish the existing 0.0.29 functionality as the 0.1.0 stable desktop release; no additional application behavior changes are introduced in this release.
- Synchronize Rust, npm workspace, lockfile, and local Arch source-package versions to 0.1.0, with fresh desktop installers and stable-channel updater metadata.

## 0.0.29 - 2026-10-11

- Stabilize OpenCode model catalogs, retain model-specific options, and confirm model, effort, and mode changes through native ACP responses and notifications.
- Require OpenCode 2.x version 2.0.26 or newer and show an upgrade diagnostic for earlier versions while retaining OpenCode 1.x support.
- Coordinate Ait-owned OpenCode ACP initialization to reduce concurrent cold-start database contention while keeping active sessions and prompts concurrent.
- Wait for shared execution capacity after admitting bounded RPCs, preserve handshake-specific event negotiation, and add actionable, bounded, redacted RPC and schema diagnostics.
- Remove avoidable desktop notification, deleted-route, and animation warnings, plus the obsolete daemon status hint.
- Migrate the official service default and saved legacy official endpoints to aitapp.cn while preserving credentials, expiry, preferred host selection, and custom service addresses.
- Run PR quality checks without desktop packaging, and keep both platform builds and nightly publication on main.

## 0.0.28 - 2026-10-11

- Preserve native usage and permission events, carry source timestamps, and accept older hosts' supported control-event formats.
- Restore startup and reconnect subscriptions using the current host's advertised capabilities, and queue workspace directory and label initialization when background tasks are busy.
- Release established timeline subscriptions before requesting replacements to avoid exhausting subscription slots during tab changes.
- Display persisted agent history before native replay, reconcile it in the background, and keep older history available through bounded cursor pages.
- Keep Changes refresh connected under contention by isolating foreground checkout work and waiting for output capacity while allowing small control responses to proceed.
- Support the hosted browser client with shared website login, persistent account sessions, online hosts, browser downloads, and an embedded management tab.
- Treat a moved ShipIt staging bundle as an unknown staged version instead of reporting a failed update diagnostic.
- Group metadata modules by capability, move Workspace lifecycle RPCs into metadata, and keep Agent Skills owned by the provider component.
- Refresh the product guide and keep changed-file UI lint independent of the repository-wide formatting check.

## 0.0.27 - 2026-10-11

- Keep the built-in daemon running after desktop quit when enabled, and reconnect to the same authenticated daemon on the next launch.
- Restore OpenCode ACP tool projections, configuration recovery, native failure reasons, and input order across live turns and history reloads.
- Keep session metadata separate from native routing aliases and reconcile native history before settling completed turns.
- Use browser-based unified sign-in, support phone-only accounts, and preserve relay credentials and online hosts across restarts.
- Import all online-service hosts after sign-in while retaining existing local connections and persistent host records.
- Load the target workspace immediately and synchronize the complete host directory in the background.
- Read timeline pages from a persistent projection index instead of scanning the full conversation history.
- Preapprove Codex computer-use tools in Auto-review while preserving explicit tool policy overrides.
- Decode Markdown file-link paths, improve unsupported-file previews, and keep terminal fonts monospace with the DOM renderer.
- Localize usage controls, remove obsolete help links and pairing RPCs, and reduce periodic daemon work.
- Repair CI quality gates and enforce repository-wide formatting alongside shared client, directory, and workspace regressions.

## 0.0.26 - 2026-10-10

- Prevent terminal crashes when wide characters are clipped by resizing or written into single-row or single-column screens.
- Accept normal startup request bursts with bounded per-group queues while keeping overload rejection and Ping responses available.
- Omit absent OpenCode default thinking options so provider snapshots and host diagnostics pass client validation.

## 0.0.25 - 2026-10-10

- Use the official OpenCode ACP interface for native questions, permissions, cancellation, and restored sessions.
- Show native account usage with pinned windows, used or remaining balances, host selection, and refresh controls.
- Offer Codex Normal, Fast, and Ultrafast speeds from the native model catalog and clear the previous speed when selecting Normal.
- Keep chat reading positions stable, improve search and selection copying, and preserve streamed Markdown block identities.
- Add configurable content width and improve image sizing, file links, Vue highlighting, and Astro parsing.
- Unify Explorer and workspace tabs and improve subagent panes, context details, and overlay interactions.
- Fix desktop window dragging, browser automation targets, and screenshot coordinates after zooming.
- Restore directory, tab, and subagent synchronization after reconnecting and stop retry loops after replica cache writes fail.
- Match OpenCode terminal colors to the desktop appearance and improve terminal editing shortcuts and external links.
- Consolidate summary contracts, isolate directory reads from execution budgets, and remove unused persistence watching.

## 0.0.24 - 2026-10-09

- Add OpenCode native session permissions with Allow, Ask, and Deny controls and distinct icons.
- Improve OpenCode v1/v2 compatibility, saved model recovery, permission denial handling, and session error diagnostics.
- Export bounded, redacted diagnostic evidence with native Harness logs as a shareable attachment.
- Hide uninstalled providers from model selectors while preserving loading and discovery errors.
- Retry failed draft session creation with updated settings and recognize PDFs consistently.
- Keep Codex asynchronous question answers in order after reloading conversation history.
- Explain Antigravity headless permission denials and native failures, and settle affected tools.
- Preserve Chinese IME input in iOS terminals; the native fix requires a separately rebuilt iOS app.
- Consolidate daemon persistence, shared domain values, server protocol, and filesystem capabilities.

## 0.0.23 - 2026-10-08

- Bring the 0.0.23 beta improvements to the stable desktop update channel.
- Reconnect desktop clients after failed local daemon transports and preserve whitespace in streamed Markdown.
- Reflect configured composer models and explain model switches deferred until the current turn finishes.
- Preview PDFs, open external files explicitly, and handle folder drops as directory attachments.
- Transfer large messages in bounded chunks and serialize file uploads with progress and acknowledgements.
- Synchronize the built-in daemon after sign-in and simplify host settings by removing the obsolete Pair device section.
- Fix workspace fork project resolution and allow workspace creation without an initial agent.
- Improve provider auxiliary generation, native session previews, model discovery, and permission compatibility.
- Publish main nightly installers with a commit hash and date, and provide downloadable desktop test packages for pull requests.

## 0.0.23-beta.1 - 2026-10-08

- Publish desktop beta installers and updater metadata on the beta channel, while keeping stable updates on the latest stable release.
- Reconnect desktop clients after a failed local daemon transport closes.
- Preserve Markdown whitespace across streamed message chunks.
- Reflect configured models in the composer and explain model switches deferred until the current turn finishes.
- Preview PDFs, open external files explicitly, and handle folder drops as directory attachments.
- Transfer large messages in bounded chunks and serialize file uploads with progress and acknowledgements.
- Synchronize the built-in desktop daemon after sign-in and simplify online host controls.
- Fix workspace fork project resolution and accept workspace creation without an initial agent.
- Improve provider auxiliary generation, native session previews, DeepSeek Harness model discovery, and permission compatibility.
- Align daemon services, file persistence, shared contracts, and RPC ownership across crates.
- Add an independent manual Google Play internal testing pipeline.

## 0.0.22 - 2026-10-07

- Import existing OpenCode and DeepSeek Harness sessions with native permissions, model settings, and resume state; expose OpenCode Build and Plan agents.
- Add Arch Linux local source and AUR binary package recipes.
- Restore live timeline delivery after the canonical method migration by adapting Rust timeline producer notifications at the protocol boundary.
- Reset an existing same-named origin branch to the local workspace reset commit with one force push, while detecting concurrent remote changes.
- Run independent agent sessions concurrently, keep operations ordered within each session, and discover providers in the background without blocking connection readiness.
- Add native Antigravity CLI support with streaming, approvals, cancellation, and restored conversations.
- Restore native DeepSeek Harness interaction, including permission changes, questions, and persistent session history.
- Improve OpenCode v2 discovery and settle completed turns from durable history.
- Preserve large tool results up to 768 KiB and paginate oversized timeline responses without losing complete entries.
- Keep restored conversation history separate from live cache overlays to avoid stale or duplicated timeline entries.
- Align the shared client, protocol, and daemon on canonical Ait method names and session events.

## 0.0.20 - 2026-10-06

- Add unified browser sign-in and registration on desktop and Android when the online service supports Authing, while keeping the existing Ait password sign-in option.
- Add iOS sign-in through the system authentication window, with callback validation, cancellation, and secure session storage.
- Show account expiration independently from session expiration; browser sign-in preserves online host discovery and per-host synchronization.
- Reset workspaces successfully when the original branch name already exists locally, preserving the renamed branch reference and protecting branches used by another worktree.

## 0.0.19 - 2026-10-05

- Reuse a stable daemon identity when host synchronization is restarted or taken over by another client; existing host bindings require the matching online-service update.
- Recover synchronization after a closed or expired registration by using a new registration ID on the next retry.
- Replace the inherited Paseo name in desktop bridge errors with Ait.
- Fix workspace reset failures when a saved path has a trailing separator and the requested path does not.

## 0.0.18 - 2026-10-05

- Keep Codex computer-use screenshots and embedded image metadata from exhausting timeline rows and hiding subsequent replies.
- Separate online-service sign-in from per-host synchronization, with independent host controls and relay leases.
- Add iOS account sign-in, online host discovery, relay connections, secure credential storage, and native downloads.
- Add workspace reset to the latest origin default branch and guide Git actions through commit, push, pull-request, and archive states.
- License Ait under Apache-2.0 and align branded copy and localized date tests.

## 0.0.17 - 2026-10-04

- Show OpenCode tool cards before streamed conclusions and rebuild previously misordered session history on refresh.
- Normalize nullable workspace and project fields to `null` in daemon responses, so directory updates clear stale client state consistently.
- Simplify Agent thinking filters, OpenCode event projection, and terminal service code; remove redundant clones and unreachable branches.

## 0.0.16 - 2026-10-04

- Push workspace and agent directory changes to connected clients, so Hosts and running sessions update promptly.
- Preserve terminal selections when a resize claims the same terminal dimensions.
- Show Codex model discovery and capacity errors while keeping hidden models excluded.
- Show Codex MCP tool names, arguments, results, and native errors without dumping the full event envelope.
- Stabilize agent creation under task-budget contention and isolate background Git fetch failures.
- Move Android APK publishing to a standalone manual workflow; version tags continue to publish desktop builds only.

## 0.0.15 - 2026-10-03

- Add native OpenCode sessions with model discovery, streaming, tool approvals, cancellation, and conversation restore.
- Add account sign-in, account host discovery, and reverse-relay connections on desktop and Android.
- Add optional Android APK publishing for ARM64 and ARMv7; tag releases build only Linux and macOS by default.
- Keep Hosts connected when diffs or directory listings exceed response limits, and improve large-diff loading.
- Show model-specific Codex reasoning options, including max and ultra when supported.
- Harden desktop navigation and connection retries; fix stale workspace notifications, literal Git paths, directory symlinks, and schedule updates.
- Handle empty repository history, oversized setup output, and offline speech capacity errors correctly.

## 0.0.14 - 2026-10-03

- Add theme-aware syntax highlighting to workspace, base, and commit diffs, including multi-line syntax and renamed files.
- Fix diff parsing when source code contains a `diff --git` string, avoiding phantom file entries.
- Align workspace sidebar change counts with the fetched origin base and clear stale counts after merge.
- Accept Codex max and ultra reasoning levels for new and resumed sessions.
- Show the running server version and update release, documentation, feedback, and community links to ait-app/ait.

## 0.0.13 - 2026-10-02

- Fix DeepSeek model discovery when optional descriptions or default reasoning metadata are absent, and accept null descriptions from older servers.
- Add a manual iOS TestFlight workflow for published stable releases, with duplicate-build checks and explicit retries.
- Configure the Ait Android submission profile for Google Play Internal testing and make legacy mobile release workflows manual.

## 0.0.12 - 2026-10-02

- Add DeepSeek Harness as a local ACP provider with model discovery, reasoning options, tool approvals, cancellation, and session restore.
- Refresh remote Git refs for active workspaces in the background and update checkout status after fetching.
- Restore workspace sidebar diff counts, pull/merge request status, CI results, and Git hover details.
- Clear completed and cancelled agent activity reliably while allowing new turns to show their running state.
- Complete localization of settings, provider usage, terminals, browser tools, and workspace navigation.
- Add an Ait iOS EAS release profile and remove the obsolete Paseo documentation link.

## 0.0.11 - 2026-09-30

- Fix live workspace and agent directory updates by translating Rust event names into the client protocol.
- Preserve directory subscription identifiers and synchronization metadata in update events.
- Complete the 0.0.10 release changes, including GitLab support and session/workspace compatibility improvements, after correcting the packaged-app startup failure.

## 0.0.10 - 2026-09-30

- Add GitLab.com and self-hosted GitLab support for merge requests, discussions, pipelines, and worktree checkout through glab.
- Improve project, workspace, and agent directory synchronization, pagination, and activity status.
- Align timeline display, conversation search, session restore, and approval waiting behavior with the client.
- Improve workspace creation with an initial agent, retry handling, pull-request checkout, and archive cleanup.
- Add terminal activity tracking and session events while preserving existing Ait authentication.

## 0.0.9 - 2026-09-28

- Add offline dictation and speech synthesis with automatic background model preparation; the first setup requires an internet connection.
- Fix file panels that keep loading by preserving request IDs across JSON and binary file responses.
- Support wider terminal panels and keep the terminal view mounted while reconnecting.
- Keep application-level terminal errors from disconnecting the Host, and improve desktop lifecycle diagnostics.

## 0.0.8 - 2026-09-28

- Search the whole conversation with match counts, Markdown-aware matching, and next/previous navigation across messages.
- Keep Codex sessions running after large generated images, and restore large image output from native history.
- Use Ait workspace metadata and `ait.json` project settings while retaining read compatibility with existing project files.
- Remove retired relay pairing, plugin interfaces, and legacy Node CLI installation entries.
- Align desktop and mobile connections with the authenticated Ait server and local Ait client packages.

## 0.0.7 - 2026-09-27

- Isolate Ait profiles, browser sessions, links, updater caches and launch variables from Paseo.
- Protect Paseo skills and Git stashes with independent Ait ownership and naming.
- Connect Remote SSH to the authenticated Rust server and separate mobile application IDs.
- Add persistent desktop server listen settings and update the copyright name to Necokeine.
- Switch Linux and macOS desktop releases to the new Ait app in apps/desktop.
- Bundle only the independent Rust server; remove legacy daemon, worker and CLI sidecars.
- Include automatic update metadata and verify packaged server startup and lifecycle.
- Repair settings navigation and the bundled release-notes page; remove unsupported plugin settings.
- Add Chinese translations for Layout, Add Project and scheduled tasks, now named 定时任务.
- Replace the home Sponsor link with Email.

## 0.0.6 - 2026-09-27

- Apply the orange swift logo across desktop, mobile and Web.
- Use Ait in application names and interface translations.
- Align desktop, mobile, Web and local workspace package versions with the Ait release.
