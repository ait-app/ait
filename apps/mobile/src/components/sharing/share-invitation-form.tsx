import { useState } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import * as Clipboard from "expo-clipboard";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { createShareInvitation } from "@/runtime/account-state";

export function ShareInvitationForm({ center, shareId }: { center: string; shareId: string }) {
  const { t } = useTranslation();
  const [recipient, setRecipient] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [link, setLink] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const contact = recipient.trim();
  const canSubmit = Boolean(contact && contact !== "@" && !submitting);

  const generate = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setLink(null);
    setCopied(false);
    setError(null);
    try {
      setLink(await createShareInvitation(center, shareId, contact));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("sharing.invitation.error"));
    } finally {
      setSubmitting(false);
    }
  };

  const copy = async () => {
    if (!link) return;
    setError(null);
    try {
      if (!(await Clipboard.setStringAsync(link))) throw new Error("Clipboard unavailable.");
      setCopied(true);
    } catch {
      setError(t("common.errors.unableToCopy"));
    }
  };

  return (
    <View style={styles.form}>
      <Field label={t("sharing.invitation.recipient")}>
        <FormTextInput
          accessibilityLabel={t("sharing.invitation.recipient")}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="email-address"
          maxLength={320}
          editable={!submitting}
          onChangeText={(value) => {
            setRecipient(value);
            setLink(null);
            setError(null);
            setCopied(false);
          }}
          onSubmitEditing={() => void generate()}
          returnKeyType="go"
        />
      </Field>
      <View style={styles.actions}>
        <Button
          size="sm"
          loading={submitting}
          disabled={!canSubmit}
          onPress={() => void generate()}
        >
          {t("sharing.invitation.generate")}
        </Button>
        {link ? (
          <Button variant="secondary" size="sm" onPress={() => void copy()}>
            {t(copied ? "common.states.copied" : "sharing.invitation.copyLink")}
          </Button>
        ) : null}
      </View>
      {link ? (
        <Text selectable style={styles.link}>
          {link}
        </Text>
      ) : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.error}>
          {error}
        </Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  form: { gap: theme.spacing[2] },
  actions: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[2] },
  link: { color: theme.colors.foreground, fontSize: theme.fontSize.sm },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.sm },
}));
