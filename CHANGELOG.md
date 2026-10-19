# Ait changelog

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
