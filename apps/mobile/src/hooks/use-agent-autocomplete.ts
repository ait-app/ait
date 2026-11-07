import {
  CLIENT_SLASH_COMMANDS,
  preferProviderCommands,
  type ClientSlashCommand,
} from "@/client-slash-commands";
import type { AutocompleteOption } from "@/components/ui/autocomplete";
import { orderAutocompleteOptions } from "@/components/ui/autocomplete-utils";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import {
  applySlashCommandReplacement,
  applySlashCommandArgumentReplacement,
  filterAndRankCommandAutocompleteEntries,
  filterInlineSkillCommandEntries,
  findActiveSlashCommand,
  findActiveSlashCommandArguments,
  getCommandArgumentSuggestions,
  type SlashCommandArgumentsRange,
  type SlashCommandRange,
} from "@/utils/agent-command-autocomplete";
import {
  applyFileMentionReplacement,
  findActiveFileMention,
  type FileMentionRange,
} from "@/utils/file-mention-autocomplete";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import type { CommandCompletionItem } from "@ait/protocol/messages";
import { applyNativeCommandCompletion } from "@/utils/native-command-completion";
import { useAgentCommandCompletionsQuery } from "./use-agent-command-completions-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  useAgentCommandsQuery,
  type AgentSlashCommand,
  type DraftCommandConfig,
} from "./use-agent-commands-query";
import { useAutocomplete } from "./use-autocomplete";

interface UseAgentAutocompleteInput {
  userInput: string;
  cursorIndex: number;
  setUserInput: (nextValue: string, selection?: { start: number; end: number }) => void;
  serverId: string;
  agentId: string;
  draftConfig?: DraftCommandConfig;
  onAutocompleteApplied?: () => void;
  onClientSlashCommand?: (command: ClientSlashCommand) => void;
  canExecuteClientSlashCommand?: boolean;
}

interface AgentAutocompleteKeyPressEvent {
  key: string;
  preventDefault: () => void;
  input: AgentAutocompleteInputSnapshot;
}

interface AgentAutocompleteInputSnapshot {
  text: string;
  selection: { start: number; end: number };
}

type AgentAutocompleteOption =
  | (AutocompleteOption & {
      type: "native_completion";
      item: CommandCompletionItem;
      inputText: string;
      inputCursor: number;
    })
  | (AutocompleteOption & {
      type: "client_command";
      command: ClientSlashCommand;
    })
  | (AutocompleteOption & { type: "provider_command" })
  | (AutocompleteOption & {
      type: "provider_argument";
      commandName: string;
      value: string;
    })
  | (AutocompleteOption & {
      type: "workspace_entry";
      entryPath: string;
      mention: FileMentionRange;
    });

interface AgentAutocompleteResult {
  isVisible: boolean;
  options: AutocompleteOption[];
  selectedIndex: number;
  isLoading: boolean;
  errorMessage?: string;
  loadingText: string;
  emptyText: string;
  onSelectOption: (option: AutocompleteOption, input?: AgentAutocompleteInputSnapshot) => void;
  onKeyPress: (event: AgentAutocompleteKeyPressEvent) => boolean;
}

interface AgentAutocompleteSnapshot {
  text: string;
  slashCommand: SlashCommandRange | null;
  fileMention: FileMentionRange | null;
}

function resolveAgentAutocompleteSnapshot(input: {
  input?: AgentAutocompleteInputSnapshot;
  userInput: string;
  cursorIndex: number;
  activeSlashCommand: SlashCommandRange | null;
  activeFileMention: FileMentionRange | null;
}): AgentAutocompleteSnapshot {
  if (!input.input) {
    return {
      text: input.userInput,
      slashCommand: input.activeSlashCommand,
      fileMention: input.activeFileMention,
    };
  }

  const text = input.input.text;
  const cursorIndex = input.input.selection.start;
  return {
    text,
    slashCommand: findActiveSlashCommand({ text, cursorIndex }),
    fileMention: findActiveFileMention({ text, cursorIndex }),
  };
}

