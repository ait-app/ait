# Accessing Ait MCP

Use this reference when the current agent has no connected Ait tools. Ait provides stateless
Streamable HTTP at `/mcp/agents`; a shell with Bash and curl can call the same tools directly.
This path does not register native tools with the Provider.

## Connection context

Use connection values supplied for the target daemon. The HTTP examples use this convention:

| Environment value  | Meaning                                                         |
| ------------------ | --------------------------------------------------------------- |
| `AIT_MCP_URL`      | Explicit complete endpoint URL, including `/mcp/agents`         |
| `AIT_SERVER_TOKEN` | Runtime Bearer credential accepted by the current daemon        |
| `AIT_AGENT_ID`     | Optional caller Agent identity from the trusted runtime or user |

The skill does not populate these values. `AIT_MCP_URL` and `AIT_AGENT_ID` are access conventions
for these examples, not claims that Provider auto-injection is implemented. If endpoint or credential
configuration is absent, report that the launcher/MCP client needs configuration. Keep credentials
in the runtime; use availability checks rather than printing their values or asking for them in chat.
For an external call, omit the caller header and provide explicit placement for creation operations.

Preserve existing authentication and connection values. A 401/403 requires correcting the supplied
connection, not removing authorization. A 404 can mean the endpoint is disabled. Repeating a mutation
after an uncertain result requires the retry checks described in the main skill.

## Initialize and discover

Build the headers in the current Bash session. The following blocks share that session; if the shell
tool starts a new process for each call, include the header setup again with each request. Run without
shell tracing:

```bash
: "${AIT_MCP_URL:?Missing runtime Ait MCP endpoint}"
: "${AIT_SERVER_TOKEN:?Missing runtime Ait credential}"
ait_mcp_headers=(
  --header "Authorization: Bearer $AIT_SERVER_TOKEN"
  --header 'Content-Type: application/json'
  --header 'Accept: application/json, text/event-stream'
  --header 'MCP-Protocol-Version: 2025-11-25'
)
if [[ -n ${AIT_AGENT_ID:-} ]]; then
  ait_mcp_headers+=(--header "X-Ait-Agent-Id: $AIT_AGENT_ID")
fi
curl --silent --show-error --fail-with-body --connect-timeout 5 --max-time 70 \
  "${ait_mcp_headers[@]}" --data-binary @- "$AIT_MCP_URL" <<'JSON'
{"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"ait-orchestration-skill","version":"1"}}}
JSON
```

Check the initialization result. The current daemon returns JSON without an MCP session ID.
Acknowledge initialization, then discover the live catalog:

```bash
curl --silent --show-error --fail-with-body --connect-timeout 5 --max-time 70 \
  "${ait_mcp_headers[@]}" --data-binary @- "$AIT_MCP_URL" <<'JSON'
{"jsonrpc":"2.0","method":"notifications/initialized"}
JSON
curl --silent --show-error --fail-with-body --connect-timeout 5 --max-time 70 \
  "${ait_mcp_headers[@]}" --data-binary @- "$AIT_MCP_URL" <<'JSON'
{"jsonrpc":"2.0","id":"catalog","method":"tools/list","params":{}}
JSON
```

The notification succeeds with an empty HTTP 202. The catalog has no continuation cursor;
individual list tools can return `nextCursor` for their own data. No standalone SSE stream or
DELETE session cleanup is required.

## Call a tool

Replace the tool name and arguments with a tool present in the live catalog. This read-only example
lists saved profiles; follow the same envelope for an authorized operation:

```bash
curl --silent --show-error --fail-with-body --connect-timeout 5 --max-time 70 \
  "${ait_mcp_headers[@]}" --data-binary @- "$AIT_MCP_URL" <<'JSON'
{"jsonrpc":"2.0","id":"profiles","method":"tools/call","params":{"name":"list_profiles","arguments":{}}}
JSON
```

Check HTTP status, JSON-RPC `error`, and `result.isError`. Successful tool data is in
`result.structuredContent`, with an equivalent text content block. An HTTP 200 alone does not prove
that an operation succeeded. After a lost mutation response, use durable request identities and
read tools to determine whether it already took effect before retrying.
