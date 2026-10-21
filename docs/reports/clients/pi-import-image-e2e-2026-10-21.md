# Pi import and image attachment browser validation

Date: 2026-10-21. Current validation passed at commit
`23997a20a7cbf1516b6fd1848dd73d504f14ea27` on `test/pi-import-image-lifecycle`,
based on `main` at `a73c16ba40ca7c5492dd24c1623f87ff9dde800d`. This branch includes the
provider image-projection fix merged in [PR #425](https://github.com/ait-app/ait/pull/425),
documented in [ADR-176](../../decisions/providers/adr-176-pi-user-image-projection.md).

These browser results are separate from the provider unit-test and tooltip unit-test
totals. The PR 3 branch was checked out with a clean working tree when validation started;
subsequent edits update this report only. The earlier combined-tree run is recorded below
as historical evidence and is not included in the current run's total.

## Environment and source snapshot

- Linux x86_64; Node.js `v26.8.1`; npm `12.0.2`.
- Playwright `1.58.2`; Chromium headless shell `145.0.7632.6`.
- Desktop viewport: 1280 × 900; mobile viewport: 390 × 844.
- Two cases on one worker, with no requested retry or parallelism override.
- Production Rust daemon and Pi adapter, isolated temporary host configuration,
  native JSONL history, and an offline Pi 1.2 RPC peer.

SHA-256 of the tested browser spec and RPC fixture:

```text
1ee22cec714dfbbd4ae782e9e70fe211ee11affe02a45f9b5441dec7c0a66239  apps/mobile/e2e/browser/import-session.pi.spec.ts
498588069859b60d57b3c86eed45b357964d5b1c5cd57c84a2f58be0729684b1  apps/mobile/e2e/fixtures/pi-import-rpc.mjs
```

## Browser acceptance

Command, run from the repository root with permission to bind isolated local test ports:

```sh
PLAYWRIGHT_BROWSERS_PATH=/tmp/ait-pr-playwright npm run test:e2e --workspace=@ait/mobile -- e2e/browser/import-session.pi.spec.ts
```

The browser-cache variable selects this environment's temporary Chromium installation;
it is not required when Chromium is already installed in Playwright's default cache.

Current branch run: **2 passed, 0 failed**, exit status 0, total duration approximately
3.2 minutes including dependency builds, daemon compilation, and Metro warmup.

| Case    | Result | Test duration |
| ------- | ------ | ------------- |
| Desktop | Passed | 20.2 seconds  |
| Mobile  | Passed | 22.4 seconds  |

Global setup rebuilt the protocol/client, highlighting, and two-way-audio packages,
successfully built the Rust daemon in approximately 65 seconds, started Metro on a dynamic
port, and warmed the web bundle. The tests used isolated daemon state; teardown stopped
the owned daemons and Metro. Both cases passed on their first attempt in this branch run,
without source changes, assertion changes, or test retries.

Both viewport cases verified:

- Workspace-scoped discovery and import of the expected Pi session.
- Retained compaction context, context edits, imported text, and the imported image preview.
- Byte-for-byte unchanged native history during listing/import and no native session writer
  opened by those operations.
- A new composer image attachment sent with a follow-up in the original native session.
- Exact image type, MIME type, and base64 payload in RPC and native JSONL history.
- Expected loaded image dimensions for imported and newly submitted images.
- No embedded image payloads in the shared timeline.
- Timeline equality across daemon restart, plus retained conversation and image display
  after browser reload.
- External history remaining absent before explicit refresh and visible afterward.
- One copy of the continuation, one preview per image, no duplicate native session file,
  and no embedded image payloads after refresh.

## Earlier combined-tree run and setup recovery

An earlier run validated the uncommitted combined working tree based on
`b2e9b093dd7f55d588b381dff8662458d15696a6`: **2 passed, 0 failed**, in approximately
1.3 minutes (desktop 19.2 seconds; mobile 22.1 seconds). Those results did not establish
validation of the independently split PR 3 branch and are superseded by the current run.

During that earlier preparation, the first sandboxed attempt built the dependencies and
daemon but stopped during global setup with `listen EPERM` on loopback. A permitted rerun
reached the test harness while the browser installation was incomplete; both cases stopped
at browser launch before their test bodies executed. Those attempts are not passing
acceptance results.

The browser download initially required network permission. Its Node-based installer
then stalled while unpacking; the cached matching browser and FFmpeg archives were
unpacked with the system `unzip`, and browser version and dynamic-library availability
were checked before the earlier successful rerun. No source change or test assertion
relaxation was needed to resolve these environment failures. The current branch run uses
that existing browser installation and runs with local-listener permission from the start.

## Type checking, lint, syntax, and formatting

```sh
npm run typecheck --workspace=@ait/mobile
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/import-session.pi.spec.ts --list
node --check apps/mobile/e2e/fixtures/pi-import-rpc.mjs
node_modules/.bin/oxfmt --check apps/mobile/e2e/browser/import-session.pi.spec.ts apps/mobile/e2e/fixtures/pi-import-rpc.mjs apps/mobile/e2e/README.md
npm run check:docs
git diff --check
git diff --cached --check
```

These checks were rerun on the PR 3 branch. The mobile TypeScript check passed without
diagnostics. Discovery listed exactly the desktop and mobile cases. Fixture syntax,
changed-file formatting, documentation checks, and staged/unstaged whitespace checks
passed. The report was also format-checked after recording the current results.

ESLint command, run from `apps/mobile`, passed with no diagnostics:

```sh
../../node_modules/.bin/eslint e2e/browser/import-session.pi.spec.ts e2e/fixtures/pi-import-rpc.mjs
```

Non-fatal run output included Node's `module.register()` deprecation, color-environment
warnings, stale Browserslist data, and a missing optional `gh` executable. They did not
prevent the current test run from completing successfully.

## Limits

The RPC peer is offline: these results do not validate an installed Pi, a real model
service, another Pi version, a native mobile build, or a second physical device.
The image assertions verify decoded thumbnail loading; they do not exercise the
full-image lightbox. Current results cover the committed PR 3 branch at `23997a20`;
later source changes require their own validation. No full Rust workspace suite was run.

## Test coverage

Coverage was not measured because it was not requested. Two passing acceptance cases
are execution results, not line or branch coverage. If requested, define a focused
instrumented scope for the client and/or provider and publish the exact source revision,
features, exclusions, covered/total lines, and report artifact.
