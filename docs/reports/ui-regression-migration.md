# UI regression repair: NEC-376 / NEC-377

Rebased on main `6ce64504` (Ait 0.0.14, mobile/daemon workspace names). Tests use isolated repositories, an offline Codex fixture, the real Rust daemon and Chromium.

## Retained changes

- Terminal Find: keep both needles inside the documented 12,000-cell bootstrap budget and wait for initial font refits before asserting viewport/navigation stability. The original 1/1 result was correct for its retained buffer.
- Migrate creation retries, reconnect and delayed File gates to Rust request IDs and production adapter aliases. Wait for model discovery and use offline fixtures instead of historical mock providers/subscriptions.
- Pause the offline Markdown producer at unfinished bold/link boundaries, then verify completion and reload. A browser-only gate could race completed-history recovery.
- Enable commit list/base-classification flags from Rust's advertised method. Test Commit empty state, dates, layout/reopen and committed Diff reload without capability skips.
- Keep origin/main sidebar refresh and max/ultra configuration regressions plus an offline Linux UI CI job.

## Removed or superseded

The unused browser stream-gate deletion and unrelated formatting were dropped from this PR. Commit capability skip guards were removed so a regression fails the tests. Paths, CI binary/package names and documentation links now follow main's workspace migration. Old baseline-specific validation prose was condensed here.

Before rebase, final CI passed all jobs and 24 Linux browser scenarios without retries/skips ([run](https://github.com/ait-app/ait/actions/runs/37035504343)). Rebased changes require fresh validation. Paid model availability, response quality and native mobile UI are outside this offline browser coverage.
