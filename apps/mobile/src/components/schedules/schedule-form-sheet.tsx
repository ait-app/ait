import { useTranslation } from "react-i18next";
import equal from "fast-deep-equal";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactElement,
  type ReactNode,
} from "react";
import { ScrollView, Text, View } from "react-native";
import { Brain, Folder, GitBranch } from "lucide-react-native";
import { StyleSheet } from "react-native-unistyles";
import type { AgentProvider } from "@ait/protocol/agent-types";
import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import { useStoreWithEqualityFn } from "zustand/traditional";
import { AdaptiveModalSheet, type SheetHeader } from "@/components/adaptive-modal-sheet";
import { ComboboxItem } from "@/components/ui/combobox";
import { Button } from "@/components/ui/button";
import { CombinedModelSelector } from "@/components/combined-model-selector";
import { useIsCompactFormFactor } from "@/constants/layout";
import { HostStatusDotSlot } from "@/components/hosts/host-picker";
import { createControlGeometry, type FieldControlSize } from "@/components/ui/control-geometry";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { Switch } from "@/components/ui/switch";
import { getProviderIcon } from "@/components/provider-icons";
import { CadenceEditor } from "@/components/schedules/cadence-editor";
import {
  SelectField,
  SelectFieldTrigger,
  type SelectFieldDisplay,
  type SelectFieldOption,
  type SelectFieldRenderOptionInput,
} from "@/components/ui/select-field";
import { scheduleModeLabel, scheduleThinkingLabel } from "@/schedules/schedule-control-labels";
import {
  mergeProviderPreferences,
  useFormPreferences,
  type FormPreferences,
} from "@/hooks/use-form-preferences";
import { useScheduleMutations } from "@/hooks/use-schedule-mutations";
import { useAggregatedAgents } from "@/hooks/use-aggregated-agents";
import { useProjects } from "@/hooks/use-projects";
import { useHosts } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { buildScheduleProjectTargets } from "@/schedules/schedule-project-targets";
import { useScheduleFormModel } from "@/schedules/use-schedule-form-model";
import { useScheduleFormProviderSnapshot } from "@/schedules/use-schedule-form-provider-snapshot";
import type {
  ScheduleFormDisplay,
  ScheduleFormHost,
  ScheduleFormModel,
  ScheduleFormSnapshot,
  ScheduleFormState,
  ScheduleFormWorkspace,
} from "@/schedules/schedule-form-model";
import {
  formatLocalDateTime,
  isValidMaxRuns,
  isValidScheduleTimezone,
  parseLocalDateTime,
} from "@/schedules/schedule-date-time";
import { useScheduleWorkspaces } from "@/schedules/use-schedule-workspaces";
import { validateCron } from "@/utils/schedule-format";
import { toErrorMessage } from "@/utils/error-messages";
import { getDeviceTimeZone } from "@/utils/device-timezone";

export interface ScheduleFormSheetProps {
  serverId?: string;
  visible: boolean;
  onClose: () => void;
  mode: "create" | "edit";
  schedule?: ScheduleSummary;
}

export function WorkspaceScheduleForm({
  serverId,
  workspace,
  onClose,
  schedule,
  onSaved,
}: {
  serverId: string;
  workspace: ScheduleFormWorkspace;
  onClose: () => void;
  schedule?: ScheduleSummary;
  onSaved?: (schedule: ScheduleSummary) => void;
}): ReactElement {
  return (
    <ScheduleFormContent
      serverId={serverId}
      workspace={workspace}
      mode={schedule ? "edit" : "create"}
      schedule={schedule}
      onSaved={onSaved}
      visible
      inline
      onClose={onClose}
    />
  );
}

function resolveCreateServerId(input: {
  mode: "create" | "edit";
  serverId: string | null | undefined;
  hosts: readonly ScheduleFormHost[];
}): string | null {
  if (input.mode === "edit") {
    return input.serverId ?? null;
  }
  if (input.serverId !== undefined) {
    return input.serverId;
  }
  if (input.hosts.length === 1) {
    return input.hosts[0]?.serverId ?? null;
  }
  return null;
}

function buildScheduleHostOptionTestId(serverId: string): string {
  return `schedule-host-option-${serverId}`;
}

function buildThinkingOptionTestId(optionId: string): string {
  return `schedule-thinking-option-${optionId}`;
}

function openKey(props: ScheduleFormSheetProps): string {
  if (props.mode === "edit") {
    return `edit:${props.serverId ?? ""}:${props.schedule?.id ?? ""}`;
  }
  return `create:${props.serverId ?? ""}`;
}

function selectScheduleHosts(
  hosts: readonly { serverId: string; label: string }[],
): (state: ReturnType<typeof useSessionStore.getState>) => ScheduleFormHost[] {
  return (state) =>
    hosts.map((host) => ({
      serverId: host.serverId,
      label: host.label,
      supportsWorkspaceMultiplicity:
        state.sessions[host.serverId]?.serverInfo?.features?.workspaceMultiplicity === true,
    }));
}

