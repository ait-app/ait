# Ait changelog

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
