# Web tooltip DOM-trigger validation

Date: 2026-10-21. Validated the tooltip changes in the working tree based on
`b2e9b093dd7f55d588b381dff8662458d15696a6`. This report belongs to the independent
tooltip PR and does not use the Pi browser suite as evidence for tooltip behavior.

## Environment and source snapshot

- Linux x86_64; Node.js `v26.8.1`; npm `12.0.2`.
- Vitest `4.1.7`, the mobile workspace's `unit` project, and JSDOM.
- Measurements and the floating surface are mocked; no native-device test was run.

SHA-256 of the tested files, unchanged since the recorded focused test run:

```text
383c78ace69677065495cccb9d48494baf5d3dee47adb74763ed0988c657fa49  apps/mobile/src/components/ui/tooltip.tsx
fc5fa3eec4ebd92298d18428dacea1d58c78c489de119205166195231f0dc7f2  apps/mobile/src/components/ui/tooltip.test.tsx
```

## Focused tests

```sh
npm run test --workspace=@ait/mobile -- --project=unit src/components/ui/tooltip.test.tsx
```

Passed with exit status 0: **1 test file, 4 passed, 0 failed**.

- Two existing cases verify disabled and interactive `asChild` trigger behavior.
- Two added cases exercise DOM and React Native trigger measurement. Each verifies that
  opening the tooltip measures the trigger once and positions bottom/start content at the
  expected coordinates after content layout.

The focused result was recorded earlier in this PR preparation; the source files have
not changed since that run. The focused tests were not rerun after documentation-only updates.

## Type checking, lint, and formatting

```sh
npm run typecheck --workspace=@ait/mobile
node_modules/.bin/oxfmt --check apps/mobile/src/components/ui/tooltip.tsx apps/mobile/src/components/ui/tooltip.test.tsx
git diff --check
git diff --cached --check
```

The complete mobile TypeScript check passed with exit status 0 and no diagnostics.
Changed-file formatting and staged/unstaged whitespace checks passed.

ESLint command, run from `apps/mobile`:

```sh
../../node_modules/.bin/eslint src/components/ui/tooltip.tsx src/components/ui/tooltip.test.tsx
```

ESLint exited 0 with **0 errors and 2 warnings**. Both warnings are `import/no-duplicates`
on the test file's existing separate React imports at lines 1 and 2; those imports were
unchanged by the tooltip fix. This is not a warning-free lint result.

## Behavior and limits

DOM triggers use `getBoundingClientRect`; React Native Views retain `measureInWindow`.
The tests exercise both API paths in JSDOM and verify the computed floating position.
They do not prove real-browser geometry, hover timing, viewport collision behavior,
or native-device positioning. No manual browser or device validation is claimed.

## Test coverage

Coverage was not measured because it was not requested. Four passing tests are not a
coverage percentage. If requested, collect focused tooltip coverage with the mobile
test tooling and publish the source revision, covered/total lines, scope, exclusions,
and report artifact.
