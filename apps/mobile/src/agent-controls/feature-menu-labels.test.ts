import { afterEach, expect, it } from "vitest";
import { i18n } from "@/i18n/i18next";
import type { AgentAutocompleteOption } from "@/hooks/use-agent-autocomplete";
import { presentFeature, presentFeatureCommand } from "./feature-menu-labels";

afterEach(async () => {
  await i18n.changeLanguage("en");
});

function command(source: string, name = "compact"): AgentAutocompleteOption {
  return {
    id: name,
    label: `/${name}`,
    type: "provider_command",
    command: {
      name,
      source,
      description: "Native description",
      argumentHint: "[argument]",
      aliases: ["compress"],
    },
  };
}

it("localizes owned commands and feature names, keeping raw commands separate", async () => {
  await i18n.changeLanguage("zh-CN");
  expect(presentFeatureCommand(command("builtin"), "omp", i18n.t)).toEqual({
    label: "压缩上下文",
    description: "/compact",
    searchTerms: ["compact", "/compress"],
  });
  expect(
    presentFeature(
      { type: "toggle", id: "advisorEnabled", label: "Advisor", value: true },
      "omp",
      i18n.t,
    ),
  ).toEqual({ label: "指导模型", description: "/advisor" });
  expect(presentFeatureCommand(command("extension"), "omp", i18n.t)?.label).toBe("compact");
  expect(presentFeatureCommand(command("skill"), "omp", i18n.t)?.label).toBe("compact");
  expect(
    presentFeatureCommand(command("extension", "TeamReview"), "omp", i18n.t)?.description,
  ).toBe("/TeamReview");
});

it("has actual translations in every supported language and updates with the active language", async () => {
  for (const locale of ["en", "zh-CN", "ja", "ko", "fr", "es", "ru", "ar", "pt-BR"]) {
    await i18n.changeLanguage(locale);
    for (const name of ["advisor", "btw", "compact", "goal", "vibe"]) {
      const row = presentFeatureCommand(command("builtin", name), "omp", i18n.t)!;
      expect(
        i18n.exists(`agentControls.commands.${name}`, { lng: locale, fallbackLng: false }),
      ).toBe(true);
      expect(row.label).not.toBe(`/${name}`);
      expect(row.description).toBe(`/${name}`);
    }
  }
});