function buildSnapshot(input: {
  mode: "create" | "edit";
  serverId: string | undefined;
  schedule: ScheduleSummary | undefined;
  hosts: readonly ScheduleFormHost[];
  projectTargets: ReturnType<typeof buildScheduleProjectTargets>;
  preferences: FormPreferences;
  timezone: string;
  workspace?: ScheduleFormWorkspace;
  agents: ScheduleFormSnapshot["defaults"]["agents"];
}): ScheduleFormSnapshot {
  const schedule = input.schedule
    ? { ...input.schedule, serverId: input.serverId, serverName: undefined }
    : undefined;
  return {
    mode: input.mode,
    schedule,
    hosts: input.hosts,
    defaults: {
      serverId: resolveCreateServerId({
        mode: input.mode,
        serverId: input.serverId,
        hosts: input.hosts,
      }),
      projectTargets: input.projectTargets,
      preferences: input.preferences,
      timezone: input.timezone,
      workspace: input.workspace,
      agents: input.agents,
    },
  };
}

function updateSelectionPreferences(input: {
  preferences: FormPreferences;
  provider: AgentProvider;
  model: string;
  mode: string;
  thinkingOptionId: string;
  isolation: "local" | "worktree";
}): FormPreferences {
  const model = input.model.trim();
  const mode = input.mode.trim();
  const thinkingOptionId = input.thinkingOptionId.trim();
  return {
    ...mergeProviderPreferences({
      preferences: input.preferences,
      provider: input.provider,
      updates: {
        model: model || undefined,
        mode: mode || undefined,
        ...(model && thinkingOptionId ? { thinkingByModel: { [model]: thinkingOptionId } } : {}),
      },
    }),
    isolation: input.isolation,
  };
}

export function ScheduleFormSheet(props: ScheduleFormSheetProps): ReactElement | null {
  const [renderedProps, setRenderedProps] = useState<ScheduleFormSheetProps | null>(() =>
    props.visible ? props : null,
  );
  const [sheetVisible, setSheetVisible] = useState(props.visible);
  const livePropsRef = useRef(props);
  const closeRequestedRef = useRef(false);
  livePropsRef.current = props;

  useEffect(() => {
    if (props.visible) {
      if (closeRequestedRef.current) {
        return;
      }
      setRenderedProps(props);
      setSheetVisible(true);
      return;
    }
    if (renderedProps) {
      setSheetVisible(false);
    }
  }, [props, renderedProps]);

  const requestClose = useCallback(() => {
    closeRequestedRef.current = true;
    setSheetVisible(false);
  }, []);

  const handleDismiss = useCallback(() => {
    const dismissedProps = livePropsRef.current;
    closeRequestedRef.current = false;
    setRenderedProps(null);
    setSheetVisible(false);
    if (dismissedProps.visible) {
      dismissedProps.onClose();
    }
  }, []);

  if (!renderedProps) {
    return null;
  }

  return (
    <ScheduleFormScope
      key={openKey(renderedProps)}
      {...renderedProps}
      visible={sheetVisible}
      onClose={requestClose}
      onDismiss={handleDismiss}
    />
  );
}

function ScheduleFormScope(props: ScheduleFormSheetProps & { onDismiss?: () => void }) {
  const { t } = useTranslation();
  const workspaces = useScheduleWorkspaces();
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const ownedId = props.schedule?.workspaceId;
  const options = useMemo(
    () =>
      workspaces
        .filter((workspace) => !props.serverId || workspace.serverId === props.serverId)
        .map((workspace) => ({
          id: `${workspace.serverId}:${workspace.id}`,
          value: `${workspace.serverId}:${workspace.id}`,
          label: workspace.name,
          description: `${workspace.serverName} · ${workspace.cwd}`,
          testID: `schedule-workspace-option-${workspace.serverId}-${workspace.id}`,
        })),
    [props.serverId, workspaces],
  );
  const workspace = workspaces.find((workspace) =>
    ownedId
      ? workspace.id === ownedId && workspace.serverId === props.serverId
      : `${workspace.serverId}:${workspace.id}` === selectedKey,
  );
  return (
    <AdaptiveModalSheet
      visible={props.visible}
      onClose={props.onClose}
      onDismiss={props.onDismiss}
      header={{
        title:
          props.mode === "edit"
            ? t("schedules.edit", { product: t("schedules.product.schedule").toLowerCase() })
            : t("schedules.new"),
      }}
      testID={workspace ? "schedule-form-sheet" : "schedule-workspace-sheet"}
    >
      {workspace ? (
        <ScheduleFormContent
          key={`${workspace.serverId}:${workspace.id}`}
          {...props}
          serverId={workspace.serverId}
          workspace={workspace}
          embedded
        />
      ) : ownedId ? (
        <Text style={styles.submitError}>{t("schedules.workspaceUnavailable")}</Text>
      ) : (
        <SelectField
          label={t("schedules.form.workspace")}
          value={selectedKey}
          selectedDisplay={null}
          options={options}
          onChange={setSelectedKey}
          placeholder={t("schedules.form.selectWorkspace")}
          emptyText={t("schedules.form.noWorkspaces")}
          hint={props.mode === "edit" ? t("schedules.legacy") : t("schedules.form.workspaceHint")}
          searchable
          triggerTestID="schedule-workspace-trigger"
        />
      )}
    </AdaptiveModalSheet>
  );
}

