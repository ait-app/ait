import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { formatAmount, formatResetLabel } from "./format";
import { formatNativeUsageText } from "./copy";
import { UsageMeter } from "./meter";
import type { UsageBalance } from "./types";

interface ResolvedBalance {
  amountText: string;
  usedPct: number | null;
}

function resolveBalance(balance: UsageBalance, locale: string, t: TFunction): ResolvedBalance {
  const { used, remaining, limit, unit } = balance;
  const format = (value: number) => formatAmount(value, unit, locale);
  if (limit != null && limit > 0) {
    const usedAmount = used ?? (remaining != null ? limit - remaining : null);
    const usedPct = usedAmount != null ? (usedAmount / limit) * 100 : null;
    const usedText = usedAmount != null ? format(usedAmount) : "—";
    return { amountText: `${usedText} / ${format(limit)}`, usedPct };
  }
  if (remaining != null) {
    return {
      amountText: t("providerUsage.remaining", { amount: format(remaining) }),
      usedPct: null,
    };
  }
  if (used != null) {
    return { amountText: format(used), usedPct: null };
  }
  return { amountText: "—", usedPct: null };
}

export function UsageBalanceBar({
  balance,
  retained = false,
}: {
  balance: UsageBalance;
  retained?: boolean;
}) {
  const { i18n, t } = useTranslation();
  const { amountText, usedPct } = resolveBalance(balance, i18n.language, t);
  const resetLabel = formatResetLabel(balance.resetsAt, t, retained);

  return (
    <View style={styles.container}>
      <View style={styles.labelRow}>
        <Text style={styles.label} numberOfLines={1}>
          {formatNativeUsageText(balance.label, t)}
        </Text>
        <Text style={styles.value}>
          {amountText}
          {resetLabel ? <Text style={styles.reset}>{` · ${resetLabel}`}</Text> : null}
        </Text>
      </View>
      {usedPct != null ? <UsageMeter percent={usedPct} tone={balance.tone ?? "default"} /> : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: {
    gap: 3,
  },
  labelRow: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  label: {
    flexShrink: 1,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  value: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  reset: {
    color: theme.colors.foregroundMuted,
    fontWeight: theme.fontWeight.normal,
  },
}));
