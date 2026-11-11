# @ait/client

Private, repository-local Ait client library. All implementation
source lives in `packages/client/src` and is built from this checkout. Consumers
use `file:` dependencies; no Paseo SDK is fetched from npm.

The initial source was imported from
`getpaseo/paseo@2c8e8a826810337492cc5a38bb0bbd705b6fb632` (`0.9.0-beta.2`).
Original attribution is preserved in [paseo/LICENSE](../../paseo/LICENSE).
The inherited Paseo Relay/E2EE transport has been removed. Ait online business
sessions use the repository-owned Hub client; dedicated downloads and account
status still use their separate Relay channels. See
[ADR-204](../../docs/decisions/clients/adr-204-hub-only-online-business-transport.md).

## Local build

```sh
npm ci
npm run verify:local-packages
npm run build:sdk
```

The App dependency is `"@ait/client": "file:../../packages/client"`; this package
in turn consumes `"@ait/protocol": "file:../protocol"`. `build:sdk` generates
JavaScript and TypeScript declarations under each package's `dist/`.

## Connecting to Ait

`DaemonClient` and the public `createPaseoClient` API speak the Rust v1 protocol
natively. Connect to `/v1/ws` and supply the daemon's Bearer token. Callers may
supply a raw `transportFactory` or `webSocketFactory` for their platform;
protocol negotiation, request correlation, subscriptions, chunks and binary frame
routing live in [the SDK protocol modules](src/protocol/connection.ts).

The App's [transport selection](../../apps/mobile/src/runtime/daemon-transport/connection.ts)
chooses Electron IPC, native WebSockets, browser one-use tickets or account Hub.
Direct TCP/SSH retains four capability channels; account Hub uses one session.
Node callers should provide a WebSocket implementation that supports Authorization
headers; standard browser WebSockets require the ticket transport.

[Maestro's connection helper](../../apps/mobile/maestro/support/ait-client.ts) and
[App E2E](../../apps/mobile/e2e/README.md) use the same native SDK without an App
protocol adapter. High-level API names such as `createPaseoClient` remain for
source compatibility; the original Paseo wire protocol is no longer a client mode.
See [ADR-211](../../docs/decisions/clients/adr-211-sdk-native-daemon-protocol.md).

## Session statistics

`DaemonClient.getAgentStatistics(agentId)` returns typed `AgentSessionStatistics`.
Check `serverInfo.features.agentStatisticsGet === true` before offering this read;
the SDK also rejects unsupported hosts without sending the RPC. Usage has scope
`provider_snapshot`: it is the latest provider snapshot, not an assured session
total or last-turn total. Unknown values stay `null`; known zero stays `0`.
Unknown usage scopes are rejected rather than interpreted as totals.
