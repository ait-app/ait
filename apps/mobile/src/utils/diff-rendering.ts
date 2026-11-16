interface StyledHighlightToken {
  text: string;
  style: string | null;
}

export function compactHighlightTokens(
  tokens: readonly StyledHighlightToken[],
): StyledHighlightToken[] {
  const compacted: StyledHighlightToken[] = [];
  for (const token of tokens) {
    if (token.text.length === 0) {
      continue;
    }
    const previous = compacted.at(-1);
    if (previous?.style === token.style) {
      previous.text += token.text;
      continue;
    }
    compacted.push({ text: token.text, style: token.style });
  }
  return compacted;
}
