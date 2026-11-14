import { providerControl as enProviderControl } from "./resources/en/modules/provider-control";
import { hostMonitor as enHostMonitor } from "./resources/en/modules/host-monitor";
import { taskBoard as enTaskBoard } from "./resources/en/modules/task-board";
import { sharing as enSharing } from "./resources/en/modules/sharing";
import { voiceTimeline as enVoiceTimeline } from "./resources/en/modules/voice-timeline";
import { bots as enBots } from "./resources/en/modules/bots";
import { onlineService as enOnlineService } from "./resources/en/modules/online-service";
import { paneFind as enPaneFind } from "./resources/en/modules/pane-find";
import { common as enCommon } from "./resources/en/modules/common";
import { shell as enShell } from "./resources/en/modules/shell";
import { composer as enComposer } from "./resources/en/modules/composer";
import { btw as enBtw } from "./resources/en/modules/btw";
import { agentControls as enAgentControls } from "./resources/en/modules/agent-controls";
import { agentStream as enAgentStream } from "./resources/en/modules/agent-stream";
import { agentPanel as enAgentPanel } from "./resources/en/modules/agent-panel";
import { sessions as enSessions } from "./resources/en/modules/sessions";
import { agentList as enAgentList } from "./resources/en/modules/agent-list";
import { message as enMessage } from "./resources/en/modules/message";
import { importSession as enImportSession } from "./resources/en/modules/import-session";
import { workspace as enWorkspace } from "./resources/en/modules/workspace";
import { workspaceLabels as enWorkspaceLabels } from "./resources/en/modules/workspace-labels";
import { changelog as enChangelog } from "./resources/en/modules/changelog";
import { sidebar as enSidebar } from "./resources/en/modules/sidebar";
import { newWorkspace as enNewWorkspace } from "./resources/en/modules/new-workspace";
import { desktop as enDesktop } from "./resources/en/modules/desktop";
import { rootError as enRootError } from "./resources/en/modules/root-error";
import { startup as enStartup } from "./resources/en/modules/startup";
import { openProject as enOpenProject } from "./resources/en/modules/open-project";
import { hostPicker as enHostPicker } from "./resources/en/modules/host-picker";
import { schedules as enSchedules } from "./resources/en/modules/schedules";
import { addProjectFlow as enAddProjectFlow } from "./resources/en/modules/add-project-flow";
import { projectPicker as enProjectPicker } from "./resources/en/modules/project-picker";
import { branchSwitcher as enBranchSwitcher } from "./resources/en/modules/branch-switcher";
import { agentAutocomplete as enAgentAutocomplete } from "./resources/en/modules/agent-autocomplete";
import { loadOlderHistory as enLoadOlderHistory } from "./resources/en/modules/load-older-history";
import { imageAttachmentPicker as enImageAttachmentPicker } from "./resources/en/modules/image-attachment-picker";
import { workspaceSetup as enWorkspaceSetup } from "./resources/en/modules/workspace-setup";
import { onboarding as enOnboarding } from "./resources/en/modules/onboarding";
import { modelSelector as enModelSelector } from "./resources/en/modules/model-selector";
import { providerCatalog as enProviderCatalog } from "./resources/en/modules/provider-catalog";
import { providerSelection as enProviderSelection } from "./resources/en/modules/provider-selection";
import { providerUsage as enProviderUsage } from "./resources/en/modules/provider-usage";
import { pairing as enPairing } from "./resources/en/modules/pairing";
import { realtimeVoice as enRealtimeVoice } from "./resources/en/modules/realtime-voice";
import { rewind as enRewind } from "./resources/en/modules/rewind";
import { diffViewer as enDiffViewer } from "./resources/en/modules/diff-viewer";
import { serviceUrl as enServiceUrl } from "./resources/en/modules/service-url";
import { downloads as enDownloads } from "./resources/en/modules/downloads";
import { menu as enMenu } from "./resources/en/modules/menu";
import { subagents as enSubagents } from "./resources/en/modules/subagents";
import { panels as enPanels } from "./resources/en/modules/panels";
import { toolCallDetails as enToolCallDetails } from "./resources/en/modules/tool-call-details";
import { toolCallGroup as enToolCallGroup } from "./resources/en/modules/tool-call-group";
import { renameModal as enRenameModal } from "./resources/en/modules/rename-modal";
import { sidebarCallout as enSidebarCallout } from "./resources/en/modules/sidebar-callout";
import { contextWindow as enContextWindow } from "./resources/en/modules/context-window";
import { review as enReview } from "./resources/en/modules/review";
import { settings as enSettings } from "./resources/en/modules/settings";
import { providerControl as arProviderControl } from "./resources/ar/modules/provider-control";
import { hostMonitor as arHostMonitor } from "./resources/ar/modules/host-monitor";
import { taskBoard as arTaskBoard } from "./resources/ar/modules/task-board";
import { sharing as arSharing } from "./resources/ar/modules/sharing";
import { voiceTimeline as arVoiceTimeline } from "./resources/ar/modules/voice-timeline";
import { onlineService as arOnlineService } from "./resources/ar/modules/online-service";
import { bots as arBots } from "./resources/ar/modules/bots";
import { paneFind as arPaneFind } from "./resources/ar/modules/pane-find";
import { common as arCommon } from "./resources/ar/modules/common";
import { shell as arShell } from "./resources/ar/modules/shell";
import { composer as arComposer } from "./resources/ar/modules/composer";
import { btw as arBtw } from "./resources/ar/modules/btw";
import { agentControls as arAgentControls } from "./resources/ar/modules/agent-controls";
import { agentStream as arAgentStream } from "./resources/ar/modules/agent-stream";
import { agentPanel as arAgentPanel } from "./resources/ar/modules/agent-panel";
import { sessions as arSessions } from "./resources/ar/modules/sessions";
import { agentList as arAgentList } from "./resources/ar/modules/agent-list";
import { message as arMessage } from "./resources/ar/modules/message";
import { importSession as arImportSession } from "./resources/ar/modules/import-session";
import { workspace as arWorkspace } from "./resources/ar/modules/workspace";
import { workspaceLabels as arWorkspaceLabels } from "./resources/ar/modules/workspace-labels";
import { changelog as arChangelog } from "./resources/ar/modules/changelog";
import { sidebar as arSidebar } from "./resources/ar/modules/sidebar";
import { newWorkspace as arNewWorkspace } from "./resources/ar/modules/new-workspace";
import { desktop as arDesktop } from "./resources/ar/modules/desktop";
import { rootError as arRootError } from "./resources/ar/modules/root-error";
import { startup as arStartup } from "./resources/ar/modules/startup";
import { openProject as arOpenProject } from "./resources/ar/modules/open-project";
import { hostPicker as arHostPicker } from "./resources/ar/modules/host-picker";
import { schedules as arSchedules } from "./resources/ar/modules/schedules";
import { addProjectFlow as arAddProjectFlow } from "./resources/ar/modules/add-project-flow";
import { projectPicker as arProjectPicker } from "./resources/ar/modules/project-picker";
import { branchSwitcher as arBranchSwitcher } from "./resources/ar/modules/branch-switcher";
import { agentAutocomplete as arAgentAutocomplete } from "./resources/ar/modules/agent-autocomplete";
import { loadOlderHistory as arLoadOlderHistory } from "./resources/ar/modules/load-older-history";
import { imageAttachmentPicker as arImageAttachmentPicker } from "./resources/ar/modules/image-attachment-picker";
import { workspaceSetup as arWorkspaceSetup } from "./resources/ar/modules/workspace-setup";
import { onboarding as arOnboarding } from "./resources/ar/modules/onboarding";
import { modelSelector as arModelSelector } from "./resources/ar/modules/model-selector";
import { providerCatalog as arProviderCatalog } from "./resources/ar/modules/provider-catalog";
import { providerSelection as arProviderSelection } from "./resources/ar/modules/provider-selection";
import { providerUsage as arProviderUsage } from "./resources/ar/modules/provider-usage";
import { pairing as arPairing } from "./resources/ar/modules/pairing";
import { realtimeVoice as arRealtimeVoice } from "./resources/ar/modules/realtime-voice";
import { rewind as arRewind } from "./resources/ar/modules/rewind";
import { diffViewer as arDiffViewer } from "./resources/ar/modules/diff-viewer";
import { serviceUrl as arServiceUrl } from "./resources/ar/modules/service-url";
import { downloads as arDownloads } from "./resources/ar/modules/downloads";
import { menu as arMenu } from "./resources/ar/modules/menu";
import { subagents as arSubagents } from "./resources/ar/modules/subagents";
import { panels as arPanels } from "./resources/ar/modules/panels";
import { toolCallDetails as arToolCallDetails } from "./resources/ar/modules/tool-call-details";
import { toolCallGroup as arToolCallGroup } from "./resources/ar/modules/tool-call-group";
import { renameModal as arRenameModal } from "./resources/ar/modules/rename-modal";
import { sidebarCallout as arSidebarCallout } from "./resources/ar/modules/sidebar-callout";
import { contextWindow as arContextWindow } from "./resources/ar/modules/context-window";
import { review as arReview } from "./resources/ar/modules/review";
import { settings as arSettings } from "./resources/ar/modules/settings";
import { providerControl as esProviderControl } from "./resources/es/modules/provider-control";
import { hostMonitor as esHostMonitor } from "./resources/es/modules/host-monitor";
import { taskBoard as esTaskBoard } from "./resources/es/modules/task-board";
import { sharing as esSharing } from "./resources/es/modules/sharing";
import { voiceTimeline as esVoiceTimeline } from "./resources/es/modules/voice-timeline";
import { onlineService as esOnlineService } from "./resources/es/modules/online-service";
import { bots as esBots } from "./resources/es/modules/bots";
import { paneFind as esPaneFind } from "./resources/es/modules/pane-find";
import { common as esCommon } from "./resources/es/modules/common";
import { shell as esShell } from "./resources/es/modules/shell";
import { composer as esComposer } from "./resources/es/modules/composer";
import { btw as esBtw } from "./resources/es/modules/btw";
import { agentControls as esAgentControls } from "./resources/es/modules/agent-controls";
import { agentStream as esAgentStream } from "./resources/es/modules/agent-stream";
import { agentPanel as esAgentPanel } from "./resources/es/modules/agent-panel";
import { sessions as esSessions } from "./resources/es/modules/sessions";
import { agentList as esAgentList } from "./resources/es/modules/agent-list";
import { message as esMessage } from "./resources/es/modules/message";
import { importSession as esImportSession } from "./resources/es/modules/import-session";
import { workspace as esWorkspace } from "./resources/es/modules/workspace";
import { workspaceLabels as esWorkspaceLabels } from "./resources/es/modules/workspace-labels";
import { changelog as esChangelog } from "./resources/es/modules/changelog";
import { sidebar as esSidebar } from "./resources/es/modules/sidebar";
import { newWorkspace as esNewWorkspace } from "./resources/es/modules/new-workspace";
import { desktop as esDesktop } from "./resources/es/modules/desktop";
import { rootError as esRootError } from "./resources/es/modules/root-error";
import { startup as esStartup } from "./resources/es/modules/startup";
import { openProject as esOpenProject } from "./resources/es/modules/open-project";
import { hostPicker as esHostPicker } from "./resources/es/modules/host-picker";
import { schedules as esSchedules } from "./resources/es/modules/schedules";
import { addProjectFlow as esAddProjectFlow } from "./resources/es/modules/add-project-flow";
import { projectPicker as esProjectPicker } from "./resources/es/modules/project-picker";
import { branchSwitcher as esBranchSwitcher } from "./resources/es/modules/branch-switcher";
import { agentAutocomplete as esAgentAutocomplete } from "./resources/es/modules/agent-autocomplete";
import { loadOlderHistory as esLoadOlderHistory } from "./resources/es/modules/load-older-history";
import { imageAttachmentPicker as esImageAttachmentPicker } from "./resources/es/modules/image-attachment-picker";
import { workspaceSetup as esWorkspaceSetup } from "./resources/es/modules/workspace-setup";
import { onboarding as esOnboarding } from "./resources/es/modules/onboarding";
import { modelSelector as esModelSelector } from "./resources/es/modules/model-selector";
import { providerCatalog as esProviderCatalog } from "./resources/es/modules/provider-catalog";
import { providerSelection as esProviderSelection } from "./resources/es/modules/provider-selection";
import { providerUsage as esProviderUsage } from "./resources/es/modules/provider-usage";
import { pairing as esPairing } from "./resources/es/modules/pairing";
import { realtimeVoice as esRealtimeVoice } from "./resources/es/modules/realtime-voice";
import { rewind as esRewind } from "./resources/es/modules/rewind";
import { diffViewer as esDiffViewer } from "./resources/es/modules/diff-viewer";
import { serviceUrl as esServiceUrl } from "./resources/es/modules/service-url";
import { downloads as esDownloads } from "./resources/es/modules/downloads";
import { menu as esMenu } from "./resources/es/modules/menu";
import { subagents as esSubagents } from "./resources/es/modules/subagents";
import { panels as esPanels } from "./resources/es/modules/panels";
import { toolCallDetails as esToolCallDetails } from "./resources/es/modules/tool-call-details";
import { toolCallGroup as esToolCallGroup } from "./resources/es/modules/tool-call-group";
import { renameModal as esRenameModal } from "./resources/es/modules/rename-modal";
import { sidebarCallout as esSidebarCallout } from "./resources/es/modules/sidebar-callout";
import { contextWindow as esContextWindow } from "./resources/es/modules/context-window";
import { review as esReview } from "./resources/es/modules/review";
import { settings as esSettings } from "./resources/es/modules/settings";
import { providerControl as frProviderControl } from "./resources/fr/modules/provider-control";
import { hostMonitor as frHostMonitor } from "./resources/fr/modules/host-monitor";
import { taskBoard as frTaskBoard } from "./resources/fr/modules/task-board";
import { sharing as frSharing } from "./resources/fr/modules/sharing";
import { voiceTimeline as frVoiceTimeline } from "./resources/fr/modules/voice-timeline";
import { onlineService as frOnlineService } from "./resources/fr/modules/online-service";
import { bots as frBots } from "./resources/fr/modules/bots";
import { paneFind as frPaneFind } from "./resources/fr/modules/pane-find";
import { common as frCommon } from "./resources/fr/modules/common";
import { shell as frShell } from "./resources/fr/modules/shell";
import { composer as frComposer } from "./resources/fr/modules/composer";
import { btw as frBtw } from "./resources/fr/modules/btw";
import { agentControls as frAgentControls } from "./resources/fr/modules/agent-controls";
import { agentStream as frAgentStream } from "./resources/fr/modules/agent-stream";
import { agentPanel as frAgentPanel } from "./resources/fr/modules/agent-panel";
import { sessions as frSessions } from "./resources/fr/modules/sessions";
import { agentList as frAgentList } from "./resources/fr/modules/agent-list";
import { message as frMessage } from "./resources/fr/modules/message";
import { importSession as frImportSession } from "./resources/fr/modules/import-session";
import { workspace as frWorkspace } from "./resources/fr/modules/workspace";
import { workspaceLabels as frWorkspaceLabels } from "./resources/fr/modules/workspace-labels";
import { changelog as frChangelog } from "./resources/fr/modules/changelog";
import { sidebar as frSidebar } from "./resources/fr/modules/sidebar";
import { newWorkspace as frNewWorkspace } from "./resources/fr/modules/new-workspace";
import { desktop as frDesktop } from "./resources/fr/modules/desktop";
import { rootError as frRootError } from "./resources/fr/modules/root-error";
import { startup as frStartup } from "./resources/fr/modules/startup";
import { openProject as frOpenProject } from "./resources/fr/modules/open-project";
import { hostPicker as frHostPicker } from "./resources/fr/modules/host-picker";
import { schedules as frSchedules } from "./resources/fr/modules/schedules";
import { addProjectFlow as frAddProjectFlow } from "./resources/fr/modules/add-project-flow";
import { projectPicker as frProjectPicker } from "./resources/fr/modules/project-picker";
import { branchSwitcher as frBranchSwitcher } from "./resources/fr/modules/branch-switcher";
import { agentAutocomplete as frAgentAutocomplete } from "./resources/fr/modules/agent-autocomplete";
import { loadOlderHistory as frLoadOlderHistory } from "./resources/fr/modules/load-older-history";
import { imageAttachmentPicker as frImageAttachmentPicker } from "./resources/fr/modules/image-attachment-picker";
import { workspaceSetup as frWorkspaceSetup } from "./resources/fr/modules/workspace-setup";
import { onboarding as frOnboarding } from "./resources/fr/modules/onboarding";
import { modelSelector as frModelSelector } from "./resources/fr/modules/model-selector";
import { providerCatalog as frProviderCatalog } from "./resources/fr/modules/provider-catalog";
import { providerSelection as frProviderSelection } from "./resources/fr/modules/provider-selection";
import { providerUsage as frProviderUsage } from "./resources/fr/modules/provider-usage";
import { pairing as frPairing } from "./resources/fr/modules/pairing";
import { realtimeVoice as frRealtimeVoice } from "./resources/fr/modules/realtime-voice";
import { rewind as frRewind } from "./resources/fr/modules/rewind";
import { diffViewer as frDiffViewer } from "./resources/fr/modules/diff-viewer";
import { serviceUrl as frServiceUrl } from "./resources/fr/modules/service-url";
import { downloads as frDownloads } from "./resources/fr/modules/downloads";
import { menu as frMenu } from "./resources/fr/modules/menu";
import { subagents as frSubagents } from "./resources/fr/modules/subagents";
import { panels as frPanels } from "./resources/fr/modules/panels";
import { toolCallDetails as frToolCallDetails } from "./resources/fr/modules/tool-call-details";
import { toolCallGroup as frToolCallGroup } from "./resources/fr/modules/tool-call-group";
import { renameModal as frRenameModal } from "./resources/fr/modules/rename-modal";
import { sidebarCallout as frSidebarCallout } from "./resources/fr/modules/sidebar-callout";
import { contextWindow as frContextWindow } from "./resources/fr/modules/context-window";
import { review as frReview } from "./resources/fr/modules/review";
import { settings as frSettings } from "./resources/fr/modules/settings";
import { providerControl as jaProviderControl } from "./resources/ja/modules/provider-control";
import { hostMonitor as jaHostMonitor } from "./resources/ja/modules/host-monitor";
import { taskBoard as jaTaskBoard } from "./resources/ja/modules/task-board";
import { sharing as jaSharing } from "./resources/ja/modules/sharing";
import { voiceTimeline as jaVoiceTimeline } from "./resources/ja/modules/voice-timeline";
import { onlineService as jaOnlineService } from "./resources/ja/modules/online-service";
import { bots as jaBots } from "./resources/ja/modules/bots";
import { paneFind as jaPaneFind } from "./resources/ja/modules/pane-find";
import { common as jaCommon } from "./resources/ja/modules/common";
import { shell as jaShell } from "./resources/ja/modules/shell";
import { composer as jaComposer } from "./resources/ja/modules/composer";
import { btw as jaBtw } from "./resources/ja/modules/btw";
import { agentControls as jaAgentControls } from "./resources/ja/modules/agent-controls";
import { agentStream as jaAgentStream } from "./resources/ja/modules/agent-stream";
import { agentPanel as jaAgentPanel } from "./resources/ja/modules/agent-panel";
import { sessions as jaSessions } from "./resources/ja/modules/sessions";
import { agentList as jaAgentList } from "./resources/ja/modules/agent-list";
import { message as jaMessage } from "./resources/ja/modules/message";
import { importSession as jaImportSession } from "./resources/ja/modules/import-session";
import { workspace as jaWorkspace } from "./resources/ja/modules/workspace";
import { workspaceLabels as jaWorkspaceLabels } from "./resources/ja/modules/workspace-labels";
import { changelog as jaChangelog } from "./resources/ja/modules/changelog";
import { sidebar as jaSidebar } from "./resources/ja/modules/sidebar";
import { newWorkspace as jaNewWorkspace } from "./resources/ja/modules/new-workspace";
import { desktop as jaDesktop } from "./resources/ja/modules/desktop";
import { rootError as jaRootError } from "./resources/ja/modules/root-error";
import { startup as jaStartup } from "./resources/ja/modules/startup";
import { openProject as jaOpenProject } from "./resources/ja/modules/open-project";
import { hostPicker as jaHostPicker } from "./resources/ja/modules/host-picker";
import { schedules as jaSchedules } from "./resources/ja/modules/schedules";
import { addProjectFlow as jaAddProjectFlow } from "./resources/ja/modules/add-project-flow";
import { projectPicker as jaProjectPicker } from "./resources/ja/modules/project-picker";
import { branchSwitcher as jaBranchSwitcher } from "./resources/ja/modules/branch-switcher";
import { agentAutocomplete as jaAgentAutocomplete } from "./resources/ja/modules/agent-autocomplete";
import { loadOlderHistory as jaLoadOlderHistory } from "./resources/ja/modules/load-older-history";
import { imageAttachmentPicker as jaImageAttachmentPicker } from "./resources/ja/modules/image-attachment-picker";
import { workspaceSetup as jaWorkspaceSetup } from "./resources/ja/modules/workspace-setup";
import { onboarding as jaOnboarding } from "./resources/ja/modules/onboarding";
import { modelSelector as jaModelSelector } from "./resources/ja/modules/model-selector";
import { providerCatalog as jaProviderCatalog } from "./resources/ja/modules/provider-catalog";
import { providerSelection as jaProviderSelection } from "./resources/ja/modules/provider-selection";
import { providerUsage as jaProviderUsage } from "./resources/ja/modules/provider-usage";
import { pairing as jaPairing } from "./resources/ja/modules/pairing";
import { realtimeVoice as jaRealtimeVoice } from "./resources/ja/modules/realtime-voice";
import { rewind as jaRewind } from "./resources/ja/modules/rewind";
import { diffViewer as jaDiffViewer } from "./resources/ja/modules/diff-viewer";
import { serviceUrl as jaServiceUrl } from "./resources/ja/modules/service-url";
import { downloads as jaDownloads } from "./resources/ja/modules/downloads";
import { menu as jaMenu } from "./resources/ja/modules/menu";
import { subagents as jaSubagents } from "./resources/ja/modules/subagents";
import { panels as jaPanels } from "./resources/ja/modules/panels";
import { toolCallDetails as jaToolCallDetails } from "./resources/ja/modules/tool-call-details";
import { toolCallGroup as jaToolCallGroup } from "./resources/ja/modules/tool-call-group";
import { renameModal as jaRenameModal } from "./resources/ja/modules/rename-modal";
import { sidebarCallout as jaSidebarCallout } from "./resources/ja/modules/sidebar-callout";
import { contextWindow as jaContextWindow } from "./resources/ja/modules/context-window";
import { review as jaReview } from "./resources/ja/modules/review";
import { settings as jaSettings } from "./resources/ja/modules/settings";
import { providerControl as koProviderControl } from "./resources/ko/modules/provider-control";
import { hostMonitor as koHostMonitor } from "./resources/ko/modules/host-monitor";
import { taskBoard as koTaskBoard } from "./resources/ko/modules/task-board";
import { sharing as koSharing } from "./resources/ko/modules/sharing";
import { voiceTimeline as koVoiceTimeline } from "./resources/ko/modules/voice-timeline";
import { onlineService as koOnlineService } from "./resources/ko/modules/online-service";
import { bots as koBots } from "./resources/ko/modules/bots";
import { paneFind as koPaneFind } from "./resources/ko/modules/pane-find";
import { common as koCommon } from "./resources/ko/modules/common";
import { shell as koShell } from "./resources/ko/modules/shell";
import { composer as koComposer } from "./resources/ko/modules/composer";
import { btw as koBtw } from "./resources/ko/modules/btw";
import { agentControls as koAgentControls } from "./resources/ko/modules/agent-controls";
import { agentStream as koAgentStream } from "./resources/ko/modules/agent-stream";
import { agentPanel as koAgentPanel } from "./resources/ko/modules/agent-panel";
import { sessions as koSessions } from "./resources/ko/modules/sessions";
import { agentList as koAgentList } from "./resources/ko/modules/agent-list";
import { message as koMessage } from "./resources/ko/modules/message";
import { importSession as koImportSession } from "./resources/ko/modules/import-session";
import { workspace as koWorkspace } from "./resources/ko/modules/workspace";
import { workspaceLabels as koWorkspaceLabels } from "./resources/ko/modules/workspace-labels";
import { changelog as koChangelog } from "./resources/ko/modules/changelog";
import { sidebar as koSidebar } from "./resources/ko/modules/sidebar";
import { newWorkspace as koNewWorkspace } from "./resources/ko/modules/new-workspace";
import { desktop as koDesktop } from "./resources/ko/modules/desktop";
import { rootError as koRootError } from "./resources/ko/modules/root-error";
import { startup as koStartup } from "./resources/ko/modules/startup";
import { openProject as koOpenProject } from "./resources/ko/modules/open-project";
import { hostPicker as koHostPicker } from "./resources/ko/modules/host-picker";
import { schedules as koSchedules } from "./resources/ko/modules/schedules";
import { addProjectFlow as koAddProjectFlow } from "./resources/ko/modules/add-project-flow";
import { projectPicker as koProjectPicker } from "./resources/ko/modules/project-picker";
import { branchSwitcher as koBranchSwitcher } from "./resources/ko/modules/branch-switcher";
import { agentAutocomplete as koAgentAutocomplete } from "./resources/ko/modules/agent-autocomplete";
import { loadOlderHistory as koLoadOlderHistory } from "./resources/ko/modules/load-older-history";
import { imageAttachmentPicker as koImageAttachmentPicker } from "./resources/ko/modules/image-attachment-picker";
import { workspaceSetup as koWorkspaceSetup } from "./resources/ko/modules/workspace-setup";
import { onboarding as koOnboarding } from "./resources/ko/modules/onboarding";
import { modelSelector as koModelSelector } from "./resources/ko/modules/model-selector";
import { providerCatalog as koProviderCatalog } from "./resources/ko/modules/provider-catalog";
import { providerSelection as koProviderSelection } from "./resources/ko/modules/provider-selection";
import { providerUsage as koProviderUsage } from "./resources/ko/modules/provider-usage";
import { pairing as koPairing } from "./resources/ko/modules/pairing";
import { realtimeVoice as koRealtimeVoice } from "./resources/ko/modules/realtime-voice";
import { rewind as koRewind } from "./resources/ko/modules/rewind";
import { diffViewer as koDiffViewer } from "./resources/ko/modules/diff-viewer";
import { serviceUrl as koServiceUrl } from "./resources/ko/modules/service-url";
import { downloads as koDownloads } from "./resources/ko/modules/downloads";
import { menu as koMenu } from "./resources/ko/modules/menu";
import { subagents as koSubagents } from "./resources/ko/modules/subagents";
import { panels as koPanels } from "./resources/ko/modules/panels";
import { toolCallDetails as koToolCallDetails } from "./resources/ko/modules/tool-call-details";
import { toolCallGroup as koToolCallGroup } from "./resources/ko/modules/tool-call-group";
import { renameModal as koRenameModal } from "./resources/ko/modules/rename-modal";
import { sidebarCallout as koSidebarCallout } from "./resources/ko/modules/sidebar-callout";
import { contextWindow as koContextWindow } from "./resources/ko/modules/context-window";
import { review as koReview } from "./resources/ko/modules/review";
import { settings as koSettings } from "./resources/ko/modules/settings";
import { providerControl as ptBRProviderControl } from "./resources/pt-BR/modules/provider-control";
import { hostMonitor as ptBRHostMonitor } from "./resources/pt-BR/modules/host-monitor";
import { taskBoard as ptBRTaskBoard } from "./resources/pt-BR/modules/task-board";
import { sharing as ptBRSharing } from "./resources/pt-BR/modules/sharing";
import { voiceTimeline as ptBRVoiceTimeline } from "./resources/pt-BR/modules/voice-timeline";
import { onlineService as ptBROnlineService } from "./resources/pt-BR/modules/online-service";
import { bots as ptBRBots } from "./resources/pt-BR/modules/bots";
import { paneFind as ptBRPaneFind } from "./resources/pt-BR/modules/pane-find";
import { common as ptBRCommon } from "./resources/pt-BR/modules/common";
import { shell as ptBRShell } from "./resources/pt-BR/modules/shell";
import { composer as ptBRComposer } from "./resources/pt-BR/modules/composer";
import { btw as ptBRBtw } from "./resources/pt-BR/modules/btw";
import { agentControls as ptBRAgentControls } from "./resources/pt-BR/modules/agent-controls";
import { agentStream as ptBRAgentStream } from "./resources/pt-BR/modules/agent-stream";
import { agentPanel as ptBRAgentPanel } from "./resources/pt-BR/modules/agent-panel";
import { sessions as ptBRSessions } from "./resources/pt-BR/modules/sessions";
import { agentList as ptBRAgentList } from "./resources/pt-BR/modules/agent-list";
import { message as ptBRMessage } from "./resources/pt-BR/modules/message";
import { importSession as ptBRImportSession } from "./resources/pt-BR/modules/import-session";
import { workspace as ptBRWorkspace } from "./resources/pt-BR/modules/workspace";
import { workspaceLabels as ptBRWorkspaceLabels } from "./resources/pt-BR/modules/workspace-labels";
import { changelog as ptBRChangelog } from "./resources/pt-BR/modules/changelog";
import { sidebar as ptBRSidebar } from "./resources/pt-BR/modules/sidebar";
import { newWorkspace as ptBRNewWorkspace } from "./resources/pt-BR/modules/new-workspace";
import { desktop as ptBRDesktop } from "./resources/pt-BR/modules/desktop";
import { rootError as ptBRRootError } from "./resources/pt-BR/modules/root-error";
import { startup as ptBRStartup } from "./resources/pt-BR/modules/startup";
import { openProject as ptBROpenProject } from "./resources/pt-BR/modules/open-project";
import { hostPicker as ptBRHostPicker } from "./resources/pt-BR/modules/host-picker";
import { schedules as ptBRSchedules } from "./resources/pt-BR/modules/schedules";
import { addProjectFlow as ptBRAddProjectFlow } from "./resources/pt-BR/modules/add-project-flow";
import { projectPicker as ptBRProjectPicker } from "./resources/pt-BR/modules/project-picker";
import { branchSwitcher as ptBRBranchSwitcher } from "./resources/pt-BR/modules/branch-switcher";
import { agentAutocomplete as ptBRAgentAutocomplete } from "./resources/pt-BR/modules/agent-autocomplete";
import { loadOlderHistory as ptBRLoadOlderHistory } from "./resources/pt-BR/modules/load-older-history";
import { imageAttachmentPicker as ptBRImageAttachmentPicker } from "./resources/pt-BR/modules/image-attachment-picker";
import { workspaceSetup as ptBRWorkspaceSetup } from "./resources/pt-BR/modules/workspace-setup";
import { onboarding as ptBROnboarding } from "./resources/pt-BR/modules/onboarding";
import { modelSelector as ptBRModelSelector } from "./resources/pt-BR/modules/model-selector";
import { providerCatalog as ptBRProviderCatalog } from "./resources/pt-BR/modules/provider-catalog";
import { providerSelection as ptBRProviderSelection } from "./resources/pt-BR/modules/provider-selection";
import { providerUsage as ptBRProviderUsage } from "./resources/pt-BR/modules/provider-usage";
import { pairing as ptBRPairing } from "./resources/pt-BR/modules/pairing";
import { realtimeVoice as ptBRRealtimeVoice } from "./resources/pt-BR/modules/realtime-voice";
import { rewind as ptBRRewind } from "./resources/pt-BR/modules/rewind";
import { diffViewer as ptBRDiffViewer } from "./resources/pt-BR/modules/diff-viewer";
import { serviceUrl as ptBRServiceUrl } from "./resources/pt-BR/modules/service-url";
import { downloads as ptBRDownloads } from "./resources/pt-BR/modules/downloads";
import { menu as ptBRMenu } from "./resources/pt-BR/modules/menu";
import { subagents as ptBRSubagents } from "./resources/pt-BR/modules/subagents";
import { panels as ptBRPanels } from "./resources/pt-BR/modules/panels";
import { toolCallDetails as ptBRToolCallDetails } from "./resources/pt-BR/modules/tool-call-details";
import { toolCallGroup as ptBRToolCallGroup } from "./resources/pt-BR/modules/tool-call-group";
import { renameModal as ptBRRenameModal } from "./resources/pt-BR/modules/rename-modal";
import { sidebarCallout as ptBRSidebarCallout } from "./resources/pt-BR/modules/sidebar-callout";
import { contextWindow as ptBRContextWindow } from "./resources/pt-BR/modules/context-window";
import { review as ptBRReview } from "./resources/pt-BR/modules/review";
import { settings as ptBRSettings } from "./resources/pt-BR/modules/settings";
import { providerControl as ruProviderControl } from "./resources/ru/modules/provider-control";
import { hostMonitor as ruHostMonitor } from "./resources/ru/modules/host-monitor";
import { taskBoard as ruTaskBoard } from "./resources/ru/modules/task-board";
import { sharing as ruSharing } from "./resources/ru/modules/sharing";
import { voiceTimeline as ruVoiceTimeline } from "./resources/ru/modules/voice-timeline";
import { onlineService as ruOnlineService } from "./resources/ru/modules/online-service";
import { bots as ruBots } from "./resources/ru/modules/bots";
import { paneFind as ruPaneFind } from "./resources/ru/modules/pane-find";
import { common as ruCommon } from "./resources/ru/modules/common";
import { shell as ruShell } from "./resources/ru/modules/shell";
import { composer as ruComposer } from "./resources/ru/modules/composer";
import { btw as ruBtw } from "./resources/ru/modules/btw";
import { agentControls as ruAgentControls } from "./resources/ru/modules/agent-controls";
import { agentStream as ruAgentStream } from "./resources/ru/modules/agent-stream";
import { agentPanel as ruAgentPanel } from "./resources/ru/modules/agent-panel";
import { sessions as ruSessions } from "./resources/ru/modules/sessions";
import { agentList as ruAgentList } from "./resources/ru/modules/agent-list";
import { message as ruMessage } from "./resources/ru/modules/message";
import { importSession as ruImportSession } from "./resources/ru/modules/import-session";
import { workspace as ruWorkspace } from "./resources/ru/modules/workspace";
import { workspaceLabels as ruWorkspaceLabels } from "./resources/ru/modules/workspace-labels";
import { changelog as ruChangelog } from "./resources/ru/modules/changelog";
import { sidebar as ruSidebar } from "./resources/ru/modules/sidebar";
import { newWorkspace as ruNewWorkspace } from "./resources/ru/modules/new-workspace";
import { desktop as ruDesktop } from "./resources/ru/modules/desktop";
import { rootError as ruRootError } from "./resources/ru/modules/root-error";
import { startup as ruStartup } from "./resources/ru/modules/startup";
import { openProject as ruOpenProject } from "./resources/ru/modules/open-project";
import { hostPicker as ruHostPicker } from "./resources/ru/modules/host-picker";
import { schedules as ruSchedules } from "./resources/ru/modules/schedules";
import { addProjectFlow as ruAddProjectFlow } from "./resources/ru/modules/add-project-flow";
import { projectPicker as ruProjectPicker } from "./resources/ru/modules/project-picker";
import { branchSwitcher as ruBranchSwitcher } from "./resources/ru/modules/branch-switcher";
import { agentAutocomplete as ruAgentAutocomplete } from "./resources/ru/modules/agent-autocomplete";
import { loadOlderHistory as ruLoadOlderHistory } from "./resources/ru/modules/load-older-history";
import { imageAttachmentPicker as ruImageAttachmentPicker } from "./resources/ru/modules/image-attachment-picker";
import { workspaceSetup as ruWorkspaceSetup } from "./resources/ru/modules/workspace-setup";
import { onboarding as ruOnboarding } from "./resources/ru/modules/onboarding";
import { modelSelector as ruModelSelector } from "./resources/ru/modules/model-selector";
import { providerCatalog as ruProviderCatalog } from "./resources/ru/modules/provider-catalog";
import { providerSelection as ruProviderSelection } from "./resources/ru/modules/provider-selection";
import { providerUsage as ruProviderUsage } from "./resources/ru/modules/provider-usage";
import { pairing as ruPairing } from "./resources/ru/modules/pairing";
import { realtimeVoice as ruRealtimeVoice } from "./resources/ru/modules/realtime-voice";
import { rewind as ruRewind } from "./resources/ru/modules/rewind";
import { diffViewer as ruDiffViewer } from "./resources/ru/modules/diff-viewer";
import { serviceUrl as ruServiceUrl } from "./resources/ru/modules/service-url";
import { downloads as ruDownloads } from "./resources/ru/modules/downloads";
import { menu as ruMenu } from "./resources/ru/modules/menu";
import { subagents as ruSubagents } from "./resources/ru/modules/subagents";
import { panels as ruPanels } from "./resources/ru/modules/panels";
import { toolCallDetails as ruToolCallDetails } from "./resources/ru/modules/tool-call-details";
import { toolCallGroup as ruToolCallGroup } from "./resources/ru/modules/tool-call-group";
import { renameModal as ruRenameModal } from "./resources/ru/modules/rename-modal";
import { sidebarCallout as ruSidebarCallout } from "./resources/ru/modules/sidebar-callout";
import { contextWindow as ruContextWindow } from "./resources/ru/modules/context-window";
import { review as ruReview } from "./resources/ru/modules/review";
import { settings as ruSettings } from "./resources/ru/modules/settings";
import { providerControl as zhCNProviderControl } from "./resources/zh-CN/modules/provider-control";
import { hostMonitor as zhCNHostMonitor } from "./resources/zh-CN/modules/host-monitor";
import { taskBoard as zhCNTaskBoard } from "./resources/zh-CN/modules/task-board";
import { sharing as zhCNSharing } from "./resources/zh-CN/modules/sharing";
import { voiceTimeline as zhCNVoiceTimeline } from "./resources/zh-CN/modules/voice-timeline";
import { bots as zhCNBots } from "./resources/zh-CN/modules/bots";
import { onlineService as zhCNOnlineService } from "./resources/zh-CN/modules/online-service";
import { paneFind as zhCNPaneFind } from "./resources/zh-CN/modules/pane-find";
import { common as zhCNCommon } from "./resources/zh-CN/modules/common";
import { shell as zhCNShell } from "./resources/zh-CN/modules/shell";
import { composer as zhCNComposer } from "./resources/zh-CN/modules/composer";
import { btw as zhCNBtw } from "./resources/zh-CN/modules/btw";
import { agentControls as zhCNAgentControls } from "./resources/zh-CN/modules/agent-controls";
import { agentStream as zhCNAgentStream } from "./resources/zh-CN/modules/agent-stream";
import { agentPanel as zhCNAgentPanel } from "./resources/zh-CN/modules/agent-panel";
import { sessions as zhCNSessions } from "./resources/zh-CN/modules/sessions";
import { agentList as zhCNAgentList } from "./resources/zh-CN/modules/agent-list";
import { message as zhCNMessage } from "./resources/zh-CN/modules/message";
import { importSession as zhCNImportSession } from "./resources/zh-CN/modules/import-session";
import { workspace as zhCNWorkspace } from "./resources/zh-CN/modules/workspace";
import { workspaceLabels as zhCNWorkspaceLabels } from "./resources/zh-CN/modules/workspace-labels";
import { changelog as zhCNChangelog } from "./resources/zh-CN/modules/changelog";
import { sidebar as zhCNSidebar } from "./resources/zh-CN/modules/sidebar";
import { newWorkspace as zhCNNewWorkspace } from "./resources/zh-CN/modules/new-workspace";
import { desktop as zhCNDesktop } from "./resources/zh-CN/modules/desktop";
import { rootError as zhCNRootError } from "./resources/zh-CN/modules/root-error";
import { startup as zhCNStartup } from "./resources/zh-CN/modules/startup";
import { openProject as zhCNOpenProject } from "./resources/zh-CN/modules/open-project";
import { hostPicker as zhCNHostPicker } from "./resources/zh-CN/modules/host-picker";
import { schedules as zhCNSchedules } from "./resources/zh-CN/modules/schedules";
import { addProjectFlow as zhCNAddProjectFlow } from "./resources/zh-CN/modules/add-project-flow";
import { projectPicker as zhCNProjectPicker } from "./resources/zh-CN/modules/project-picker";
import { branchSwitcher as zhCNBranchSwitcher } from "./resources/zh-CN/modules/branch-switcher";
import { agentAutocomplete as zhCNAgentAutocomplete } from "./resources/zh-CN/modules/agent-autocomplete";
import { loadOlderHistory as zhCNLoadOlderHistory } from "./resources/zh-CN/modules/load-older-history";
import { imageAttachmentPicker as zhCNImageAttachmentPicker } from "./resources/zh-CN/modules/image-attachment-picker";
import { workspaceSetup as zhCNWorkspaceSetup } from "./resources/zh-CN/modules/workspace-setup";
import { onboarding as zhCNOnboarding } from "./resources/zh-CN/modules/onboarding";
import { modelSelector as zhCNModelSelector } from "./resources/zh-CN/modules/model-selector";
import { providerCatalog as zhCNProviderCatalog } from "./resources/zh-CN/modules/provider-catalog";
import { providerSelection as zhCNProviderSelection } from "./resources/zh-CN/modules/provider-selection";
import { providerUsage as zhCNProviderUsage } from "./resources/zh-CN/modules/provider-usage";
import { pairing as zhCNPairing } from "./resources/zh-CN/modules/pairing";
import { realtimeVoice as zhCNRealtimeVoice } from "./resources/zh-CN/modules/realtime-voice";
import { rewind as zhCNRewind } from "./resources/zh-CN/modules/rewind";
import { diffViewer as zhCNDiffViewer } from "./resources/zh-CN/modules/diff-viewer";
import { serviceUrl as zhCNServiceUrl } from "./resources/zh-CN/modules/service-url";
import { downloads as zhCNDownloads } from "./resources/zh-CN/modules/downloads";
import { menu as zhCNMenu } from "./resources/zh-CN/modules/menu";
import { subagents as zhCNSubagents } from "./resources/zh-CN/modules/subagents";
import { panels as zhCNPanels } from "./resources/zh-CN/modules/panels";
import { toolCallDetails as zhCNToolCallDetails } from "./resources/zh-CN/modules/tool-call-details";
import { toolCallGroup as zhCNToolCallGroup } from "./resources/zh-CN/modules/tool-call-group";
import { renameModal as zhCNRenameModal } from "./resources/zh-CN/modules/rename-modal";
import { sidebarCallout as zhCNSidebarCallout } from "./resources/zh-CN/modules/sidebar-callout";
import { contextWindow as zhCNContextWindow } from "./resources/zh-CN/modules/context-window";
import { review as zhCNReview } from "./resources/zh-CN/modules/review";
import { settings as zhCNSettings } from "./resources/zh-CN/modules/settings";

