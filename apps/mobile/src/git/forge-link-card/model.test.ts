import { describe, expect, it } from "vitest";
import type { ForgeSearchItem } from "@ait/protocol/messages";
import {
  describeForgeLinkItem,
  isForgeReferenceLink,
  resolveForgeLinkRef,
  selectForgeLinkItem,
} from "./model";

const REMOTE = "git@github.com:acme/app.git";

function item(overrides: Partial<ForgeSearchItem> = {}): ForgeSearchItem {
  return {
    kind: "change_request",
    forge: "github",
    number: 795,
    title: "Show questionnaires one step at a time",
    url: "https://github.com/acme/app/pull/795",
    state: "open",
    body: null,
    labels: [],
    projectPath: "acme/app",
    ...overrides,
  };
}

describe("isForgeReferenceLink", () => {
  it.each([
    "https://github.com/acme/app/pull/795",
    "https://github.com/other/repo/issues/12#issuecomment-1",
    "https://gitlab.com/group/sub/app/-/merge_requests/73/diffs",
    "https://git.example.com/acme/app/pulls/31",
  ])("accepts %s", (href) => {
    expect(isForgeReferenceLink(href)).toBe(true);
  });

  it.each([
    "https://github.com/acme/app",
    "https://github.com/acme/app/pulls",
    "https://github.com/acme/app/issues/new",
    "https://example.com/docs",
    "src/pull/12.ts",
  ])("rejects %s", (href) => {
    expect(isForgeReferenceLink(href)).toBe(false);
  });
});

describe("resolveForgeLinkRef", () => {
  it("resolves a change request and an issue in the workspace repository", () => {
    expect(resolveForgeLinkRef("https://github.com/acme/app/pull/795/files", REMOTE)).toEqual({
      kind: "change_request",
      number: 795,
    });
    expect(resolveForgeLinkRef("https://github.com/acme/app/issues/12", REMOTE)).toEqual({
      kind: "issue",
      number: 12,
    });
  });

  it("ignores links the workspace cannot look up", () => {
    expect(resolveForgeLinkRef("https://github.com/other/repo/pull/795", REMOTE)).toBeNull();
    expect(resolveForgeLinkRef("https://github.com/acme/app/tree/main", REMOTE)).toBeNull();
    expect(resolveForgeLinkRef("src/app.ts", REMOTE)).toBeNull();
    expect(resolveForgeLinkRef("https://github.com/acme/app/pull/795", null)).toBeNull();
  });
});

describe("selectForgeLinkItem", () => {
  it("matches kind as well as number, since both share one number space only on some forges", () => {
    const issue = item({ kind: "issue", number: 795 });
    const changeRequest = item();
    expect(
      selectForgeLinkItem([issue, changeRequest], { kind: "change_request", number: 795 }),
    ).toBe(changeRequest);
    expect(selectForgeLinkItem([issue], { kind: "change_request", number: 795 })).toBeNull();
  });
});

describe("describeForgeLinkItem", () => {
  it("describes a change request with its author and diff size", () => {
    expect(
      describeForgeLinkItem(
        item({
          author: "octocat",
          authorAvatarUrl: "https://github.com/octocat.png?size=64",
          updatedAt: "2026-11-20T10:00:00Z",
          additions: 963,
          deletions: 354,
          changedFiles: 15,
        }),
      ),
    ).toEqual({
      kind: "change_request",
      title: "Show questionnaires one step at a time",
      reference: "acme/app #795",
      state: "open",
      author: "octocat",
      authorAvatarUrl: "https://github.com/octocat.png?size=64",
      updatedAt: new Date("2026-11-20T10:00:00Z"),
      diffStat: { additions: 963, deletions: 354, changedFiles: 15 },
    });
  });

  it("leaves out what an older daemon or another forge does not report", () => {
    const details = describeForgeLinkItem(
      item({ projectPath: undefined, updatedAt: "not a date" }),
    );
    expect(details).toMatchObject({
      reference: "#795",
      author: null,
      authorAvatarUrl: null,
      updatedAt: null,
      diffStat: null,
    });
  });

  it("uses the forge's own number prefix and state words", () => {
    expect(
      describeForgeLinkItem(item({ forge: "gitlab", state: "merged", projectPath: "group/app" })),
    ).toMatchObject({ reference: "group/app !795", state: "merged" });
    expect(
      describeForgeLinkItem(item({ forge: "gitlab", kind: "issue", state: "opened" })),
    ).toMatchObject({ reference: "acme/app #795", state: "open" });
    expect(describeForgeLinkItem(item({ state: "CLOSED" })).state).toBe("closed");
  });
});
