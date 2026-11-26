# Ait changelog

## Unreleased

- Workspace Bots and their agents can read retained output after a terminal command exits.
  Scoped tools omit directory arguments they cannot accept and explain how to correct invalid
  directory or provider/model arguments when creating agents.

- Older desktop clients keep their Bot page when connecting to a shared-context Host, instead
  of being told to update the Host. Their per-chat reset is refused without changing shared
  context; update the client to reset the Bot context.

- Add independent `aitd-bin` and `aitd-git` Arch packages with an opt-in systemd user service.
  Stable releases can publish both desktop and daemon binary recipes; Git recipes remain locally maintained.

- Bots accept compatible application-bot sender identities from Feishu and Lark. After a restart,
  uncertain executions keep their records while fresh context resumes inputs that never ran.
  In-flight channel replies have up to ten seconds to finish during shutdown.

- A Workspace Bot now shares its context across chats and channels. Each Feishu group can
  independently listen to every message or respond only to mentions, and direct chats can be
  paused separately. Bots can read messages from other bots, rich text and card bodies, stay
  silent during unrelated group conversation, and send task updates back to their source thread.
  Resetting the Bot context affects every entry point; upgrade storage requires a backup before
  downgrading.

- Fixed Feishu and Lark channel lists being rejected by the generated WebSocket response validator.
  Feishu QR setup now displays status-query errors and can retry the same scan without creating
  another app.

## 0.2.27 - 2026-11-25

- Agents can work across the hosts of one account. `host_list` shows the account's other hosts and
  whether each can take calls, and the agent, workspace and provider tools (13 of them, from
  `agent_create` to `profile_list`) accept an optional `hostId` that runs them on that host, so an
  agent can create, prompt and wait for agents there. A host accepts such calls only after you turn
  on `mcp.acceptPeers` (off by default); turning it on trusts every host of the account, because
  another host's agent can then run agents, in any mode, in any directory on this one. It needs an
  account service that supports peer sessions, and it does not change calls without `hostId`.