export const en = {
  providerControl: enProviderControl,
  hostMonitor: enHostMonitor,
  taskBoard: enTaskBoard,
  sharing: enSharing,
  voiceTimeline: enVoiceTimeline,
  bots: enBots,
  onlineService: enOnlineService,
  paneFind: enPaneFind,
  common: enCommon,
  shell: enShell,
  composer: enComposer,
  btw: enBtw,
  agentControls: enAgentControls,
  agentStream: enAgentStream,
  agentPanel: enAgentPanel,
  sessions: enSessions,
  agentList: enAgentList,
  message: enMessage,
  importSession: enImportSession,
  workspace: enWorkspace,
  workspaceLabels: enWorkspaceLabels,
  changelog: enChangelog,
  sidebar: enSidebar,
  newWorkspace: enNewWorkspace,
  desktop: enDesktop,
  rootError: enRootError,
  startup: enStartup,
  openProject: enOpenProject,
  hostPicker: enHostPicker,
  schedules: enSchedules,
  addProjectFlow: enAddProjectFlow,
  projectPicker: enProjectPicker,
  branchSwitcher: enBranchSwitcher,
  agentAutocomplete: enAgentAutocomplete,
  loadOlderHistory: enLoadOlderHistory,
  imageAttachmentPicker: enImageAttachmentPicker,
  workspaceSetup: enWorkspaceSetup,
  onboarding: enOnboarding,
  modelSelector: enModelSelector,
  providerCatalog: enProviderCatalog,
  providerSelection: enProviderSelection,
  providerUsage: enProviderUsage,
  pairing: enPairing,
  realtimeVoice: enRealtimeVoice,
  rewind: enRewind,
  diffViewer: enDiffViewer,
  serviceUrl: enServiceUrl,
  downloads: enDownloads,
  menu: enMenu,
  subagents: enSubagents,
  panels: enPanels,
  toolCallDetails: enToolCallDetails,
  toolCallGroup: enToolCallGroup,
  renameModal: enRenameModal,
  sidebarCallout: enSidebarCallout,
  contextWindow: enContextWindow,
  review: enReview,
  settings: enSettings,
};

