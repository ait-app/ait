# ADR-180: Independent native session-list response admission

- Status: Accepted.
- Date: 2026-10-21.
- Scope: `crates/provider`; ordinary and single-connection WebSocket transports.
- Extends [ADR-088](adr-088-provider-catalog-startup-isolation.md) and
  [ADR-091](adr-091-independent-session-execution.md).

## Context

The import sheet requests recent sessions from every eligible provider. The ordinary client
sends these requests over one WebSocket channel. Although the provider worker can run reads
independently, session-list dispatch awaited native work before returning control to the socket
reader. A slow OpenCode listing therefore delayed a subsequent Pi listing. The single-connection
transport had the same problem within its provider capability worker.

The [latency investigation](../../reports/providers/pi-session-list-performance-2026-10-21.md)
reproduced a Pi request taking 10.65 seconds behind OpenCode, versus 17.6 ms on another connection
to the same daemon at the same time. Existing bounded catalog-response admission already solves
this transport ownership problem for model discovery.

## Decision

- Route `provider.sessions.recent.list.request` through the existing catalog response-admission
  path. Reserve its command and response capacity synchronously, register a tracked response
  task, and return control to the connection dispatcher before native listing completes.
- Share the existing 64 command permits and 64 discovery/list response slots. Keep a response
  slot until the result is handed to the bounded outbound queue or its response owner is
  dropped. Exhaustion returns the existing `catalog_busy` error.
- Keep listing execution in the existing independent provider read path. Retain its shared
  eight-operation history budget and provider-query concurrency limit. Do not route session
  listing through model-catalog caching or change provider-specific discovery semantics.
- Preserve the original request ID and source connection for each reply. Accepted listing
  work remains tracked through disconnect and shutdown. Responses to independent requests can
  arrive out of order.
- Keep Agent import, refresh, resume, mutation ordering, and lifecycle ownership unchanged.
  Use the same provider dispatcher for both WebSocket transport modes; do not add per-provider
  physical connections or broaden generic request concurrency.

## Consequences

One provider's session listing no longer occupies the socket reader or provider capability
worker while another provider's listing is waiting to be admitted. A list can still wait for
history capacity, native work, filesystem I/O, or output capacity. Pi descriptor caching and
OpenCode prompt-preview optimization remain separate changes.

Provider-owned tests cover dispatch responsiveness, correlated validation errors, response
capacity retained after computation, and shutdown after disconnect. Daemon transport tests
hold a native list behind a deterministic gate and require a later listing to reply before
the gate opens, in both ordinary and single-connection modes.
