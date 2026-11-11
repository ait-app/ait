import { describe, expect, it } from "vitest";
import { buildGithubChannelConfig, type GithubChannelDraft } from "./github-channel-model";

function draft(overrides: Partial<GithubChannelDraft> = {}): GithubChannelDraft {
  return {
    channelId: "11111111-1111-4111-8111-111111111111",
    appId: "123456",
    installationId: "789012",
    mention: "ait-review",
    privateKeyRef: "env:AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY",
    webhookSecretRef: "env:AIT_SERVER_CREDENTIAL_GITHUB_WEBHOOK_SECRET",
    enabled: true,
    repositories: [
      {
        key: "one",
        fullName: "example/backend",
        projectId: "project-backend",
        allowedSenders: "maintainer, release-manager",
      },
    ],
    ...overrides,
  };
}

describe("GitHub Channel form model", () => {
  it("normalizes a bot mention and sender list", () => {
    const result = buildGithubChannelConfig(draft({ mention: "@ait-review[bot]" }));
    expect(result).toEqual({
      error: null,
      config: expect.objectContaining({
        mention: "ait-review",
        repositories: [
          expect.objectContaining({
            allowedSenders: ["maintainer", "release-manager"],
          }),
        ],
      }),
    });
  });

  it("rejects duplicate repositories ignoring case", () => {
    const result = buildGithubChannelConfig(
      draft({
        repositories: [
          draft().repositories[0],
          {
            ...draft().repositories[0],
            key: "two",
            fullName: "Example/Backend",
          },
        ],
      }),
    );
    expect(result).toEqual({ config: null, error: "duplicateRepository" });
  });

  it("accepts only private-key credential references, never key values", () => {
    const result = buildGithubChannelConfig(draft({ privateKeyRef: "actual-secret" }));
    expect(result).toEqual({ config: null, error: "privateKeyRef" });
  });
});
