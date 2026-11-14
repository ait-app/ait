export function normalizeBranchOptionName(input: string | null | undefined): string | null {
  const trimmed = input?.trim();
  if (!trimmed || trimmed === "HEAD") {
    return null;
  }

  let normalized = trimmed;
  if (normalized.startsWith("refs/heads/")) {
    normalized = normalized.slice("refs/heads/".length);
  } else if (normalized.startsWith("refs/remotes/")) {
    normalized = normalized.slice("refs/remotes/".length);
  }
  if (normalized.startsWith("origin/")) {
    normalized = normalized.slice("origin/".length);
  }

  return normalized.length > 0 && normalized !== "HEAD" ? normalized : null;
}
