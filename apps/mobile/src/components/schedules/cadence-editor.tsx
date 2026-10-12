import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useCallback, useMemo, useReducer, useState, type ReactNode } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import type { ScheduleCadence } from "@ait/protocol/schedule/types";
import type { FieldControlSize } from "@/components/ui/control-geometry";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { SelectField, type SelectFieldOption } from "@/components/ui/select-field";
import {
  CADENCE_PRESET_OPTIONS,
  getCadencePresetOptions,
  resolveCronPresetDisplay,
  resolveCronPresetId,
} from "@/schedules/schedule-cadence-options";
import { getDeviceTimeZone } from "@/utils/device-timezone";
import {
  everyMsToParts,
  partsToEveryMs,
  type IntervalUnit,
  describeCron,
  validateCron,
} from "@/utils/schedule-format";

import { formatLocalDateTime, parseLocalDateTime } from "@/schedules/schedule-date-time";

type CronCadence = Extract<ScheduleCadence, { type: "cron" }>;

export interface CadenceEditorProps {
  value: ScheduleCadence;
  onChange: (next: ScheduleCadence) => void;
  error?: string;
  size?: FieldControlSize;
}

function getCronPreview(
  expression: string,
  timezone: string,
  error: string | null,
  t: TFunction,
): string | null {
  if (error || !expression) {
    return null;
  }
  return describeCron({ type: "cron", expression, timezone }, t) ?? expression;
}

function buildCronCadence(expression: string, timezone: string): CronCadence {
  return { type: "cron", expression, timezone };
}

function CronCadenceEditor({
  value,
  onChange,
  error,
  size = "md",
}: CadenceEditorProps & { value: CronCadence }) {
  const { t } = useTranslation();
  const presetOptions = useMemo<SelectFieldOption<string>[]>(
    () =>
      getCadencePresetOptions(t).map((option) => ({
        ...option,
        value: option.id,
        testID: `schedule-cadence-preset-${option.id}`,
      })),
    [t],
  );
  const deviceTimeZone = useMemo(getDeviceTimeZone, []);
  const normalizedValue = value;
  const [cronText, setCronText] = useState(() => normalizedValue.expression);
  const [fieldResetKey, bumpFieldResetKey] = useReducer((key: number) => key + 1, 0);
  const cronTimeZone = normalizedValue.timezone ?? deviceTimeZone;
  const trimmedCron = cronText.trim();
  const localCronError = trimmedCron ? validateCron(trimmedCron, t) : null;
  const effectiveError = error ?? localCronError;
  const preview = getCronPreview(trimmedCron, cronTimeZone, effectiveError ?? null, t);
  const currentCadence = useMemo<CronCadence>(
    () => buildCronCadence(trimmedCron, cronTimeZone),
    [cronTimeZone, trimmedCron],
  );
  const selectedPresetId = resolveCronPresetId(currentCadence);
  const selectedPresetDisplay = resolveCronPresetDisplay(currentCadence, t);

  const handlePresetChange = useCallback(
    (presetId: string) => {
      const preset = CADENCE_PRESET_OPTIONS.find((option) => option.id === presetId);
      if (!preset) {
        return;
      }
      setCronText(preset.expression);
      bumpFieldResetKey();
      onChange(buildCronCadence(preset.expression, cronTimeZone));
    },
    [cronTimeZone, onChange],
  );

  const handleCronChange = useCallback(
    (text: string) => {
      setCronText(text);
      onChange(buildCronCadence(text.trim(), cronTimeZone));
    },
    [cronTimeZone, onChange],
  );

  let feedback: ReactNode = null;
  if (effectiveError) {
    feedback = <Text style={styles.error}>{effectiveError}</Text>;
  } else if (preview) {
    feedback = <Text style={styles.preview}>{preview}</Text>;
  }

  return (
    <Field label={t("schedules.cadence.label")}>
      <View style={styles.stack}>
        <SelectField
          label={t("schedules.cadence.label")}
          value={selectedPresetId === "custom" ? null : selectedPresetId}
          selectedDisplay={selectedPresetDisplay}
          options={presetOptions}
          onChange={handlePresetChange}
          placeholder={t("schedules.cadence.select")}
          emptyText={t("schedules.cadence.empty")}
          searchable={false}
          title={t("schedules.cadence.label")}
          size={size}
          triggerTestID="schedule-cadence-preset-trigger"
          field={false}
        />

        <FormTextInput
          size={size}
          testID="cadence-cron-expression"
          accessibilityLabel={t("schedules.cadence.expression")}
          initialValue={cronText}
          resetKey={`cadence-cron-${fieldResetKey}`}
          onChangeText={handleCronChange}
          placeholder="0 9 * * *"
          autoCapitalize="none"
          autoCorrect={false}
          spellCheck={false}
          style={styles.cronInput}
        />
        <Field label={t("schedules.cadence.timezone")}>
          <FormTextInput
            size={size}
            testID="cadence-timezone"
            accessibilityLabel={t("schedules.cadence.timezone")}
            initialValue={cronTimeZone}
            onChangeText={(timezone) => onChange(buildCronCadence(trimmedCron, timezone.trim()))}
            autoCapitalize="none"
            autoCorrect={false}
            placeholder={deviceTimeZone}
          />
        </Field>
        {feedback}
      </View>
    </Field>
  );
}

