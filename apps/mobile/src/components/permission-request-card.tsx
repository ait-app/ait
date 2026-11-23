import React, { useCallback, useEffect, useMemo, useState } from "react";
import { Pressable, Text, View, type PressableStateCallbackType } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { ArrowUpRight, Check, X } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { useIsMutating, useMutation } from "@tanstack/react-query";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type {
  AgentPermissionAction,
  AgentPermissionResponse,
  ToolCallDetail,
} from "@ait/protocol/agent-types";
import { respondToQuestionWithTimeout } from "@/agent-stream/question-response";
import { PlanCard } from "@/components/plan-card";
import { QuestionFormCard } from "@/components/question-form-card";
import { parseQuestionFormQuestions } from "@/components/question-form-card-core";
import { ToolCallDetailsContent } from "@/components/tool-call-details";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { Button } from "@/components/ui/button";
import { useIsCompactFormFactor } from "@/constants/layout";
import type { Theme } from "@/styles/theme";
import type { PendingPermission } from "@/types/shared";
import { toErrorMessage } from "@/utils/error-messages";

const ThemedLoadingSpinner = withUnistyles(LoadingSpinner);
const ThemedCheckIcon = withUnistyles(Check);
const ThemedXIcon = withUnistyles(X);

const primaryColorMapping = (theme: Theme) => ({
  color: theme.colors.foreground,
});
const mutedColorMapping = (theme: Theme) => ({
  color: theme.colors.foregroundMuted,
});

const pressableStyle = ({
  pressed,
  hovered = false,
}: PressableStateCallbackType & { hovered?: boolean }) => [
  permissionStyles.optionButton,
  hovered ? permissionStyles.optionButtonHovered : null,
  pressed ? permissionStyles.optionButtonPressed : null,
];

interface PermissionActionButtonProps {
  action: AgentPermissionAction;
  isRespondingAction: boolean;
  isResponding: boolean;
  isPrimary: boolean;
  Icon: typeof ThemedCheckIcon;
  testID: string;
  onPress: (action: AgentPermissionAction) => void;
}

function PermissionActionButton({
  action,
  isRespondingAction,
  isResponding,
  isPrimary,
  Icon,
  testID,
  onPress,
}: PermissionActionButtonProps) {
  const handlePress = useCallback(() => onPress(action), [onPress, action]);
  const optionTextStyle = isPrimary
    ? [permissionStyles.optionText, permissionStyles.optionTextPrimary]
    : permissionStyles.optionText;
  const colorMapping = isPrimary ? primaryColorMapping : mutedColorMapping;
  return (
    <Pressable
      accessibilityRole="button"
      testID={testID}
      style={pressableStyle}
      onPress={handlePress}
      disabled={isResponding}
    >
      {isResponding && isRespondingAction ? (
        <ThemedLoadingSpinner size="small" uniProps={colorMapping} />
      ) : (
        <View style={permissionStyles.optionContent}>
          <Icon size={14} uniProps={colorMapping} />
          <Text style={optionTextStyle}>{action.label}</Text>
        </View>
      )}
    </Pressable>
  );
}

