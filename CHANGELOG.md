# Ait changelog

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
