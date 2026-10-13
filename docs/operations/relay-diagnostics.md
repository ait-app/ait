# Relay reconnect diagnostics

The daemon emits structured relay lifecycle logs at `info` and failures at `warn`.
Use the daemon's `log_level = "info"` configuration (or `--log-level info`) to
retain normal closes as well as failures. Desktop-managed daemon stderr is in
`daemon.log` under its configured data directory.

Correlate a center access log's `/relay/sessions/<id>/client` and `/daemon` requests
with `relay_session_id` in the Host daemon log. A WebSocket `101` only establishes
that the upgrade succeeded; it does not prove the connection remained healthy.

## Fields and interpretation

- `relay_control` carries `generation` and `node_session_id`. A control failure
  closes its active data tasks; those tasks may be dropped before emitting their
  individual completion events.
- `relay_session` carries `relay_session_id` and control `epoch`. Its completion
  includes `elapsed_ms`; setup failures include the safe public error category.
- `relay_endpoint.side` identifies `remote_relay` (Host to center) or `local_daemon`
  (bridge to the local application WebSocket). Read and write refer to that socket.
- `close_frame` includes a numeric `close_code` when the peer supplied one. `eof`
  means the stream ended. These alone do not establish a network root cause.
- `peer_queue_timeout` means forwarding to the other endpoint's bounded queue
  exceeded five seconds. `write_timeout` means a socket write exceeded five seconds.
  `pong_queue_full`, `peer_queue_closed`, and `incoming_queue_closed` identify other
  bridge exit paths. The sibling endpoint can finish as a consequence of cancellation;
  inspect the preceding cause event rather than treating both finishes as failures.
- `operation`, `error_kind`, and optional `os_error` distinguish socket failures
  without formatting peer-controlled errors. Setup `stage` distinguishes remote
  connect, pairing, client hello, local connect, and local hello write. Explicit
  timeout events also report `timeout_ms`.
- `center_cancelled` means a control command cancelled the session. A control
  `cancelled` event can mean stop, replacement, or runtime shutdown.

Tickets, bearer tokens, URLs, message bodies, raw error strings, and peer close
reason text are intentionally omitted. A `protocol` or `capacity` category may
require further targeted investigation; this logging does not claim a root cause
or alter retries, deadlines, queue sizes, or forwarding behavior.

## Incident motivating this instrumentation

On 2026-10-12, 13:19–13:21 UTC+8, one Firefox client repeatedly established relay
sessions that lasted about 3.5–4 seconds. Matched center proxy records mostly showed
`client closed` on the daemon leg and `upstream closed` on the browser leg. The
center service containers had no restart or OOM indication. These particular short
sessions did not match the configured 60-second heartbeat or 75-second proxy idle
timeouts.

This narrows the investigation to the Host direction, but does not distinguish
an application close, bridge error, or network interruption. Existing Host logs
had no corresponding close reason, and the bridge discarded transport errors.
The instrumentation supplies the missing evidence for a subsequent reproduction;
it is not a confirmed fix for that incident.

## Connection reuse and recovery

Relay Host runtimes connect saved hosts when their account becomes available and
retain their active client across page subscriptions and host metadata updates.
Fresh account discovery warms a host once when it becomes available again; repeated
unchanged discovery does not restart its connection. A failed initial probe can be
retried immediately on recovery, even when no active client exists. Concurrent
recovery signals share the in-flight probe.

Foregrounding and visible browser/Electron `online` events bypass pending reconnect
backoff. Healthy sockets are verified and retained. Native account suspension marks
discovery stale; successful discovery after resume or account recovery wakes existing
relay hosts. This does not add a native OS network listener or keep native relay
sockets alive while the account is suspended.

Active relay clients retry after 500 ms, then 1, 2, 4, 8, 16 and at most 30 seconds
on consecutive failures. One-shot host probes still disable automatic retries.
The 45-second setup deadline and pairing/hello deadlines are unchanged.

The Rust relay connector shares one TLS configuration and bounded session cache
across control, data, download and subsequent reconnect sockets. Certificate and
hostname verification remain enabled; TLS early data remains disabled. TLS session
resumption requires support from the center's TLS endpoint. Outbound relay and
loopback WebSockets enable `TCP_NODELAY`.

See the [implementation validation](../reports/clients/relay-connect-latency.md).