type WidenStringLeaves<T> = {
  [K in keyof T]: T[K] extends string ? string : WidenStringLeaves<T[K]>;
};

export type TranslationResources = WidenStringLeaves<typeof en>;

export const ar: TranslationResources = {
  providerControl: arProviderControl,
  hostMonitor: arHostMonitor,
  taskBoard: arTaskBoard,
  sharing: arSharing,
  voiceTimeline: arVoiceTimeline,
  onlineService: arOnlineService,
  bots: arBots,
  paneFind: arPaneFind,
  common: arCommon,
  shell: arShell,
  composer: arComposer,
  btw: arBtw,
  agentControls: arAgentControls,
  agentStream: arAgentStream,
  agentPanel: arAgentPanel,
  sessions: arSessions,
  agentList: arAgentList,
  message: arMessage,
  importSession: arImportSession,
  workspace: arWorkspace,
  workspaceLabels: arWorkspaceLabels,
  changelog: arChangelog,
  sidebar: arSidebar,
  newWorkspace: arNewWorkspace,
  desktop: arDesktop,
  rootError: arRootError,
  startup: arStartup,
  openProject: arOpenProject,
  hostPicker: arHostPicker,
  schedules: arSchedules,
  addProjectFlow: arAddProjectFlow,
  projectPicker: arProjectPicker,
  branchSwitcher: arBranchSwitcher,
  agentAutocomplete: arAgentAutocomplete,
  loadOlderHistory: arLoadOlderHistory,
  imageAttachmentPicker: arImageAttachmentPicker,
  workspaceSetup: arWorkspaceSetup,
  onboarding: arOnboarding,
  modelSelector: arModelSelector,
  providerCatalog: arProviderCatalog,
  providerSelection: arProviderSelection,
  providerUsage: arProviderUsage,
  pairing: arPairing,
  realtimeVoice: arRealtimeVoice,
  rewind: arRewind,
  diffViewer: arDiffViewer,
  serviceUrl: arServiceUrl,
  downloads: arDownloads,
  menu: arMenu,
  subagents: arSubagents,
  panels: arPanels,
  toolCallDetails: arToolCallDetails,
  toolCallGroup: arToolCallGroup,
  renameModal: arRenameModal,
  sidebarCallout: arSidebarCallout,
  contextWindow: arContextWindow,
  review: arReview,
  settings: arSettings,
};