export function PermissionRequestCard({
  permission,
  client,
  panel = false,
  compact = false,
  onOpenDetails,
}: {
  panel?: boolean;
  compact?: boolean;
  onOpenDetails?: () => void;
  permission: PendingPermission;
  client: DaemonClient | null;
}) {
  const { t } = useTranslation();
  const isMobile = useIsCompactFormFactor();

  const { request } = permission;
  const isPlanRequest = request.kind === "plan";
  const isCommand = (panel || compact) && request.name === "commandExecution";
  const title = isPlanRequest
    ? t("agentStream.permission.plan")
    : isCommand && (!request.title || request.title === request.name)
      ? t("taskBoard.runCommand")
      : (request.title ?? request.name ?? t("agentStream.permission.required"));
  const description = request.description ?? "";
  const resolvedToolCallDetail = useMemo<ToolCallDetail>(() => {
    if (
      (panel || compact) &&
      request.name === "commandExecution" &&
      typeof request.input?.command === "string" &&
      Object.keys(request.input).every((key) =>
        ["command", "cwd", "threadId", "turnId", "itemId"].includes(key),
      )
    ) {
      return {
        type: "shell",
        command: request.input.command,
        ...(typeof request.input.cwd === "string" ? { cwd: request.input.cwd } : {}),
      };
    }
    return (
      request.detail ?? {
        type: "unknown" as const,
        input: request.input ?? null,
        output: null,
      }
    );
  }, [panel, compact, request.name, request.detail, request.input]);
  const resolvedActions = useMemo((): AgentPermissionAction[] => {
    if (request.kind === "question") {
      return [];
    }
    if (Array.isArray(request.actions) && request.actions.length > 0) {
      return request.actions;
    }
    return [
      {
        id: "reject",
        label: t("agentStream.permission.deny"),
        behavior: "deny",
        variant: "danger",
        intent: "dismiss",
      },
      {
        id: "accept",
        label: isPlanRequest
          ? t("agentStream.permission.implement")
          : t("agentStream.permission.accept"),
        behavior: "allow",
        variant: "primary",
      },
    ];
  }, [isPlanRequest, request, t]);

  const planMarkdown = useMemo(() => {
    if (!request) {
      return undefined;
    }
    const planFromMetadata =
      typeof request.metadata?.planText === "string" ? request.metadata.planText : undefined;
    if (planFromMetadata) {
      return planFromMetadata;
    }
    const candidate = request.input?.["plan"];
    if (typeof candidate === "string") {
      return candidate;
    }
    return undefined;
  }, [request]);

  const mutationKey = ["permission-response", permission.agentId, request.id];
  const pendingResponses = useIsMutating({ mutationKey });
  const permissionMutation = useMutation({
    mutationKey,
    networkMode: "always",
    retry: false,
    mutationFn: async (input: {
      agentId: string;
      requestId: string;
      response: AgentPermissionResponse;
    }) => {
      if (!client) {
        throw new Error(t("common.errors.daemonClientUnavailable"));
      }
      if (request.kind === "question") {
        return respondToQuestionWithTimeout(client, input);
      }
      return client.respondToPermissionAndWait(
        input.agentId,
        input.requestId,
        input.response,
        15000,
      );
    },
  });
  const {
    reset: resetPermissionMutation,
    mutateAsync: respondToPermission,
    isPending: isOwnResponsePending,
  } = permissionMutation;
  const isResponding = isOwnResponsePending || pendingResponses > 0;

  const [respondingActionId, setRespondingActionId] = useState<string | null>(null);

  useEffect(() => {
    resetPermissionMutation();
    setRespondingActionId(null);
  }, [permission.request.id, resetPermissionMutation]);
  const handleResponse = useCallback(
    (response: AgentPermissionResponse) => {
      respondToPermission({
        agentId: permission.agentId,
        requestId: permission.request.id,
        response,
      }).catch((error) => {
        console.error("[PermissionRequestCard] Failed to respond to permission:", error);
      });
    },
    [permission.agentId, permission.request.id, respondToPermission],
  );
  const handleActionPress = useCallback(
    (action: AgentPermissionAction) => {
      setRespondingActionId(action.id);
      if (action.behavior === "allow") {
        handleResponse({
          behavior: "allow",
          selectedActionId: action.id,
        });
        return;
      }
      handleResponse({
        behavior: "deny",
        selectedActionId: action.id,
        message: "Denied by user",
      });
    },
    [handleResponse],
  );

  const optionsContainerStyle = useMemo(
    () => [
      permissionStyles.optionsContainer,
      !isMobile && permissionStyles.optionsContainerDesktop,
    ],
    [isMobile],
  );

  if (compact && request.kind === "question") {
    const questions = parseQuestionFormQuestions(request.input);
    return (
      <View style={permissionStyles.compact}>
        <Text style={permissionStyles.compactSummary} numberOfLines={3}>
          {questions?.map((question) => question.question).join(" · ") || description || title}
        </Text>
        <Button
          size="xs"
          variant="outline"
          leftIcon={ArrowUpRight}
          onPress={onOpenDetails}
          testID="task-answer"
          disabled={!client}
        >
          {t("taskBoard.answer")}
        </Button>
      </View>
    );
  }

  if (request.kind === "question") {
    return (
      <QuestionFormCard
        permission={permission}
        onRespond={handleResponse}
        isResponding={isResponding || !client || permissionMutation.isSuccess}
        responseError={
          permissionMutation.variables?.requestId === permission.request.id && !isResponding
            ? permissionMutation.error?.message
            : null
        }
      />
    );
  }

  const footer = (
    <>
      {!panel && !compact ? (
        <Text testID="permission-request-question" style={permissionStyles.question}>
          {t("agentStream.permission.question")}
        </Text>
      ) : null}

      {permissionMutation.isError ? (
        <Text accessibilityRole="alert" style={permissionStyles.error}>
          {toErrorMessage(permissionMutation.error)}
        </Text>
      ) : null}
      <View style={compact ? permissionStyles.compactOptions : optionsContainerStyle}>
        {resolvedActions.map((action) => {
          const isPrimary = action.variant === "primary";
          const isRespondingAction = respondingActionId === action.id;
          const Icon = action.behavior === "allow" ? ThemedCheckIcon : ThemedXIcon;
          let testID: string;
          if (action.behavior === "deny") testID = "permission-request-deny";
          else if (action.id === "accept" || action.id === "implement")
            testID = "permission-request-accept";
          else testID = `permission-request-action-${action.id}`;

          if (panel || compact) {
            return (
              <Button
                key={action.id}
                variant={!compact && isPrimary ? "default" : "outline"}
                size={compact && !isMobile ? "xs" : "sm"}
                leftIcon={compact ? (action.behavior === "allow" ? Check : X) : undefined}
                style={
                  compact
                    ? action.behavior === "allow"
                      ? permissionStyles.allowButton
                      : permissionStyles.denyButton
                    : undefined
                }
                textStyle={
                  compact
                    ? action.behavior === "allow"
                      ? permissionStyles.allowText
                      : permissionStyles.denyText
                    : undefined
                }
                testID={testID}
                loading={isResponding && isRespondingAction}
                disabled={!client || isResponding || permissionMutation.isSuccess}
                onPress={() => handleActionPress(action)}
              >
                {action.label}
              </Button>
            );
          }
          return (
            <PermissionActionButton
              key={action.id}
              action={action}
              isRespondingAction={isRespondingAction}
              isResponding={isResponding || !client || permissionMutation.isSuccess}
              isPrimary={isPrimary}
              Icon={Icon}
              testID={testID}
              onPress={handleActionPress}
            />
          );
        })}
      </View>
    </>
  );

  if (compact) {
    const summary =
      resolvedToolCallDetail.type === "shell"
        ? resolvedToolCallDetail.command
        : planMarkdown || description || (request.input ? JSON.stringify(request.input) : "");
    return (
      <View style={permissionStyles.compact}>
        <Pressable
          onPress={onOpenDetails}
          accessibilityRole="button"
          accessibilityLabel={t("taskBoard.details")}
        >
          <Text style={permissionStyles.compactSummary} numberOfLines={3}>
            <Text style={permissionStyles.compactTitle}>{title}</Text>
            {summary && summary !== title ? ` · ${summary}` : ""}
          </Text>
        </Pressable>
        {footer}
      </View>
    );
  }

  if (isPlanRequest && planMarkdown) {
    return (
      <PlanCard
        title={title}
        description={description}
        text={planMarkdown}
        outcome="pending"
        footer={footer}
        testID="permission-plan-card"
        disableOuterSpacing
      />
    );
  }

  return (
    <View style={panel ? permissionStyles.panel : permissionStyles.container}>
      <Text style={permissionStyles.title}>{title}</Text>

      {description ? <Text style={permissionStyles.description}>{description}</Text> : null}

      {planMarkdown ? (
        <PlanCard
          title={t("agentStream.permission.proposedPlan")}
          text={planMarkdown}
          testID="permission-plan-card"
          disableOuterSpacing
        />
      ) : null}

      {!isPlanRequest ? (
        <ToolCallDetailsContent detail={resolvedToolCallDetail} maxHeight={200} />
      ) : null}

      {footer}
    </View>
  );
}

