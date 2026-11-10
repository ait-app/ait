# Ait 0.2.7 release validation

Prepared on 2026-11-11 from main `2f3d2610f68f55340b589af0ba36f555e62ea210`.
The comparison baseline is the actual 0.2.6 `BUILD-INFO.json` source,
`b10dbf72807c163c42e364d40af661048fd1afd9`, which is also the `v0.2.6` tag commit.
The release contains #640, #641, #642, #644, #633, #643 and #645.

This preparation synchronizes Cargo/npm versions and locks, PKGBUILD, CHANGELOG,
iOS 0.2.7 / build 2007999 and the local ExpoTwoWayAudio Pod version/checksum.
Production-identity Expo prebuild, pinned CocoaPods install and `pod install --deployment`
passed. The prebuilt Info.plist is semantically identical to the committed one, which changes
only the two version fields. Unrelated generated image, asset-catalog and plist formatting
changes were archived locally and discarded. No Android or iOS binary was built or submitted
during preparation.

The release scope is desktop/standalone daemon, GitHub formal Release/Latest and COS beta.
As the user explicitly authorized for this release, the stable `v0.2.7` tag also triggers
the `release.yml` jobs that publish to Google Play internal testing (#643) and start the
iOS internal TestFlight build in Xcode Cloud (#645). Those jobs do not block the desktop
Release, and their outcome is recorded separately. COS stable/latest and the manual download
entries stay unchanged; there is no stable promotion, and website and server production
deployment are out of scope. The release orchestrator records actual delivery.

## Validation

- `npm run verify:release`: Cargo, 7 npm packages, lockfiles and tag agree on 0.2.7.
- `npm run test:release`: 74 passed.
- `npm run test:mobile-release`: 47 passed; Xcode Cloud metadata checks: 5 passed.
- `npm run build:desktop-main` and desktop/mobile typechecks passed.
- `node scripts/verify.mjs --base b10dbf72`: 23 checks passed, including docs, formatting,
  `cargo fmt`, workspace clippy with `-D warnings`, oxlint and UI checks. Its tests: Rust nextest
  for the seven changed crates 1088 passed, and the doc-test step passed with no doc tests;
  protocol 851, client 566 passed and 1 skipped, highlight 103, desktop 564 passed and 17 skipped,
  affected mobile tests 884, and release workflow scripts 77.
- Related TS measurement: protocol 471 passed; client 358 passed, 1 skipped; mobile 602 passed.

## Test failures corrected before release

None. No test or fixture was changed in this preparation.

## Test coverage

[Machine-readable coverage evidence](release-0.2.7-coverage.json) includes per-file TS results,
requested source lists, expanded commands and source fingerprints. Measurements used macOS arm64,
Rust 1.98.1/default features and Node 22.23.4, on the release source with the 0.2.7 version
changes applied. This is scoped coverage, not whole-product coverage.

### Rust

Every crate with source changes since the published 0.2.6 source was selected: daemon, api, bot,
domain, model, persistence and relay. `verify.mjs --base b10dbf72` selects the same seven crates.
The run executed 1088 tests: 1088 passed, none skipped. No full workspace test or coverage run
was requested or executed locally.

```sh
CARGO_TARGET_DIR=<cache> cargo llvm-cov nextest --locked \
  -p api -p bot -p domain -p model -p persistence -p relay -p daemon \
  --json --summary-only --output-path <evidence>/rust-cov.json --test-threads 4
```

| Crate               | Covered / total lines | Line coverage |
| ------------------- | --------------------- | ------------- |
| daemon              | 4029 / 4272           | 94.31%        |
| api                 | 6487 / 6926           | 93.66%        |
| bot                 | 5669 / 6051           | 93.69%        |
| domain              | 706 / 714             | 98.88%        |
| model               | 1642 / 1715           | 95.74%        |
| persistence         | 1887 / 1979           | 95.35%        |
| relay               | 1370 / 1401           | 97.79%        |
| **Selected crates** | **21790 / 23058**     | **94.50%**    |

No comparison with 0.2.6 is made. That measurement selected metadata and provider instead of
model and relay, and a crate's line coverage also depends on which other crates' tests run, so
neither the totals nor the per-crate figures share a scope. Linux-only behavior is not exercised
by this macOS measurement; release Linux checks run on Linux. The Webhook channel is tested
with the local automation policy and a fake center; the production Ait center's webhook
endpoints are outside this run.

### TypeScript

Vitest 4.1.7/V8 ran `related --run` against non-test source files changed since the baseline,
excluding i18n resources. The evidence JSON contains each expanded command, its workspace, the
source list and `--coverage.include` options. Mobile uses the unit project; browser/native device
tests are not part of this measurement. Desktop and highlight have no changed source and were
not measured. `@vitest/coverage-v8` 4.1.7 was installed with `npm install --no-save` for the
measurement and is not part of the lockfile.

| Workspace | Covered / total lines | Line coverage | Related tests         |
| --------- | --------------------- | ------------- | --------------------- |
| protocol  | 671 / 693             | 96.82%        | 471 passed            |
| client    | 33 / 36               | 91.66%        | 358 passed, 1 skipped |
| mobile    | 1171 / 2276           | 51.44%        | 602 passed            |

The file scope differs from the prior release's measurement, so no TS percentage change is claimed.
The Task overview screen, card and detail components, the sidebar and command-center entries,
the root layout and the refactored agent stream view have no unit-test line coverage; the board
model is fully covered. The browser e2e specs added for the task board and schedule templates were
not run in this preparation. These figures do not establish native mobile behavior, the production
Ait center's webhook support, or the Google Play and TestFlight jobs, which run only from the tag.
Release asset/signing gates are separate checks.
