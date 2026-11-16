import { describe, expect, it } from "vitest";
import { i18n } from "@/i18n/i18next";
import {
  getTurnDurationLabel,
  getTurnFooterAccessibilityLabel,
} from "./assistant-turn-footer-label";

describe("getTurnDurationLabel", () => {
  it("says how long the turn worked", () => {
    expect(getTurnDurationLabel(14_000, i18n.t)).toBe("Worked for 14s");
  });

  it("reads in the active app language", async () => {
    await i18n.changeLanguage("fr");
    try {
      expect(getTurnDurationLabel(14_000, i18n.t)).toBe("A travaillé pendant 14s");
    } finally {
      await i18n.changeLanguage("en");
    }
  });
});

describe("getTurnFooterAccessibilityLabel", () => {
  it("announces the duration and the end time together", () => {
    expect(getTurnFooterAccessibilityLabel("Worked for 14s", "3:04 PM", i18n.t)).toBe(
      "Worked for 14s, ended 3:04 PM",
    );
  });

  it("reads in the active app language", async () => {
    await i18n.changeLanguage("fr");
    try {
      expect(getTurnFooterAccessibilityLabel("A travaillé pendant 14s", "15:04", i18n.t)).toBe(
        "A travaillé pendant 14s, fin : 15:04",
      );
    } finally {
      await i18n.changeLanguage("en");
    }
  });
});