export const es: TranslationResources = {
  providerControl: esProviderControl,
  hostMonitor: esHostMonitor,
  taskBoard: esTaskBoard,
  sharing: esSharing,
  voiceTimeline: esVoiceTimeline,
  onlineService: esOnlineService,
  bots: esBots,
  paneFind: esPaneFind,
  common: esCommon,
  shell: esShell,
  composer: esComposer,
  btw: esBtw,
  agentControls: esAgentControls,
  agentStream: esAgentStream,
  agentPanel: esAgentPanel,
  sessions: esSessions,
  agentList: esAgentList,
  message: esMessage,
  importSession: esImportSession,
  workspace: esWorkspace,
  workspaceLabels: esWorkspaceLabels,
  changelog: esChangelog,
  sidebar: esSidebar,
  newWorkspace: esNewWorkspace,
  desktop: esDesktop,
  rootError: esRootError,
  startup: esStartup,
  openProject: esOpenProject,
  hostPicker: esHostPicker,
  schedules: esSchedules,
  addProjectFlow: esAddProjectFlow,
  projectPicker: esProjectPicker,
  branchSwitcher: esBranchSwitcher,
  agentAutocomplete: esAgentAutocomplete,
  loadOlderHistory: esLoadOlderHistory,
  imageAttachmentPicker: esImageAttachmentPicker,
  workspaceSetup: esWorkspaceSetup,
  onboarding: esOnboarding,
  modelSelector: esModelSelector,
  providerCatalog: esProviderCatalog,
  providerSelection: esProviderSelection,
  providerUsage: esProviderUsage,
  pairing: esPairing,
  realtimeVoice: esRealtimeVoice,
  rewind: esRewind,
  diffViewer: esDiffViewer,
  serviceUrl: esServiceUrl,
  downloads: esDownloads,
  menu: esMenu,
  subagents: esSubagents,
  panels: esPanels,
  toolCallDetails: esToolCallDetails,
  toolCallGroup: esToolCallGroup,
  renameModal: esRenameModal,
  sidebarCallout: esSidebarCallout,
  contextWindow: esContextWindow,
  review: esReview,
  settings: esSettings,
};

