import React from "react";
import { useTranslation } from "react-i18next";
import { SelectField } from "@/components/ui/select-field";

export function BotSelect({
  label,
  value,
  options,
  onChange,
  disabled = false,
  testID,
  onAccess,
}: {
  label: string;
  value: string;
  options: { id: string; label: string }[];
  onChange: (value: string) => void;
  disabled?: boolean;
  testID: string;
  /** Refresh the provider catalog when the picker opens or a choice is committed. */
  onAccess?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <SelectField
      label={label}
      value={value}
      selectedDisplay={{
        label:
          options.find((option) => option.id === value)?.label || value || t("bots.defaultOption"),
      }}
      options={options.map((option) => ({ ...option, value: option.id }))}
      onOpen={onAccess}
      onChange={(value) => {
        onAccess?.();
        onChange(value);
      }}
      disabled={disabled}
      searchable
      placeholder={t("bots.defaultOption")}
      emptyText={t("bots.defaultOption")}
      testID={testID}
      triggerTestID={`${testID}-trigger`}
    />
  );
}
