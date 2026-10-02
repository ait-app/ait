# UI regression repair: NEC-376 / NEC-377

Baseline: main `3ab58e57` (PRs 148 and 149 included). Validation uses isolated temporary repositories, offline Codex fixtures, the real Rust server and Chromium; no personal sessions or authenticated model requests.

## Findings and changes

- NEC-376: the original first Terminal Find needle was outside the documented 12,000-cell bootstrap snapshot. Reduce fixture padding and assert both needles are retained before expecting two matches. Settle initial font refits before asserting viewport stability; select the platform-appropriate Find shortcut.
- NEC-377: migrate creation retry, reconnect, delayed File and streamed Markdown gates to actual Rust request IDs and production adapter aliases. Wait for model discovery before submitting and replace obsolete subscriptions/mock Provider helpers with isolated offline Codex fixtures.
- Rust already supplies commit history and required base classification, but the browser adapter omitted the two feature flags. Enable them from the advertised list method and test positive/negative capability mapping.
- Add origin/main sidebar base refresh and max/ultra thinking configuration regressions. Thinking options are fixture-advertised: paid model availability and response quality are outside this validation.
- Add an offline `ui-e2e` CI job, with screenshots only as artifacts. Traces can contain temporary credentials and are not uploaded.

## Local validation

macOS, Chromium, latest-baseline Rust server: the complete focused plus core lifecycle run passed 27 tests; two Commit tests were then restored by the capability fix and all three Commit tests passed. Final coverage is 29 passing scenarios and one Linux-only shortcut scenario skipped on macOS. Covered creation/repeated submission/idempotent retry, disconnect and remount recovery, File transition cancellation, streamed Chat completion/reload, Terminal search/viewport stability, Commit dates/layout/reopen, committed Diff reload, origin-base refresh and max/ultra propagation.

Related terminal/runtime tests: 65 passed, one existing skip. Capability mapping unit tests after the final fix: 23 passed. App and desktop type checks, changed-file ESLint and package-link verification passed. No Rust source changed; the server builds successfully and CI runs full Rust checks because its Python Provider fixture changed.

Other inherited E2E scenarios still need individual migration; these results do not claim the entire inherited suite or native mobile UI passes. The PR remains subject to remote CI and review before merge.

## CI follow-up

The first Linux run passed Rust/UI checks and 22 browser scenarios, but exposed a streamed Markdown fixture race and one compact Find retry. The fixture now pauses its real native deltas at unfinished Markdown boundaries until assertions release each stage, preventing completed-history catch-up from bypassing a browser-only gate. Compact Find settles initial font refits before navigation. The focused local rerun passed six scenarios with the Linux-only shortcut skipped. Final remote CI must run on the follow-up commit before ticket closure.
