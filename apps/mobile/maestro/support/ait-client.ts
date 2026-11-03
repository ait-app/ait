import { randomUUID } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { DaemonClient } from "@ait/client/internal/daemon-client";
import { createWebSocketTransportFactory } from "@ait/client/internal/daemon-client-websocket-transport";
import { parseHostPort, serializeConnectionUriForStorage } from "@ait/protocol/daemon-endpoints";
import { createNodeWebSocketFactory } from "../../e2e/support/helpers/node-ws-factory";

export function readMaestroConnection(
  env: Readonly<Record<string, string | undefined>> = process.env,
) {
  const raw = env.AIT_MAESTRO_SERVER_URL;
  const token = env.AIT_MAESTRO_TOKEN;
  if (!raw || !token?.trim()) {
    throw new Error("Set AIT_MAESTRO_SERVER_URL and AIT_MAESTRO_TOKEN for an Ait test server");
  }
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error("AIT_MAESTRO_SERVER_URL must be an HTTP(S) origin or a /v1/ws URL");
  }
  if (
    !["http:", "https:", "ws:", "wss:"].includes(url.protocol) ||
    !["", "/", "/v1/ws"].includes(url.pathname) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      "Use an Ait origin or /v1/ws URL; pass authentication only via AIT_MAESTRO_TOKEN",
    );
  }
  const useTls = url.protocol === "https:" || url.protocol === "wss:";
  const port = Number(url.port || (useTls ? 443 : 80));
  const native = parseHostPort(env.AIT_MAESTRO_DIRECT_ENDPOINT ?? `${url.hostname}:${port}`);
  const http = new URL(url);
  http.protocol = useTls ? "https:" : "http:";
  http.pathname = "/v1/server/info";
  url.protocol = useTls ? "wss:" : "ws:";
  url.pathname = "/v1/ws";
  return {
    token,
    port,
    hostname: url.hostname,
    infoUrl: http.toString(),
    websocketUrl: url.toString(),
    connectionUri: serializeConnectionUriForStorage({ ...native, useTls, password: token }),
  };
}

export async function connectMaestroClient(
  env: Readonly<Record<string, string | undefined>> = process.env,
) {
  const connection = readMaestroConnection(env);
  const response = await fetch(connection.infoUrl, {
    headers: { Authorization: `Bearer ${connection.token}` },
    signal: AbortSignal.timeout(5_000),
  });
  if (!response.ok) throw new Error(`Ait server info returned HTTP ${response.status}`);
  const info = (await response.json()) as {
    server_id?: string;
    protocol?: { major?: number };
    lifecycle?: string;
  };
  if (!info.server_id || info.protocol?.major !== 1 || info.lifecycle !== "ready") {
    throw new Error("The configured endpoint is not a ready Ait v1 server");
  }
  const client = new DaemonClient({
    url: connection.websocketUrl,
    password: connection.token,
    clientId: `ait-maestro-${randomUUID()}`,
    clientType: "cli",
    transportFactory: createWebSocketTransportFactory(createNodeWebSocketFactory()),
  });
  try {
    await client.connect();
    return client;
  } catch (error) {
    await client.close().catch(() => undefined);
    throw error;
  }
}

export async function addMaestroProjects(client: DaemonClient, paths: string[], receipt: string) {
  const ids: string[] = [];
  for (const cwd of paths) {
    const result = await client.addProject(cwd);
    if (result.error || !result.project) throw new Error(result.error ?? "Missing Ait project");
    ids.push(result.project.projectId);
    // Persist each successful creation so interrupted setup can still clean up.
    await writeFile(receipt, JSON.stringify(ids), { mode: 0o600 });
  }
  return ids;
}

export async function removeMaestroProjects(client: DaemonClient, receipt: string) {
  const ids: unknown = JSON.parse(await readFile(receipt, "utf8"));
  if (!Array.isArray(ids) || ids.some((id) => typeof id !== "string")) {
    throw new Error("Invalid Maestro project receipt");
  }
  for (const id of ids) {
    await client.removeProject(id);
  }
}
