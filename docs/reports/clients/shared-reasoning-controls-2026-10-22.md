# Shared reasoning-control selection validation

Date: 2026-10-22.
Baseline: `b8fadd89e7cf5ca64f609c215a69279b3bf56459`.
Source snapshot: `546d7567b47da72663fc4abf5f4851d11c66b2d0`, before this report was added.

## Finding and scope

The shared composer could display and check the first reasoning option even when no
concrete reasoning ID was selected. An explicit ID absent from the current catalog could
also be replaced in the display by the first option. The literal option ID `default` was
treated as an inheritance placeholder for existing agents, although providers can use it
as an actual selectable ID.

For example, with `off` first in a catalog that has no advertised default, an unspecified
request could look like an explicit `off` selection. With an option named `default` and
labeled "Adaptive effort", the displayed selection could differ from the submitted ID.

The implementation follows [ADR-183](../../decisions/clients/adr-183-shared-reasoning-control-selection.md)
and applies to reasoning selection, labels, and dropdown state in the shared Web, mobile,
and desktop renderer controls.

## Changes

- Preserve explicit reasoning IDs, including `default` and IDs absent from the catalog.
- Resolve existing-agent catalog defaults through `defaultThinkingOptionId` or the
  option's `isDefault` marker, without guessing from catalog order.
- Use the same resolved selection for the composer label and dropdown checkmark.
- Share reasoning-label formatting between existing-agent and draft controls.
- Display the localized "Default" label when the resolved selection is unspecified;
  keep the draft command's `thinkingOptionId` omitted in that case.
- Add the label to all nine existing translation resources and add selection/command regressions.

Profile and creation-form default initialization, provider validation, native RPC behavior,
and effective runtime metadata are outside this PR's implementation scope.

## Validation

Environment: Linux x86_64, Node.js `26.8.1`, npm `12.0.2`, and Vitest `4.1.7`.
The mobile workspace's `unit` project used its normal configuration and worker parallelism.

The successful focused regression run passed **345 tests across 37 files, with 0 failed
and 0 skipped**, in **20.08 seconds** as reported by Vitest:

```sh
npm run test --workspace=@ait/mobile -- --project unit src/composer src/provider-selection src/agent-profiles
```

Validation used application files matching the source snapshot and SHA-256 values below.
Subsequent edits changed only documentation. Tests were not rerun solely for those edits;
the total counts each test once from the successful run.

The two changed test files provide the following automated evidence:

| Case                                                     | Expected behavior                                           | Test file             |
| -------------------------------------------------------- | ----------------------------------------------------------- | --------------------- |
| No explicit ID and no catalog default                    | Selection remains unspecified; label is "Default"           | `utils.test.ts`       |
| An option has `isDefault: true`                          | Resolve that option as the catalog default                  | `utils.test.ts`       |
| Explicit ID is absent from the catalog                   | Preserve the ID and format its fallback label               | `utils.test.ts`       |
| Selected ID is literally `default`                       | Use its catalog label, including "Adaptive effort"          | Both files            |
| Explicit `default` conflicts with catalog default `high` | Display and submit `default`                                | `input-draft.test.ts` |
| Catalog default is `default` with no explicit override   | Display and submit `default`                                | `input-draft.test.ts` |
| Draft has no known reasoning default                     | Show "Default" and omit `thinkingOptionId` from the command | `input-draft.test.ts` |

Files: [control helper regressions](../../../apps/mobile/src/composer/agent-controls/utils.test.ts)
and [draft command regressions](../../../apps/mobile/src/composer/draft/input-draft.test.ts).
The remaining tests exercise directly related composer, provider-selection, and profile behavior.

The local protocol/client SDK was rebuilt and the related suites rerun successfully.
No assertions or configured test timeouts were relaxed.

Additional checks passed:

