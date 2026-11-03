import WebSocket from "ws";
import { createPaseoClient, type PaseoClientConfig } from "@ait/client";

const token = process.env.AIT_SERVER_TOKEN;
if (!token) throw new Error("Set AIT_SERVER_TOKEN for the target daemon");
const client = createPaseoClient({
  url: process.env.AIT_SERVER_URL ?? "ws://127.0.0.1:7316/v1/ws",
  password: token,
  webSocketFactory: (url, options) =>
    new WebSocket(url, { headers: options?.headers }) as unknown as ReturnType<
      NonNullable<PaseoClientConfig["webSocketFactory"]>
    >,
});

await client.connect();

try {
  const agent = await client.agents.create({
    config: {
      provider: "codex/gpt-5.5",
    },
    cwd: process.cwd(),
    prompt: "Inspect this repository and name the next useful task.",
  });

  const result = await agent.waitForFinish();
  if (result.status !== "idle") {
    throw new Error(result.error ?? `Agent stopped with status: ${result.status}`);
  }

  console.log(result.lastMessage);
} finally {
  await client.close();
}