export const fr: TranslationResources = {
  providerControl: frProviderControl,
  hostMonitor: frHostMonitor,
  taskBoard: frTaskBoard,
  sharing: frSharing,
  voiceTimeline: frVoiceTimeline,
  onlineService: frOnlineService,
  bots: frBots,
  paneFind: frPaneFind,
  common: frCommon,
  shell: frShell,
  composer: frComposer,
  btw: frBtw,
  agentControls: frAgentControls,
  agentStream: frAgentStream,
  agentPanel: frAgentPanel,
  sessions: frSessions,
  agentList: frAgentList,
  message: frMessage,
  importSession: frImportSession,
  workspace: frWorkspace,
  workspaceLabels: frWorkspaceLabels,
  changelog: frChangelog,
  sidebar: frSidebar,
  newWorkspace: frNewWorkspace,
  desktop: frDesktop,
  rootError: frRootError,
  startup: frStartup,
  openProject: frOpenProject,
  hostPicker: frHostPicker,
  schedules: frSchedules,
  addProjectFlow: frAddProjectFlow,
  projectPicker: frProjectPicker,
  branchSwitcher: frBranchSwitcher,
  agentAutocomplete: frAgentAutocomplete,
  loadOlderHistory: frLoadOlderHistory,
  imageAttachmentPicker: frImageAttachmentPicker,
  workspaceSetup: frWorkspaceSetup,
  onboarding: frOnboarding,
  modelSelector: frModelSelector,
  providerCatalog: frProviderCatalog,
  providerSelection: frProviderSelection,
  providerUsage: frProviderUsage,
  pairing: frPairing,
  realtimeVoice: frRealtimeVoice,
  rewind: frRewind,
  diffViewer: frDiffViewer,
  serviceUrl: frServiceUrl,
  downloads: frDownloads,
  menu: frMenu,
  subagents: frSubagents,
  panels: frPanels,
  toolCallDetails: frToolCallDetails,
  toolCallGroup: frToolCallGroup,
  renameModal: frRenameModal,
  sidebarCallout: frSidebarCallout,
  contextWindow: frContextWindow,
  review: frReview,
  settings: frSettings,
};