function ScheduleFormContent({
  serverId,
  visible,
  onClose,
  onDismiss,
  mode,
  schedule,
  workspace,
  inline = false,
  embedded = false,
  onSaved,
}: ScheduleFormSheetProps & {
  onDismiss?: () => void;
  workspace?: ScheduleFormWorkspace;
  inline?: boolean;
  embedded?: boolean;
  onSaved?: (schedule: ScheduleSummary) => void;
}): ReactElement {
  const { t } = useTranslation();
  const controlSize: FieldControlSize = useIsCompactFormFactor() ? "md" : "sm";
  const { projects } = useProjects();
  const hostProfiles = useHosts();
  const hosts = useStoreWithEqualityFn(
    useSessionStore,
    useMemo(() => selectScheduleHosts(hostProfiles), [hostProfiles]),
    equal,
  );
  const { preferences, updatePreferences } = useFormPreferences();
  const projectTargets = useMemo(() => buildScheduleProjectTargets(projects), [projects]);
  const timezone = useMemo(getDeviceTimeZone, []);
  const { agents } = useAggregatedAgents({ includeArchived: true });
  const agentOptions = useMemo(
    () =>
      agents
        .filter(
          (agent) =>
            agent.serverId === serverId && agent.workspaceId === workspace?.id && !agent.archivedAt,
        )
        .map((agent) => ({
          id: agent.id,
          title: agent.title?.trim() || t("schedules.untitledAgent"),
        })),
    [agents, serverId, t, workspace?.id],
  );
  const snapshot = useMemo(
    () =>
      buildSnapshot({
        mode,
        serverId,
        schedule,
        hosts,
        projectTargets,
        preferences,
        timezone,
        workspace,
        agents: agentOptions,
      }),
    [
      hosts,
      mode,
      preferences,
      projectTargets,
      schedule,
      serverId,
      timezone,
      workspace,
      agentOptions,
    ],
  );
  const model = useScheduleFormModel(snapshot);
  const state = useSyncExternalStore(model.subscribe, model.getState, model.getState);
  const providerSnapshot = useScheduleFormProviderSnapshot(model, state);
  const mutationServerId = state.selectedServerId ?? serverId ?? "";
  const { createSchedule, updateSchedule, isCreating, isUpdating } = useScheduleMutations({
    serverId: mutationServerId,
  });

  const isSubmitting = isCreating || isUpdating;
  const now = Date.now();
  const cadenceError =
    state.cadence.type === "cron"
      ? (validateCron(state.cadence.expression, t) ??
        (isValidScheduleTimezone(state.cadence.timezone ?? timezone)
          ? null
          : t("schedules.cadence.invalidTimezone")))
      : state.cadence.type === "once"
        ? !Number.isFinite(Date.parse(state.cadence.at))
          ? t("schedules.form.invalidDateTime")
          : Date.parse(state.cadence.at) <= now
            ? t("schedules.form.futureTime")
            : null
        : !Number.isSafeInteger(state.cadence.everyMs) || state.cadence.everyMs <= 0
          ? t("schedules.cadence.invalidInterval")
          : null;
  const startAt = state.startAt.trim() ? parseLocalDateTime(state.startAt) : null;
  const expiresAt = state.expiresAt.trim() ? parseLocalDateTime(state.expiresAt) : null;
  const startAtError =
    state.startAt.trim() && !startAt
      ? t("schedules.form.invalidDateTime")
      : startAt &&
          state.startAt !== formatLocalDateTime(schedule?.startAt) &&
          Date.parse(startAt) <= now
        ? t("schedules.form.futureTime")
        : null;
  const firstAt =
    state.cadence.type === "once"
      ? Date.parse(state.cadence.at)
      : startAt
        ? Date.parse(startAt)
        : state.cadence.type === "every" && mode === "create"
          ? now + state.cadence.everyMs
          : now;
  const expiresAtError =
    state.expiresAt.trim() && !expiresAt
      ? t("schedules.form.invalidDateTime")
      : expiresAt && Date.parse(expiresAt) <= firstAt
        ? t("schedules.form.expiryAfterStart")
        : null;
  const canSubmit =
    state.canSubmit && !cadenceError && !startAtError && !expiresAtError && !isSubmitting;

  const persistPreferences = useCallback(async () => {
    const provider = state.selectedProvider;
    if (!provider) {
      return;
    }
    await updatePreferences((current) =>
      updateSelectionPreferences({
        preferences: current,
        provider,
        model: state.selectedModel,
        mode: state.selectedMode,
        thinkingOptionId: state.selectedThinkingOptionId,
        isolation: state.isolation,
      }),
    );
  }, [
    state.isolation,
    state.selectedMode,
    state.selectedModel,
    state.selectedProvider,
    state.selectedThinkingOptionId,
    updatePreferences,
  ]);

  const handleSubmit = useCallback(async () => {
    if (!canSubmit) return;
    model.setSubmitError(null);
    try {
      if (!state.workspace) throw new Error(t("schedules.form.selectWorkspace"));
      if (!isValidMaxRuns(state.maxRuns)) throw new Error(t("schedules.form.invalidMaxRuns"));
      const maxRuns = state.maxRuns.trim() ? Number(state.maxRuns) : null;
      const startAt = state.startAt.trim() ? parseLocalDateTime(state.startAt) : null;
      const expiresAt = state.expiresAt.trim() ? parseLocalDateTime(state.expiresAt) : null;
      const config = {
        ...(schedule?.target.type === "new-agent" ? schedule.target.config : {}),
        provider: state.selectedProvider!,
        cwd: state.workspace.cwd,
        model: state.selectedModel || undefined,
        modeId: state.selectedMode || undefined,
        thinkingOptionId: state.selectedThinkingOptionId || undefined,
        archiveOnFinish: state.archiveOnFinish,
        title: state.name.trim() || undefined,
      };
      delete config.isolation;
      const target =
        state.targetKind === "agent"
          ? { type: "agent" as const, agentId: state.selectedAgentId }
          : { type: "new-agent" as const, config };
      if (state.targetKind === "new-agent") await persistPreferences();
      const fields = {
        name: state.name.trim() || null,
        prompt: state.prompt.trim(),
        maxRuns,
        expiresAt,
      };
      if (mode === "edit" && schedule) {
        await updateSchedule({
          id: schedule.id,
          ...fields,
          target,
          ...(!schedule.workspaceId ? { workspaceId: state.workspace.id } : {}),
          ...(state.submitCadence ? { cadence: state.submitCadence } : {}),
          ...(state.startAt !== formatLocalDateTime(schedule.startAt) ? { startAt } : {}),
        });
        if (onSaved) onSaved(schedule);
        else onClose();
      } else {
        const created = await createSchedule({
          ...fields,
          name: fields.name ?? undefined,
          workspaceId: state.workspace.id,
          cadence: state.cadence,
          target,
          runOnCreate: false,
          maxRuns: maxRuns ?? undefined,
          expiresAt: expiresAt ?? undefined,
          ...(startAt ? { startAt } : {}),
        });
        if (onSaved) onSaved(created);
        else onClose();
      }
    } catch (error) {
      model.setSubmitError(t("schedules.form.submitError", { message: toErrorMessage(error) }));
    }
  }, [
    canSubmit,
    createSchedule,
    mode,
    model,
    onClose,
    onSaved,
    persistPreferences,
    schedule,
    state,
    t,
    updateSchedule,
  ]);

  const handleSubmitPress = useCallback(() => {
    void handleSubmit();
  }, [handleSubmit]);

  const header = useMemo<SheetHeader>(() => {
    if (mode !== "edit") {
      return { title: t("schedules.new") };
    }
    return {
      title: t("schedules.edit", { product: t("schedules.product.schedule").toLowerCase() }),
    };
  }, [mode, t]);

  const footer = useMemo(
    () => (
      <View style={styles.footer}>
        <Button
          style={styles.footerButton}
          variant="secondary"
          onPress={onClose}
          disabled={isSubmitting}
        >
          {t("common.actions.cancel")}
        </Button>
        <Button
          style={styles.footerButton}
          variant="default"
          onPress={handleSubmitPress}
          disabled={!canSubmit}
          loading={isSubmitting}
          testID="schedule-form-submit"
        >
          {t(mode === "edit" ? "schedules.save" : "schedules.create")}
        </Button>
      </View>
    ),
    [canSubmit, handleSubmitPress, isSubmitting, mode, onClose, t],
  );

  const fields = (
    <ScheduleFormFields
      model={model}
      state={state}
      providerSnapshot={providerSnapshot}
      controlSize={controlSize}
      cadenceError={cadenceError}
      startAtError={startAtError}
      expiresAtError={expiresAtError}
      mutationServerId={mutationServerId}
    />
  );

  if (embedded)
    return (
      <View style={styles.inlineFields}>
        {fields}
        {footer}
      </View>
    );

  if (inline) {
    return (
      <ScrollView style={styles.inlineContainer} contentContainerStyle={styles.inlineContent}>
        <View style={styles.inlineFields} testID="workspace-schedule-form">
          {fields}
          {footer}
        </View>
      </ScrollView>
    );
  }

  return (
    <AdaptiveModalSheet
      header={header}
      visible={visible}
      onClose={onClose}
      onDismiss={onDismiss}
      footer={footer}
      testID="schedule-form-sheet"
    >
      {fields}
    </AdaptiveModalSheet>
  );
}

