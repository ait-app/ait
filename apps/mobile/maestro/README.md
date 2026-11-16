# Maestro Flows

This directory contains local mobile UI flows. Keep flows small enough that a
failure screenshot proves the intended behavior, not just that the app launched.

## Ait connection and local packages

Install and build from the repository root:

```sh
npm ci
npm run verify:local-packages
npm run build:ui-deps
```

Run a dedicated Ait Rust daemon for native testing, then export its origin and
Bearer token in the shell running the harness. Both values are required; the
scripts never default to a developer server or the old `/ws` protocol.

```sh
export AIT_MAESTRO_SERVER_URL=http://127.0.0.1:48123
export AIT_MAESTRO_TOKEN=<token-from-your-test-server>
```

`support/ait-client.ts` checks authenticated `/v1/server/info`, then connects via
`/v1/ws` using `@ait/client` and the production Rust adapter. No relay build is
needed. Host, port, TLS and token are supplied to the App's advanced connection
URI field. `AIT_MAESTRO_DIRECT_ENDPOINT` optionally overrides the device-visible
`host:port` (for an iOS physical device, use the server's LAN address).

Flows are rendered as parsed YAML into private output directories. The renderer
copies only referenced flows, sets the debug app id consistently, and safely
quotes tokens and project names. Rendered flow files contain the test token and
are deleted on harness exit; screenshots/logs remain local test artifacts. Use a
dedicated test-server token for these native automation sessions.

## iOS Sidebar Close Regression

`ios-sidebar-close-regression.yaml` exercises close swipes over a semantic header
and the nested workspace list without activating the content below the swipe.
Start the `dev.ait.mobile.debug` dev client against the intended Metro server first;
the flow preserves that running connection.

```bash
maestro test apps/mobile/maestro/ios-sidebar-close-regression.yaml --udid <simulator-udid>
```

## iOS Sidebar Drag Cancellation Regression

`test-sidebar-drag-cancellation-ios.sh` creates three temporary projects and
uses real native slow swipes to cover successful reorder, a second touch during
the drop spring, horizontal close cancellation, outer-list scroll recovery, and
a subsequent reorder. The harness removes its Ait projects and local fixture
directories on exit.

Start the debug dev client against the intended Metro server, then run:

```bash
bash apps/mobile/maestro/test-sidebar-drag-cancellation-ios.sh
```

## New Workspace Android Flow

Use these files when debugging or extending workspace creation on Android:

- `test-workspace-create-android-crash.sh` runs the full regression harness.
- `workspace-create-android-crash.yaml` is the full Maestro flow used by the
  harness.
- `record-workspace-create-android-focus.sh` records only the focused repro
  window after setup.
- `workspace-create-android-ready-sidebar.yaml` stages the app with the Android
  sidebar open and a prepared project visible.
- `workspace-create-android-create-focused.yaml` starts from that staged sidebar
  and performs the actual workspace creation.

The reusable pieces live in `flows/`:

- `flows/android-dev-client.yaml` handles Expo dev launcher/dev menu screens.
- `flows/connect-direct-if-welcome.yaml` connects to the Ait server only when
  the welcome screen is visible.
- `flows/open-prepared-project-sidebar.yaml` waits for the home screen, opens
  the compact Android sidebar, and waits for the prepared project.
- `flows/new-workspace-open-from-sidebar.yaml` taps the project row's
  new-workspace action and waits for `/new`.
- `flows/new-workspace-select-codex-gpt54.yaml` selects a real provider/model.
- `flows/new-workspace-submit-and-assert-created.yaml` taps `Create` and proves
  the app landed on the created workspace.

Compose new workspace scenarios out of these primitives instead of copying the
old full flow. The shell scripts render the top-level flows and their referenced nested flows
into the same temp directory, preserving relative `runFlow` paths while replacing
`${AIT_MAESTRO_*}` placeholders.

The flow is intentionally strict. It must:

1. Open a prepared project from Ait.
2. Tap the project row's new-workspace action.
3. Select an actual provider/model before tapping `Create`.
4. Tap `Create`.
5. Assert the app lands on a workspace header and the draft composer.
6. Assert `New workspace`, `Select a model`, and the Android redbox text are not
   visible.
7. For the shell harness, grep logcat for `failed to insert view` and
   `specified child already has a parent`.

Do not weaken this flow to only wait for `message-input-root`. That can pass on
the wrong route. The header assertion and the `New workspace` negative assertion
are what prove the redirect actually completed.

The scripts assume a development build with package id `dev.ait.mobile.debug`, an
already-running authenticated Ait server configured as above, and a connected
Android device or emulator. ADB reverses the configured local server port; the
scripts do not restart that server. Android reversal requires a loopback server URL.

```bash
bash apps/mobile/maestro/test-workspace-create-android-crash.sh
bash apps/mobile/maestro/record-workspace-create-android-focus.sh
```

Optional environment:

```bash
AIT_MAESTRO_APP_ID=dev.ait.mobile.debug
AIT_MAESTRO_DIRECT_ENDPOINT=127.0.0.1:48123
AIT_MAESTRO_PROJECT_PATH=/path/to/git/repo
```

## Validation without a mobile device

```sh
npm run test --workspace=@ait/mobile -- maestro/support
E2E_AITD_BIN="$PWD/target/debug/daemon" npm run test:e2e --workspace=@ait/mobile -- e2e/browser/maestro-ait.spec.ts
```

The second command validates authentication, project preparation, opening and
receipt-based cleanup against an isolated Rust daemon. It does not exercise native
gestures. Those require an installed Maestro CLI and a connected device/simulator.
