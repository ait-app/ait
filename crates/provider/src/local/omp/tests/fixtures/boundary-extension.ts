import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerCommand("boundary-write", {
    description: "Exercise the OS write boundary in an isolated test workspace",
    handler: async () => {
      writeFileSync(process.env.OMP_BOUNDARY_INSIDE!, "inside");
      let outside = "ok";
      try {
        writeFileSync(process.env.OMP_BOUNDARY_OUTSIDE!, "outside");
      } catch (error) {
        outside = (error as NodeJS.ErrnoException).code ?? "unknown";
      }
      await pi.sendMessage({
        customType: "boundary-write",
        content: `boundary:inside=ok,outside=${outside}`,
        display: true,
      });
    },
  });
  pi.registerCommand("boundary-child", {
    description: "Create an owned descendant for EOF and process-group cleanup",
    handler: async (_args, ctx) => {
      const child = spawn("/bin/sleep", ["300"], { stdio: "ignore" });
      writeFileSync(
        process.env.OMP_BOUNDARY_CHILD_RECORD!,
        JSON.stringify({ native: process.pid, child: child.pid }),
      );
      ctx.ui.notify("boundary child started", "info");
    },
  });
}
