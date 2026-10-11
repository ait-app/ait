import { afterEach, expect, test } from "vitest";
import { i18n } from "@/i18n/i18next";
import { usageCopy } from "./copy";

afterEach(async () => {
  await i18n.changeLanguage("en");
});

test("resolves labels and login remedies in the current language", async () => {
  expect(usageCopy.refreshAll).toBe("Refresh all");
  await i18n.changeLanguage("zh-CN");
  expect(usageCopy.refreshAll).toBe("全部刷新");
  expect(usageCopy.displayRemaining).toBe("剩余");
  expect(usageCopy.hostUnavailable("Laptop")).toBe("连接到 Laptop 以查看用量");
  expect(usageCopy.problem({ kind: "rejected", status: 403, refreshedBy: "codex login" })).toBe(
    "登录被拒绝（HTTP 403）。运行 codex login 以刷新登录。",
  );
  expect(
    usageCopy.problem(
      { kind: "expired", expiresAt: "2026-10-01T09:00:00Z" },
      new Date("2026-10-01T12:00:00Z"),
    ),
  ).toBe("登录已过期（3 小时前）。请重新登录。");
  expect(usageCopy.problem({ kind: "no_quota", detail: "" })).toBe(
    "此登录方式不提供套餐用量。请通过提供方 CLI 登录。",
  );
  await i18n.changeLanguage("en");
  expect(usageCopy.refreshAll).toBe("Refresh all");
});

test.each([
  [
    { kind: "expired", expiresAt: "2026-10-01T09:00:00Z", refreshedBy: "claude" },
    "Login expired 3h ago. Run claude to refresh it.",
  ],
  [{ kind: "expired", expiresAt: "2026-10-01T09:00:00Z" }, "Login expired 3h ago. Sign in again."],
  [
    { kind: "rejected", status: 403, refreshedBy: "codex" },
    "Login rejected (HTTP 403). Run codex to refresh it.",
  ],
  [{ kind: "rejected", status: 401 }, "Login rejected (HTTP 401). Sign in again."],
  [{ kind: "no_quota", detail: "No active coding plan" }, "No active coding plan"],
] as const)("problem %j", (problem, sentence) => {
  expect(usageCopy.problem(problem, new Date("2026-10-01T12:00:00Z"))).toBe(sentence);
});