export const ja: TranslationResources = {
  providerControl: jaProviderControl,
  hostMonitor: jaHostMonitor,
  taskBoard: jaTaskBoard,
  sharing: jaSharing,
  voiceTimeline: jaVoiceTimeline,
  onlineService: jaOnlineService,
  bots: jaBots,
  paneFind: jaPaneFind,
  common: jaCommon,
  shell: jaShell,
  composer: jaComposer,
  btw: jaBtw,
  agentControls: jaAgentControls,
  agentStream: jaAgentStream,
  agentPanel: jaAgentPanel,
  sessions: jaSessions,
  agentList: jaAgentList,
  message: jaMessage,
  importSession: jaImportSession,
  workspace: jaWorkspace,
  workspaceLabels: jaWorkspaceLabels,
  changelog: jaChangelog,
  sidebar: jaSidebar,
  newWorkspace: jaNewWorkspace,
  desktop: jaDesktop,
  rootError: jaRootError,
  startup: jaStartup,
  openProject: jaOpenProject,
  hostPicker: jaHostPicker,
  schedules: jaSchedules,
  addProjectFlow: jaAddProjectFlow,
  projectPicker: jaProjectPicker,
  branchSwitcher: jaBranchSwitcher,
  agentAutocomplete: jaAgentAutocomplete,
  loadOlderHistory: jaLoadOlderHistory,
  imageAttachmentPicker: jaImageAttachmentPicker,
  workspaceSetup: jaWorkspaceSetup,
  onboarding: jaOnboarding,
  modelSelector: jaModelSelector,
  providerCatalog: jaProviderCatalog,
  providerSelection: jaProviderSelection,
  providerUsage: jaProviderUsage,
  pairing: jaPairing,
  realtimeVoice: jaRealtimeVoice,
  rewind: jaRewind,
  diffViewer: jaDiffViewer,
  serviceUrl: jaServiceUrl,
  downloads: jaDownloads,
  menu: jaMenu,
  subagents: jaSubagents,
  panels: jaPanels,
  toolCallDetails: jaToolCallDetails,
  toolCallGroup: jaToolCallGroup,
  renameModal: jaRenameModal,
  sidebarCallout: jaSidebarCallout,
  contextWindow: jaContextWindow,
  review: jaReview,
  settings: jaSettings,
};

