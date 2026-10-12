import React from "react";
import { Text, View } from "react-native";
import { z } from "zod";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import type { AgentPermissionResponse } from "@ait/protocol/agent-types";
import { Button } from "@/components/ui/button";
import { QuestionFormCard } from "@/components/question-form-card";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { botStyles as styles } from "./styles";

const PermissionSchema = z.object({
  id: z.string(),
  provider: z.string().default(""),
  name: z.string().default(""),
  kind: z.enum(["tool", "plan", "question", "mode", "other"]).default("other"),
  title: z.string().optional(),
  description: z.string().optional(),
  input: z.record(z.string(), z.unknown()).optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
  actions: z
    .array(z.object({ id: z.string(), label: z.string(), behavior: z.enum(["allow", "deny"]) }))
    .optional(),
});

export function BotPermission({
  serverId,
  botId,
  turnId,
  permission,
  onResolved,
}: {
  serverId: string;
  botId: string;
  turnId: string;
  permission: unknown;
  onResolved: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const parsed = PermissionSchema.safeParse(permission);
  const mutation = useMutation({
    mutationFn: async (response: AgentPermissionResponse) => {
      if (!client || !parsed.success) throw new Error(t("bots.requestFailed"));
      await client.botRequest("bot.permission.resolve.request", {
        botId,
        turnId,
        permissionId: parsed.data.id,
        response,
      });
    },
    onSuccess: onResolved,
  });
  if (!parsed.success) return <Text style={styles.error}>{t("bots.requestFailed")}</Text>;
  const request = parsed.data;
  const actions = request.actions?.length
    ? request.actions
    : [
        { id: "allow", label: t("bots.allow"), behavior: "allow" as const },
        { id: "deny", label: t("bots.deny"), behavior: "deny" as const },
      ];
  return (
    <View style={styles.card}>
      <Text style={styles.text}>{request.title || request.name || t("bots.approval")}</Text>
      {request.description ? <Text style={styles.hint}>{request.description}</Text> : null}
      {request.kind === "question" ? (
        <QuestionFormCard
          permission={{ key: request.id, agentId: botId, request }}
          onRespond={(response) => mutation.mutate(response)}
          isResponding={mutation.isPending}
        />
      ) : (
        <>
          {request.input ? (
            <Text selectable style={styles.hint}>
              {JSON.stringify(request.input, null, 2)}
            </Text>
          ) : null}
          <View style={styles.row}>
            {actions.map((action) => (
              <Button
                key={action.id}
                size="sm"
                variant="outline"
                disabled={mutation.isPending}
                onPress={() =>
                  mutation.mutate({
                    behavior: action.behavior,
                    ...(request.actions?.length ? { selectedActionId: action.id } : {}),
                  })
                }
              >
                {action.label}
              </Button>
            ))}
          </View>
        </>
      )}
      {mutation.error ? <Text style={styles.error}>{mutation.error.message}</Text> : null}
    </View>
  );
}
