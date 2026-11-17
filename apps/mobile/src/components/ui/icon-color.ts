import type { Theme } from "@/styles/theme";

export const mutedIconColorMapping = (theme: Theme) => ({
  color: theme.colors.foregroundMuted,
});

export const extraMutedIconColorMapping = (theme: Theme) => ({
  color: theme.colors.foregroundExtraMuted,
});

export const foregroundIconColorMapping = (theme: Theme) => ({
  color: theme.colors.foreground,
});

export const destructiveIconColorMapping = (theme: Theme) => ({
  color: theme.colors.destructive,
});

export const accentForegroundIconColorMapping = (theme: Theme) => ({
  color: theme.colors.accentForeground,
});

export const statusSuccessIconColorMapping = (theme: Theme) => ({
  color: theme.colors.statusSuccess,
});

export const statusDangerIconColorMapping = (theme: Theme) => ({
  color: theme.colors.statusDanger,
});

export const statusWarningIconColorMapping = (theme: Theme) => ({
  color: theme.colors.statusWarning,
});
