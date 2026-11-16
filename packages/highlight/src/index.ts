export type { HighlightStyle, HighlightToken } from "./types.js";
export { getLanguageForFile, isLanguageSupported } from "./parsers.js";
export { createCodeMirrorHighlightStyle } from "./syntax-roles.js";
export { highlightCode } from "./highlighter.js";
export { darkHighlightColors, lightHighlightColors } from "./colors.js";
export type { SyntaxThemeId } from "./themes.js";
export { SYNTAX_THEME_OPTIONS, isSyntaxThemeId, resolveSyntaxColors } from "./themes.js";
