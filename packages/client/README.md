# @ait/client

Private, repository-local Ait client compatibility library. All implementation
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

The library retains the compatibility client API and wire types. Ait uses its
Rust v1 protocol, so App and native test consumers must supply the production
[transport adapter](../../apps/mobile/src/runtime/rust-daemon/transport.ts),
connect to `/v1/ws`, and provide the server's Bearer token. Browser connections
use the production ticket transport. Package renaming alone does not replace
this adapter.

[Maestro's connection helper](../../apps/mobile/maestro/support/ait-client.ts) shows
a Node connection using the local `@ait/client/internal/*` modules and the same
Rust adapter. [App E2E](../../apps/mobile/e2e/README.md) starts an isolated Ait server.

The high-level client API and examples inherited from Paseo still use compatibility
names such as `createPaseoClient`; those names are not independent Ait transport
implementations. Imports and workspace builds now consistently use `@ait/*`.
