# Ait 0.2.6 release validation

Prepared on 2026-11-10 from main `84f89951e080f848260c283bf01c5d35950fb00e`.
The comparison baseline is the actual 0.2.5 `BUILD-INFO.json` source,
`1e9c0886dc44ed9cfdbb04d1459c2b1d87ec5b13`. The retained 0.2.5 validation checkout
`2cdde80a3441f5617638a2cedcc491175caf7a9c` has the same Git tree as that published source.

This preparation synchronizes Cargo/npm versions and locks, PKGBUILD, CHANGELOG,
iOS 0.2.6 / build 2006999 and the local ExpoTwoWayAudio Pod version/checksum.
Production-identity Expo prebuild, pinned CocoaPods install and `pod install --deployment`
passed. Unrelated generated image changes were archived locally and discarded.
No Android or iOS binary was built or submitted.

The release scope is desktop/standalone daemon, GitHub formal Release/Latest and COS beta.
COS stable/latest and manual download entries remain at 0.2.2; website and server production
deployment belong to a separate stable promotion. The release orchestrator records actual delivery.

## Validation

- `npm run verify:release`: Cargo/npm package versions, lockfiles and tag agree.
- `npm run test:release`: 74 passed.
- `npm run test:mobile-release`: 40 passed; Xcode Cloud metadata checks: 5 passed.
- `npm run build:desktop-main` and desktop/mobile typechecks passed.
- Mobile unit project: 706 files, 6994 tests passed after the test fixes below.
- Related desktop measurement: 193 passed, 8 skipped. Protocol: 534 passed. Client: 524 passed, 1 skipped.
- Mobile related measurement after fixes: 297 files, 3755 tests passed.
- Docs, formatting, applicable UI lint/checks, Rust formatting and Bot clippy were checked.

## Test failures corrected before release

These changes affect tests and their configuration, without changing production behavior.
Claude Fable 5.2 with high thinking reviewed and validated them in the release workspace.

1. **Bot message snapshot race.** A turn becomes `running` before its first provider poll persists
   messages. The multi-message test previously asserted message content as soon as it saw that
   status, sometimes observing an empty array. It now waits, with a five-second timeout, for a
   running snapshot containing a message, then retains the exact two-message text/status assertion.
   The original test failed 6/50 sequentially and 400/400 at 16-way parallelism in the final
   comparison. The correction passed 50/50 sequentially and 400/400 at the same parallelism;
   all 194 Bot crate tests passed on the final source.
2. **Layout migration fixture overwritten by earlier saves.** Stores in the same test file share
   storage and a cross-window Promise queue. A pending save from another test could overwrite a
   directly injected legacy fixture before rehydration. Fixture writers now let the mocked
   storage's pending Promise writes finish first. Real migration and built-in-tab assertions are
   retained. The unfixed file failed 10/10; the fixed file passed 20/20, 168 tests per run.
3. **Sidebar badges failed before loading tests.** The import chain reaches
   `@floating-ui/react-native`; its external CommonJS entry requires real React Native, whose Flow
   source Node cannot parse. A unit-project-only alias chooses the package's ESM entry, keeping
   the existing React Native Web alias effective. Browser-project resolution is unchanged.
   All six badges tests pass, including five repeated runs. The existing alias list is preserved.

The two mobile failures also reproduced on the identical 0.2.5 source tree. Desktop EPERM failures
from a restricted runner were rerun with local-port/process access and passed.

## Test coverage

[Machine-readable coverage evidence](release-0.2.6-coverage.json) includes per-file TS results,
requested source lists, expanded commands and source fingerprints. Measurements used macOS arm64,
Rust 1.98.1/default features and Node 22.23.4. Production source fingerprints remained identical
through the subsequent test/config fixes; the mobile measurement was rerun after those fixes.
This is scoped coverage, not whole-product coverage.

### Rust

Every crate with source changes since the published 0.2.5 source was selected: daemon, api, bot,
domain, metadata, persistence and provider. The initial run executed 2880 tests: 2879 passed,
one Bot race failed, and 65 were skipped. After fixing that race, all 194 Bot crate tests passed;
the other crates' production source was unchanged. The report combines the initial profiles with
the Bot revalidation profiles. On the final Fable-reviewed test source, the seven directly affected
Bot service tests were rerun with instrumentation and all passed; the line totals were unchanged.
No full workspace test or coverage run was requested or executed.

```sh
CARGO_TARGET_DIR=<cache> cargo llvm-cov nextest --locked \
  -p api -p bot -p domain -p metadata -p persistence -p provider -p daemon \
  --json --summary-only --output-path <evidence>/rust-cov.json --test-threads 4
CARGO_TARGET_DIR=<cache> cargo llvm-cov nextest --no-clean --locked \
  -p api -p bot -p domain -p metadata -p persistence -p provider -p daemon \
  -E 'package(bot)' --json --summary-only --output-path <evidence>/rust-cov.json --test-threads 4
CARGO_TARGET_DIR=<cache> cargo llvm-cov nextest --no-clean --locked \
  -p api -p bot -p domain -p metadata -p persistence -p provider -p daemon \
  -E 'package(bot) & test(service::tests::)' \
  --json --summary-only --output-path <evidence>/rust-cov-final.json --test-threads 4
```

| Crate               | Covered / total lines | Line coverage |
| ------------------- | --------------------- | ------------- |
| daemon              | 3967 / 4204           | 94.36%        |
| api                 | 6375 / 6806           | 93.67%        |
| bot                 | 5315 / 5678           | 93.61%        |
| domain              | 714 / 714             | 100.00%       |
| metadata            | 7978 / 8356           | 95.48%        |
| persistence         | 1883 / 1966           | 95.78%        |
| provider            | 44698 / 47222         | 94.66%        |
| **Selected crates** | **70930 / 74946**     | **94.64%**    |

The previous 0.2.5 measurement for the same seven crates/default features was 69522/73490,
94.60%; the current result is approximately +0.04 percentage points. Source size also increased.
Linux-only sandbox behavior is not exercised by this macOS measurement; release Linux checks
run on Linux. Skipped native-provider/network cases remain outside this local run.

### TypeScript

Vitest 4.1.7/V8 ran `related --run` against non-test source files changed since the baseline,
excluding generated validators and i18n resources. The evidence JSON contains each expanded
command, its workspace, the source list and `--coverage.include` options. Mobile uses the unit
project; browser/native device tests are not part of this measurement.

| Workspace | Covered / total lines | Line coverage | Related tests         |
| --------- | --------------------- | ------------- | --------------------- |
| protocol  | 35 / 52               | 67.30%        | 534 passed            |
| client    | 2147 / 2765           | 77.64%        | 524 passed, 1 skipped |
| desktop   | 798 / 1458            | 54.73%        | 193 passed, 8 skipped |
| mobile    | 8525 / 16642          | 51.22%        | 3755 passed           |

The file scope differs from the prior release's measurement, so no TS percentage change is claimed.
Changed Electron entry points and UI modules include integration paths that unit tests do not
execute. These figures do not establish signed-app update behavior, native mobile behavior,
Linux sandbox compatibility, or production-server protocol compatibility. Release asset/signing
gates and stable-promotion production acceptance are separate checks.