| Command or check                                   | Result                                                  |
| -------------------------------------------------- | ------------------------------------------------------- |
| `npm run build:sdk`                                | Local protocol/client SDK build passed                  |
| `npm run typecheck --workspace=@ait/mobile`        | Passed without diagnostics after the SDK rebuild        |
| `npm run check:ui`                                 | All 13 changed UI files passed; 0 warnings and 0 errors |
| Changed-file `node_modules/.bin/oxfmt --check`     | All 13 changed TypeScript/TSX files passed              |
| `npm run check:docs`                               | Documentation checker tests and local links passed      |
| Changed-document `node_modules/.bin/oxfmt --check` | ADR, report, and index formatting passed                |
| `git diff --check` and staged whitespace checks    | Passed                                                  |

Documentation and formatting checks are rerun when adding this report. Application test,
SDK build, typecheck, and UI lint results above refer to the successful earlier validation
of the same application source, rather than a new run triggered by the report.

## Limits

- Automated reasoning regressions exercise selection helpers and draft command construction.
  They do not mount the changed dropdown to verify its actual checkmark or trigger rendering.
- The shared unit suites include related component/hook tests, but no browser acceptance
  suite, manual browser inspection, native mobile build, or physical-device validation was run.
- Provider execution and effective runtime readback were not exercised against external
  model services. The tests establish client-side selection and command behavior.
- No macOS or Windows validation was run. The "Default" label describes an unspecified
  resolved selection; it does not claim a particular effective native reasoning level.

## Test coverage

Coverage was **not measured** because it was not requested. The passing test counts are
execution results, not line or branch coverage. No instrumented artifact, percentage, or
comparison with a measured baseline is claimed.

This PR changes no Rust code in `bins/` or `crates/`; Rust tests were skipped under the
repository rules. No full Rust workspace suite was run.

## Merge Danger

**Door:** two-way.

Reverting the PR restores the previous reasoning-selection display and fallback behavior.
There is no storage migration or protocol schema change.

**Blast Radius:** shared existing-agent and draft reasoning controls, across providers.

Catalog default resolution and the shared selection helper also affect consumers such as
the existing child-agent facts line. These consumers preserve explicit IDs and no longer
use the first option as a display fallback.

## Source snapshot

SHA-256 values for all 13 changed application source and test files:

```text
17e685731efcce91b55ecf59be7ea1f4d672c892b2628c898806888a45868747  apps/mobile/src/composer/agent-controls/index.tsx
b16775b626fe041e296a224d8432b0355598fbd7d2839685a5d7e437403ce1f1  apps/mobile/src/composer/agent-controls/utils.test.ts
71a29ecbe6e311fe2e1133b87fef8bf6d2950a70ba40ead6c11565469fa452f3  apps/mobile/src/composer/agent-controls/utils.ts
02756dc699fbb499548f28769e89b6a8adc46e06b3212b429540101327663978  apps/mobile/src/composer/draft/input-draft.test.ts
510744d0ae2cc5693d3c9dbeca23ef3cc256f076cb8087f071ea848db0cf2734  apps/mobile/src/i18n/resources/ar.ts
3d45f43fd56003ce2a4c21e4a664a136b395a3a4013ee851a712d7cd3066f421  apps/mobile/src/i18n/resources/en.ts
60dbfd635944d50ca7199d2ff3a1a06c3cf6406038b60866b6fcdbcf2f560b3b  apps/mobile/src/i18n/resources/es.ts
97f3b1e85fd5a9ccf1e27ef0b0a103fef48ceccdbfc8d2e32a7b2a32e7888f8c  apps/mobile/src/i18n/resources/fr.ts
4a8c7e8c91bb344d12adf1431166f463504d0a2e73e5d3128ab9017878020bf5  apps/mobile/src/i18n/resources/ja.ts
0ee10831f37324c1f0b4b5b538e74e3e8d5cc601c5617b598b3af68983eb6054  apps/mobile/src/i18n/resources/ko.ts
df019f508d618ad78be624cd2117697d09f6d15161866bac0c1bfb43197b080b  apps/mobile/src/i18n/resources/pt-BR.ts
337b6c633be4f2d1ad85c2414f432509de782e91b582d709e1596276a867693a  apps/mobile/src/i18n/resources/ru.ts
c82ea72d109317bf770e499ace9fb6d3856e5e000d384bc116a02f38fb58e56f  apps/mobile/src/i18n/resources/zh-CN.ts
```
