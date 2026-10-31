//! Closed worker operations and their ownership requirements. Wire names are decoded at admission.

use crate::rpc::ErrorCode;

/// The resource an operation must read or exclusively own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Catalog,
    Discovery,
    Read,
    Create,
    WorkspaceCreate,
    Native,
    Bot,
    Mutation,
    Timeline,
    Session,
}

/// A validated identity location; parent and plugin identities cannot fall through as free reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Agent,
    Parent,
    AppendedAgent,
}

impl Target {
    /// Return the JSON pointer used only when adapting the wire request.
    pub(crate) const fn pointer(self) -> &'static str {
        match self {
            Self::Agent => "/agentId",
            Self::Parent => "/parentAgentId",
            Self::AppendedAgent => "/request/agentId",
        }
    }
}

/// Every operation accepted by the native worker, including trusted host-only commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    Create,
    Resume,
    Send,
    Cancel,
    Wait,
    Model,
    Thinking,
    Configure,
    Mode,
    Feature,
    Permission,
    RecentSessions,
    Import,
    Refresh,
    ForkContext,
    Rewind,
    Commands,
    Subagents,
    ChildTimeline,
    Diagnostic,
    Usage,
    Available,
    Models,
    Modes,
    Features,
    Snapshot,
    RefreshSnapshot,
    Get,
    List,
    History,
    Update,
    Archive,
    Delete,
    Detach,
    ClearAttention,
    CloseItems,
    Timeline,
    SearchTimeline,
    Prompts,
    Bot,
    WorkspaceCreate,
    RetireWorkspaces,
    PrepareDirectory,
    ResolveIdentities,
    VoiceSend,
    VoiceStatus,
    VoiceCancel,
    Realtime,
    AppendTimeline,
    Wake,
}

impl Operation {
    /// Decode a wire method before reserving or mutating native resources.
    /// # Errors
    /// Unknown and connection-owned methods are not worker operations.
    pub(crate) fn parse(method: &str) -> Result<Self, ErrorCode> {
        match method {
            "agent.create.request" => Ok(Self::Create),
            "agent.resume.request" => Ok(Self::Resume),
            "agent.message.send.request" => Ok(Self::Send),
            "agent.cancel.request" => Ok(Self::Cancel),
            "agent.finish.wait.request" => Ok(Self::Wait),
            "agent.model.set.request" => Ok(Self::Model),
            "agent.thinking.set.request" => Ok(Self::Thinking),
            "agent.config.apply.request" => Ok(Self::Configure),
            "agent.mode.set.request" => Ok(Self::Mode),
            "agent.feature.set.request" => Ok(Self::Feature),
            "agent.permission.resolve.request" => Ok(Self::Permission),
            "provider.sessions.recent.list.request" => Ok(Self::RecentSessions),
            "agent.import.request" => Ok(Self::Import),
            "agent.refresh.request" => Ok(Self::Refresh),
            "agent.fork_context.request" => Ok(Self::ForkContext),
            "agent.rewind.request" => Ok(Self::Rewind),
            "agent.commands.list.request" => Ok(Self::Commands),
            "agent.provider_subagents.list.request" => Ok(Self::Subagents),
            "agent.provider_subagents.timeline.get.request" => Ok(Self::ChildTimeline),
            "provider.diagnostic.request" => Ok(Self::Diagnostic),
            "provider.usage.list.request" => Ok(Self::Usage),
            "provider.available.list.request" => Ok(Self::Available),
            "provider.models.list.request" => Ok(Self::Models),
            "provider.modes.list.request" => Ok(Self::Modes),
            "provider.features.list.request" => Ok(Self::Features),
            "provider.snapshot.get.request" => Ok(Self::Snapshot),
            "provider.snapshot.refresh.request" => Ok(Self::RefreshSnapshot),
            "agent.get.request" => Ok(Self::Get),
            "agent.list.request" => Ok(Self::List),
            "agent.history.get.request" => Ok(Self::History),
            "agent.update.request" => Ok(Self::Update),
            "agent.archive.request" => Ok(Self::Archive),
            "agent.delete.request" => Ok(Self::Delete),
            "agent.detach.request" => Ok(Self::Detach),
            "agent.attention.clear.request" => Ok(Self::ClearAttention),
            "agent.items.close.request" => Ok(Self::CloseItems),
            "agent.timeline.get.request" => Ok(Self::Timeline),
            "agent.timeline.search.request" => Ok(Self::SearchTimeline),
            "agent.timeline.list_prompts.request" => Ok(Self::Prompts),
            "internal.bot" => Ok(Self::Bot),
            "internal.workspace.agent.create" => Ok(Self::WorkspaceCreate),
            "internal.workspace.retire" => Ok(Self::RetireWorkspaces),
            "internal.agent.directory.prepare" => Ok(Self::PrepareDirectory),
            "internal.agent.identities.resolve" => Ok(Self::ResolveIdentities),
            "internal.voice.send" => Ok(Self::VoiceSend),
            "internal.voice.status" => Ok(Self::VoiceStatus),
            "internal.voice.cancel" => Ok(Self::VoiceCancel),
            "internal.realtime.start" => Ok(Self::Realtime),
            "internal.timeline.append" => Ok(Self::AppendTimeline),
            "internal.execution.wake" => Ok(Self::Wake),
            _ => Err(ErrorCode::MethodNotFound),
        }
    }

