# Paseo Client SDK Examples

These examples use only the public SDK root:

```ts
import { createPaseoClient, type PaseoClient } from "@ait/client";
```

Build the repository-local library first with `npm run build:sdk` from the repository
root. These examples target Ait's Rust daemon at `/v1/ws`. Supply its Bearer token
and a platform WebSocket factory that supports headers (Node), or the App's
one-use ticket transport (browser). No protocol adapter is required. Connection helpers accept `PaseoClientConfig`, including the URL, Bearer token and
platform factory; see [quickstart.ts](quickstart.ts) for a complete Node configuration
and the [library README](../README.md) for transport details.

- `quickstart.ts` runs one agent and prints its reply. It is a standalone script with the URL at the top; the rest export functions.
- `workspaces.ts` covers creating a fresh workspace, opening by directory, refreshing, and archiving.
- `agents-and-providers.ts` covers provider discovery, creating agents, and waiting for turns.
- `events-and-timeline.ts` covers subscribing to workspace, agent, and timeline events, plus refetching a timeline page.
- `issue-to-agent.ts` turns an issue record into a visible Paseo workspace and agent.
- `parallel-review.ts` launches several reviewers concurrently and cleans them up.
- `provider-settings.ts` covers provider settings that are currently daemon config-backed.

Provider profiles, provider env vars, custom binaries, and additional models are still raw daemon config behavior. The SDK exposes them through `client.config.get()` and `client.config.patch()` until first-class provider settings RPCs exist.