const permissionStyles = StyleSheet.create((theme) => ({
  compact: {
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
    paddingTop: theme.spacing[2],
    gap: theme.spacing[2],
    alignItems: "flex-start",
  },
  compactOptions: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[1.5] },
  compactSummary: { color: theme.colors.foreground, fontSize: theme.fontSize.sm, lineHeight: 18 },
  compactTitle: { fontWeight: theme.fontWeight.semibold },
  allowButton: { borderColor: theme.colors.statusSuccess },
  denyButton: { borderColor: theme.colors.statusDanger },
  allowText: { color: theme.colors.statusSuccess },
  denyText: { color: theme.colors.statusDanger },
  panel: { gap: theme.spacing[3] },
  error: { color: theme.colors.statusDanger, fontSize: theme.fontSize.base },
  container: {
    marginVertical: theme.spacing[3],
    padding: theme.spacing[3],
    borderRadius: theme.spacing[2],
    borderWidth: 1,
    gap: theme.spacing[2],
    backgroundColor: theme.colors.surface1,
    borderColor: theme.colors.border,
  },
  title: {
    fontSize: theme.fontSize.base,
    lineHeight: 22,
    color: theme.colors.foreground,
  },
  description: {
    fontSize: theme.fontSize.base,
    lineHeight: 20,
    color: theme.colors.foregroundMuted,
  },
  section: {
    gap: theme.spacing[2],
  },
  sectionTitle: {
    fontSize: theme.fontSize.sm,
  },
  question: {
    fontSize: theme.fontSize.base,
    marginTop: theme.spacing[1],
    marginBottom: theme.spacing[1],
    color: theme.colors.foregroundMuted,
  },
  optionsContainer: {
    gap: theme.spacing[2],
  },
  optionsContainerDesktop: {
    flexDirection: "row",
    flexWrap: "wrap",
    justifyContent: "flex-start",
    alignItems: "center",
    width: "100%",
  },
  optionButton: {
    paddingVertical: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    borderRadius: theme.borderRadius.md,
    alignItems: "center",
    borderWidth: theme.borderWidth[1],
    backgroundColor: theme.colors.surface1,
    borderColor: theme.colors.borderAccent,
  },
  optionButtonHovered: {
    backgroundColor: theme.colors.surface2,
  },
  optionButtonPressed: {
    opacity: 0.9,
  },
  optionContent: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  optionText: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.normal,
    color: theme.colors.foregroundMuted,
  },
  optionTextPrimary: {
    color: theme.colors.foreground,
  },
}));
