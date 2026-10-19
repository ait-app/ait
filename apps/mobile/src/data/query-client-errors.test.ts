import { afterEach, expect, it } from "vitest";
import { appErrorReporter } from "@/diagnostics/error-reporter";
import { queryClient } from "./query-client";

afterEach(() => {
  queryClient.clear();
  appErrorReporter.takePending();
});

it("records query failures with their key and the original exception without a toast", async () => {
  appErrorReporter.takePending();
  const error = new Error("Query diagnostics fixture", { cause: new Error("Host unavailable") });
  await expect(
    queryClient.fetchQuery({
      queryKey: ["workspace", "workspace-1"],
      queryFn: () => {
        throw error;
      },
      retry: false,
    }),
  ).rejects.toBe(error);
  expect(appErrorReporter.takePending()).toEqual([]);
  const report = appErrorReporter.getReports().at(-1)!;
  expect(report.source).toBe("query");
  expect(report.context).toContain("workspace-1");
  expect(report.details).toContain("Host unavailable");
});

it("reports mutation failures without recording request bodies or variables", async () => {
  appErrorReporter.takePending();
  const mutation = queryClient.getMutationCache().build(queryClient, {
    mutationKey: ["save-settings"],
    mutationFn: async (_variables: { password: string }) => {
      throw new Error("Mutation diagnostics fixture");
    },
  });
  await expect(mutation.execute({ password: "private-password" })).rejects.toThrow(
    "Mutation diagnostics fixture",
  );
  const [report] = appErrorReporter.takePending();
  expect(report.source).toBe("mutation");
  expect(report.context).toContain("save-settings");
  expect(JSON.stringify(report)).not.toContain("private-password");
});