interface DirectorySuggestionEntry {
  path: string;
  kind: "file" | "directory";
}

type AvailableCommand =
  | { source: "client"; command: ClientSlashCommand }
  | { source: "provider"; command: AgentSlashCommand };

function normalizeDraftCommandConfig(
  draftConfig?: DraftCommandConfig,
): DraftCommandConfig | undefined {
  if (!draftConfig) {
    return undefined;
  }

  const cwd = draftConfig.cwd.trim();
  if (!cwd) {
    return undefined;
  }

  const modeId = draftConfig.modeId?.trim() ?? "";
  const model = draftConfig.model?.trim() ?? "";
  const thinkingOptionId = draftConfig.thinkingOptionId?.trim() ?? "";
  const featureValues = draftConfig.featureValues;
  return {
    provider: draftConfig.provider,
    cwd,
    ...(modeId ? { modeId } : {}),
    ...(model ? { model } : {}),
    ...(thinkingOptionId ? { thinkingOptionId } : {}),
    ...(featureValues && Object.keys(featureValues).length > 0 ? { featureValues } : {}),
  };
}

function mapDirectorySuggestionsToEntries(payload: {
  entries?: { path: string; kind: string }[];
  directories?: string[];
}): DirectorySuggestionEntry[] {
  if (Array.isArray(payload.entries) && payload.entries.length > 0) {
    return payload.entries.flatMap((entry) => {
      if (
        !entry ||
        typeof entry.path !== "string" ||
        (entry.kind !== "file" && entry.kind !== "directory")
      ) {
        return [];
      }
      return [{ path: entry.path, kind: entry.kind }];
    });
  }

  return (payload.directories ?? []).map((path) => ({
    path,
    kind: "directory" as const,
  }));
}

function mapCommandToOption(entry: AvailableCommand, t: TFunction): AgentAutocompleteOption {
  const command = entry.command;
  const base = {
    id: command.name,
    label: `/${command.name}`,
    detail: command.argumentHint || undefined,
    description:
      entry.source === "client" ? t(entry.command.descriptionKey) : entry.command.description,
    kind: "command" as const,
  };
  if (entry.source === "client") {
    return {
      ...base,
      type: "client_command",
      command: entry.command,
    };
  }
  return {
    ...base,
    type: "provider_command",
  };
}

type AutocompleteMode = "command" | "file" | null;

interface BuildAutocompleteOptionsInput {
  isVisible: boolean;
  mode: AutocompleteMode;
  commands: AgentSlashCommand[];
  isDraftContext: boolean;
  commandFilterQuery: string;
  activeSlashCommand: SlashCommandRange | null;
  activeArguments: SlashCommandArgumentsRange | null;
  preferProviderCommands: boolean;
  activeFileMention: FileMentionRange | null;
  fileSuggestions: DirectorySuggestionEntry[];
  t: TFunction;
}