export const ko: TranslationResources = {
  providerControl: koProviderControl,
  hostMonitor: koHostMonitor,
  taskBoard: koTaskBoard,
  sharing: koSharing,
  voiceTimeline: koVoiceTimeline,
  onlineService: koOnlineService,
  bots: koBots,
  paneFind: koPaneFind,
  common: koCommon,
  shell: koShell,
  composer: koComposer,
  btw: koBtw,
  agentControls: koAgentControls,
  agentStream: koAgentStream,
  agentPanel: koAgentPanel,
  sessions: koSessions,
  agentList: koAgentList,
  message: koMessage,
  importSession: koImportSession,
  workspace: koWorkspace,
  workspaceLabels: koWorkspaceLabels,
  changelog: koChangelog,
  sidebar: koSidebar,
  newWorkspace: koNewWorkspace,
  desktop: koDesktop,
  rootError: koRootError,
  startup: koStartup,
  openProject: koOpenProject,
  hostPicker: koHostPicker,
  schedules: koSchedules,
  addProjectFlow: koAddProjectFlow,
  projectPicker: koProjectPicker,
  branchSwitcher: koBranchSwitcher,
  agentAutocomplete: koAgentAutocomplete,
  loadOlderHistory: koLoadOlderHistory,
  imageAttachmentPicker: koImageAttachmentPicker,
  workspaceSetup: koWorkspaceSetup,
  onboarding: koOnboarding,
  modelSelector: koModelSelector,
  providerCatalog: koProviderCatalog,
  providerSelection: koProviderSelection,
  providerUsage: koProviderUsage,
  pairing: koPairing,
  realtimeVoice: koRealtimeVoice,
  rewind: koRewind,
  diffViewer: koDiffViewer,
  serviceUrl: koServiceUrl,
  downloads: koDownloads,
  menu: koMenu,
  subagents: koSubagents,
  panels: koPanels,
  toolCallDetails: koToolCallDetails,
  toolCallGroup: koToolCallGroup,
  renameModal: koRenameModal,
  sidebarCallout: koSidebarCallout,
  contextWindow: koContextWindow,
  review: koReview,
  settings: koSettings,
};

