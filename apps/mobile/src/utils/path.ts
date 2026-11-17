/** Strips trailing separators but keeps a bare root ("/", "C:/"), normalized to forward slashes. */
export function trimTrailingSeparators(value: string): string {
  if (value === "/" || /^[A-Za-z]:[\\/]?$/.test(value)) {
    return value.replace(/\\/g, "/");
  }
  return value.replace(/[\\/]+$/, "");
}

export function isAbsolutePath(value: string): boolean {
  return value.startsWith("/") || value.startsWith("\\\\") || /^[A-Za-z]:[\\/]/.test(value);
}

/** True for "~" and for paths under it ("~/x", "~\\x"); "~user" is not home-relative. */
export function isHomeRelativePath(value: string): boolean {
  return value === "~" || value.startsWith("~/") || value.startsWith("~\\");
}

/** Trims a working directory and drops trailing slashes, keeping a bare "/" root. */
export function normalizeCwd(cwd: string): string {
  const trimmed = cwd.trim();
  if (trimmed === "/") {
    return trimmed;
  }
  return trimmed.replace(/\/+$/, "");
}
