import {
  createWebSocketTransportFactory,
  defaultWebSocketFactory,
} from "@ait/client/internal/daemon-client-websocket-transport";
import { getBrowserAccount, registerBrowserAccountTransport } from "../browser-account";
import { streamNativeAccountDownload } from "./native-account-download";
import type { TransportFactory } from "./types";

const connect = createWebSocketTransportFactory(defaultWebSocketFactory);
/** Browsers authenticate with the server's one-use, Origin-bound subprotocol. */
export const connectBrowserRelay: TransportFactory = ({ url, headers }) => {
  const ticket = headers?.Authorization?.replace(/^Bearer /, "");
  if (!ticket || !/^[A-Za-z0-9_-]+$/.test(ticket)) throw new Error("Invalid relay ticket.");
  return connect({ url, protocols: [`ait.ticket.${ticket}`] });
};
const dependencies = {
  account: getBrowserAccount,
  register: registerBrowserAccountTransport,
  connect: connectBrowserRelay,
};

const MAX_DOWNLOAD_BYTES = 200 * 1024 * 1024;
export async function downloadBrowserAccountFile(input: {
  hostId: string;
  center: string;
  token: string;
  fileName: string;
  mimeType: string | null;
  progress(bytesWritten: number, totalBytes: number): void;
}): Promise<void> {
  const chunks: ArrayBuffer[] = [];
  let received = 0;
  await streamNativeAccountDownload(
    {
      ...input,
      write(bytes) {
        received += bytes.byteLength;
        if (received > MAX_DOWNLOAD_BYTES)
          throw new Error(
            "Browser downloads are limited to 200 MiB. Use the desktop app for larger files.",
          );
        chunks.push(Uint8Array.from(bytes).buffer);
      },
    },
    dependencies,
  );
  const url = URL.createObjectURL(
    new Blob(chunks, { type: input.mimeType || "application/octet-stream" }),
  );
  try {
    const link = document.createElement("a");
    link.href = url;
    link.download = input.fileName;
    document.body.append(link);
    link.click();
    link.remove();
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
}