- The account and help menus, the sidebar host menu and the host picker for opening a project or
  importing a session keep the host rows they were opened with. A host that goes offline no longer
  moves the highlight to another machine or lets Enter reach a different host, an offline host that
  is still registered can still open its settings, and the host monitor says that the machine is
  offline in the title bar while the last sample stays readable
  ([#844](https://github.com/ait-app/ait/pull/844)).

- The Sign in tile on the home screen opens Settings and Online service instead of the login
  website, and it is hidden on a client without an online service account
  ([#840](https://github.com/ait-app/ait/pull/840)).

- A tool without a typed detail shows its leading argument in the collapsed header and lays out its
  input and output by content instead of raw JSON: short values as label and value rows, code and
  commands as code blocks, MCP results unwrapped, and images counted instead of printed
  ([#841](https://github.com/ait-app/ait/pull/841)).

- The agent directory is delivered page by page within the Hub message limit, so the first
  synchronization of a large account no longer exceeds that limit, closes the relayed session and
  repeats on every reconnect
  ([#843](https://github.com/ait-app/ait/pull/843)).

- A host that embeds the provider runtime can read an Agent's stored timeline rows, its subagents
  and its events
  ([#838](https://github.com/ait-app/ait/pull/838)).

- A prompt can carry up to 192 KiB of text, so a message that is wrapped in its own context is not
  cut short
  ([#839](https://github.com/ait-app/ait/pull/839)).

- An unattended session can be kept to the MCP servers it brings. The provider option strictMcp
  keeps a Claude or Codex session to those servers, strictMcpKeep names the inherited servers it
  may keep, and provider.mcp.list.request lists the servers a session in a directory would load
  ([#836](https://github.com/ait-app/ait/pull/836)).

- Antigravity keeps the reason of a protocol failure and collects the native logs of the session
  ([#842](https://github.com/ait-app/ait/pull/842)).

- The SQLite dependency is updated to 0.41 for RUSTSEC-2026-0459
  ([#837](https://github.com/ait-app/ait/pull/837)).

## 0.2.26 - 2026-11-24

- The Git control at the top right of a workspace is one button that is always pressable. It
  commits, rebases, pulls, pushes, merges the PR, opens the PR, creates the PR or resets a delivered
  workspace, and a state it cannot fix in one press, such as conflicts or an unknown Git status,
  opens the panel instead of showing a disabled label. A PR that can be merged is merged from
  this button with the merge method you chose, so the separate "Merge PR" button is gone.
- The Git menu is now a panel with a status summary and the groups Changes, Sync, Pull request and
  Local. An action that cannot run is greyed out and says why in the row instead of in a toast
  after you click. When a PR allows more than one merge method, you choose it once on a sub-page
  and it is remembered on this device for each host and project. "Merge locally" names the branch
  it merges into, and "Reset workspace" sits last in the destructive style; once your work is
  merged, its confirmation is a single short question.
- A branch that has diverged from its remote copy is fixed with "Rebase remote", which replays
  your commits onto the remote branch, keeps local merges, moves no other local branch, and leaves
  the checkout unchanged if it hits conflicts. It refuses a branch whose rebase would duplicate
  commits that are already published, such as one that was rebased onto its base branch after it was
  pushed, and a branch that is pushed to a different remote branch. It replaces "Pull and push" and
  needs a host that supports it; on an older host the button shows the state and opens the panel
  ([#833](https://github.com/ait-app/ait/pull/833)).

- The feature menu in the composer collects the session feature switches and the operations they
  allow, and names the built-in features in all nine languages with the command beside the name. The
  native Goal state of a Codex or OMP session appears as a capsule next to the change count with the
  operations that session allows, such as editing, pausing, resuming and clearing it. A token budget
  is shown only when the host reports one
  ([#832](https://github.com/ait-app/ait/pull/832)).

- When you are signed in, the account entry sits first in the sidebar footer and fills the row, so
  the host and new-project icons move to the right
  ([#831](https://github.com/ait-app/ait/pull/831)).

- A message sent to a MiniMax Code agent right after a turn was cancelled is delivered instead of
  refused. The prompt is resubmitted while the runtime still reports the previous turn as active,
  and a session state update that arrives late no longer counts as the prompt being accepted
  ([#830](https://github.com/ait-app/ait/pull/830)).

- A long chat that contains rows without a height no longer crashes the message list. The
  synchronous row measurement is limited to eight passes per frame and the remaining work
  continues on the next frame
  ([#834](https://github.com/ait-app/ait/pull/834)).

## 0.2.25 - 2026-11-23

- Stopping an Agent refreshes that session's authoritative state, clearing a stale busy indicator
  when the host has already finished while preserving a turn that is still running.

- OMP slash commands distinguish native extensions and aliases from Ait-managed built-ins.
  Ambiguous ownership is reported before an input can be sent or a local shortcut can archive a session.

- Model presets and ordinary model choices now have separate selection states. Selecting an
  ordinary model exits the preset and restores independent role and fallback settings; changes
  saved in the selected preset are used on later turns.
- Running Oh My Pi subagents can be guided or stopped from their parent Agent when the parent
  session is connected and owns the native writer. The rest of the parent session continues.

- A user message that a harness filled with XML-style context blocks, such as a Claude Code
  `<system-reminder>`, pasted content or an IDE selection, shows those blocks as collapsed
  sections instead of raw markup. A block is recognized only when it cannot be ordinary prose, so
  HTML and a sentence that mentions a tag inline stay part of the text, and the session preview
  and the one-line titles use the prose alone
  ([#817](https://github.com/ait-app/ait/pull/817)).

- A failing Agent keeps the RPC context it failed in, so the failure is answered and logged under
  the request that caused it. The Agent I/O error now says that the operation failed and asks for
  the current state to be checked, instead of promising that the same key can be retried
  ([#823](https://github.com/ait-app/ait/pull/823)).

- Forking an Agent from inside a running Codex tool call works again. The boundary is resolved
  against the whole projection and the history up to it is exported in the state it had at that
  point, so a tool that finished after the boundary no longer refuses the fork
  ([#824](https://github.com/ait-app/ait/pull/824)).

- Claude, Codex and MiniMax Code ask for permission automatically by default, so a new session no
  longer waits for a permission mode to be picked. A Bot session, an ephemeral Claude probe and a
  resumed MiniMax session keep the permissions they had. Antigravity and MiniMax Code can also
  generate the metadata of a title, a branch name, a commit message or a pull request with an
  auxiliary small model, such as gemini-4-hydrogen, gemini-3.8-flash, gpt-oss-120b or MiniMax
  M3.1 Flash, in an isolated turn that may not use a tool
  ([#825](https://github.com/ait-app/ait/pull/825)).

- Signing in from the welcome screen opens a page that confirms the account, lists the hosts on it
  with their state, and offers to open the projects once a host of the account is online. A sign-in
  that finishes after its sheet was already closed still lands on that page
  ([#828](https://github.com/ait-app/ait/pull/828)).

## 0.2.24 - 2026-11-22

- The MCP tool catalog was reorganized, and every tool is now named `<domain>[_<object>]_<verb>`, such as `agent_create`, `schedule_pause`, `bot_ticket_list` and `ticket_report`. The previous names still work: `tools/list` shows only the new ones, but `tools/call` resolves a retired name to its tool, rewrites the argument names that tool used before and grants nothing its tool does not; every retired name is counted and logged on its first call and on each doubling, and none of them is removed before 2026-12-22. Argument names were aligned as well, with `agent_update.title`, `scheduleId` on the schedule tools, `heartbeatId` on `heartbeat_delete` and `schedule_update.modeId`, the new `heartbeat_list` returns the calling agent's heartbeats, and `kill_agent` now returns the archive result ([#814](https://github.com/ait-app/ait/pull/814)).

- Every workspace has one notepad that the daemon holds for it, and the agents working in that workspace and the people opening its Notepad tab use the same one. Agents read and write it through the `notepad_read` and `notepad_write` tools, where an append adds a block signed with the author and the time instead of overwriting anyone, and a replace has to name the revision it read or it is refused with the current revision. The tab polls every three seconds while it is focused, autosaves with the revision it read and offers a reload on a conflict, and a host without the feature shows a hint to update the host ([#814](https://github.com/ait-app/ait/pull/814)).

- The notepad is now a Markdown file in the data directory, at `<data-dir>/notepads/<workspace id>/notepad.md`, outside the checkout, and the Notepad tab opens it in the same file editor as any other file, so live updates, conditional saves, conflict prompts and the Markdown preview all come from the file pane. The file is created on the first read, a non-empty `.ait-notebook.md` from earlier versions is imported once and left untouched, and notes written with a build of today's main are not carried over. The notepad is read-only on iOS and Android, where it shows a rendered preview ([#815](https://github.com/ait-app/ait/pull/815)).

## 0.2.23 - 2026-11-22

- The Android build no longer hides the app from Google Play on devices that do not report the portrait feature, such as Chromebooks and desktop-style devices. The Expo config keeps the app locked to portrait, which Android reads as a hardware requirement, so a config plugin now declares it as required="false"; the lock itself stays as it is on phones, because it is a layout preference rather than a hardware need ([#809](https://github.com/ait-app/ait/pull/809)).

- While the online service is signed in, the sidebar footer replaces the Import session, Help and Settings buttons with a single account button that shows the account name and how long the account has left, and those three actions move into its menu; signed out, the footer keeps its three buttons. The time left names its scale only while a week or more remains and is counted in whole days below two days and in whole hours below that, then "Expired"; an account without an expiry shows "Never expires". The account snapshot only records the expiry seen at sign-in, so the button re-reads it from the service every 10 minutes while the app is visible and the account is online, and falls back to the saved value ([#810](https://github.com/ait-app/ait/pull/810)).

- The sidebar host status line shows only the host name, CPU and memory. The network upload and download readings are gone from the line and the two readings are grouped at the right edge instead of being spread across the row, while the details popup is unchanged and still shows the network rates and the history chart ([#811](https://github.com/ait-app/ait/pull/811)).

- The sidebar account menu now ends with Sign out, which runs the same account_logout command as the Settings button without a confirmation and reports a failure as a toast, because the menu has already closed. A second click is ignored while a sign-out is still running ([#812](https://github.com/ait-app/ait/pull/812)).

## 0.2.22 - 2026-11-21

- Usage is no longer a modal. Every host has its own Settings → Host → Usage page, opened from the sidebar Usage row or from the usage summary, with the host chosen among the connected ones. The plan-usage cards, the usage history chart and the per-host tables were redesigned for that page, the modal's host filter and its options popover are gone, and the compact layout closes the sidebar before the page opens ([#803](https://github.com/ait-app/ait/pull/803)).

- The sidebar no longer has a Usage row, because usage now lives in each host's settings page. The usage summary stays in the sidebar only while windows are pinned, and clicking it opens the usage page of the host it reports; the old "Summary in sidebar" switch is gone, since pinning now controls the summary, window rows can always be pinned, and the stored "usage" sidebar preference is migrated once and dropped ([#805](https://github.com/ait-app/ait/pull/805)).

- The Shared row is hidden while both shared lists are empty. "Shared with me" and "Shared by me" are now read from every connected Host instead of only the incoming list, so the row disappears when there is nothing to share, and the Settings → Appearance toggle still keeps a row the user turned off hidden ([#806](https://github.com/ait-app/ait/pull/806)).

- The new workspace form gives the worktree isolation level its own icon, so the isolation picker no longer draws the same glyph as the branch picker next to it. Local, New worktree and Sandboxed worktree now read as a progression, and the icons match the ones the sidebar uses for worktree workspaces ([#802](https://github.com/ait-app/ait/pull/802)).

## 0.2.21 - 2026-11-21

- Creation receipts are no longer limited by the size of the JSON registry. Each receipt is stored in its own row in `creations/receipts.sqlite3`, keyed by receipt ID, so a creation intent that carries inline images can no longer push the whole history past the aggregate file limit. The original `creations/receipts.json` is kept as a pre-upgrade backup and is imported one record at a time in a single transaction; a failed import is rolled back and retried on the next startup, the idempotency algorithm and the serialized intent are unchanged, a disk-full failure is reported as `resource_exhausted`, and committed receipts stay available for replay. A daemon that is downgraded reads the pre-upgrade JSON, and receipts created while downgraded are not merged into an already migrated database ([#793](https://github.com/ait-app/ait/pull/793)).

- "Archive workspace" moved out of the Git button and is now the last item of the workspace actions menu (⋯) next to the title, with its shortcut hint and pending state, on wide and compact layouts; the sidebar workspace menu and the keyboard shortcut are unchanged, and the command palette keeps "Archive workspace" as an entry of its own. Where the Git button used to recommend archiving, it now recommends "Reset workspace" when the reset can actually run — a managed worktree with a known initial branch and an origin — and otherwise shows no primary button. The wide header title is a breadcrumb that reads project name › workspace name and leaves out a project name that only repeats the workspace name, while the compact header keeps its two lines and now lists the terminal profiles in the tab switcher footer ([#796](https://github.com/ait-app/ait/pull/796)).

- The new workspace form asks for the isolation first: the Local / Worktree / Sandboxed worktree picker is now shown before the starting branch picker, in the compact and in the desktop layout. Only the order changed; the visibility rules and the control behaviour are the same ([#798](https://github.com/ait-app/ait/pull/798)).

- In the web and desktop layout, resting the pointer on a link to an issue or pull/merge request of the workspace's own repository opens a card with its title, repository and number, author, last update, diff size, changed files and state; the card opens only for a link that resolves against the workspace's remote, and GitLab links carry no diff size. The "Workspaces" heading above the workspace list is gone, and its display-preferences button now sits on every project row, revealed on hover like the neighbouring buttons and always shown on touch layouts ([#797](https://github.com/ait-app/ait/pull/797)).

- A MiniMax Code `ask_user` questionnaire now opens as one popup question per step instead of one tab per form field: the native step order, the header, the description and the option descriptions are kept, and the extra free-text field of a step becomes that question's single input. The answers are converted back into the native form fields, and a question's description is now shown under the question text for every provider that sends one. The read-only questionnaire renderer is gone; an expanded "Ask user" row shows its input and output like any other tool, while the collapsed row still shows the question text ([#795](https://github.com/ait-app/ait/pull/795)).

- The Antigravity provider lists each model once and offers its thinking levels separately. The effort variants that the native catalogue prints as separate models (for example `gemini-4-neon-high`) are grouped into the model they belong to, and the levels become that model's thinking options ordered from low to max; other models keep their full record and no thinking options. A session is started with `--model <model> --effort <level>`, and the effort argument is left out when no level was chosen. A level the model does not offer is rejected instead of being passed on, a full variant slug saved before grouping still resolves to a model plus an effort, and an explicitly chosen level overrides the one named by the slug ([#794](https://github.com/ait-app/ait/pull/794)).

- The Workspace Bot can send images on the channels that support them: a Markdown image is parsed and loaded with a bounded size, and the picture is sent natively through Telegram, Feishu/Lark and QQ. The channel capability boundaries are documented, and a channel that cannot carry media keeps its previous behaviour ([#791](https://github.com/ait-app/ait/pull/791)).

- The desktop browser view no longer crashes while it is torn down, and the page it shows is kept during the teardown. The browser-view destroyed event no longer throws when the view releases its web contents, and the teardown race is covered by a regression test ([#790](https://github.com/ait-app/ait/pull/790)).

- The Host daemon must be upgraded for the Bot channel media change.

## 0.2.20 - 2026-11-20

- Export the entire saved Timeline, or its conversation messages and voice transcripts, as readable Markdown or inert HTML from context details without interrupting an active turn. Saved result cards and attachment references are preserved; size limits fail explicitly instead of truncating. Downloads are single-use, cancellable and checked against the receipt size; missing artifacts never trigger an automatic re-export. Older hosts show the feature as unavailable. This adds typed export/cancel RPCs and SDK methods plus narrowly validated-origin GET/OPTIONS CORS on the existing file-download endpoint; existing message schemas, token/visitor authorization and provider-native interfaces are unchanged ([#656](https://github.com/ait-app/ait/pull/656)).

- Group chats and private chats are open to every sender on all message channels. Telegram, Feishu, Lark, WeChat, QQ and iMessage no longer filter senders, so an unfamiliar group member or a private-chat user is no longer silently dropped by an outdated list. The old sender list is kept only for compatibility and is no longer checked or used for admission, the sender input field and the QQ pairing whitelist are removed, and the platform credentials, connection checks, group switch, Telegram mention and reply triggers and the Feishu and Lark mention triggers stay as they are ([#781](https://github.com/ait-app/ait/pull/781), [#783](https://github.com/ait-app/ait/pull/783), ADR-290, which supersedes ADR-289).

- A Codex turn that returns a finished text-only message asynchronously is no longer treated as a question. A message is a question only when it carries a questions payload; a text-only async message now uses the existing assistant timeline and completion paths, so the question parser can no longer reject a whole history page and block the next turn when the session is reopened ([#782](https://github.com/ait-app/ait/pull/782)).

- Archiving a workspace or session keeps its connection when the daemon worker queue is saturated. Only the periodic activity heartbeat is shed, an admission error stays correlated with its request, a late desktop send after the Hub session closed is harmless and forwards the close reason to the renderer, and an SDK subscription release is bound to the connection that started it ([#784](https://github.com/ait-app/ait/pull/784), ADR-291).

- The mobile app no longer freezes while the provider list is refreshed. The refresh query now starts a new refresh instead of reusing a settled batch, so the loop stops running on resolved promises without ever yielding ([#786](https://github.com/ait-app/ait/pull/786)).

- The BTW window stays above the keyboard on phones. The native modal viewport and the shared keyboard animation keep the window above the Android and iOS keyboards, the window height and drag range are capped to the space that is available, and the saved drag position is restored when the keyboard closes ([#779](https://github.com/ait-app/ait/pull/779)).

- Cancelling an OMP turn while input is being delivered is bounded. Cancelling the pending input and the turn share one 10-second budget when replacement input is admitted; on timeout the live session is closed and the queued input is dispatched through a resumed session, and the OMP process group survives the shutdown wait so an interrupted shutdown can still be retried or cleaned up ([#780](https://github.com/ait-app/ait/pull/780)).

- The workspace browser stays aligned with its pane when the window is zoomed. The pane bounds are converted from host page pixels to device-independent pixels and the guest zoom is mirrored onto the embedded view, so the page is no longer drawn above and to the left of the pane or scaled wrongly after View to Zoom In ([#777](https://github.com/ait-app/ait/pull/777)).

- Updating an Agent with an unchanged record no longer rewrites the agent registry or wakes the directory observers. The records are compared under the registry lock before the shared snapshot, the last change time and the write state are updated ([#778](https://github.com/ait-app/ait/pull/778)).

- The Host daemon must be upgraded for the Bot channel, relay and OMP fixes.

## 0.2.19 - 2026-11-19

- MiniMax Code models and thinking levels are now chosen separately: every model is listed once (for example `M3.1-Flash` and `M3`), and the thinking variant and effort move into that model's thinking options (`Default`, `Low`, `Medium`, `High`, `Xhigh` and `Max` for `M3.1-Flash`; `Off` and `Thinking` for `M3`). The non-thinking `M3.1-Flash` entry is no longer refused with `Invalid model reasoning`, variants the native agent rejects are left out of the thinking options, a model chosen without a thinking option keeps the current variant when possible and falls back to the model's other variants when the native agent refuses it, and saved configurations that hold a full native value keep working ([#774](https://github.com/ait-app/ait/pull/774)).
- QQ private messages are answered again: a message ID longer than 128 bytes was dropped while the message was being accepted, so a long-ID private message never reached the model. A long ID now uses a SHA-256 deduplication key, the complete original message ID is kept for the passive reply, and a duplicate inbound message is still executed and delivered only once ([#775](https://github.com/ait-app/ait/pull/775)).
- Ordinary chat is answered directly instead of going through the workspace: a greeting no longer requires a workspace lookup, and when the current session exposes no Ait tools the Bot says so instead of reaching for local configuration and HTTP calls, which made a single "hi" run 116 shell calls and take about 11 minutes to answer ([#775](https://github.com/ait-app/ait/pull/775)).
- The Host daemon must be upgraded for the QQ and Bot instruction fixes.

## 0.2.18 - 2026-11-18

- Test feature: a Workspace Bot task can follow the pull request its agent opens (or one the Bot names), on GitHub or GitLab through the Host's signed-in `gh` or `glab`. Review comments by people who can write to the repository (on GitHub the owner, and organization members or collaborators whose write access Ait confirms once per person; when it cannot, for example because the Host's account cannot push to the repository, their comments count as anyone else's; after three failed lookups the comments waiting on them go to the Bot, and that person's later comments are looked up again), requested changes and failed checks go straight to the task's agent once it is idle, marked as data from the forge rather than instructions, with titles, names and paths quoted on one line, without involving the Bot; comments by anyone else only reach the Bot, which tells you, bot accounts are ignored, and your unsubmitted review drafts are not read. The Bot hears when the pull request is merged or closed or its checks pass again, and whenever the agent reports it is blocked, done or failed; a merged pull request closes the task as done, which a new comment within 7 days reopens, and a task the Bot closes after reporting to you keeps following its open pull request until it is merged, including your own review from the same account. Cancelling a finished task stops following its pull request, and so does a pull request the forge reports missing eight reads in a row, including a deleted repository or one the Host's forge account can no longer see (also while access is temporarily lost), which the task records and tells the Bot about. An agent that reports it is blocked can ask one question, shown on the task; the Host owner can answer it in the task details, or the Bot can pass on an answer you give it in chat, and the answer reaches that agent directly (it is refused if the task has meanwhile moved on to another agent's question). The Host daemon must be upgraded; tasks that used these features cannot be read by an older daemon.

- The Workspace Bot hears again from a schedule Agent kept after its run ("After each run: keep"): only the run itself is reported by the run result, and later turns you or the Bot start notify as for any other Agent. At the open task limit, Ait also deletes tasks that a crash left without any Agent for over an hour, without waking the Bot.

- Model presets are reworked in the provider settings: a preset can now reliably take over the model you already selected, the list shows the upstream provider next to the model name, presets are pinned to the top of the settings and the model cards, and the OMP per-role fallback model chain can be edited and applied. Reading the provider catalogue no longer runs one probe process per model on every keystroke, so switching presets is fast ([#765](https://github.com/ait-app/ait/pull/765), ADR-286).
- The key strip under the terminal on phones is regrouped and restyled: Esc and Tab sit above the modifiers they combine with, the arrows form their own cluster, Paste and the keyboard toggle become icon buttons above a wide Enter, a latched modifier inverts until the next key, and the two rows line up on web. Every control, test id and key behaviour is unchanged ([#764](https://github.com/ait-app/ait/pull/764)).
- An Agent that an active or paused schedule points at is no longer closed by closing its tab, a bulk close or a pane close; the app warns instead ([#763](https://github.com/ait-app/ait/pull/763)).
- Claude usage readings send the native client headers again and read the rate-limit delay from the provider: an HTTP 429 is now reported as a rate limit instead of a generic failure, and a forced refresh waits for the delay the provider asked for ([#762](https://github.com/ait-app/ait/pull/762), ADR-285).
- Errors that used to disappear now reach you: the settings screens (providers, host, metadata model, agent and terminal profiles, daemon restart and stop, app update) report failures instead of showing nothing, pausing, resuming, running now or deleting a schedule reports a failure instead of looking like a press that did nothing, and adding a host now uses the connection URI you typed in the Advanced section instead of the default address ([#761](https://github.com/ait-app/ait/pull/761)).
- Text you typed is no longer lost: the workspace notebook keeps autosaving while you type, a message the composer turns down stays in the input, Escape in the slash-command popup clears only the command, a failed schedule edit no longer replaces the form with an error screen, and a project script rename no longer keeps edits after Cancel ([#761](https://github.com/ait-app/ait/pull/761)).
- The agent status file is written far less often: the record for every Agent is kept in memory and flushed every 60 seconds, on flush and on close instead of being rewritten with an fsync on every status change, and a file left behind by a crash is recovered from the timeline ([#768](https://github.com/ait-app/ait/pull/768), ADR-288).

- Long streaming sessions no longer freeze the interface: the web stream viewport keeps its reading position while the list scrolls, resize measurements run once per animation frame instead of synchronously inside the resize and mutation observers, and row width validation rounds to whole pixels so cached heights stop churning. When the desktop renderer still hangs, the main process collects a diagnostic snapshot and force-crashes it to bring the window back after repeated unresponsive events ([#770](https://github.com/ait-app/ait/pull/770), [#772](https://github.com/ait-app/ait/pull/772)).
- Opening a new session no longer freezes the interface while the provider's native process starts: creating an Agent and creating a Workspace with its first Agent are admitted immediately and completed in the background, so workspace lists, directory reads and provider queries stay responsive during native startup, and an interrupted creation still settles its receipt ([#770](https://github.com/ait-app/ait/pull/770)).

## 0.2.16 - 2026-11-18

- Long streaming sessions no longer freeze the interface: the web stream viewport's scroll
  reconciliation is convergence-guarded, resize measurements run once per animation frame
  instead of synchronously inside ResizeObserver and MutationObserver callbacks, and row
  width validation rounds to whole pixels so cached heights stop churning. When the desktop
  renderer still hangs, the main process now collects a diagnostic snapshot (URL, process
  metrics) and force-crashes the renderer to recover the window after repeated
  unresponsive events.
- The side-question (BTW) window is now an independent floating panel: drag it by the title bar and it stops at the window edge, it opens against the right edge by default, and the input area starts collapsed behind a Follow button. Send and stop became round buttons and the window is capped at 384x420. A draft you already typed stays in place when the input is collapsed again.
- The desktop workspace browser runs in its own WebContentsView, so a page opened in the workspace browser is rendered by the new embedded surface.
- DeepSeek Harness sessions that are not running can be browsed and previewed read-only from the import sheet: the preview shows the older messages in full and in order across native page boundaries, taken from a fixed cut of the session. Previewing never creates, resumes or activates a session, and hosts without the preview method keep the previous behavior.
- The mobile input recalls a previous prompt with the arrow keys.
- Claude Code exposes its native switches as model roles: subagent, workflow, subagent-effort, small-fast, opus, sonnet, haiku and fable can be turned on and off in the provider settings. Saving them no longer flashes an error or flips the switches back, and the daemon no longer serializes every request behind a single lock, so the settings stay responsive.
- A side question that fails to send no longer reports an error while the answer is still on its way.
- A slow native provider operation no longer blocks the connection of a remote session.
- Opening a new session no longer freezes the interface while the provider's native process starts: creating an Agent and creating a Workspace with its first Agent are admitted immediately and completed in the background, so workspace lists, directory reads and provider queries stay responsive during native startup, and an interrupted creation still settles its receipt.
- OMP slash commands answer with a command card instead of assistant text: the reply is shown as a tool call that can be expanded, the command is not written to the provider, and it no longer interrupts a running turn.
- TODO cards keep the same spacing on all four sides.
- The mobile app keeps the projections it already has while the host is briefly out of date, instead of dropping them.

## 0.2.15 - 2026-11-17

- An experimental Workspace notebook tab lets a workspace keep shared notes and TODOs in `.ait-notebook.md`, with live collaboration between the people who have it. Guests who only have the file see a read-only view, and a conflicting write is shown instead of being applied over someone else's change.
- The MiniMax `ask_user` questionnaire is shown as a question form instead of raw JSON: the options and their descriptions, whether one or many can be chosen, the recommended option and the waiting-for-answer state.
- Agent profiles open in their own settings page instead of a popup sheet, and editing a profile offers an explicit "Leave unchanged" choice for the model, mode and thinking level. A stored value the host no longer offers is shown as unresolved and is kept when you save, so fields written by a newer client survive the edit.
- Claude turns no longer fail with an "IdempotencyConflict" when a streamed block and the assistant snapshot number their blocks differently, and a rejected runtime event now keeps its error code in the failure notice instead of showing a generic provider error.
- The TODO list is shown correctly in more situations: the Claude task list no longer gains phantom "task added" capsules after every update, a Codex plan that returns to an earlier state no longer shows a stale step after a restart, and inserting or reordering a task in a list without ids no longer reports the moved tasks as newly added.
- A QQ group message that only carries the author's id is accepted again, so Bot replies reach groups that send the legacy shape.

## 0.2.14 - 2026-11-16

- A schedule that runs a new Agent each time now has an explicit "After each run" choice: keep the session in the Workspace so you can keep talking after the task finishes, or archive it (default). The choice is kept when the schedule is edited, when the execution target changes, and for older records that never set it.
- The MiniMax provider answers the questionnaires of its native session: Ait's question form shows the choices, multi-select answers and custom values, and the answer is validated and sent back to the provider.
- A recovered Agent is no longer reported as idle while it is still being admitted. The background recovery path now applies the same admission fence as foreground requests, so the wait for a recovered Agent no longer ends early or flakes.
- The host monitor in the sidebar is now one compact row: a status dot, the host name, CPU, the network up/down rates and memory. The "Workspace host / Pinned host / Host offline" label, the trailing chevron and the mini progress bars are gone; an offline host reads as a muted dot with "—" values, and the pin/follow state stays in the details panel.

## 0.2.13 - 2026-11-15

- Workspace rows in the status-grouped sidebar offer Share and "Open in file manager", as rows grouped by project already did.
- A project folder opened from the desktop shell that cannot be added now shows an error instead of failing silently.
- Creating or editing a schedule no longer changes the isolation choice the new-workspace form remembers.
- Ait agent tool calls in a conversation show the target agent's title, and their details list "Internal" next to "Notify on finish".
- An agent tab shows "Agent not found" only when the agent is missing; other load failures show the error with Retry.
- A desktop update that fails to download or install shows the failure with Retry instead of "Up to date".
- A failed copy in the file explorer shows an error, and the copy buttons in the workspace hover card and Bot webhook setup return to their idle state after a moment.
- Text that appeared in English under every language is now translated, including the workspace screen, the new-workspace form, the pull request panel, the terminal and sidebar menus. About 1,600 mistranslated strings, mostly Arabic, Spanish and French, are corrected.
- Configuration that would grow past the size its own reader accepts is now refused instead of being written as a file that can no longer be loaded: a daemon `config.json` beyond 4 MiB, a project `ait.json` beyond 4 MiB, and a registry file beyond 32 MiB. Writing a project `ait.json` keeps the mode the file already had, so a shared checkout stays readable.
- One schedule whose occurrence cannot be saved no longer holds back every other schedule; the others keep running.
- A hub download the peer already completed is reported as done instead of as cancelled.
- A provider that exits right after its last frame no longer loses that frame: the frame is delivered and the closed stream is reported as closed instead of as a protocol error.
- Resuming an ACP session that answers with another session id is rejected instead of addressing a session the agent does not have.
- Reading the DeepSeek Harness environment skips a variable that is not valid Unicode instead of aborting the session, and its summary commands now end with the client's deadline instead of waiting for a wedged native process.
- Test feature: the Workspace Bot can look up its own records by id instead of guessing: a task's full request and report history, an earlier turn of a conversation, a notification's full text, the agent behind a task, and in your Ait conversation what happened in its other chats, such as a task handed over in Telegram. Long text in the Bot's overview and wake-ups is cut short with a pointer to the whole of it. Each external chat can read only itself and the tasks it started, and can see, direct or stop only the agents of those tasks, as can the agents it starts, so one person's conversation and results never reach another's chat; the Workspace memory, schedules, terminals and scripts stay shared by all chats. Agents working on a task can read that task, including what the other agents on it reported. The Bot tab can show the Bot's conversations in other channels read-only, to the owner and to guests (guests could already read them through the Host API). The Host daemon must be upgraded.

## 0.2.12 - 2026-11-14

0.2.11 was tagged but never released, so its changes ship in 0.2.12.

- The Task overview keeps its search box visible, and the task count heading opens the filters. It shows the active Host and project scope, such as "mba.local / ait 12 tasks", and the filter icon is highlighted while any filter or sort is set.
- Usage history shows reported tokens and costs without the "≥" lower-bound marker, and large numbers use K, M and B in every language, such as `12.3K` or `2.5B`.
- When an Agent turn finishes or fails, the Host refreshes that checkout's status, at most once a minute per directory, so the Changes panel switches between uncommitted changes and the base comparison without a manual refresh.
- Provider task lists appear as a `TODO 1/3` capsule above the input, opening a shared task card with phases, numbering and status. Claude, Codex, OMP, Pi, DeepSeek Harness, OpenCode and ACP agents use the same view, and the current list survives reloads and scrolling back through history.
- Messages Ait sends on your behalf, such as schedule and heartbeat runs and Bot instructions or wake notifications, show their origin instead of raw tags, and schedule prompts can no longer break out of their envelope.
- The workspace sidebar no longer spins forever because of child agents left marked as running by a closed session.
- Oh My Pi model roles come from the installed OMP's live profile, and only assignments that the live catalog resolves are accepted.
- Test feature: Workspace Bots can connect to international Lark through manual setup in Bot settings, separately from China's Feishu.

- Manage named model presets in Provider settings and select them below the default models in the model picker when the Provider supports model roles. A preset applies its model, thinking level and complete role assignments together, preserving permissions and tools. Oh My Pi uses its native preset library, with project overrides kept read-only; other Providers store presets in Ait. Unavailable models remain editable in settings and cannot be applied by mistake. The Host daemon must be upgraded.
- `/btw` opens an independent floating conversation above the main input with streamed answers, continued follow-ups, saved history and separate cancellation. The window follows the input's height and stays clear of it. Expand a settled side conversation into a full session draft with its frozen parent context and question/answer history, then continue working there. Main and side conversations run independently through a shared provider interface; support depends on the installed provider adapter and runtime. The Host daemon must be upgraded.
- Mathematical formulas render in the shared Markdown engine, including BTW windows and expanded conversations. Web and desktop use local MathML with Ait theme sizing and colors; native apps need a new build to enable the math module.

- Test feature: the Workspace Bot keeps a list of tasks. Every agent the Bot starts belongs to a task, and agents report progress, report when they are blocked on you, and report when they are done or give up. The Bot no longer waits inside a turn for long-running work: it hands the work to an agent, tells you it has started and ends its turn, then wakes in the same conversation when an agent finishes, fails, needs permission or reports, and closes the task after telling you the result. In the Bot tab, a Tasks button above the message box shows how many tasks are open and how many are waiting on you. It lists them with tasks waiting on you first, why each one waits, its agents and the latest report, and shows recently closed tasks on request. Opening a task shows the request, the full report history and the outcome; the Host owner can mark it done, cancel it or reopen it, and guests can only read it. Workspace updates that belong to a task link to it. A Bot keeps at most 32 open tasks and the 64 most recently closed; at the limit, tasks whose agents are all archived are closed automatically before a new one opens. Giving an agent more work reopens its task, and an agent that reports its result and then ends its turn wakes the Bot once, not twice. Claude agents send their reports without asking for approval; a Codex agent in a mode that asks before using tools, and agents on other providers, may ask you to approve the report first. Closing a task does not stop its agents. The Host daemon must be upgraded; once an agent has reported on a task, downgrading to 0.2.10 or earlier means removing the Bot data.

- Opening a model picker, the profile editor, a schedule form or provider settings now refreshes that Host's model catalog in the background, sharing one refresh across every open picker instead of re-querying per picker. Providers already being discovered are not refreshed again. If discovery fails, the last usable models stay selectable with a visible error and retry, and remembered model/mode/thinking choices are kept instead of being silently replaced. Host changes, same-client reconnects and working-directory changes fence late refresh results. This changes internal UI refresh/open callbacks and their consumers; public SDK, daemon protocol, saved configuration and native provider interfaces are unchanged.
- Agent profiles now open in their own settings page instead of a popup sheet: the profile list navigates to a nested create/edit route with the usual Back navigation on compact screens, and the model picker keeps only quick-apply plus a shortcut to manage profiles. Editing a profile offers an explicit "Leave unchanged" option for model, mode and thinking, so unspecified fields stay unspecified instead of silently taking the host's defaults. A stored model, mode or thinking level that the host no longer offers is shown as unresolved in the list and the editor and is preserved on save; fields written by a newer client survive edits by this one. Saved profile configuration and the daemon protocol are unchanged, so older hosts keep their existing unsupported/unavailable states.

- Building the desktop app from source: the daemon binary override is now `AITD_BIN`. `AIT_SERVER_BIN` is still read until 2027-05-14; "Ait server" now names the separate server project.

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

- DeepSeek Harness: browse and preview inactive native session history read-only from the import sheet, with complete, chronologically ordered older messages across native-page boundaries on a fixed cut. Reopening takes a fresh snapshot; unfinished-turn status is resolved at that cut, never guessed from the displayed page. Previewing never creates, resumes or activates a native session, and the import flow is unchanged. Hosts without the preview method keep the previous behavior; only providers that advertise read-only preview show the preview action.
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
