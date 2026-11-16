import type { SupportedLocale } from "./locales";
import * as ar from "./resources/ar";
import * as en from "./resources/en";
import * as es from "./resources/es";
import * as fr from "./resources/fr";
import * as ja from "./resources/ja";
import * as ko from "./resources/ko";
import * as ptBR from "./resources/pt-BR";
import * as ru from "./resources/ru";
import * as zhCN from "./resources/zh-CN";

type WidenStringLeaves<T> = {
  [K in keyof T]: T[K] extends string ? string : WidenStringLeaves<T[K]>;
};

/** The shape every locale must fill in: English with its string literals widened. */
export type TranslationResources = WidenStringLeaves<typeof en>;

/**
 * Every locale's translations, keyed by locale code.
 *
 * Each locale lists its own components in `resources/<locale>/index.ts`; a component is a module
 * that exists in `resources/en/`.
 */
export const resources = {
  ar,
  en,
  es,
  fr,
  ja,
  ko,
  "pt-BR": ptBR,
  ru,
  "zh-CN": zhCN,
} satisfies Record<SupportedLocale, TranslationResources>;

export { en };
