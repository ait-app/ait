# @ait/protocol

Private Ait workspace containing the local schemas, codecs and compatibility wire
types used by [@ait/client](../client/README.md) and its native daemon protocol implementation.
Source is checked in under `packages/protocol/src`; consumers use explicit `file:`
dependencies. Build with `npm run build:sdk` from the repository root.

The package namespace is independent from Paseo. Schemas describe the supported
Ait surface; retired Hub, Plugin management, Chat and Loop messages are absent
from the protocol and SDK. Pinned upstream audit snapshots remain as historical
evidence. Plugin timeline display items remain supported. Upstream attribution
remains in [paseo/LICENSE](../../paseo/LICENSE).

Paseo tool-call presentation is also owned here. Clients import display functions
and types from `@ait/protocol/paseo-tool-presentation/index`; tool labels, summary
context and detail specifications belong in the shared catalog, not client-local
tables. See the [presentation spec](../../docs/architecture/tool-call-presentation.md)
and [ADR-285](../../docs/decisions/clients/adr-285-shared-tool-call-presentation.md).