function buildCommandAutocompleteOptions(input: BuildAutocompleteOptionsInput) {
  if (!input.isVisible) {
    return [];
  }

  if (input.mode === "command") {
    if (input.activeArguments) {
      const commandName = input.activeArguments.commandName;
      return orderAutocompleteOptions(
        getCommandArgumentSuggestions(input.commands, input.activeArguments),
      ).map((suggestion) => ({
        type: "provider_argument" as const,
        id: `${commandName}:${suggestion.name}`,
        label: suggestion.name,
        detail: suggestion.usage,
        description: suggestion.description,
        kind: "command" as const,
        commandName,
        value: suggestion.name,
      }));
    }
    const providerCommands = input.commands.map((command) => ({
      source: "provider" as const,
      command,
    }));
    const builtinNames = new Set(
      CLIENT_SLASH_COMMANDS.flatMap((command) => [command.name, ...(command.aliases ?? [])]),
    );
    const rootCommands: AvailableCommand[] = [
      ...(!input.isDraftContext && !input.preferProviderCommands
        ? CLIENT_SLASH_COMMANDS.map((command) => ({
            source: "client" as const,
            command,
          }))
        : []),
      ...providerCommands.filter(
        ({ command }) => input.preferProviderCommands || !builtinNames.has(command.name),
      ),
    ];
    const availableCommands: AvailableCommand[] =
      input.activeSlashCommand?.position === "inline"
        ? filterInlineSkillCommandEntries(providerCommands)
        : rootCommands;
    const matches = filterAndRankCommandAutocompleteEntries(
      availableCommands,
      input.commandFilterQuery,
    );
    const orderedMatches = orderAutocompleteOptions(matches);
    return orderedMatches.map((entry) => mapCommandToOption(entry, input.t));
  }

  const activeFileMention = input.activeFileMention;
  if (input.mode === "file" && activeFileMention) {
    const orderedEntries = orderAutocompleteOptions(input.fileSuggestions);
    return orderedEntries.map((entry) => ({
      type: "workspace_entry" as const,
      id: `${entry.kind}:${entry.path}`,
      label: entry.path,
      kind: entry.kind,
      entryPath: entry.path,
      mention: activeFileMention,
    }));
  }

  return [];
}

function resolveAutocompleteMode(args: {
  showFileAutocomplete: boolean;
  showCommandAutocomplete: boolean;
}): AutocompleteMode {
  if (args.showFileAutocomplete) {
    return "file";
  }
  if (args.showCommandAutocomplete) {
    return "command";
  }
  return null;
}

function resolveAutocompleteIsVisible(args: {
  mode: AutocompleteMode;
  canLoadCommands: boolean;
  serverId: string;
  autocompleteCwd: string;
}): boolean {
  if (args.mode === "command") {
    return args.canLoadCommands;
  }
  if (args.mode === "file") {
    return Boolean(args.serverId) && args.autocompleteCwd.length > 0;
  }
  return false;
}

function resolveCanLoadCommands(args: {
  serverId: string;
  agentId: string;
  isDraftContext: boolean;
}): boolean {
  if (!args.serverId) {
    return false;
  }
  return Boolean(args.agentId) || args.isDraftContext;
}

function resolveAutocompleteIsLoading(args: {
  mode: AutocompleteMode;
  isCommandsLoading: boolean;
  fileSuggestionsIsPending: boolean;
  fileSuggestionsIsLoading: boolean;
  optionsLength: number;
}): boolean {
  if (args.mode === "command") {
    return args.isCommandsLoading && args.optionsLength === 0;
  }
  if (args.mode === "file") {
    return (
      args.fileSuggestionsIsPending || (args.fileSuggestionsIsLoading && args.optionsLength === 0)
    );
  }
  return false;
}

function resolveAutocompleteErrorMessage(args: {
  mode: AutocompleteMode;
  isCommandError: boolean;
  commandError: Error | null;
  fileSuggestionsError: unknown;
  t: TFunction;
}): string | undefined {
  if (args.mode === "command") {
    return args.isCommandError
      ? (args.commandError?.message ?? args.t("agentAutocomplete.failedToLoad"))
      : undefined;
  }
  if (args.mode === "file") {
    return args.fileSuggestionsError instanceof Error
      ? args.fileSuggestionsError.message
      : undefined;
  }
  return undefined;
}

