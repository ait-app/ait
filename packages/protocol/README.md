# @ait/protocol

Private Ait workspace containing the local schemas, codecs and compatibility wire
types used by [@ait/client](../client/README.md) and the App Rust transport.
Source is checked in under `packages/protocol/src`; consumers use explicit `file:`
dependencies. Build with `npm run build:sdk` from the repository root.

The package namespace is independent from Paseo. Schemas describe the supported
Ait surface; retired Hub, Plugin management, Chat and Loop messages are absent
from the protocol and SDK. Pinned upstream audit snapshots remain as historical
evidence. Plugin timeline display items remain supported. Upstream attribution
remains in [paseo/LICENSE](../../paseo/LICENSE).