export const ptBR: TranslationResources = {
  providerControl: ptBRProviderControl,
  hostMonitor: ptBRHostMonitor,
  taskBoard: ptBRTaskBoard,
  sharing: ptBRSharing,
  voiceTimeline: ptBRVoiceTimeline,
  onlineService: ptBROnlineService,
  bots: ptBRBots,
  paneFind: ptBRPaneFind,
  common: ptBRCommon,
  shell: ptBRShell,
  composer: ptBRComposer,
  btw: ptBRBtw,
  agentControls: ptBRAgentControls,
  agentStream: ptBRAgentStream,
  agentPanel: ptBRAgentPanel,
  sessions: ptBRSessions,
  agentList: ptBRAgentList,
  message: ptBRMessage,
  importSession: ptBRImportSession,
  workspace: ptBRWorkspace,
  workspaceLabels: ptBRWorkspaceLabels,
  changelog: ptBRChangelog,
  sidebar: ptBRSidebar,
  newWorkspace: ptBRNewWorkspace,
  desktop: ptBRDesktop,
  rootError: ptBRRootError,
  startup: ptBRStartup,
  openProject: ptBROpenProject,
  hostPicker: ptBRHostPicker,
  schedules: ptBRSchedules,
  addProjectFlow: ptBRAddProjectFlow,
  projectPicker: ptBRProjectPicker,
  branchSwitcher: ptBRBranchSwitcher,
  agentAutocomplete: ptBRAgentAutocomplete,
  loadOlderHistory: ptBRLoadOlderHistory,
  imageAttachmentPicker: ptBRImageAttachmentPicker,
  workspaceSetup: ptBRWorkspaceSetup,
  onboarding: ptBROnboarding,
  modelSelector: ptBRModelSelector,
  providerCatalog: ptBRProviderCatalog,
  providerSelection: ptBRProviderSelection,
  providerUsage: ptBRProviderUsage,
  pairing: ptBRPairing,
  realtimeVoice: ptBRRealtimeVoice,
  rewind: ptBRRewind,
  diffViewer: ptBRDiffViewer,
  serviceUrl: ptBRServiceUrl,
  downloads: ptBRDownloads,
  menu: ptBRMenu,
  subagents: ptBRSubagents,
  panels: ptBRPanels,
  toolCallDetails: ptBRToolCallDetails,
  toolCallGroup: ptBRToolCallGroup,
  renameModal: ptBRRenameModal,
  sidebarCallout: ptBRSidebarCallout,
  contextWindow: ptBRContextWindow,
  review: ptBRReview,
  settings: ptBRSettings,
};

export const ru: TranslationResources = {
  providerControl: ruProviderControl,
  hostMonitor: ruHostMonitor,
  taskBoard: ruTaskBoard,
  sharing: ruSharing,
  voiceTimeline: ruVoiceTimeline,
  onlineService: ruOnlineService,
  bots: ruBots,
  paneFind: ruPaneFind,
  common: ruCommon,
  shell: ruShell,
  composer: ruComposer,
  btw: ruBtw,
  agentControls: ruAgentControls,
  agentStream: ruAgentStream,
  agentPanel: ruAgentPanel,
  sessions: ruSessions,
  agentList: ruAgentList,
  message: ruMessage,
  importSession: ruImportSession,
  workspace: ruWorkspace,
  workspaceLabels: ruWorkspaceLabels,
  changelog: ruChangelog,
  sidebar: ruSidebar,
  newWorkspace: ruNewWorkspace,
  desktop: ruDesktop,
  rootError: ruRootError,
  startup: ruStartup,
  openProject: ruOpenProject,
  hostPicker: ruHostPicker,
  schedules: ruSchedules,
  addProjectFlow: ruAddProjectFlow,
  projectPicker: ruProjectPicker,
  branchSwitcher: ruBranchSwitcher,
  agentAutocomplete: ruAgentAutocomplete,
  loadOlderHistory: ruLoadOlderHistory,
  imageAttachmentPicker: ruImageAttachmentPicker,
  workspaceSetup: ruWorkspaceSetup,
  onboarding: ruOnboarding,
  modelSelector: ruModelSelector,
  providerCatalog: ruProviderCatalog,
  providerSelection: ruProviderSelection,
  providerUsage: ruProviderUsage,
  pairing: ruPairing,
  realtimeVoice: ruRealtimeVoice,
  rewind: ruRewind,
  diffViewer: ruDiffViewer,
  serviceUrl: ruServiceUrl,
  downloads: ruDownloads,
  menu: ruMenu,
  subagents: ruSubagents,
  panels: ruPanels,
  toolCallDetails: ruToolCallDetails,
  toolCallGroup: ruToolCallGroup,
  renameModal: ruRenameModal,
  sidebarCallout: ruSidebarCallout,
  contextWindow: ruContextWindow,
  review: ruReview,
  settings: ruSettings,
};

export const zhCN: TranslationResources = {
  providerControl: zhCNProviderControl,
  hostMonitor: zhCNHostMonitor,
  taskBoard: zhCNTaskBoard,
  sharing: zhCNSharing,
  voiceTimeline: zhCNVoiceTimeline,
  bots: zhCNBots,
  onlineService: zhCNOnlineService,
  paneFind: zhCNPaneFind,
  common: zhCNCommon,
  shell: zhCNShell,
  composer: zhCNComposer,
  btw: zhCNBtw,
  agentControls: zhCNAgentControls,
  agentStream: zhCNAgentStream,
  agentPanel: zhCNAgentPanel,
  sessions: zhCNSessions,
  agentList: zhCNAgentList,
  message: zhCNMessage,
  importSession: zhCNImportSession,
  workspace: zhCNWorkspace,
  workspaceLabels: zhCNWorkspaceLabels,
  changelog: zhCNChangelog,
  sidebar: zhCNSidebar,
  newWorkspace: zhCNNewWorkspace,
  desktop: zhCNDesktop,
  rootError: zhCNRootError,
  startup: zhCNStartup,
  openProject: zhCNOpenProject,
  hostPicker: zhCNHostPicker,
  schedules: zhCNSchedules,
  addProjectFlow: zhCNAddProjectFlow,
  projectPicker: zhCNProjectPicker,
  branchSwitcher: zhCNBranchSwitcher,
  agentAutocomplete: zhCNAgentAutocomplete,
  loadOlderHistory: zhCNLoadOlderHistory,
  imageAttachmentPicker: zhCNImageAttachmentPicker,
  workspaceSetup: zhCNWorkspaceSetup,
  onboarding: zhCNOnboarding,
  modelSelector: zhCNModelSelector,
  providerCatalog: zhCNProviderCatalog,
  providerSelection: zhCNProviderSelection,
  providerUsage: zhCNProviderUsage,
  pairing: zhCNPairing,
  realtimeVoice: zhCNRealtimeVoice,
  rewind: zhCNRewind,
  diffViewer: zhCNDiffViewer,
  serviceUrl: zhCNServiceUrl,
  downloads: zhCNDownloads,
  menu: zhCNMenu,
  subagents: zhCNSubagents,
  panels: zhCNPanels,
  toolCallDetails: zhCNToolCallDetails,
  toolCallGroup: zhCNToolCallGroup,
  renameModal: zhCNRenameModal,
  sidebarCallout: zhCNSidebarCallout,
  contextWindow: zhCNContextWindow,
  review: zhCNReview,
  settings: zhCNSettings,
};