export function useAgentAutocomplete(input: UseAgentAutocompleteInput): AgentAutocompleteResult {
  const { t } = useTranslation();
  const {
    userInput,
    cursorIndex,
    setUserInput,
    serverId,
    agentId,
    draftConfig,
    onAutocompleteApplied,
    onClientSlashCommand,
    canExecuteClientSlashCommand,
  } = input;

  const activeSlashCommand = useMemo(
    () =>
      findActiveSlashCommand({
        text: userInput,
        cursorIndex,
      }),
    [cursorIndex, userInput],
  );
  const activeArguments = useMemo(
    () => findActiveSlashCommandArguments({ text: userInput, cursorIndex }),
    [userInput, cursorIndex],
  );
  const nativeCommandContext = /^\s*\//u.test(userInput.slice(0, cursorIndex));
  const showCommandAutocomplete =
    activeSlashCommand !== null || activeArguments !== null || nativeCommandContext;
  const commandFilterQuery = activeArguments
    ? `${activeArguments.commandName} ${activeArguments.query}`
    : (activeSlashCommand?.query ?? "");

  const activeFileMention = useMemo(
    () =>
      findActiveFileMention({
        text: userInput,
        cursorIndex,
      }),
    [cursorIndex, userInput],
  );
  const showFileAutocomplete = activeFileMention !== null;
  const fileFilterQuery = activeFileMention?.query ?? "";
  const [debouncedFileFilterQuery, setDebouncedFileFilterQuery] = useState(fileFilterQuery);

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedFileFilterQuery(fileFilterQuery), 180);
    return () => clearTimeout(timer);
  }, [fileFilterQuery]);

  const normalizedDraftConfig = useMemo(
    () => normalizeDraftCommandConfig(draftConfig),
    [draftConfig],
  );

  const isDraftContext = normalizedDraftConfig !== undefined;
  const queryDraftConfig = normalizedDraftConfig;
  const canLoadCommands = resolveCanLoadCommands({
    serverId,
    agentId,
    isDraftContext,
  });

  const agentCwd = useSessionStore(
    (state) => state.sessions[serverId]?.agents?.get(agentId)?.cwd ?? "",
  );
  const agentProvider = useSessionStore(
    (state) => state.sessions[serverId]?.agents?.get(agentId)?.provider,
  );
  const prefersProviderCommands = preferProviderCommands(
    queryDraftConfig?.provider ?? agentProvider,
  );
  const autocompleteCwd = useMemo(() => {
    if (isDraftContext) {
      return queryDraftConfig?.cwd ?? "";
    }
    return agentCwd.trim();
  }, [agentCwd, isDraftContext, queryDraftConfig]);

  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);

  const mode =
    prefersProviderCommands && nativeCommandContext
      ? "command"
      : resolveAutocompleteMode({
          showFileAutocomplete,
          showCommandAutocomplete,
        });
  const canShowAutocomplete = resolveAutocompleteIsVisible({
    mode,
    canLoadCommands,
    serverId,
    autocompleteCwd,
  });

  const {
    commands,
    isLoading: isCommandsLoading,
    isError,
    error,
  } = useAgentCommandsQuery({
    serverId,
    agentId,
    enabled: mode === "command" && canLoadCommands,
    draftConfig: queryDraftConfig,
    refreshOnOpen: prefersProviderCommands,
  });

  const [completionRevision, setCompletionRevision] = useState(0);
  const native = useAgentCommandCompletionsQuery({
    serverId,
    agentId,
    draftConfig: queryDraftConfig,
    text: userInput,
    cursor: cursorIndex,
    enabled:
      prefersProviderCommands && nativeCommandContext && canLoadCommands && !isCommandsLoading,
    revision: completionRevision,
  });
  const useNative = native.active && native.result?.supported !== false;

  const [dismissedInput, setDismissedInput] = useState<string | null>(null);
  const dismissedInputRef = useRef<string | null>(null);
  const dismissAutocomplete = useCallback((text: string) => {
    dismissedInputRef.current = text;
    setDismissedInput(text);
  }, []);
  useEffect(() => {
    if (dismissedInputRef.current !== userInput) {
      dismissedInputRef.current = null;
      setDismissedInput(null);
    }
  }, [userInput]);
  useEffect(() => {
    dismissedInputRef.current = null;
    setDismissedInput(null);
  }, [serverId, agentId]);
  const isVisible =
    canShowAutocomplete &&
    dismissedInput !== userInput &&
    !(useNative && !native.isLoading && native.result?.items.length === 0) &&
    !(
      mode === "command" &&
      !useNative &&
      activeArguments &&
      getCommandArgumentSuggestions(commands, activeArguments).length === 0
    );

  const fileSuggestionsQuery = useQuery({
    queryKey: [
      "directorySuggestions",
      serverId,
      autocompleteCwd,
      debouncedFileFilterQuery,
      true,
      true,
    ],
    queryFn: async (): Promise<DirectorySuggestionEntry[]> => {
      if (!client) {
        throw new Error(t("common.errors.daemonClientUnavailable"));
      }
      const response = await client.getDirectorySuggestions({
        cwd: autocompleteCwd,
        query: debouncedFileFilterQuery,
        limit: 50,
        includeFiles: true,
        includeDirectories: true,
      });
      if (response.error) {
        throw new Error(response.error);
      }
      return mapDirectorySuggestionsToEntries(response);
    },
    enabled:
      mode === "file" &&
      Boolean(serverId) &&
      autocompleteCwd.length > 0 &&
      Boolean(client) &&
      isConnected,
    retry: false,
    staleTime: 15_000,
    placeholderData: keepPreviousData,
  });

  const options = useMemo<AgentAutocompleteOption[]>(() => {
    if (useNative) {
      if (!isVisible) return [];
      return orderAutocompleteOptions(native.result?.items ?? []).map((item) => ({
        type: "native_completion" as const,
        id: item.id,
        label: item.label,
        description: item.description,
        detail: item.detail ?? item.hint,
        kind: "command" as const,
        item,
        inputText: userInput,
        inputCursor: cursorIndex,
      }));
    }
    return buildCommandAutocompleteOptions({
      activeFileMention,
      commandFilterQuery,
      commands,
      activeSlashCommand,
      activeArguments,
      preferProviderCommands: prefersProviderCommands,
      fileSuggestions: fileSuggestionsQuery.data ?? [],
      isDraftContext,
      isVisible,
      mode,
      t,
    });
  }, [
    activeFileMention,
    activeSlashCommand,
    activeArguments,
    prefersProviderCommands,
    commandFilterQuery,
    commands,
    fileSuggestionsQuery.data,
    isDraftContext,
    isVisible,
    mode,
    t,
    useNative,
    native.result,
    userInput,
    cursorIndex,
  ]);

  const onSelectOption = useCallback(
    (option: AutocompleteOption, snapshot?: AgentAutocompleteInputSnapshot) => {
      const selected = option as AgentAutocompleteOption;
      if (selected.type === "native_completion") {
        const text = snapshot?.text ?? userInput;
        const selection = snapshot?.selection ?? {
          start: cursorIndex,
          end: cursorIndex,
        };
        if (
          text !== selected.inputText ||
          selection.start !== selected.inputCursor ||
          selection.end !== selection.start
        )
          return;
        const edited = applyNativeCommandCompletion(text, selected.item);
        if (!edited) return;
        setUserInput(edited.text, {
          start: edited.cursor,
          end: edited.cursor,
        });
        if (selected.item.triggerCompletion) setCompletionRevision((value) => value + 1);
        else dismissAutocomplete(edited.text);
        onAutocompleteApplied?.();
        return;
      }
      if (selected.type === "provider_argument") {
        const text = snapshot?.text ?? userInput;
        const range = findActiveSlashCommandArguments({
          text,
          cursorIndex: snapshot?.selection.start ?? cursorIndex,
        });
        if (
          !range ||
          range.commandName !== selected.commandName ||
          !getCommandArgumentSuggestions(commands, range).some(
            (suggestion) => suggestion.name === selected.value,
          )
        )
          return;
        const nextInput = applySlashCommandArgumentReplacement({
          text,
          range,
          value: selected.value,
        });
        const cursor = range.start + selected.value.length + 1;
        setUserInput(nextInput, { start: cursor, end: cursor });
        onAutocompleteApplied?.();
        return;
      }
      const current = resolveAgentAutocompleteSnapshot({
        input: snapshot,
        userInput,
        cursorIndex,
        activeSlashCommand,
        activeFileMention,
      });
      const selectedIsCommand =
        selected.type === "client_command" || selected.type === "provider_command";
      if (snapshot && selectedIsCommand && !current.slashCommand) return;
      if (
        selected.type === "client_command" &&
        selected.command.execution === "immediate" &&
        canExecuteClientSlashCommand &&
        onClientSlashCommand
      ) {
        onClientSlashCommand(selected.command);
        return;
      }

      if (selectedIsCommand) {
        if (!current.slashCommand) {
          setUserInput(`/${selected.id} `);
          onAutocompleteApplied?.();
          return;
        }

        const nextInput = applySlashCommandReplacement({
          text: current.text,
          command: current.slashCommand,
          commandName: selected.id,
        });
        setUserInput(nextInput);
        onAutocompleteApplied?.();
        return;
      }

      if (!current.fileMention) return;
      const nextInput = applyFileMentionReplacement({
        text: current.text,
        mention: current.fileMention,
        relativePath: selected.entryPath,
      });
      setUserInput(nextInput);
      onAutocompleteApplied?.();
    },
    [
      canExecuteClientSlashCommand,
      onAutocompleteApplied,
      onClientSlashCommand,
      setUserInput,
      userInput,
      cursorIndex,
      activeFileMention,
      activeSlashCommand,
      commands,
      dismissAutocomplete,
    ],
  );

  const selectOptionFromKeyPress = useCallback(
    (option: AutocompleteOption, event?: AgentAutocompleteKeyPressEvent) =>
      onSelectOption(option, event?.input),
    [onSelectOption],
  );

  const { selectedIndex, onKeyPress } = useAutocomplete({
    isVisible,
    options,
    query: mode === "command" ? commandFilterQuery : fileFilterQuery,
    onSelectOption: selectOptionFromKeyPress,
    onEscape:
      activeArguments || prefersProviderCommands
        ? () => dismissAutocomplete(userInput)
        : mode === "command" && activeSlashCommand?.position === "start"
          ? () => setUserInput("")
          : undefined,
  });

  const handleKeyPress = useCallback(
    (event: AgentAutocompleteKeyPressEvent) => {
      const text = event.input.text;
      const cursor = event.input.selection.start;
      const commandContext = /^\s*\//u.test(text.slice(0, cursor));
      if (prefersProviderCommands && commandContext) {
        if (dismissedInputRef.current === text) return false;
        if (event.key === "Escape") {
          event.preventDefault();
          dismissAutocomplete(text);
          return true;
        }
        if (
          (event.key === "Tab" || event.key === "Enter") &&
          (text !== userInput || cursor !== cursorIndex || isCommandsLoading || native.isLoading)
        ) {
          event.preventDefault();
          return true;
        }
      }
      return onKeyPress(event);
    },
    [
      prefersProviderCommands,
      dismissAutocomplete,
      userInput,
      cursorIndex,
      isCommandsLoading,
      native.isLoading,
      onKeyPress,
    ],
  );

  const isLoading = resolveAutocompleteIsLoading({
    mode,
    isCommandsLoading: useNative ? native.isLoading : isCommandsLoading,
    fileSuggestionsIsPending: fileSuggestionsQuery.isPending,
    fileSuggestionsIsLoading: fileSuggestionsQuery.isLoading,
    optionsLength: options.length,
  });
  const errorMessage = resolveAutocompleteErrorMessage({
    mode,
    isCommandError: useNative ? !!native.error : isError,
    commandError: useNative ? native.error : error,
    fileSuggestionsError: fileSuggestionsQuery.error,
    t,
  });

  const loadingText =
    mode === "file"
      ? t("agentAutocomplete.searchingWorkspace")
      : t("agentAutocomplete.loadingCommands");
  const emptyText =
    mode === "file" ? t("agentAutocomplete.noFiles") : t("agentAutocomplete.noCommands");

  return {
    isVisible,
    options,
    selectedIndex,
    isLoading,
    errorMessage,
    loadingText,
    emptyText,
    onSelectOption,
    onKeyPress: handleKeyPress,
  };
}