interface ScheduleFormFieldsProps {
  model: ScheduleFormModel;
  state: ScheduleFormState;
  providerSnapshot: ReturnType<typeof useScheduleFormProviderSnapshot>;
  controlSize: FieldControlSize;
  cadenceError: string | null;
  startAtError: string | null;
  expiresAtError: string | null;
  mutationServerId: string;
}

function ScheduleFormFields({
  model,
  state,
  providerSnapshot,
  controlSize,
  cadenceError,
  startAtError,
  expiresAtError,
  mutationServerId,
}: ScheduleFormFieldsProps): ReactElement {
  const { t } = useTranslation();

  return (
    <>
      {state.workspace ? (
        <Field label={t("schedules.form.workspace")}>
          <View style={styles.workspaceField} testID="schedule-workspace-target">
            <Text style={styles.workspaceName} numberOfLines={1}>
              {state.workspace.name}
            </Text>
            <Text style={styles.workspacePath} numberOfLines={1}>
              {state.workspace.cwd}
            </Text>
          </View>
        </Field>
      ) : null}
      <SelectField
        label={t("schedules.form.target")}
        value={state.targetKind}
        selectedDisplay={{
          label: t(
            state.targetKind === "agent"
              ? "schedules.form.existingAgent"
              : "schedules.form.newAgent",
          ),
        }}
        options={[
          {
            id: "new-agent",
            value: "new-agent",
            label: t("schedules.form.newAgent"),
            testID: "schedule-target-new-agent",
          },
          {
            id: "agent",
            value: "agent",
            label: t("schedules.form.existingAgent"),
            testID: "schedule-target-agent",
          },
        ]}
        onChange={model.setTargetKind}
        searchable={false}
        size={controlSize}
        placeholder={t("schedules.form.target")}
        emptyText={t("schedules.form.noAgents")}
        triggerTestID="schedule-target-kind-trigger"
      />
      <Field label={t("schedules.form.name")}>
        <FormTextInput
          size={controlSize}
          testID="schedule-name-input"
          accessibilityLabel={t("schedules.form.nameLabel")}
          initialValue={state.name}
          onChangeText={model.setName}
          placeholder={t("schedules.form.optional")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>

      <Field label={t("schedules.form.prompt")}>
        <FormTextInput
          size={controlSize}
          testID="schedule-prompt-input"
          accessibilityLabel={t("schedules.form.prompt")}
          initialValue={state.prompt}
          onChangeText={model.setPrompt}
          placeholder={t("schedules.form.promptPlaceholder")}
          style={styles.multilineInput}
          multiline
          numberOfLines={4}
          textAlignVertical="top"
        />
      </Field>

      <ScheduleTargetFields
        model={model}
        state={state}
        providerSnapshot={providerSnapshot}
        controlSize={controlSize}
        mutationServerId={mutationServerId}
      />

      <CadenceEditor
        value={state.cadence}
        onChange={model.setCadence}
        error={cadenceError ?? undefined}
        size={controlSize}
      />

      {state.cadence.type !== "once" ? (
        <Field
          label={t("schedules.form.startAt")}
          error={startAtError ?? undefined}
          hint={t("schedules.cadence.localTime", { timezone: getDeviceTimeZone() })}
        >
          <FormTextInput
            size={controlSize}
            initialValue={state.startAt}
            onChangeText={model.setStartAt}
            placeholder={t("schedules.form.startAtPlaceholder")}
            testID="schedule-start-at-input"
            accessibilityLabel={t("schedules.form.startAt")}
            autoCapitalize="none"
            autoCorrect={false}
          />
        </Field>
      ) : null}
      <Field
        label={t("schedules.form.expiresAt")}
        error={expiresAtError ?? undefined}
        hint={t("schedules.cadence.localTime", { timezone: getDeviceTimeZone() })}
      >
        <FormTextInput
          size={controlSize}
          initialValue={state.expiresAt}
          onChangeText={model.setExpiresAt}
          placeholder="YYYY-MM-DD HH:mm"
          testID="schedule-expires-at-input"
          accessibilityLabel={t("schedules.form.expiresAt")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      <Field
        label={t("schedules.form.maxRuns")}
        hint={t("schedules.form.maxRunsHint")}
        error={!isValidMaxRuns(state.maxRuns) ? t("schedules.form.invalidMaxRuns") : undefined}
      >
        <FormTextInput
          size={controlSize}
          testID="schedule-max-runs-input"
          accessibilityLabel={t("schedules.form.maxRuns")}
          initialValue={state.maxRuns}
          onChangeText={model.setMaxRuns}
          placeholder={t("schedules.form.unlimited")}
          keyboardType="number-pad"
        />
      </Field>

      {state.submitError ? <Text style={styles.submitError}>{state.submitError}</Text> : null}
    </>
  );
}

interface ScheduleTargetFieldsProps {
  model: ScheduleFormModel;
  state: ScheduleFormState;
  providerSnapshot: ReturnType<typeof useScheduleFormProviderSnapshot>;
  controlSize: FieldControlSize;
  mutationServerId: string;
}

function ScheduleTargetFields({
  model,
  state,
  providerSnapshot,
  controlSize,
  mutationServerId,
}: ScheduleTargetFieldsProps): ReactElement {
  const { t } = useTranslation();
  const hostOptions = useMemo<SelectFieldOption<string>[]>(
    () =>
      state.hosts.map((host) => ({
        id: host.serverId,
        value: host.serverId,
        label: host.label,
        testID: buildScheduleHostOptionTestId(host.serverId),
      })),
    [state.hosts],
  );
  const selectedHost = state.hosts.find((host) => host.serverId === state.selectedServerId) ?? null;
  const selectedHostDisplay = useMemo<SelectFieldDisplay | null>(() => {
    if (selectedHost) {
      return { label: selectedHost.label };
    }
    if (state.selectedServerId) {
      return { label: state.selectedServerId };
    }
    return null;
  }, [selectedHost, state.selectedServerId]);
  const projectOptions = state.projectOptions;
  const modeOptions = useMemo<SelectFieldOption<string>[]>(
    () =>
      state.modeOptions.map((option) => ({
        id: option.id,
        value: option.id,
        label: scheduleModeLabel(option, t),
      })),
    [state.modeOptions, t],
  );
  const thinkingOptions = useMemo<SelectFieldOption<string>[]>(
    () =>
      state.availableThinkingOptions.map((option) => ({
        id: option.id,
        value: option.id,
        label: scheduleThinkingLabel(option, t),
        testID: buildThinkingOptionTestId(option.id),
      })),
    [state.availableThinkingOptions, t],
  );
  const handleSelectHost = useCallback(
    (nextServerId: string) => {
      model.setHost(nextServerId);
    },
    [model],
  );
  const handleSelectProject = useCallback(
    (optionId: string, display: ScheduleFormDisplay) => {
      model.setProject(optionId, display);
    },
    [model],
  );
  const handleSelectModel = useCallback(
    (provider: AgentProvider, modelId: string) => {
      model.setModel(provider, modelId);
    },
    [model],
  );
  const handleSelectMode = useCallback(
    (modeId: string) => {
      model.setSessionMode(modeId);
    },
    [model],
  );
  const handleSelectThinking = useCallback(
    (thinkingOptionId: string) => {
      model.setThinking(thinkingOptionId);
    },
    [model],
  );
  const handleModelOpen = useCallback(() => {
    providerSnapshot.refetchIfStale(state.selectedProvider);
  }, [providerSnapshot, state.selectedProvider]);
  const handleRetryProvider = useCallback(
    (provider: AgentProvider) => {
      void providerSnapshot.refresh([provider]);
    },
    [providerSnapshot],
  );
  const renderHostOption = useCallback(
    (input: SelectFieldRenderOptionInput<string>) => <HostOptionItem {...input} />,
    [],
  );
  const renderProjectOption = useCallback(
    (input: SelectFieldRenderOptionInput<string>) => <ProjectOptionItem {...input} />,
    [],
  );
  const renderThinkingOption = useCallback(
    (input: SelectFieldRenderOptionInput<string>) => <ThinkingOptionItem {...input} />,
    [],
  );
  const modelTriggerLeading = useMemo(
    () => <ProviderGlyph provider={state.selectedProvider} serverId={state.selectedServerId} />,
    [state.selectedProvider, state.selectedServerId],
  );
  const renderModelTrigger = useCallback(
    ({
      selectedModelLabel,
      disabled,
      isOpen,
      hovered,
      pressed,
    }: {
      selectedModelLabel: string;
      onPress: () => void;
      disabled: boolean;
      isOpen: boolean;
      hovered: boolean;
      pressed: boolean;
    }): ReactNode => {
      const displayLabel = state.selectedModelDisplay?.label ?? selectedModelLabel;
      return (
        <SelectFieldTrigger
          label={displayLabel}
          isPlaceholder={!state.selectedModel}
          placeholder={displayLabel}
          leading={modelTriggerLeading}
          disabled={disabled}
          active={hovered || pressed || isOpen}
          size={controlSize}
          testID="schedule-model-trigger"
        />
      );
    },
    [controlSize, modelTriggerLeading, state.selectedModel, state.selectedModelDisplay],
  );

  if (state.targetKind === "agent") {
    const options = state.agentOptions.map((agent) => ({
      id: agent.id,
      value: agent.id,
      label: agent.title,
      testID: `schedule-agent-option-${agent.id}`,
    }));
    return (
      <SelectField
        label={t("schedules.form.existingAgent")}
        value={state.selectedAgentId || null}
        selectedDisplay={options.find((option) => option.value === state.selectedAgentId) ?? null}
        options={options}
        onChange={model.setAgent}
        placeholder={t("schedules.form.selectAgent")}
        emptyText={t("schedules.form.noAgents")}
        hint={t("schedules.form.agentHint")}
        searchable
        size={controlSize}
        triggerTestID="schedule-agent-trigger"
      />
    );
  }

  return (
    <>
      {!state.workspace && (state.mode === "edit" || state.hosts.length > 1) ? (
        <SelectField
          label={t("schedules.form.host")}
          value={state.selectedServerId}
          selectedDisplay={selectedHostDisplay}
          options={hostOptions}
          onChange={handleSelectHost}
          placeholder={t("schedules.form.selectHost")}
          emptyText={t("schedules.form.noHosts")}
          disabled={state.mode === "edit"}
          searchable={false}
          title={t("schedules.form.host")}
          size={controlSize}
          triggerTestID="schedule-host-trigger"
          renderOption={renderHostOption}
        />
      ) : null}

      {state.disclosure.showProjectField ? (
        <SelectField
          label={t("schedules.form.project")}
          value={state.selectedProjectOptionId || null}
          selectedDisplay={state.projectDisplay}
          options={projectOptions}
          onChange={handleSelectProject}
          placeholder={t("schedules.form.selectProject")}
          emptyText={t("schedules.form.noProjects")}
          disabled={!state.selectedServerId}
          hint={!state.selectedServerId ? t("schedules.form.chooseHost") : undefined}
          searchable
          searchPlaceholder={t("schedules.form.searchProjects")}
          title={t("schedules.form.selectProject")}
          size={controlSize}
          triggerTestID="schedule-project-trigger"
          renderOption={renderProjectOption}
        />
      ) : null}

      {state.disclosure.showModelField ? (
        <Field label={t("schedules.form.model")}>
          <CombinedModelSelector
            providers={state.modelSelectorProviders}
            selectedProvider={state.selectedProvider ?? ""}
            selectedModel={state.selectedModel}
            onSelect={handleSelectModel}
            isLoading={providerSnapshot.isLoading || providerSnapshot.isFetching}
            renderTrigger={renderModelTrigger}
            triggerFill
            serverId={mutationServerId}
            disabled={!state.selectedServerId}
            onOpen={handleModelOpen}
            onRetryProvider={handleRetryProvider}
            isRetryingProvider={providerSnapshot.isRefreshing}
          />
        </Field>
      ) : null}

      {state.disclosure.showThinkingField ? (
        <SelectField
          label={t("schedules.form.thinking")}
          value={state.selectedThinkingOptionId || null}
          selectedDisplay={
            state.selectedThinkingOptionId
              ? {
                  label: scheduleThinkingLabel(
                    state.availableThinkingOptions.find(
                      (option) => option.id === state.selectedThinkingOptionId,
                    ) ?? { id: state.selectedThinkingOptionId },
                    t,
                  ),
                }
              : null
          }
          options={thinkingOptions}
          onChange={handleSelectThinking}
          placeholder={t("schedules.form.selectThinking")}
          emptyText={t("schedules.form.noThinking")}
          searchable={thinkingOptions.length > 6}
          title={t("schedules.form.selectThinking")}
          size={controlSize}
          triggerTestID="schedule-thinking-trigger"
          renderOption={renderThinkingOption}
        />
      ) : null}

      {state.disclosure.showModeField ? (
        <SelectField
          label={t("schedules.form.mode")}
          value={state.selectedMode || null}
          selectedDisplay={
            state.selectedMode
              ? {
                  label: scheduleModeLabel(
                    state.modeOptions.find((option) => option.id === state.selectedMode) ?? {
                      id: state.selectedMode,
                    },
                    t,
                  ),
                }
              : { label: t("schedules.form.defaultMode") }
          }
          options={modeOptions}
          onChange={handleSelectMode}
          placeholder={t("schedules.form.defaultMode")}
          emptyText={t("schedules.form.noModes")}
          disabled={modeOptions.length === 0}
          hint={modeOptions.length === 0 ? t("schedules.form.noModelModes") : undefined}
          searchable={modeOptions.length > 6}
          title={t("schedules.form.selectMode")}
          size={controlSize}
          triggerTestID="schedule-mode-trigger"
        />
      ) : null}

      {state.disclosure.showIsolationField ? (
        <ScheduleIsolationField model={model} state={state} size={controlSize} />
      ) : null}

      {state.disclosure.showArchiveOnFinishField ? (
        <Field label={t("schedules.form.archive")}>
          <Switch
            value={state.archiveOnFinish}
            onValueChange={model.setArchiveOnFinish}
            accessibilityLabel={t("schedules.form.archive")}
            testID="schedule-archive-on-finish-switch"
          />
        </Field>
      ) : null}
    </>
  );
}

function ScheduleIsolationField({
  model,
  state,
  size,
}: {
  model: ScheduleFormModel;
  state: ScheduleFormState;
  size: FieldControlSize;
}): ReactElement {
  const { t } = useTranslation();
  const options = useMemo<SelectFieldOption<"local" | "worktree">[]>(
    () => [
      {
        id: "local",
        value: "local",
        label: t("schedules.form.local"),
        testID: "schedule-isolation-local",
      },
      {
        id: "worktree",
        value: "worktree",
        label: t("schedules.form.worktree"),
        testID: "schedule-isolation-worktree",
      },
    ],
    [t],
  );
  const selectedDisplay = useMemo<SelectFieldDisplay>(
    () => ({
      label: t(
        state.effectiveIsolation === "worktree"
          ? "schedules.form.worktree"
          : "schedules.form.local",
      ),
    }),
    [state.effectiveIsolation, t],
  );
  const triggerLeading = useMemo(
    () => (
      <View style={styles.optionIconBox}>
        {state.effectiveIsolation === "worktree" ? (
          <GitBranch size={16} color={styles.providerIcon.color} />
        ) : (
          <Folder size={16} color={styles.providerIcon.color} />
        )}
      </View>
    ),
    [state.effectiveIsolation],
  );
  const handleSelectIsolation = useCallback(
    (value: "local" | "worktree") => {
      model.setIsolation(value);
    },
    [model],
  );
  const renderIsolationOption = useCallback(
    (input: SelectFieldRenderOptionInput<"local" | "worktree">) => (
      <IsolationOptionItem {...input} />
    ),
    [],
  );

  return (
    <SelectField
      label={t("schedules.form.isolation")}
      value={state.effectiveIsolation}
      selectedDisplay={selectedDisplay}
      options={options}
      onChange={handleSelectIsolation}
      placeholder={t("schedules.form.selectIsolation")}
      emptyText={t("schedules.form.noIsolation")}
      searchable={false}
      title={t("schedules.form.isolation")}
      size={size}
      testID="schedule-isolation"
      triggerTestID="schedule-isolation-trigger"
      triggerLeading={triggerLeading}
      renderOption={renderIsolationOption}
    />
  );
}

function IsolationOptionItem({
  option,
  selected,
  active,
  onPress,
}: SelectFieldRenderOptionInput<"local" | "worktree">): ReactElement {
  const leadingSlot = useMemo(
    () => (
      <View style={styles.optionIconBox}>
        {option.value === "worktree" ? (
          <GitBranch size={16} color={styles.providerIcon.color} />
        ) : (
          <Folder size={16} color={styles.providerIcon.color} />
        )}
      </View>
    ),
    [option.value],
  );

  return (
    <ComboboxItem
      testID={option.testID}
      label={option.label}
      selected={selected}
      active={active}
      onPress={onPress}
      leadingSlot={leadingSlot}
    />
  );
}

function HostOptionItem({
  option,
  selected,
  active,
  onPress,
}: SelectFieldRenderOptionInput<string>): ReactElement {
  const leadingSlot = useMemo(() => <HostStatusDotSlot serverId={option.value} />, [option.value]);

  return (
    <ComboboxItem
      testID={option.testID}
      label={option.label}
      selected={selected}
      active={active}
      onPress={onPress}
      leadingSlot={leadingSlot}
    />
  );
}

function ProjectOptionItem({
  option,
  selected,
  active,
  onPress,
}: SelectFieldRenderOptionInput<string>): ReactElement {
  const leadingSlot = useMemo(
    () => (
      <View style={styles.optionIconBox}>
        <Folder size={16} color={styles.providerIcon.color} />
      </View>
    ),
    [],
  );

  return (
    <ComboboxItem
      testID={option.testID}
      label={option.label}
      selected={selected}
      active={active}
      onPress={onPress}
      leadingSlot={leadingSlot}
    />
  );
}

function ThinkingOptionItem({
  option,
  selected,
  active,
  onPress,
}: SelectFieldRenderOptionInput<string>): ReactElement {
  const leadingSlot = useMemo(
    () => (
      <View style={styles.optionIconBox}>
        <Brain size={16} color={styles.providerIcon.color} />
      </View>
    ),
    [],
  );

  return (
    <ComboboxItem
      testID={option.testID}
      label={option.label}
      selected={selected}
      active={active}
      onPress={onPress}
      leadingSlot={leadingSlot}
    />
  );
}

function ProviderGlyph({
  provider,
  serverId,
}: {
  provider: string | null;
  serverId: string | null;
}): ReactElement | null {
  if (!provider) {
    return null;
  }
  const Icon = getProviderIcon(provider, serverId);
  return <Icon size={16} color={styles.providerIcon.color} />;
}

const styles = StyleSheet.create((theme) => {
  const geometry = createControlGeometry(theme);

  return {
    inlineContainer: {
      flex: 1,
    },
    inlineContent: {
      padding: theme.spacing[4],
      alignItems: "center",
    },
    inlineFields: {
      width: "100%",
      maxWidth: 640,
      gap: theme.spacing[4],
    },
    workspaceField: {
      gap: theme.spacing[1],
    },
    workspaceName: {
      color: theme.colors.foreground,
      fontSize: theme.fontSize.base,
    },
    workspacePath: {
      color: theme.colors.foregroundMuted,
      fontSize: theme.fontSize.sm,
    },
    multilineInput: {
      minHeight: 96,
    },
    readonlyField: {
      flexDirection: "row",
      alignItems: "center",
      backgroundColor: theme.colors.surface2,
      borderWidth: 1,
      borderColor: theme.colors.border,
    },
    readonlyFieldSm: {
      ...geometry.formTextInputSm,
    },
    readonlyFieldMd: {
      ...geometry.formTextInputMd,
    },
    readonlyText: {
      flex: 1,
      minWidth: 0,
      color: theme.colors.foreground,
    },
    readonlyTextSm: {
      fontSize: theme.fontSize.base,
    },
    readonlyTextMd: {
      fontSize: theme.fontSize.base,
    },
    optionIconBox: {
      width: 18,
      height: 18,
      alignItems: "center",
      justifyContent: "center",
    },
    footer: {
      flex: 1,
      flexDirection: "row",
      gap: theme.spacing[3],
    },
    footerButton: {
      flex: 1,
    },
    submitError: {
      color: theme.colors.palette.red[300],
      fontSize: theme.fontSize.sm,
    },
    providerIcon: {
      color: theme.colors.foregroundMuted,
    },
  };
});
