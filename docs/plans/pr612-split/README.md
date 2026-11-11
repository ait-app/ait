# PR #612 split execution contract

Status: **task00 inventory reviewed, validated and published in [Draft PR #647](https://github.com/ait-app/ait/pull/647)**.
PR #647 was closed unmerged on 2026-11-11 with the owner comment “仅作为记录 单独文档修改不计入合并”
(record only; standalone documentation changes are not counted as merges) and reopened the same day
as the live central execution index; it remains a Draft and is never merged or released.
The split plan and product features are not delivered as a whole. Implementation proceeds per task
under its own acceptance gates; current per-task state lives in [tasks.json](tasks.json) and the
execution register below. No merge or release is authorized.
The task00 `commit` field pins the checked inventory/reconciliation snapshot; subsequent publication
metadata records its immutable hash without a self-referential commit hash.

## Authority and review files

- [Frozen sources](sources.json): exact [PR #612](https://github.com/ait-app/ait/pull/612) body,
  its 33 numbered feature sections, and the exact 88-record command TSV with SHA-256 and Git blob identity.
- [Section dispositions](sections.json): every source section, source body/paths, destination tasks and decision.
- [Command dispositions](commands.json): every source command and alias, original nine fields, destination,
  decision and source row. A blocked builtin or a historical test is not a delivered replacement GUI.
- [Net-diff dispositions](net-diff.json): every one of the 547 donor changed paths, per-path added/deleted
  counts, destination tasks, decision, directly cited sections, and latest-main overlap flag.
- [Main coverage and reconciliation](main-reconciliation.json): four code-proven reusable behaviors,
  four explicit deletion/rejected-import decisions, and all 26 main overlaps individually reconciled
  with pinned blobs, hunk locations and behavior-specific preservation rules.
- [Task records](tasks.json): 00–33 and S1–S3, with the exact required fields `id`, `priority`, `depends_on`,
  `donor_sections`, `donor_paths`, `implementation`, `acceptance`, `branch`, `base`, `commit`, `pr_url`,
  `checks`, `blockers`; `title` and `status` are additional navigation fields.
- [Repository/access evidence](evidence.json): actual root, worktree, donor, fetched main, access and prior work.
- [Inventory verifier](verify.mjs): dependency-free, read-only mechanical validation; run from repository root
  with `node docs/plans/pr612-split/verify.mjs`.

The user-supplied constraints in this contract and the task records supersede conflicting donor design.
`sources.json` remains an unmodified historical snapshot, including explicitly incomplete donor work.
Embedded phrases such as “真实 Tai 已验收” are **historical donor claims**, not task00 or current
split validation. Task00 `donor_paths` includes screenshot/report paths for reference only:
do not copy those assets into this plan or treat them as new acceptance evidence.
The mappings are **planned dispositions, not proof that implementation or feature acceptance is complete**.
`rewrite` means adapt behavior rather than cherry-pick; `migrate` still requires current-main reconciliation
and tests; `reference` keeps historical evidence without importing it as current proof; `exclude` rejects
that feature/import; `defer` records a command outside the authorized delivery queue.
The required 主线已覆盖、迁移、重写、后续和删除 categories are represented by behavior-level
`main-covered`, path/command `migrate`/`rewrite`/`defer`, and explicit `delete` decisions in
`main-reconciliation.json`. `reference`/`exclude` distinguish historical evidence and rejected feature imports.
No entire donor file delta is proven fully main-covered; an overlapping path is never called covered.
Main already supplies native Codex Goal operations/cancellation, Profile partial-patch semantics, shared
provider snapshot scope/cache/revision handling and sandboxed OMP native transport. Reuse those facts,
but their remaining UI/lifecycle/isolation acceptance is still work. The 26 overlap decisions preserve
main's extracted permission card, schedule templates, webhook security/capability gates, release scripts
and translations. Main ADR-233/234/235 are occupied; donor numbers must not be copied.
Multi-task source files require hunk-level separation. Empty `donor_sections`/`donor_paths` on new
provider-specific work mean it extends the user plan rather than pretending a donor implementation exists.
Empty release fields are JSON `null`, not invented branches, commits or PRs. `priority` is the supplied
queue order, not a fabricated severity. Every task except 00 explicitly depends on 00.

## Frozen baseline and safe workspace

- Root `/home/KirisameLonnet/Developers/ait`: HEAD `19a829a462919c31693eac2c5caac86c207defac`,
  branch `fix/provider-recovery-and-error-details`, 0 staged / 66 unstaged / 17 untracked paths at inspection.
  These are user changes: leave them untouched; no reset, stash, checkout, clean or overwrite.
- Fetched main/base for task00: `248a93cf6ab409c8151e3a45ef6803ad418d6305` (release preparation #646).
- Donor/PR head: `fd2798e46aa90a8dea48dd8a15e84028dbb981b4`; PR is open, not merged and not Draft.
- Donor/main common ancestor: `84f89951e080f848260c283bf01c5d35950fb00e`. Donor net diff is **547 files, +41,826/−1,125**.
  Main has separately changed 153 paths since that ancestor. Exactly 26 donor paths overlap those changes.
  Comparing the latest-main tree directly to donor yields 674 paths, +42,862/−12,305; that is **not**
  the donor feature inventory and must not roll back main-only work (including bot/webhook/release work).
- Task00 worktree: `/home/KirisameLonnet/.ait-server-desktop/worktrees/1y2t6fw1/pr612-split-docs`;
  branch `docs/pr612-split-task00`. Root source files are not the baseline for any split task.
- GitHub fetch/read/push/admin access succeeded as `KirisameLonnet`; no credential content is recorded.
- No `docs/plans/pr612-split` existed in root, donor, fetched main or reachable path history. Existing
  donor and `pr612-acceptance-final` worktrees are historical work, not completed split tasks.

Reproduce the source inventory with `git diff --numstat 84f89951e080f848260c283bf01c5d35950fb00e fd2798e46aa90a8dea48dd8a15e84028dbb981b4` and
`git show fd2798e46aa90a8dea48dd8a15e84028dbb981b4:docs/archive/reports/providers/pr612-omp-command-dispositions-2026-11-10.tsv`. The live PR body can change; use the frozen body/hash rather than silently
refreshing the 33-section source. Re-fetch main before each implementation slice and record its actual base.

## Non-negotiable product and boundary constraints

### Delivery discipline

Each task produces a focused **Draft PR**, never one giant replacement PR. Stack only on verified,
explicitly recorded unmerged dependencies. Record actual branch/base/commit/PR/checks/blockers.
Mark delivered only after full task acceptance and its Draft PR exist; planned routing and passing tests
alone are not delivery. Integrate the required SDK, UI, protocol and docs within each feature slice;
define old-client/old-host downgrade and preserve existing regressions. Do not duplicate uncertain
operations, user inputs, continuation arbiters or native writers. Add feature-typed ports only for actual
consumers, never a generic `native_action` gateway. Do not create a `modelPresets` store or copy OMP's
fixed role set onto other providers. No bot, webhook or Telegram feature work is included.

Before changing a path, run `node scripts/adr.mjs governs <path>`. Durable ownership, dependency,
protocol/storage/security/lifecycle changes require a new unused ADR number, generated index and current
architecture/spec updates. No new ADR is needed for this inventory-only task. Validation evidence belongs
in each PR description, not a new report under `docs/reports`.

Use targeted format/clippy/build/tests and runtime smoke for affected backend code, and affected frontend,
SDK/protocol and docs checks. **No full Rust workspace tests or workspace-wide coverage**. Documentation-only
work skips Rust. Existing donor pass counts, ignored tests, screenshots and local-model fixtures remain
historical and are never recast as current live-model/platform acceptance. macOS/Windows, native keyboards,
screen readers and signed packages are not proven by 390px browser screenshots.
Required platform acceptance cannot be replaced by a blocker note. Task01 remains incomplete if
required macOS/Linux packaging, signature or notarization verification has not actually passed.

### OMP native boundary, compatibility and distribution

Ait uses direct native Session RPC. A standalone Rust `ait-omp-acp` CLI owns command-line parsing, stdio,
logging and exit behavior; ACP conversion and native RPC implementation remain in provider. Remove
`aitd omp-acp` and the in-process ACP client. `_omp/rpc` is reserved for external ACP negotiation, not the
internal native path. Preserve legacy handles/history/aliases without rewriting native logs. Sandbox policy
must apply to create, restore and configuration-triggered reopen. Only user-installed OMP is allowed:
no bundled/downloaded SDK, runtime or Bun. Bridge version aligns with desktop macOS/Linux packages,
archives, signing/notarization and release asset verification; task execution does not publish releases.

Runtime/receipt handling must preserve unrelated frames, per-request failures and owner fencing; a failed
poll must still settle durable command receipts. Avoid shared process state that lets independent operations
steal output. Native completion, not elapsed timeout, determines lifecycle. Close stdin, terminate and reap
owned process groups/children when necessary. Raw director result JSON is not an acceptable normal UI card.
Director tool visibility or prompt instructions are not a security isolation guarantee.

### Profiles, native configuration, roles and commands

Model catalog refresh is shared, nonblocking and retains usable cached candidates on error. Existing
AgentProfile uses a normal settings list and nested edit flow. OMP native preset CRUD/apply is separate:
no rename, no same-name synchronization and no additional modelPresets store. Respect project/global
ownership, cwd/provider/host identity and deletion confirmation. Roles are dynamically discovered; Claude
advisor is runtime-gated and probe failure cannot erase baseline features.
Unknown persisted values remain intact and visibly unresolved. Catalog refresh is keyed by host,
provider and cwd; late replies from a previous identity cannot overwrite current state.
Unspecified Profile fields keep their current values. A native preset applies as one explicit change
limited to its owned model/role fields, never permissions or tools. Generic UI never parses OMP
strings, aliases or thinking suffixes; that interpretation belongs to the provider adapter.

Settings display native-confirmed values with field-level partial success and absent/clear/set semantics.
Commands preserve actual builtin/plugin/skill/template/project provenance. Same-name plugins are not
blacklisted as builtins; filtered management/terminal builtins are neither navigation shortcuts nor prompts.
Hints come from real metadata, remain out of draft/copy/submission/history, and respect multiline/IME editing.
Native startup forms support multi-select, empty values, multiline, secrets, cancel and draft recovery;
approvals remain responsive while other operations wait. Widgets/editor updates are ephemeral, keyed and
owner-fenced. Account objects have readable labels, credential protection and identity-bound usage invalidation.
Read-only usage reuses AgentUsage/run/Timeline/account data: unknown is not zero; cumulative values are not summed.
Current/run/account/child scopes stay distinct and read-only; reading statistics creates no chat or model call.

### BTW: fixed snapshot and safe isolation

Capture the display context already persisted at submission; persist the topic snapshot as needed,
without forcing main-conversation persistence. Use the same selected provider/model, never a summarizer
route. The main conversation continues; side work is neither the main writer nor main queue.
Allow one active BTW, with streaming, cancel, history, collapse and explicit end. After native accepted or
uncertain acceptance, never fall back to another execution. Follow-ups reuse the original snapshot plus
BTW Q/A, not fresh main context. Transfer creates a **new nonrunning draft**: no inference, native-session
rebind or original-log rewrite. This replaces the donor's native branch-promotion semantics.

- OMP: public native BTW.
- Claude: isolated run with no tools, MCP or hooks.
- Codex: isolated auxiliary thread.
- OpenCode: temporary ACP session, tool denial and cleanup.
- DSH: isolated text-only profile.
- Pi: dedicated launcher, public no-tools/no-extensions capability.
- Antigravity: temporary directory/process; verify denials of file, command, browser and MCP effects.
- MiniMax: **excluded by user decision (2026-11-11)**; task13 is out of the delivery queue, not delivered
  and not an overall blocker. Ordinary fork/prompt or permission-off never qualified as isolation.
  Task19 (DSH and MiniMax native Goal) is unaffected by this exclusion.

### Goal: native-first strategy and single-writer fallback

Choose native-first versus fallback strategy once per run. Fallback continues the **same main writer** only
after terminal turn and history commit, with pending user input first. Persist goal/version/state/turn/receipt.
Accept scoped tool results or a validated dedicated checkpoint containing verdict/evidence/nextAction only
for the current checkpoint. Allow one malformed repair then pending; three no-progress checkpoints become
blocked. Pause allows the current turn to finish; stop cancels. Restart reconciles uncertain receipts and
native state before any continuation. Codex has one continuation arbiter, including restart recovery.
Do not invent a default budget, fake estimate or hard cap. Guided Goal must obtain user confirmation before
starting. Provider-native Goal claims require their own facts/evidence, not OMP screenshots for DSH/MiniMax.
Reliable budget enforcement requires backend metering. Editing an existing goal or changing runtime
budget is not part of minimum delivery.

### History, child operations, exports and Vibe

Child guide/stop validates the parent and single-writer ownership; observation stays read-only. History
selection uses readable native objects, not raw IDs, and commits native identity with projection.
Timeline Markdown/HTML, OMP native HTML and DSH ZIP must become **actual client downloads** with appropriate
authentication, bounds, cancellation and cleanup, not daemon-local paths. DSH attachments are regular files,
size-matched and bounded; upload binary streams, then submit native receipts, not JSON/base64 contents.
The donor's 256 MiB attachment limit is a candidate implementation limit to revalidate in task28,
not a fixed user contract.
DSH inactive history/preview never activates a writer. OpenCode uses actual command catalogs and terminal
lifecycle, never timeout guesses; preserve existing DSH/OpenCode MCP delivery and draft-history isolation.
Vibe is an OMP-only Ait adapter over public RPC/extensions, not claimed upstream TUI behavior; validate
confirmed mode state, in-flight exit, worker parent identity, restoration, bounded retention and cleanup.

### Deferred and excluded scope

Native statistics/status dashboards and incomplete Plan/Loop/Queue/Tan/OMFG/Cleanse workflows are not
silently marked complete; the command inventory records all additional out-of-queue commands explicitly.
Blocked builtin text does not imply its GUI replacement exists. Terminal-only commands do not acquire fake
Ait equivalents. No bot/webhook/Telegram additions. Preserve latest-main unrelated changes.
Paseo #2056 closed because discussion moved, not because delivered; #3758 tracks plugins progress;
#5003 fork-context remains pending. These are user-supplied provenance notes, not independent live verification.

### Support slices

S1 rewrites the desktop whitelist and adds tests for eight `AIT_SERVER_*_BIN` names (CODEX, CLAUDE,
ANTIGRAVITY, OPENCODE, DEEPSEEK*HARNESS, PI, OMP, MINIMAX_CODE), plus
`AIT_SERVER_DEEPSEEK_HARNESS_PROFILE` and `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT`; unrelated
`AIT_SERVER*`values remain stripped. S2 excludes normalized`/packages/protocol/`sources from the enabled
React Compiler and adds tests. S3 adds`package:daemon`as an alias for the existing`node scripts/release-assets.mjs daemon`implementation (platform, version, binary, destination and optional`--dev LABEL`), with focused tests. Main has none of these three donor changes; donor lacks sufficient S1 tests.

## Execution queue

Detailed implementation, acceptance, source paths and release/check state are authoritative in
[tasks.json](tasks.json); the table below is the review-time queue, not a second mutable completion ledger.

### Execution register (2026-11-11, central docs owner update)

Live ownership and delivery pointers; only the owning worker writes its task branch. The register
prevents duplicate ownership. `migration-refresh-recovery` (former central docs owner) is frozen
after recurring external backend 400s; this worktree is now the sole central 647 docs metadata owner.
New proposed ADRs stay **proposed pending review**; historical accepted ADRs are not rewritten.

- **00** delivered inventory — Draft [#647](https://github.com/ait-app/ait/pull/647), evidence `7ac9b4ac`.
  Closed unmerged as record-only (owner comment 2026-11-11), reopened as the central index; still Draft.
- **01** blocked — Draft [#651](https://github.com/ait-app/ait/pull/651), branch
  `refactor/pr612-omp-native-boundary`, head `43a0e952` (= verified code `92abc77d` + interface
  disclosure `22efa58f` + Linux payload cleanup; ADR-237 proposed). Linux native/archive/normal-0077
  installed deb+rpm acceptance passes. **Only** actual macOS build/Developer ID signature/notarization
  is blocked. Not delivered. Interface ledger (removed `aitd omp-acp` → standalone
  `ait-omp-acp [--omp-bin]`, internal ACP client removal, `_omp/rpc` external-negotiation only,
  sandbox on create/restore/reopen, legacy handle/native-history compatibility, UI/public SDK wire
  unchanged) is recorded in tasks.json `01.interface_disclosure` and ADR-237 (approved context on #651).
- **02** delivered (director-approved full scope/actual smoke/compat ledger) — Draft
  [#659](https://github.com/ait-app/ait/pull/659), branch `feat/pr612-task02-model-catalog-refresh`,
  head `efca94b8` (implementation `f14f4589`), base `248a93cf`; no Rust source/tests. Focused 58
  fencing tests incl. same-client epoch regression, type/lint/docs passed; actual desktop +
  changed-catalog (bg_666, artifact://2818) and supported compact Diagnostic/Refresh/return
  (bg_688, artifact://2891) passed. ADR-236 proposed; internal UI contract changed, public
  Provider/API/SDK unchanged (tasks.json `02.interface_disclosure`).
- **03** now actionable (02 delivered); owner `profile-settings`, design-ready (ADR-244 reserved).
- **04** delivered — Draft [#650](https://github.com/ait-app/ait/pull/650), branch
  `pr612-split-task04-statistics`, current head `a0d80312` (implementation `c187052e`; ADR-238 kept
  proposed during review).
- **05** active, **unverified** — sole owner `dynamic-model-roles`, branch
  `feat/pr612-task05-native-config` stacked on task01 published head `22efa58f`; an acknowledged
  partial-fields bug is being fixed; no Draft yet. ADR-241 proposed. The user deadline does not
  relax security/scope.
- **06** active — owner `command-provenance` (agent frozen after recurring external 400; work persists
  on `feat/pr612-task06-command-provenance`, base `22efa58f`); no Draft yet. ADR-242 proposed.
- **07** in review, **not delivered** — Draft [#657](https://github.com/ait-app/ait/pull/657), branch
  `feat/pr612-task07-structured-interactions` @ `cc4a5745`, base task01 `22efa58f`; owner
  `structured-interactions`; **unverified startup failure and secret gating remain open**, plus
  security/concurrency verification pending. ADR-243 proposed. Additive optional question fields
  only; permission envelope/events/resolve unchanged (tasks.json `07.interface_disclosure`).
- **09/16/21/24/25/32/33** queued behind their dependencies; ADR-246/245/247/248/249/251/252
  reserved respectively. 16/17 Goal designs are complete research artifacts, blocked on 05/07
  verification; no Goal code.
- **13** excluded by explicit user decision (2026-11-11) — not delivered, not a required overall
  blocker; task19 (DSH/MiniMax native Goal) unaffected. This is a user exclusion, not a claim that
  all eight BTW providers are delivered.
- **26** delivered — Draft [#656](https://github.com/ait-app/ait/pull/656), branch
  `pr612-split-task26-timeline-export`, head `1648320c` (implementation `3059e638`), stacks on
  delivered 04 head `a0d80312`. ADR-239 proposed (amends ADR-049 for validated-origin download CORS).
  Four-surface interface ledger in tasks.json `26.interface_disclosure`.
- **27** active — sole owner `omp-native-html-export`, implementing integration of 01 + 04 + 26;
  ADR-265 proposed. No public branch/Draft yet.
- **30** in progress, **not delivered** — existing Draft [#655](https://github.com/ait-app/ait/pull/655),
  branch `feat/pr612-task30-dsh-inactive-history`, implementation `de50e8a5`, actual HEAD `62334d12`
  (no-preview branch); browser read-only preview still unverified and a pagination correction is
  required. Sole recovery owner is now `session-statistics` (`opencode-command-lifecycle` frozen after
  external 400 — no product blocker; original `dsh-history` owner also frozen); the recovery owner
  must inspect actual HEAD before mutating. No extra API changes. ADR-240 proposed; interface ledger
  in tasks.json `30.interface_disclosure`.
- **31** blocked — native capability: official OpenCode 2.1.3 custom commands end_turn before model
  requests and public ACP lacks a reliable terminal signal; needs upstream completion semantics or a
  public attach surface (not a credential request). Catalog+cleanup reachable after 06; owner
  `opencode-command-lifecycle` persists; ADR-250 reserved. Not user-excluded.
- **S1** delivered, director-accepted — Draft [#652](https://github.com/ait-app/ait/pull/652), branch
  `fix/pr612-s1-desktop-env-allowlist`, head `234acc9c` (implementation `9ba98138`).
- **S2** delivered, director-accepted — Draft [#653](https://github.com/ait-app/ait/pull/653), branch
  `fix/pr612-s2-react-compiler-protocol-exclusion`, head `c721b12c` (implementation `94e048ea`).
- **S3** delivered, director-accepted — Draft [#654](https://github.com/ait-app/ait/pull/654), branch
  `fix/pr612-s3-package-daemon-alias`, head `3e49d50f` (implementation `cdae0123`).
- All other tasks remain research/design-only until their dependencies verify.

| ID  | Task                                             | Dependencies           | Initial state                       |
| --- | ------------------------------------------------ | ---------------------- | ----------------------------------- |
| 00  | Inventory and split contract                     | —                      | delivered (Draft #647, record-only) |
| 01  | OMP native Session and standalone ACP bridge     | 00                     | planned                             |
| 02  | Shared model catalog refresh                     | 00                     | delivered (Draft #659)              |
| 03  | Existing AgentProfile settings                   | 00, 02                 | planned                             |
| 04  | Typed read-only usage                            | 00                     | delivered (Draft #650)              |
| 05  | Native-confirmed configuration                   | 00, 01                 | planned                             |
| 06  | Native command provenance and completion         | 00, 01                 | planned                             |
| 07  | Structured native interactions                   | 00, 01                 | planned                             |
| 08  | Ephemeral widgets and editor                     | 00, 07                 | planned                             |
| 09  | BTW contract with OMP native and Claude fallback | 00, 01, 07             | planned                             |
| 10  | Codex OpenCode and DSH BTW                       | 00, 09                 | planned                             |
| 11  | Pi BTW                                           | 00, 09                 | planned                             |
| 12  | Antigravity BTW                                  | 00, 09                 | planned                             |
| 13  | MiniMax BTW                                      | 00, 09                 | excluded (user decision)            |
| 14  | BTW follow-up and lifecycle                      | 00, 09                 | planned                             |
| 15  | BTW transfer to nonrunning draft                 | 00, 14                 | planned                             |
| 16  | Unified Goal strategy and fallback               | 00, 05, 07             | planned                             |
| 17  | Codex native Goal                                | 00, 16                 | planned                             |
| 18  | OMP native Goal                                  | 00, 01, 16             | planned                             |
| 19  | DSH and MiniMax native Goal                      | 00, 16                 | planned                             |
| 20  | Guided Goal                                      | 00, 16                 | planned                             |
| 21  | Dynamic model roles                              | 00, 03, 05             | planned                             |
| 22  | Claude advisor                                   | 00, 21                 | planned                             |
| 23  | OMP native presets                               | 00, 21                 | planned                             |
| 24  | Child guide and stop                             | 00, 07                 | planned                             |
| 25  | History object targeting                         | 00, 06, 07             | planned                             |
| 26  | Persisted Timeline export                        | 00, 04                 | delivered (Draft #656, stacks #650) |
| 27  | OMP native HTML export                           | 00, 01, 26             | planned                             |
| 28  | DSH commands and attachment receipts             | 00, 06, 07             | planned                             |
| 29  | DSH ZIP export                                   | 00, 26, 28             | planned                             |
| 30  | Inactive DSH history                             | 00                     | planned                             |
| 31  | OpenCode command lifecycle                       | 00, 06                 | planned                             |
| 32  | Provider account objects                         | 00, 07                 | planned                             |
| 33  | OMP-only Vibe                                    | 00, 01, 05, 07, 21, 24 | planned                             |
| S1  | Desktop provider environment allowlist           | 00                     | delivered (Draft #652)              |
| S2  | React Compiler schema exclusion                  | 00                     | delivered (Draft #653)              |
| S3  | Reuse daemon packaging entry point               | 00                     | delivered (Draft #654)              |

## Reproducible task00 checks

```sh
node docs/plans/pr612-split/verify.mjs
npm run check:docs
git diff --check
```

Also format/check only changed documentation and this verifier with the repository formatter.
The verifier checks pinned sources/hashes, all 33 sections, all 88 commands/aliases, the exact 547-path
Git numstat inventory, all 37 task records/dependencies and bidirectional mappings. It does not claim
semantic implementation acceptance. Runtime checks and model inference are not part of task00.

## Test coverage

**Not applicable — no Rust behavior changed.** No Rust tests or coverage measurement are authorized
for this documentation-only task. Mechanical inventory and documentation check results are recorded in
task00's `checks` field and Draft PR; future feature tasks must record their own actual verification.