    /// Return the canonical wire name for downstream protocol projection and diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "agent.create.request",
            Self::Resume => "agent.resume.request",
            Self::Send => "agent.message.send.request",
            Self::Cancel => "agent.cancel.request",
            Self::Wait => "agent.finish.wait.request",
            Self::Model => "agent.model.set.request",
            Self::Thinking => "agent.thinking.set.request",
            Self::Configure => "agent.config.apply.request",
            Self::Mode => "agent.mode.set.request",
            Self::Feature => "agent.feature.set.request",
            Self::Permission => "agent.permission.resolve.request",
            Self::RecentSessions => "provider.sessions.recent.list.request",
            Self::Import => "agent.import.request",
            Self::Refresh => "agent.refresh.request",
            Self::ForkContext => "agent.fork_context.request",
            Self::Rewind => "agent.rewind.request",
            Self::Commands => "agent.commands.list.request",
            Self::Subagents => "agent.provider_subagents.list.request",
            Self::ChildTimeline => "agent.provider_subagents.timeline.get.request",
            Self::Diagnostic => "provider.diagnostic.request",
            Self::Usage => "provider.usage.list.request",
            Self::Available => "provider.available.list.request",
            Self::Models => "provider.models.list.request",
            Self::Modes => "provider.modes.list.request",
            Self::Features => "provider.features.list.request",
            Self::Snapshot => "provider.snapshot.get.request",
            Self::RefreshSnapshot => "provider.snapshot.refresh.request",
            Self::Get => "agent.get.request",
            Self::List => "agent.list.request",
            Self::History => "agent.history.get.request",
            Self::Update => "agent.update.request",
            Self::Archive => "agent.archive.request",
            Self::Delete => "agent.delete.request",
            Self::Detach => "agent.detach.request",
            Self::ClearAttention => "agent.attention.clear.request",
            Self::CloseItems => "agent.items.close.request",
            Self::Timeline => "agent.timeline.get.request",
            Self::SearchTimeline => "agent.timeline.search.request",
            Self::Prompts => "agent.timeline.list_prompts.request",
            Self::Bot => "internal.bot",
            Self::WorkspaceCreate => "internal.workspace.agent.create",
            Self::RetireWorkspaces => "internal.workspace.retire",
            Self::PrepareDirectory => "internal.agent.directory.prepare",
            Self::ResolveIdentities => "internal.agent.identities.resolve",
            Self::VoiceSend => "internal.voice.send",
            Self::VoiceStatus => "internal.voice.status",
            Self::VoiceCancel => "internal.voice.cancel",
            Self::Realtime => "internal.realtime.start",
            Self::AppendTimeline => "internal.timeline.append",
            Self::Wake => "internal.execution.wake",
        }
    }

    /// Classify the ownership requirement exhaustively; adding an operation requires a decision.
    pub(crate) const fn access(self) -> Access {
        match self {
            Self::Create => Access::Create,
            Self::Resume | Self::Import => Access::Native,
            Self::Send
            | Self::Cancel
            | Self::Model
            | Self::Thinking
            | Self::Configure
            | Self::Mode
            | Self::Feature
            | Self::Permission
            | Self::Refresh
            | Self::ForkContext
            | Self::Rewind
            | Self::Subagents
            | Self::ChildTimeline
            | Self::Update
            | Self::Detach
            | Self::ClearAttention
            | Self::VoiceSend
            | Self::VoiceStatus
            | Self::VoiceCancel
            | Self::Realtime
            | Self::AppendTimeline
            | Self::Wake => Access::Session,
            Self::Wait
            | Self::Get
            | Self::List
            | Self::History
            | Self::PrepareDirectory
            | Self::ResolveIdentities => Access::Read,
            Self::RecentSessions | Self::Commands | Self::Diagnostic | Self::Usage => {
                Access::Discovery
            }
            Self::Available
            | Self::Models
            | Self::Modes
            | Self::Features
            | Self::Snapshot
            | Self::RefreshSnapshot => Access::Catalog,
            Self::Archive | Self::Delete | Self::CloseItems | Self::RetireWorkspaces => {
                Access::Mutation
            }
            Self::Timeline | Self::SearchTimeline | Self::Prompts => Access::Timeline,
            Self::Bot => Access::Bot,
            Self::WorkspaceCreate => Access::WorkspaceCreate,
        }
    }

    /// Select the exact request identity, independently of operation naming conventions.
    pub(crate) const fn target(self) -> Option<Target> {
        match self {
            Self::Send
            | Self::Cancel
            | Self::Wait
            | Self::Model
            | Self::Thinking
            | Self::Configure
            | Self::Mode
            | Self::Feature
            | Self::Permission
            | Self::Refresh
            | Self::ForkContext
            | Self::Rewind
            | Self::Commands
            | Self::Usage
            | Self::Get
            | Self::Update
            | Self::Archive
            | Self::Delete
            | Self::Detach
            | Self::ClearAttention
            | Self::Timeline
            | Self::SearchTimeline
            | Self::Prompts
            | Self::VoiceSend
            | Self::VoiceStatus
            | Self::VoiceCancel
            | Self::Realtime
            | Self::Wake => Some(Target::Agent),
            Self::Subagents | Self::ChildTimeline => Some(Target::Parent),
            Self::AppendTimeline => Some(Target::AppendedAgent),
            Self::Create
            | Self::Resume
            | Self::RecentSessions
            | Self::Import
            | Self::Diagnostic
            | Self::Available
            | Self::Models
            | Self::Modes
            | Self::Features
            | Self::Snapshot
            | Self::RefreshSnapshot
            | Self::List
            | Self::History
            | Self::CloseItems
            | Self::Bot
            | Self::WorkspaceCreate
            | Self::RetireWorkspaces
            | Self::PrepareDirectory
            | Self::ResolveIdentities => None,
        }
    }
}

#[cfg(test)]
mod tests;