export function CadenceEditor({ value, onChange, error, size = "md" }: CadenceEditorProps) {
  const { t } = useTranslation();
  const timezone = useMemo(getDeviceTimeZone, []);
  const options = useMemo<SelectFieldOption<ScheduleCadence["type"]>[]>(
    () => [
      {
        id: "once",
        value: "once",
        label: t("schedules.cadence.once"),
        testID: "schedule-cadence-kind-once",
      },
      {
        id: "every",
        value: "every",
        label: t("schedules.cadence.interval"),
        testID: "schedule-cadence-kind-every",
      },
      {
        id: "cron",
        value: "cron",
        label: t("schedules.cadence.calendar"),
        testID: "schedule-cadence-kind-cron",
      },
    ],
    [t],
  );
  const selectKind = useCallback(
    (kind: ScheduleCadence["type"]) => {
      if (kind === value.type) return;
      onChange(
        kind === "once"
          ? { type: "once", at: new Date(Date.now() + 60 * 60 * 1000).toISOString() }
          : kind === "every"
            ? { type: "every", everyMs: 60 * 60 * 1000 }
            : { type: "cron", expression: "0 9 * * *", timezone },
      );
    },
    [onChange, timezone, value.type],
  );
  return (
    <View style={styles.stack}>
      <SelectField
        label={t("schedules.cadence.trigger")}
        value={value.type}
        selectedDisplay={options.find((option) => option.value === value.type) ?? null}
        placeholder={t("schedules.cadence.select")}
        emptyText={t("schedules.cadence.empty")}
        options={options}
        onChange={selectKind}
        searchable={false}
        size={size}
        triggerTestID="schedule-cadence-kind-trigger"
      />
      {value.type === "cron" ? (
        <CronCadenceEditor value={value} onChange={onChange} error={error} size={size} />
      ) : value.type === "every" ? (
        <IntervalCadenceEditor value={value} onChange={onChange} error={error} size={size} />
      ) : (
        <Field
          label={t("schedules.cadence.triggerAt")}
          hint={t("schedules.cadence.localTime", { timezone })}
          error={error}
        >
          <FormTextInput
            size={size}
            initialValue={formatLocalDateTime(value.at)}
            onChangeText={(text) => onChange({ type: "once", at: parseLocalDateTime(text) ?? "" })}
            placeholder="YYYY-MM-DD HH:mm"
            testID="cadence-once-at"
            accessibilityLabel={t("schedules.cadence.triggerAt")}
            autoCapitalize="none"
            autoCorrect={false}
          />
        </Field>
      )}
    </View>
  );
}

function IntervalCadenceEditor({
  value,
  onChange,
  error,
  size,
}: CadenceEditorProps & { value: Extract<ScheduleCadence, { type: "every" }> }) {
  const { t } = useTranslation();
  const initial = everyMsToParts(value.everyMs);
  const [amount, setAmount] = useState(String(initial.value));
  const [unit, setUnit] = useState<IntervalUnit>(initial.unit);
  const options = useMemo<SelectFieldOption<IntervalUnit>[]>(
    () =>
      (["milliseconds", "seconds", "minutes", "hours", "days"] as const).map((unit) => ({
        id: unit,
        value: unit,
        label: t(`schedules.cadence.units.${unit}`),
      })),
    [t],
  );
  return (
    <Field label={t("schedules.cadence.interval")} error={error}>
      <View style={styles.stack}>
        <FormTextInput
          size={size}
          initialValue={amount}
          keyboardType="number-pad"
          testID="cadence-interval-amount"
          accessibilityLabel={t("schedules.cadence.intervalAmount")}
          onChangeText={(text) => {
            setAmount(text);
            onChange({
              type: "every",
              everyMs: /^[1-9]\d*$/.test(text.trim()) ? partsToEveryMs(Number(text), unit) : 0,
            });
          }}
        />
        <SelectField
          label={t("schedules.cadence.intervalUnit")}
          value={unit}
          options={options}
          searchable={false}
          size={size}
          selectedDisplay={options.find((option) => option.value === unit) ?? null}
          placeholder={t("schedules.cadence.select")}
          emptyText={t("schedules.cadence.empty")}
          triggerTestID="cadence-interval-unit"
          onChange={(next) => {
            setUnit(next);
            onChange({
              type: "every",
              everyMs: /^[1-9]\d*$/.test(amount.trim()) ? partsToEveryMs(Number(amount), next) : 0,
            });
          }}
        />
      </View>
    </Field>
  );
}

const styles = StyleSheet.create((theme) => ({
  stack: {
    gap: theme.spacing[3],
  },
  cronInput: {
    fontFamily: theme.fontFamily.mono,
  },
  preview: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
  },
  error: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.palette.red[300],
  },
}));
